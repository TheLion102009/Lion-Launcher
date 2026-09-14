use crate::core::profiles::ProfileManager;
use crate::types::profile::{Profile, ProfileList};
use crate::types::version::ModLoader;
use base64::{engine::general_purpose, Engine as _};
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::time::SystemTime;
use walkdir::WalkDir;

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
    manager
        .create_profile(profile)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_profile(profile_id: String) -> Result<ProfileList, String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    manager
        .delete_profile(&profile_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_profile(
    profile_id: String,
    updates: serde_json::Value,
) -> Result<ProfileList, String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let mut profiles = manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles
        .get_profile_mut(&profile_id)
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
        let args: Vec<String> = java_args
            .iter()
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

    manager
        .save_profiles(&profiles)
        .await
        .map_err(|e| e.to_string())?;
    Ok(profiles)
}

#[tauri::command]
pub async fn launch_profile(
    app_handle: tauri::AppHandle,
    profile_id: String,
    username: String,
) -> Result<(), String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let mut profiles = manager.load_profiles().await.map_err(|e| e.to_string())?;

    // Clone profile for launching
    let profile_to_launch = profiles
        .get_profile(&profile_id)
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
            tokio::fs::create_dir_all(&profile_to_launch.game_dir)
                .await
                .ok();

            // Merge mit existierenden Profil-Settings (beh├ñlt version etc.)
            let final_content = if profile_options.exists() {
                if let Ok(existing) = tokio::fs::read_to_string(&profile_options).await {
                    merge_for_profile(&existing, &combined)
                } else {
                    combined.clone()
                }
            } else {
                combined.clone()
            };

            tokio::fs::write(&profile_options, &final_content)
                .await
                .ok();
            tracing::info!("Synced combined settings to profile before launch");

            // Speichere auch in shared_options.txt f├╝r Referenz
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
    manager
        .save_profiles(&profiles)
        .await
        .map_err(|e| e.to_string())?;

    // Hole Account-Daten (UUID, Username, Token) vom aktiven Account
    // WICHTIG: Verwende refreshed Funktion um abgelaufene Tokens automatisch zu erneuern!
    let (account_uuid, account_username, access_token) =
        crate::gui::auth::get_active_access_token_refreshed()
            .await
            .unwrap_or_else(|| {
                // Fallback f├╝r Offline-Accounts
                let uuid = uuid::Uuid::new_v4().to_string().replace("-", "");
                (uuid, username.clone(), "0".to_string())
            });

    tracing::info!(
        "Launching Minecraft: username={}, uuid={}, has_valid_token={}",
        account_username,
        account_uuid,
        access_token != "0"
    );

    let _progress_guard = setup_launch_progress_bridge(&app_handle);

    let launcher = crate::core::minecraft::MinecraftLauncher::new().map_err(|e| e.to_string())?;
    let result = launcher
        .launch(
            &profile_to_launch,
            &account_username,
            &account_uuid,
            if access_token == "0" {
                None
            } else {
                Some(&access_token)
            },
        )
        .await
        .map_err(|e| e.to_string());

    // Sender entfernen damit der Empf├ñnger-Thread sauber beendet
    crate::core::minecraft::clear_launch_progress_sender();

    result.map(|_| ())
}

#[tauri::command]
pub async fn prepare_profile_download(
    app_handle: tauri::AppHandle,
    profile_id: String,
) -> Result<(), String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile_to_prepare = profiles
        .get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?
        .clone();

    let _progress_guard = setup_launch_progress_bridge(&app_handle);

    let launcher = crate::core::minecraft::MinecraftLauncher::new().map_err(|e| e.to_string())?;
    launcher
        .prepare_profile(&profile_to_prepare)
        .await
        .map_err(|e| e.to_string())
        .map(|_| ())
}

#[tauri::command]
pub async fn export_profile_share_data(
    profile_id: String,
    include_world_data: bool,
    include_logs_data: bool,
    include_settings_data: bool,
) -> Result<serde_json::Value, String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = manager.load_profiles().await.map_err(|e| e.to_string())?;
    let profile = profiles
        .get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let mut payload = serde_json::Map::new();

    if include_settings_data {
        let options_path = profile.game_dir.join("options.txt");
        if let Ok(options_txt) = tokio::fs::read_to_string(&options_path).await {
            payload.insert(
                "settings".to_string(),
                serde_json::json!({ "options_txt": options_txt }),
            );
        }
    }

    if include_world_data {
        let saves_dir = profile.game_dir.join("saves");
        if let Some(zip_b64) = zip_directory_to_base64(&saves_dir).map_err(|e| e.to_string())? {
            payload.insert(
                "worlds_archive_base64".to_string(),
                serde_json::Value::String(zip_b64),
            );
        }
    }

    if include_logs_data {
        let logs_dir = profile.game_dir.join("logs");
        if let Some(zip_b64) = zip_directory_to_base64(&logs_dir).map_err(|e| e.to_string())? {
            payload.insert(
                "logs_archive_base64".to_string(),
                serde_json::Value::String(zip_b64),
            );
        }
    }

    Ok(serde_json::Value::Object(payload))
}

#[tauri::command]
pub async fn import_profile_share_data(
    profile_id: String,
    data: serde_json::Value,
    import_world_data: bool,
    import_logs_data: bool,
    import_settings_data: bool,
) -> Result<(), String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = manager.load_profiles().await.map_err(|e| e.to_string())?;
    let profile = profiles
        .get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    if import_settings_data {
        if let Some(options_txt) = data
            .get("settings")
            .and_then(|s| s.get("options_txt"))
            .and_then(|v| v.as_str())
        {
            tokio::fs::create_dir_all(&profile.game_dir)
                .await
                .map_err(|e| e.to_string())?;
            tokio::fs::write(profile.game_dir.join("options.txt"), options_txt)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    if import_world_data {
        if let Some(b64) = data.get("worlds_archive_base64").and_then(|v| v.as_str()) {
            extract_base64_zip_to_dir(b64, &profile.game_dir.join("saves"))
                .map_err(|e| e.to_string())?;
        }
    }

    if import_logs_data {
        if let Some(b64) = data.get("logs_archive_base64").and_then(|v| v.as_str()) {
            extract_base64_zip_to_dir(b64, &profile.game_dir.join("logs"))
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn save_lion_file_with_dialog(
    default_file_name: String,
    content: String,
) -> Result<Option<String>, String> {
    let suggested_name = if default_file_name.trim().is_empty() {
        "profile.lion".to_string()
    } else {
        default_file_name
    };

    let save_path = rfd::FileDialog::new()
        .add_filter("Lion Profile", &["lion"])
        .set_file_name(&suggested_name)
        .save_file();

    match save_path {
        Some(path) => {
            tokio::fs::write(&path, content)
                .await
                .map_err(|e| format!("Failed to write .lion file: {}", e))?;
            Ok(Some(path.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

struct LaunchProgressGuard;

impl Drop for LaunchProgressGuard {
    fn drop(&mut self) {
        crate::core::minecraft::clear_launch_progress_sender();
    }
}

fn setup_launch_progress_bridge(app_handle: &tauri::AppHandle) -> LaunchProgressGuard {
    let (progress_tx, progress_rx) = std::sync::mpsc::sync_channel::<(String, u8)>(8);
    crate::core::minecraft::set_launch_progress_sender(progress_tx);

    let app_for_progress = app_handle.clone();
    std::thread::spawn(move || {
        use tauri::Emitter;
        while let Ok((status, percent)) = progress_rx.recv() {
            app_for_progress
                .emit(
                    "launch-progress",
                    serde_json::json!({
                        "status": status,
                        "percent": percent
                    }),
                )
                .ok();
        }
    });

    LaunchProgressGuard
}

fn zip_directory_to_base64(source_dir: &Path) -> anyhow::Result<Option<String>> {
    if !source_dir.exists() || !source_dir.is_dir() {
        return Ok(None);
    }

    let mut has_entries = false;
    for entry in std::fs::read_dir(source_dir)? {
        if entry.is_ok() {
            has_entries = true;
            break;
        }
    }
    if !has_entries {
        return Ok(None);
    }

    let mut cursor = Cursor::new(Vec::<u8>::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = {
            let opts = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            #[cfg(unix)]
            let opts = opts.unix_permissions(0o755);
            opts
        };

        for entry in WalkDir::new(source_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            let rel = match path.strip_prefix(source_dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if rel.as_os_str().is_empty() {
                continue;
            }

            let rel_name = rel.to_string_lossy().replace('\\', "/");
            if path.is_dir() {
                zip.add_directory(rel_name, options)?;
            } else if path.is_file() {
                zip.start_file(rel_name, options)?;
                let mut f = std::fs::File::open(path)?;
                std::io::copy(&mut f, &mut zip)?;
            }
        }

        zip.finish()?;
    }

    let bytes = cursor.into_inner();
    if bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some(general_purpose::STANDARD.encode(bytes)))
}

fn extract_base64_zip_to_dir(base64_data: &str, target_dir: &Path) -> anyhow::Result<()> {
    let bytes = general_purpose::STANDARD.decode(base64_data)?;
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)?;

    std::fs::create_dir_all(target_dir)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let Some(safe_rel_path) = file.enclosed_name().map(|p| p.to_owned()) else {
            continue;
        };
        let out_path = target_dir.join(safe_rel_path);

        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            out_file.write_all(&buffer)?;
        }
    }

    Ok(())
}

// ==================== SETTINGS SYNC FUNKTIONEN ====================

/// Sammelt alle options.txt von allen Profilen mit Sync und merged sie.
/// Die neueste ├änderung hat Vorrang.
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

    // Sortiere nach Zeit (├ñlteste zuerst, damit neueste ├╝berschreibt)
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

/// Merged combined options in ein Profil, beh├ñlt aber profil-spezifische Keys
fn merge_for_profile(existing: &str, combined: &str) -> String {
    let mut values: HashMap<String, String> = HashMap::new();

    // Blacklist: Diese Keys werden nicht ├╝berschrieben (version-spezifisch)
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

    // ├£bernehme alle combined Werte
    for (key, value) in parse_options(combined) {
        values.insert(key, value);
    }

    // Stelle Blacklist-Werte wieder her
    for (key, value) in preserved {
        values.insert(key, value);
    }

    // Erstelle String
    let mut lines: Vec<String> = values.iter().map(|(k, v)| format!("{}:{}", k, v)).collect();
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

/// Findet die neueste Version einer Datei ├╝ber alle Profile
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

            // Hole ├änderungszeit
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

        // ├£berspringe wenn bereits vorhanden und gleich oder neuer
        if target_path.exists() {
            if let (Ok(source_meta), Ok(target_meta)) = (
                std::fs::metadata(&source_path),
                std::fs::metadata(&target_path),
            ) {
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
