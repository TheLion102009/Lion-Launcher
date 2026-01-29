use crate::core::profiles::ProfileManager;
use crate::types::profile::{Profile, ProfileList};
use crate::types::version::ModLoader;
use std::time::SystemTime;
use std::collections::HashMap;

#[tauri::command]
pub async fn get_profiles() -> Result<ProfileList, String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    manager.load_profiles().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_profile(
    name: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
) -> Result<ProfileList, String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;

    let mod_loader = match loader.as_str() {
        "vanilla" => ModLoader::Vanilla,
        "fabric" => ModLoader::Fabric,
        "forge" => ModLoader::Forge,
        "neoforge" => ModLoader::NeoForge,
        "quilt" => ModLoader::Quilt,
        _ => return Err("Invalid mod loader".to_string()),
    };

    let profile = Profile::new(name, minecraft_version, mod_loader, loader_version);
    manager.create_profile(profile).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_profile(profile_id: String) -> Result<ProfileList, String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    manager.delete_profile(&profile_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_profile(profile_id: String, updates: serde_json::Value) -> Result<ProfileList, String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let mut profiles = manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile_mut(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    // Update fields from JSON
    if let Some(name) = updates.get("name").and_then(|v| v.as_str()) {
        profile.name = name.to_string();
    }

    if let Some(mc_version) = updates.get("minecraft_version").and_then(|v| v.as_str()) {
        profile.minecraft_version = mc_version.to_string();
    }

    if let Some(loader) = updates.get("loader").and_then(|v| v.as_str()) {
        use crate::types::version::ModLoader;
        profile.loader.loader = match loader {
            "fabric" => ModLoader::Fabric,
            "forge" => ModLoader::Forge,
            "neoforge" => ModLoader::NeoForge,
            "quilt" => ModLoader::Quilt,
            _ => ModLoader::Vanilla,
        };
    }

    if let Some(loader_version) = updates.get("loader_version").and_then(|v| v.as_str()) {
        if !loader_version.is_empty() {
            profile.loader.version = loader_version.to_string();
        }
    }

    if let Some(memory) = updates.get("memory_mb").and_then(|v| v.as_u64()) {
        profile.memory_mb = Some(memory as u32);
    }

    if let Some(java_args) = updates.get("java_args").and_then(|v| v.as_array()) {
        let args: Vec<String> = java_args.iter()
            .filter_map(|a| a.as_str())
            .map(|s| s.to_string())
            .collect();
        profile.java_args = if args.is_empty() { None } else { Some(args) };
    }

    // Icon path wird als Base64 Data URL gespeichert
    if let Some(icon) = updates.get("icon_path").and_then(|v| v.as_str()) {
        if icon.starts_with("data:image") {
            profile.icon_path = Some(std::path::PathBuf::from(icon));
        }
    }

    manager.save_profiles(&profiles).await.map_err(|e| e.to_string())?;
    Ok(profiles)
}

#[tauri::command]
pub async fn launch_profile(profile_id: String, username: String) -> Result<(), String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let mut profiles = manager.load_profiles().await.map_err(|e| e.to_string())?;

    // Clone profile for launching
    let profile_to_launch = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?
        .clone();

    // Settings-Sync VOR dem Start: Sammle alle options.txt und merge
    if profile_to_launch.settings_sync {
        tracing::info!("Running auto-sync before launch...");

        // 1. OPTIONS.TXT - Sammle alle und merge (neueste gewinnt)
        let combined = create_combined_options(&profiles.profiles).await;

        if !combined.is_empty() {
            let profile_options = profile_to_launch.game_dir.join("options.txt");

            // Stelle sicher, dass das Profil-Verzeichnis existiert
            tokio::fs::create_dir_all(&profile_to_launch.game_dir).await.ok();

            // Merge mit existierenden Profil-Settings (behält version etc.)
            let final_content = if profile_options.exists() {
                if let Ok(existing) = tokio::fs::read_to_string(&profile_options).await {
                    merge_for_profile(&existing, &combined)
                } else {
                    combined.clone()
                }
            } else {
                combined.clone()
            };

            tokio::fs::write(&profile_options, &final_content).await.ok();
            tracing::info!("Synced combined settings to profile before launch");

            // Speichere auch in shared_options.txt für Referenz
            let shared_file = crate::config::defaults::shared_settings_file();
            if let Some(parent) = shared_file.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
            tokio::fs::write(&shared_file, &combined).await.ok();
        }

        // 2. SERVERS.DAT - Kopiere die neueste Server-Liste
        if let Some(latest_servers) = find_latest_file("servers.dat", &profiles.profiles).await {
            let target = profile_to_launch.game_dir.join("servers.dat");
            if latest_servers != target {
                if let Err(e) = tokio::fs::copy(&latest_servers, &target).await {
                    tracing::warn!("Failed to sync servers.dat: {}", e);
                } else {
                    tracing::info!("Synced servers.dat from {:?}", latest_servers);
                }
            }
        }

        // 3. RESOURCEPACKS - Kopiere/Sync den resourcepacks Ordner
        sync_resourcepacks(&profiles.profiles, &profile_to_launch.game_dir).await;
    }

    // Update last played
    if let Some(profile) = profiles.get_profile_mut(&profile_id) {
        profile.update_last_played();
    }
    manager.save_profiles(&profiles).await.map_err(|e| e.to_string())?;

    // Hole Account-Daten (UUID, Username, Token) vom aktiven Account
    let (account_uuid, account_username, access_token) =
        crate::gui::auth::get_active_access_token()
            .unwrap_or_else(|| {
                // Fallback für Offline-Accounts
                let uuid = uuid::Uuid::new_v4().to_string().replace("-", "");
                (uuid, username.clone(), "0".to_string())
            });

    tracing::info!(
        "Launching Minecraft: username={}, uuid={}, has_valid_token={}",
        account_username,
        account_uuid,
        access_token != "0"
    );

    let launcher = crate::core::minecraft::MinecraftLauncher::new().map_err(|e| e.to_string())?;
    launcher.launch(
        &profile_to_launch,
        &account_username,
        &account_uuid,
        if access_token == "0" { None } else { Some(&access_token) }
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

// ==================== SETTINGS SYNC FUNKTIONEN ====================


/// Sammelt alle options.txt von allen Profilen mit Sync und merged sie.
/// Die neueste Änderung hat Vorrang.
async fn create_combined_options(profiles: &[Profile]) -> String {
    // Sammle alle options.txt mit Zeitstempel
    let mut all_options: Vec<(SystemTime, std::path::PathBuf)> = Vec::new();

    for profile in profiles {
        if !profile.settings_sync {
            continue;
        }

        let options_path = profile.game_dir.join("options.txt");
        if options_path.exists() {
            if let Ok(metadata) = std::fs::metadata(&options_path) {
                let mut time = SystemTime::UNIX_EPOCH;
                if let Ok(modified) = metadata.modified() {
                    time = time.max(modified);
                }
                all_options.push((time, options_path));
            }
        }
    }

    if all_options.is_empty() {
        tracing::info!("No options.txt files found for sync");
        return String::new();
    }

    // Sortiere nach Zeit (älteste zuerst, damit neueste überschreibt)
    all_options.sort_by_key(|(time, _)| *time);

    tracing::info!("Found {} options.txt files for sync", all_options.len());

    // Starte mit leerer HashMap
    let mut combined: HashMap<String, String> = HashMap::new();

    // Lese auch shared_options.txt als Fallback
    let shared_file = crate::config::defaults::shared_settings_file();
    if shared_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&shared_file) {
            for (key, value) in parse_options(&content) {
                combined.insert(key, value);
            }
        }
    }

    // Merge alle (sortiert nach Zeit)
    for (_, path) in &all_options {
        if let Ok(content) = std::fs::read_to_string(path) {
            for (key, value) in parse_options(&content) {
                combined.insert(key, value);
            }
        }
    }

    tracing::info!("Combined {} settings from all profiles", combined.len());

    // Erstelle String
    let mut lines: Vec<String> = combined
        .iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect();
    lines.sort();
    lines.join("\n")
}

/// Merged combined options in ein Profil, behält aber profil-spezifische Keys
fn merge_for_profile(existing: &str, combined: &str) -> String {
    let mut values: HashMap<String, String> = HashMap::new();

    // Blacklist: Diese Keys werden nicht überschrieben (version-spezifisch)
    let blacklist = ["version"];

    // Lese existierende Werte
    let existing_values: HashMap<String, String> = parse_options(existing).into_iter().collect();

    // Speichere Blacklist-Werte vom existierenden Profil
    let mut preserved: HashMap<String, String> = HashMap::new();
    for key in &blacklist {
        if let Some(value) = existing_values.get(*key) {
            preserved.insert(key.to_string(), value.clone());
        }
    }

    // Übernehme alle combined Werte
    for (key, value) in parse_options(combined) {
        values.insert(key, value);
    }

    // Stelle Blacklist-Werte wieder her
    for (key, value) in preserved {
        values.insert(key, value);
    }

    // Erstelle String
    let mut lines: Vec<String> = values
        .iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect();
    lines.sort();
    lines.join("\n")
}

/// Parst options.txt in Key-Value Paare
fn parse_options(content: &str) -> Vec<(String, String)> {
    let mut values = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            values.push((key.to_string(), value.to_string()));
        }
    }
    values
}

/// Findet die neueste Version einer Datei über alle Profile
async fn find_latest_file(filename: &str, profiles: &[Profile]) -> Option<std::path::PathBuf> {
    let mut latest_time = SystemTime::UNIX_EPOCH;
    let mut latest_path: Option<std::path::PathBuf> = None;

    for profile in profiles {
        if !profile.settings_sync {
            continue;
        }

        let file_path = profile.game_dir.join(filename);

        if let Ok(metadata) = std::fs::metadata(&file_path) {
            let mut time = SystemTime::UNIX_EPOCH;

            if let Ok(created) = metadata.created() {
                time = time.max(created);
            }
            if let Ok(modified) = metadata.modified() {
                time = time.max(modified);
            }

            if latest_path.is_none() || time > latest_time {
                latest_time = time;
                latest_path = Some(file_path);
            }
        }
    }

    latest_path
}

/// Synchronisiert resourcepacks von allen Profilen in das Ziel-Profil
async fn sync_resourcepacks(profiles: &[Profile], target_game_dir: &std::path::Path) {
    let target_resourcepacks = target_game_dir.join("resourcepacks");

    // Erstelle resourcepacks Ordner falls nicht vorhanden
    if let Err(e) = tokio::fs::create_dir_all(&target_resourcepacks).await {
        tracing::warn!("Failed to create resourcepacks dir: {}", e);
        return;
    }

    // Sammle alle resourcepacks von allen Profilen
    let mut all_packs: HashMap<String, (SystemTime, std::path::PathBuf)> = HashMap::new();

    for profile in profiles {
        if !profile.settings_sync {
            continue;
        }

        let resourcepacks_dir = profile.game_dir.join("resourcepacks");

        if !resourcepacks_dir.exists() {
            continue;
        }

        let Ok(entries) = std::fs::read_dir(&resourcepacks_dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let filename = match path.file_name() {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue,
            };

            // Hole Änderungszeit
            let mut time = SystemTime::UNIX_EPOCH;
            if let Ok(metadata) = std::fs::metadata(&path) {
                if let Ok(modified) = metadata.modified() {
                    time = time.max(modified);
                }
            }

            // Behalte nur die neueste Version jedes Packs
            if let Some((existing_time, _)) = all_packs.get(&filename) {
                if time > *existing_time {
                    all_packs.insert(filename, (time, path));
                }
            } else {
                all_packs.insert(filename, (time, path));
            }
        }
    }

    // Kopiere alle Packs ins Ziel-Profil
    let mut synced_count = 0;
    for (filename, (_, source_path)) in all_packs {
        let target_path = target_resourcepacks.join(&filename);

        // Überspringe wenn bereits vorhanden und gleich oder neuer
        if target_path.exists() {
            if let (Ok(source_meta), Ok(target_meta)) = (std::fs::metadata(&source_path), std::fs::metadata(&target_path)) {
                let source_time = source_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                let target_time = target_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if target_time >= source_time {
                    continue;
                }
            }
        }

        // Kopiere Datei oder Ordner
        if source_path.is_dir() {
            if let Err(e) = copy_dir_recursive(&source_path, &target_path).await {
                tracing::warn!("Failed to copy resourcepack dir {}: {}", filename, e);
            } else {
                synced_count += 1;
            }
        } else {
            if let Err(e) = tokio::fs::copy(&source_path, &target_path).await {
                tracing::warn!("Failed to copy resourcepack {}: {}", filename, e);
            } else {
                synced_count += 1;
            }
        }
    }

    if synced_count > 0 {
        tracing::info!("Synced {} resourcepacks to profile", synced_count);
    }
}

/// Kopiert einen Ordner rekursiv
async fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dst).await?;

    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }

    Ok(())
}

