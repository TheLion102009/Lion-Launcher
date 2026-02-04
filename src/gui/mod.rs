pub mod mod_browser;
pub mod profile_manager;
pub mod settings;
pub mod components;
pub mod themes;
pub mod auth;

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
pub async fn get_profile_logs(profile_id: String, log_type: String) -> Result<String, String> {
    use crate::core::profiles::ProfileManager;

    tracing::info!("get_profile_logs called: profile_id={}, log_type={}", profile_id, log_type);

    let profile_manager = ProfileManager::new().map_err(|e| {
        tracing::error!("Failed to create ProfileManager: {}", e);
        e.to_string()
    })?;

    let profiles = profile_manager.load_profiles().await.map_err(|e| {
        tracing::error!("Failed to load profiles: {}", e);
        e.to_string()
    })?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| {
            tracing::error!("Profile not found: {}", profile_id);
            "Profile not found".to_string()
        })?;

    let logs_dir = profile.game_dir.join("logs");
    tracing::info!("Looking for logs in: {:?}", logs_dir);

    let log_file = match log_type.as_str() {
        "latest" => logs_dir.join("latest.log"),
        "debug" => logs_dir.join("debug.log"),
        "crash" => {
            // Finde neuesten Crash-Report
            let crash_dir = profile.game_dir.join("crash-reports");
            if crash_dir.exists() {
                let mut entries: Vec<_> = std::fs::read_dir(&crash_dir)
                    .map_err(|e| e.to_string())?
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map_or(false, |ext| ext == "txt"))
                    .collect();
                entries.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
                entries.last()
                    .map(|e| e.path())
                    .ok_or_else(|| "Keine Crash-Reports gefunden".to_string())?
            } else {
                return Ok("📋 Keine Crash-Reports vorhanden\n\nDer crash-reports Ordner existiert nicht.".to_string());
            }
        }
        _ => return Err("Unbekannter Log-Typ".to_string()),
    };

    tracing::info!("Log file path: {:?}, exists: {}", log_file, log_file.exists());

    // Prüfe ob Log-Datei existiert
    if !log_file.exists() {
        tracing::warn!("Log file does not exist: {:?}", log_file);
        return Ok(format!(
            "📋 Log-Datei nicht gefunden\n\n\
            Pfad: {:?}\n\n\
            Mögliche Gründe:\n\
            • Minecraft wurde noch nie gestartet\n\
            • Minecraft konnte nicht starten\n\n\
            Starte Minecraft und versuche es erneut.",
            log_file
        ));
    }

    // Lese Log-Datei asynchron
    let content = match tokio::fs::read_to_string(&log_file).await {
        Ok(c) => {
            tracing::info!("Read log file: {} bytes", c.len());
            c
        }
        Err(e) => {
            tracing::error!("Failed to read log file: {}", e);
            return Ok(format!(
                "⚠️ Fehler beim Lesen der Log-Datei\n\n\
                Fehler: {}\n\
                Pfad: {:?}",
                e, log_file
            ));
        }
    };

    // Falls leer
    if content.is_empty() {
        return Ok("📄 Log-Datei ist leer\n\nDie Datei existiert, enthält aber keine Daten.".to_string());
    }

    // Nur letzte 10000 Zeilen für Performance
    let lines: Vec<&str> = content.lines().collect();
    let start = if lines.len() > 10000 { lines.len() - 10000 } else { 0 };
    let truncated: String = lines[start..].join("\n");

    tracing::info!("Returning {} lines of logs", lines.len() - start);
    Ok(truncated)
}

#[tauri::command]
pub async fn open_profile_folder(profile_id: String, subfolder: Option<String>) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let path = if let Some(sub) = subfolder {
        profile.game_dir.join(sub)
    } else {
        profile.game_dir.clone()
    };

    // Erstelle Ordner falls nicht vorhanden
    tokio::fs::create_dir_all(&path).await.map_err(|e| e.to_string())?;

    // Öffne Ordner
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Repariert ein Profil, indem Minecraft und Loader-Dateien neu heruntergeladen werden
#[tauri::command]
pub async fn repair_profile(profile_id: String) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;
    use crate::config::defaults;

    tracing::info!("Repairing profile: {}", profile_id);

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let mc_version = &profile.minecraft_version;
    let loader = &profile.loader.loader;

    tracing::info!("Profile: {} - MC {} with {:?}", profile.name, mc_version, loader);

    // Lösche Version-spezifische Dateien
    let versions_dir = defaults::launcher_dir().join("versions").join(mc_version);
    let libraries_dir = defaults::launcher_dir().join("libraries");

    // Lösche die Minecraft Version JAR und JSON
    if versions_dir.exists() {
        tracing::info!("Removing version directory: {:?}", versions_dir);
        tokio::fs::remove_dir_all(&versions_dir).await.ok();
    }

    // Lösche Loader-spezifische Installer
    match loader {
        crate::types::version::ModLoader::NeoForge => {
            // Lösche NeoForge Installer
            let pattern = format!("neoforge-");
            if let Ok(entries) = std::fs::read_dir(&libraries_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.contains(&pattern) && name.contains("installer") {
                        tracing::info!("Removing NeoForge installer: {:?}", entry.path());
                        tokio::fs::remove_file(entry.path()).await.ok();
                    }
                }
            }
            // Lösche NeoForge Libraries
            let neoforge_libs = libraries_dir.join("net").join("neoforged");
            if neoforge_libs.exists() {
                tracing::info!("Removing NeoForge libraries: {:?}", neoforge_libs);
                tokio::fs::remove_dir_all(&neoforge_libs).await.ok();
            }
        }
        crate::types::version::ModLoader::Forge => {
            // Lösche Forge Installer
            let pattern = format!("forge-{}", mc_version);
            if let Ok(entries) = std::fs::read_dir(&libraries_dir) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.contains(&pattern) && name.contains("installer") {
                        tracing::info!("Removing Forge installer: {:?}", entry.path());
                        tokio::fs::remove_file(entry.path()).await.ok();
                    }
                }
            }
            // Lösche Forge Libraries
            let forge_libs = libraries_dir.join("net").join("minecraftforge");
            if forge_libs.exists() {
                tracing::info!("Removing Forge libraries: {:?}", forge_libs);
                tokio::fs::remove_dir_all(&forge_libs).await.ok();
            }
        }
        crate::types::version::ModLoader::Fabric => {
            // Lösche Fabric Libraries
            let fabric_libs = libraries_dir.join("net").join("fabricmc");
            if fabric_libs.exists() {
                tracing::info!("Removing Fabric libraries: {:?}", fabric_libs);
                tokio::fs::remove_dir_all(&fabric_libs).await.ok();
            }
        }
        crate::types::version::ModLoader::Quilt => {
            // Lösche Quilt Libraries
            let quilt_libs = libraries_dir.join("org").join("quiltmc");
            if quilt_libs.exists() {
                tracing::info!("Removing Quilt libraries: {:?}", quilt_libs);
                tokio::fs::remove_dir_all(&quilt_libs).await.ok();
            }
        }
        crate::types::version::ModLoader::Vanilla => {
            // Nichts zu löschen
        }
    }

    tracing::info!("Profile repair completed. Next launch will re-download everything.");
    Ok(())
}

/// Leert den Cache eines Profils (temporäre Dateien, Shader-Cache, etc.)
#[tauri::command]
pub async fn clear_profile_cache(profile_id: String) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;

    tracing::info!("Clearing cache for profile: {}", profile_id);

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let game_dir = &profile.game_dir;

    // Lösche temporäre Ordner
    let cache_dirs = vec![
        game_dir.join(".cache"),
        game_dir.join("shadercache"),
        game_dir.join("ShaderCache"),
        game_dir.join(".mixin.out"),
        game_dir.join("crash-reports"),
        game_dir.join("debug"),
    ];

    for cache_dir in cache_dirs {
        if cache_dir.exists() {
            tracing::info!("Removing cache directory: {:?}", cache_dir);
            tokio::fs::remove_dir_all(&cache_dir).await.ok();
        }
    }

    // Lösche temporäre Dateien
    let temp_files = vec![
        game_dir.join("hs_err_pid*.log"),
        game_dir.join("launcher.log"),
        game_dir.join("classpath_debug.txt"),
    ];

    for pattern in temp_files {
        if let Some(parent) = pattern.parent() {
            if let Some(filename) = pattern.file_name() {
                let filename_str = filename.to_string_lossy();
                if let Ok(entries) = std::fs::read_dir(parent) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        // Einfacher Pattern-Match für Wildcard
                        if filename_str.contains("*") {
                            let prefix = filename_str.split('*').next().unwrap_or("");
                            if name.starts_with(prefix) {
                                tracing::info!("Removing temp file: {:?}", entry.path());
                                tokio::fs::remove_file(entry.path()).await.ok();
                            }
                        } else if name == filename_str {
                            tracing::info!("Removing temp file: {:?}", entry.path());
                            tokio::fs::remove_file(entry.path()).await.ok();
                        }
                    }
                }
            }
        }
    }

    tracing::info!("Cache cleared for profile: {}", profile.name);
    Ok(())
}

// Re-export commands for convenience
pub use mod_browser::*;
pub use profile_manager::*;
pub use settings::*;

// ==================== MOD-VERWALTUNG ====================

#[derive(serde::Serialize)]
pub struct InstalledMod {
    pub filename: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub disabled: bool,
    pub icon_url: Option<String>,
    pub has_update: bool,
    pub latest_version: Option<String>,
    pub mod_id: Option<String>,
}

#[tauri::command]
pub async fn get_installed_mods(profile_id: String) -> Result<Vec<InstalledMod>, String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let mods_dir = profile.game_dir.join("mods");

    if !mods_dir.exists() {
        return Ok(Vec::new());
    }

    let mut installed_mods = Vec::new();

    let entries = std::fs::read_dir(&mods_dir).map_err(|e| e.to_string())?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if let Some(ext) = path.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();

            // .jar = aktiv, .jar.disabled = deaktiviert
            if ext_str == "jar" || ext_str == "disabled" {
                let filename = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                let disabled = filename.ends_with(".disabled");

                // Versuche Metadaten-Datei zu lesen
                let meta_path = if disabled {
                    // Für .disabled Dateien: filename.disabled -> filename.jar.meta.json
                    let base = filename.trim_end_matches(".disabled");
                    mods_dir.join(format!("{}.meta.json", base))
                } else {
                    path.with_extension("jar.meta.json")
                };

                let (mut name, mut version, mut mod_id, mut icon_url) = (None, None, None, None);

                // Versuche Metadaten zu laden
                if meta_path.exists() {
                    if let Ok(meta_content) = std::fs::read_to_string(&meta_path) {
                        if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&meta_content) {
                            name = meta.get("mod_name").and_then(|v| v.as_str()).map(|s| s.to_string());
                            version = meta.get("version").and_then(|v| v.as_str()).map(|s| s.to_string());
                            mod_id = meta.get("mod_id").and_then(|v| v.as_str()).map(|s| s.to_string());
                            icon_url = meta.get("icon_url").and_then(|v| v.as_str()).map(|s| s.to_string());
                        }
                    }
                }

                // Fallback: Extrahiere aus Dateinamen
                if name.is_none() || mod_id.is_none() {
                    let clean_name = filename
                        .trim_end_matches(".disabled")
                        .trim_end_matches(".jar");
                    let (extracted_name, extracted_version, extracted_mod_id) = extract_mod_info(clean_name);

                    if name.is_none() {
                        name = extracted_name;
                    }
                    if version.is_none() {
                        version = extracted_version;
                    }
                    if mod_id.is_none() {
                        mod_id = extracted_mod_id;
                    }
                }

                installed_mods.push(InstalledMod {
                    filename,
                    name,
                    version,
                    disabled,
                    icon_url,
                    has_update: false,
                    latest_version: None,
                    mod_id,
                });
            }
        }
    }

    // Sortiere nach Name
    installed_mods.sort_by(|a, b| {
        a.name.as_deref().unwrap_or(&a.filename)
            .to_lowercase()
            .cmp(&b.name.as_deref().unwrap_or(&b.filename).to_lowercase())
    });

    Ok(installed_mods)
}

/// Extrahiert Mod-Name, Version und mögliche Mod-ID aus dem Dateinamen
fn extract_mod_info(clean_name: &str) -> (Option<String>, Option<String>, Option<String>) {
    // Bekannte Muster:
    // sodium-fabric-0.5.8+mc1.20.4
    // iris-mc1.20.4-1.6.17
    // fabric-api-0.92.0+1.20.4

    // Versuche "+mc" oder "-mc" als Trenner zu finden
    if let Some(mc_idx) = clean_name.find("+mc").or_else(|| clean_name.find("-mc")) {
        let before_mc = &clean_name[..mc_idx];

        // Finde letzte Version vor +mc/-mc
        if let Some(ver_idx) = before_mc.rfind('-') {
            let name_part = &before_mc[..ver_idx];
            let version_part = &before_mc[ver_idx + 1..];

            // Prüfe ob version_part mit Zahl beginnt
            if version_part.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                let mod_id = name_part.split('-').next().map(|s| s.to_lowercase());
                return (
                    Some(name_part.replace('-', " ").replace('_', " ")),
                    Some(format!("{}{}", version_part, &clean_name[mc_idx..])),
                    mod_id,
                );
            }
        }
    }

    // Fallback: Einfaches Muster name-version
    if let Some(idx) = clean_name.rfind('-') {
        let potential_version = &clean_name[idx + 1..];
        if potential_version.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            let name_part = &clean_name[..idx];
            let mod_id = name_part.split('-').next().map(|s| s.to_lowercase());
            return (
                Some(name_part.replace('-', " ").replace('_', " ")),
                Some(potential_version.to_string()),
                mod_id,
            );
        }
    }

    // Kein Muster gefunden
    let mod_id = clean_name.split('-').next().map(|s| s.to_lowercase());
    (Some(clean_name.replace('-', " ").replace('_', " ")), None, mod_id)
}

#[tauri::command]
pub async fn toggle_mod(profile_id: String, filename: String, enable: bool) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let mods_dir = profile.game_dir.join("mods");
    let current_path = mods_dir.join(&filename);

    if !current_path.exists() {
        return Err(format!("Mod-Datei nicht gefunden: {}", filename));
    }

    let new_filename = if enable {
        // Aktivieren: .jar.disabled -> .jar
        filename.trim_end_matches(".disabled").to_string()
    } else {
        // Deaktivieren: .jar -> .jar.disabled
        if filename.ends_with(".disabled") {
            filename.clone()
        } else {
            format!("{}.disabled", filename)
        }
    };

    let new_path = mods_dir.join(&new_filename);

    if current_path != new_path {
        std::fs::rename(&current_path, &new_path).map_err(|e| e.to_string())?;
        tracing::info!("Mod toggled: {} -> {}", filename, new_filename);
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_mod(profile_id: String, filename: String) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let mod_path = profile.game_dir.join("mods").join(&filename);

    if !mod_path.exists() {
        return Err(format!("Mod-Datei nicht gefunden: {}", filename));
    }

    std::fs::remove_file(&mod_path).map_err(|e| e.to_string())?;
    tracing::info!("Mod deleted: {}", filename);

    Ok(())
}

#[tauri::command]
pub async fn bulk_toggle_mods(profile_id: String, filenames: Vec<String>, enable: bool) -> Result<(), String> {
    for filename in filenames {
        toggle_mod(profile_id.clone(), filename, enable).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn bulk_delete_mods(profile_id: String, filenames: Vec<String>) -> Result<(), String> {
    for filename in filenames {
        delete_mod(profile_id.clone(), filename).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn check_mod_updates(profile_id: String, _mc_version: String, _loader: String) -> Result<Vec<ModUpdateInfo>, String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let _profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let mods = get_installed_mods(profile_id.clone()).await?;
    let mut updates = Vec::new();

    // Für jede installierte Mod, versuche Update zu finden
    for mod_info in mods {
        if let Some(mod_id) = &mod_info.mod_id {
            // Versuche Mod auf Modrinth zu finden
            if let Ok(modrinth_info) = search_modrinth_by_name(mod_id).await {
                if let Some(latest) = modrinth_info {
                    let has_update = mod_info.version.as_ref()
                        .map(|v| v != &latest.version)
                        .unwrap_or(false);

                    if has_update {
                        updates.push(ModUpdateInfo {
                            filename: mod_info.filename.clone(),
                            current_version: mod_info.version.clone(),
                            latest_version: Some(latest.version),
                            mod_id: latest.mod_id,
                            icon_url: latest.icon_url,
                        });
                    }
                }
            }
        }
    }

    Ok(updates)
}

#[derive(serde::Serialize)]
pub struct ModUpdateInfo {
    pub filename: String,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub mod_id: String,
    pub icon_url: Option<String>,
}

struct ModrinthSearchResult {
    mod_id: String,
    version: String,
    icon_url: Option<String>,
}

async fn search_modrinth_by_name(name: &str) -> Result<Option<ModrinthSearchResult>, String> {
    // Einfache Modrinth-Suche
    let url = format!(
        "https://api.modrinth.com/v2/search?query={}&limit=1",
        urlencoding::encode(name)
    );

    let client = reqwest::Client::new();
    let response = client.get(&url)
        .header("User-Agent", "Lion-Launcher/1.0")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Ok(None);
    }

    #[derive(serde::Deserialize)]
    struct SearchResponse {
        hits: Vec<SearchHit>,
    }

    #[derive(serde::Deserialize)]
    struct SearchHit {
        project_id: String,
        latest_version: Option<String>,
        icon_url: Option<String>,
    }

    let result: SearchResponse = response.json().await.map_err(|e| e.to_string())?;

    if let Some(hit) = result.hits.first() {
        Ok(Some(ModrinthSearchResult {
            mod_id: hit.project_id.clone(),
            version: hit.latest_version.clone().unwrap_or_default(),
            icon_url: hit.icon_url.clone(),
        }))
    } else {
        Ok(None)
    }
}

// ==================== RESOURCE PACKS ====================

#[derive(serde::Serialize)]
pub struct InstalledResourcePack {
    pub name: String,
    pub icon_path: Option<String>,
    pub is_folder: bool,
    pub size: u64,
}

#[tauri::command]
pub async fn get_installed_resourcepacks(profile_id: String) -> Result<Vec<InstalledResourcePack>, String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let rp_dir = profile.game_dir.join("resourcepacks");

    if !rp_dir.exists() {
        return Ok(Vec::new());
    }

    let mut packs = Vec::new();

    let entries = std::fs::read_dir(&rp_dir).map_err(|e| e.to_string())?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let is_folder = path.is_dir();
        let size = if !is_folder {
            std::fs::metadata(&path)
                .ok()
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            0
        };

        // Suche nach pack.png Icon
        let icon_path = if is_folder {
            let icon = path.join("pack.png");
            if icon.exists() {
                Some(icon.to_string_lossy().to_string())
            } else {
                None
            }
        } else if name.ends_with(".zip") {
            // Für ZIP-Dateien könnten wir das Icon extrahieren, aber das ist aufwendig
            // Verwende Placeholder
            None
        } else {
            None
        };

        packs.push(InstalledResourcePack {
            name,
            icon_path,
            is_folder,
            size,
        });
    }

    // Sortiere alphabetisch
    packs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(packs)
}

// ==================== SHADER PACKS ====================

#[tauri::command]
pub async fn get_installed_shaderpacks(profile_id: String) -> Result<Vec<InstalledResourcePack>, String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let shader_dir = profile.game_dir.join("shaderpacks");

    if !shader_dir.exists() {
        return Ok(Vec::new());
    }

    let mut packs = Vec::new();

    let entries = std::fs::read_dir(&shader_dir).map_err(|e| e.to_string())?;

    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let is_folder = path.is_dir();
        let size = if !is_folder {
            std::fs::metadata(&path)
                .ok()
                .map(|m| m.len())
                .unwrap_or(0)
        } else {
            0
        };

        packs.push(InstalledResourcePack {
            name,
            icon_path: None,
            is_folder,
            size,
        });
    }

    packs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(packs)
}

// ==================== SETTINGS SYNC ====================

/// Synchronisiert die Minecraft-Einstellungen (options.txt) zwischen Profilen
/// und einer globalen shared_options.txt

#[tauri::command]
pub async fn sync_settings_to_profile(profile_id: String) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;
    use crate::config::defaults::shared_settings_file;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    if !profile.settings_sync {
        return Ok(()); // Sync ist für dieses Profil deaktiviert
    }

    let shared_file = shared_settings_file();
    let profile_options = profile.game_dir.join("options.txt");

    // Wenn shared_options.txt existiert, merge sie ins Profil
    if shared_file.exists() {
        let shared_content = tokio::fs::read_to_string(&shared_file)
            .await
            .map_err(|e| format!("Konnte shared_options.txt nicht lesen: {}", e))?;

        // Stelle sicher, dass das Verzeichnis existiert
        if let Some(parent) = profile_options.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }

        // Wenn Profil bereits options.txt hat, merge
        let final_content = if profile_options.exists() {
            let existing_content = tokio::fs::read_to_string(&profile_options)
                .await
                .map_err(|e| format!("Konnte existierende options.txt nicht lesen: {}", e))?;

            // Merge: Existing bleibt Basis, shared wird darüber gelegt (aber nicht Blacklist)
            merge_options_content(&existing_content, &shared_content)
        } else {
            // Keine existierende options.txt - einfach shared nehmen
            shared_content
        };

        tokio::fs::write(&profile_options, &final_content)
            .await
            .map_err(|e| format!("Konnte options.txt nicht schreiben: {}", e))?;

        tracing::info!("Settings synced to profile: {} (merged with existing)", profile_id);
    }

    Ok(())
}

#[tauri::command]
pub async fn sync_settings_from_profile(_profile_id: String) -> Result<(), String> {
    // Rufe die automatische Sync-Funktion auf
    auto_sync_all_settings().await
}

/// Automatische Settings-Synchronisation:
/// Sammelt alle options.txt von allen Profilen, sortiert nach Änderungszeit,
/// und merged sie zusammen. Die neueste hat Vorrang (außer Blacklist-Keys).
/// Dann werden alle Profile mit Sync aktualisiert.
pub async fn auto_sync_all_settings() -> Result<(), String> {
    use crate::core::profiles::ProfileManager;
    use crate::config::defaults::shared_settings_file;
    use std::time::SystemTime;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    // Sammle alle options.txt Pfade mit ihrer Änderungszeit
    let mut options_files: Vec<(SystemTime, std::path::PathBuf, String)> = Vec::new();

    for profile in &profiles.profiles {
        // Nur Profile mit aktiviertem Sync
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
                if let Ok(created) = metadata.created() {
                    time = time.max(created);
                }

                options_files.push((time, options_path, profile.id.clone()));
            }
        }
    }

    if options_files.is_empty() {
        tracing::info!("No options.txt files found for sync");
        return Ok(());
    }

    // Sortiere nach Zeit (älteste zuerst, damit neueste überschreibt)
    options_files.sort_by_key(|(time, _, _)| *time);

    tracing::info!("Found {} options.txt files for sync", options_files.len());

    // Starte mit leerer HashMap
    let mut combined_values: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    // Lese shared_options.txt als Basis (falls vorhanden)
    let shared_file = shared_settings_file();
    if shared_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&shared_file) {
            for (key, value) in parse_options_txt(&content) {
                combined_values.insert(key, value);
            }
        }
    }

    // Merge alle options.txt (sortiert nach Zeit, neueste zuletzt = überschreibt)
    for (_, path, _profile_id) in &options_files {
        if let Ok(content) = std::fs::read_to_string(path) {
            for (key, value) in parse_options_txt(&content) {
                // Blacklist-Keys werden nur hinzugefügt wenn sie noch nicht existieren
                if !is_blacklisted_key(&key) {
                    combined_values.insert(key, value);
                } else if !combined_values.contains_key(&key) {
                    combined_values.insert(key, value);
                }
            }
        }
    }

    // Erstelle den kombinierten options.txt String
    let combined_content = create_options_txt_string(&combined_values);

    // Speichere in shared_options.txt
    if let Some(parent) = shared_file.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(&shared_file, &combined_content)
        .await
        .map_err(|e| format!("Konnte shared_options.txt nicht schreiben: {}", e))?;

    tracing::info!("Created combined shared_options.txt with {} settings", combined_values.len());

    // Jetzt alle Profile mit Sync aktualisieren
    let mut synced_count = 0;
    for profile in &profiles.profiles {
        if !profile.settings_sync {
            continue;
        }

        let profile_options = profile.game_dir.join("options.txt");

        // Merge: Behalte profil-spezifische Keys (Blacklist)
        let final_content = if profile_options.exists() {
            if let Ok(existing) = std::fs::read_to_string(&profile_options) {
                merge_options_content(&existing, &combined_content)
            } else {
                combined_content.clone()
            }
        } else {
            // Erstelle Verzeichnis falls nötig
            if let Some(parent) = profile_options.parent() {
                tokio::fs::create_dir_all(parent).await.ok();
            }
            combined_content.clone()
        };

        if let Err(e) = tokio::fs::write(&profile_options, &final_content).await {
            tracing::error!("Failed to sync to profile {}: {}", profile.name, e);
        } else {
            synced_count += 1;
        }
    }

    tracing::info!("Auto-synced settings to {} profiles", synced_count);
    Ok(())
}

/// Parst eine options.txt in Key-Value Paare
fn parse_options_txt(content: &str) -> Vec<(String, String)> {
    let mut values = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some((key, value)) = line.split_once(':') {
            values.push((key.to_string(), value.to_string()));
        }
    }
    values
}

/// Erstellt einen options.txt String aus einer HashMap
fn create_options_txt_string(values: &std::collections::HashMap<String, String>) -> String {
    let mut lines: Vec<String> = values
        .iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect();
    lines.sort(); // Sortiere für konsistente Reihenfolge
    lines.join("\n")
}

/// Prüft ob ein Key in der Blacklist ist (nicht synchronisiert werden soll)
fn is_blacklisted_key(key: &str) -> bool {
    // Nur version bleibt profil-spezifisch
    matches!(key, "version")
}

#[tauri::command]
pub async fn toggle_settings_sync(profile_id: String, enabled: bool) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let mut profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    if let Some(profile) = profiles.get_profile_mut(&profile_id) {
        profile.settings_sync = enabled;
        profile_manager.save_profiles(&profiles).await.map_err(|e| e.to_string())?;

        // Wenn aktiviert, synchronisiere sofort
        if enabled {
            // Kopiere shared settings ins Profil (wenn vorhanden)
            sync_settings_to_profile(profile_id).await?;
        }

        tracing::info!("Settings sync {} for profile", if enabled { "enabled" } else { "disabled" });
    } else {
        return Err("Profile not found".to_string());
    }

    Ok(())
}

#[tauri::command]
pub async fn get_settings_sync_status(profile_id: String) -> Result<bool, String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    Ok(profile.settings_sync)
}


/// Interne Merge-Funktion
fn merge_options_content(existing: &str, new_content: &str) -> String {
    use std::collections::HashMap;

    // Keys die NICHT synchronisiert werden sollen (version-spezifisch)
    let blacklist: Vec<&str> = vec![
        "version",           // Minecraft version number - bleibt profil-spezifisch
    ];

    // Parse beide in key-value Maps
    let mut settings: HashMap<String, String> = HashMap::new();

    // Parse existierende Settings und behalte sie
    for line in existing.lines() {
        if let Some((key, value)) = parse_option_line(line) {
            settings.insert(key, value);
        }
    }

    // Merge neue Settings (überschreibt existierende, außer Blacklist)
    for line in new_content.lines() {
        if let Some((key, value)) = parse_option_line(line) {
            // Überspringe Keys in der Blacklist
            if !blacklist.contains(&key.as_str()) {
                settings.insert(key, value);
            } else {
                // Wenn Key in Blacklist ist und noch nicht existiert, füge ihn hinzu
                // (für neue Profile)
                if !settings.contains_key(&key) {
                    settings.insert(key, value);
                }
            }
        }
    }

    // Sortiere und schreibe zurück
    let mut lines: Vec<String> = settings
        .into_iter()
        .map(|(k, v)| format!("{}:{}", k, v))
        .collect();
    lines.sort();

    lines.join("\n")
}

fn parse_option_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    if let Some(colon_pos) = line.find(':') {
        let key = line[..colon_pos].to_string();
        let value = line[colon_pos + 1..].to_string();
        Some((key, value))
    } else {
        None
    }
}

