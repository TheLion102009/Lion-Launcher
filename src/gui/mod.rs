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

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let logs_dir = profile.game_dir.join("logs");

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
                return Err("Keine Crash-Reports gefunden".to_string());
            }
        }
        _ => return Err("Unbekannter Log-Typ".to_string()),
    };

    if !log_file.exists() {
        return Err(format!("Log-Datei nicht gefunden: {:?}", log_file));
    }

    // Lese nur die letzten 500KB um Performance zu erhalten
    let content = tokio::fs::read_to_string(&log_file)
        .await
        .map_err(|e| e.to_string())?;

    // Nur letzte 10000 Zeilen
    let lines: Vec<&str> = content.lines().collect();
    let start = if lines.len() > 10000 { lines.len() - 10000 } else { 0 };
    let truncated: String = lines[start..].join("\n");

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

