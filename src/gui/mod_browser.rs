use crate::core::mods::ModManager;
use crate::types::mod_info::{ModInfo, ModVersion, ModSearchQuery, SortOption};

#[tauri::command]
pub async fn search_mods(
    query: String,
    game_version: Option<String>,
    loader: Option<String>,
    sort_by: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<Vec<ModInfo>, String> {
    let search_query = ModSearchQuery {
        query,
        game_version,
        loader,
        categories: Vec::new(),
        offset: offset.unwrap_or(0),
        limit: limit.unwrap_or(20),
        sort_by: match sort_by.as_deref() {
            Some("downloads") => SortOption::Downloads,
            Some("updated") => SortOption::Updated,
            Some("newest") => SortOption::Newest,
            _ => SortOption::Relevance,
        },
    };

    let manager = ModManager::new(None).map_err(|e| e.to_string())?;
    manager.search_mods(&search_query, true, false).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_mod_versions(mod_id: String, source: String) -> Result<Vec<ModVersion>, String> {
    let manager = ModManager::new(None).map_err(|e| e.to_string())?;

    let mod_source = match source.as_str() {
        "modrinth" => crate::types::mod_info::ModSource::Modrinth,
        "curseforge" => crate::types::mod_info::ModSource::CurseForge,
        _ => return Err("Invalid source".to_string()),
    };

    let mod_info = ModInfo {
        id: mod_id,
        source: mod_source,
        slug: String::new(),
        name: String::new(),
        description: String::new(),
        icon_url: None,
        author: String::new(),
        downloads: 0,
        categories: Vec::new(),
        versions: Vec::new(),
        game_versions: Vec::new(),
        loaders: Vec::new(),
        project_url: String::new(),
        updated_at: String::new(),
    };

    manager.get_mod_versions(&mod_info).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn install_mod(
    profile_id: String,
    mod_id: String,
    version_id: Option<String>,  // Optional - wenn None, finden wir die passende Version
    source: String,
) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let mut profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile_mut(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let mods_dir = profile.game_dir.join("mods");

    // Stelle sicher dass der mods-Ordner existiert
    tokio::fs::create_dir_all(&mods_dir).await.map_err(|e| e.to_string())?;

    let mc_version = profile.minecraft_version.clone();
    let loader = profile.loader.loader.to_string().to_lowercase();

    tracing::info!("Installing mod {} for {} {} to {:?}", mod_id, mc_version, loader, mods_dir);

    let mod_source = match source.as_str() {
        "modrinth" => crate::types::mod_info::ModSource::Modrinth,
        "curseforge" => crate::types::mod_info::ModSource::CurseForge,
        _ => return Err("Invalid source".to_string()),
    };

    let manager = ModManager::new(None).map_err(|e| e.to_string())?;

    // Hole Icon-URL und Name von Modrinth (für Metadaten)
    let (icon_url, mod_name) = if mod_source == crate::types::mod_info::ModSource::Modrinth {
        let url = format!("https://api.modrinth.com/v2/project/{}", mod_id);
        match reqwest::get(&url).await {
            Ok(response) => {
                if let Ok(json) = response.json::<serde_json::Value>().await {
                    let icon = json.get("icon_url").and_then(|v| v.as_str()).map(|s| s.to_string());
                    let name = json.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
                    (icon, name)
                } else {
                    (None, None)
                }
            }
            Err(_) => (None, None)
        }
    } else {
        (None, None)
    };

    // Hole alle Versionen der Mod
    let all_versions = manager.get_mod_versions_raw(&mod_id, mod_source.clone())
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("Found {} versions for mod {}", all_versions.len(), mod_id);

    // Finde die passende Version für unser Profil (MC-Version + Loader)
    let matching_version = if let Some(vid) = version_id {
        // Spezifische Version wurde angegeben
        all_versions.iter().find(|v| v.id == vid)
    } else {
        // Finde passende Version für MC-Version und Loader
        all_versions.iter().find(|v| {
            let has_mc_version = v.game_versions.iter().any(|gv| gv == &mc_version);
            let has_loader = v.loaders.iter().any(|l| l.to_lowercase() == loader);

            if has_mc_version && has_loader {
                tracing::info!("Found matching version: {} (mc: {:?}, loaders: {:?})",
                    v.version_number, v.game_versions, v.loaders);
                true
            } else {
                false
            }
        })
    };

    let version = matching_version
        .ok_or_else(|| format!(
            "Keine passende Mod-Version gefunden für Minecraft {} mit {}. \
             Diese Mod unterstützt möglicherweise nicht deine Kombination.",
            mc_version, loader
        ))?;

    tracing::info!("Installing version: {} ({})", version.version_number, version.id);

    manager.download_mod(version, &mods_dir)
        .await
        .map_err(|e| e.to_string())?;

    // Speichere Metadaten neben der JAR-Datei
    let primary_file = version.files.iter().find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| "No files in version".to_string())?;

    let jar_path = mods_dir.join(&primary_file.filename);
    let meta_path = jar_path.with_extension("jar.meta.json");

    let metadata = serde_json::json!({
        "mod_id": mod_id,
        "mod_name": mod_name,
        "icon_url": icon_url,
        "version": version.version_number,
        "source": source,
    });

    if let Err(e) = tokio::fs::write(&meta_path, metadata.to_string()).await {
        tracing::warn!("Failed to write metadata file: {}", e);
        // Nicht kritisch, fahre fort
    }

    tracing::info!("Mod {} installed successfully to {:?}", mod_id, mods_dir);

    profile.add_mod(mod_id.clone());
    profile_manager.save_profiles(&profiles).await.map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn uninstall_mod(
    profile_id: String,
    mod_id: String,
    mod_filename: String,
) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let mut profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile_mut(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let mods_dir = profile.game_dir.join("mods");

    let manager = ModManager::new(None).map_err(|e| e.to_string())?;
    manager.uninstall_mod(&mod_filename, &mods_dir)
        .await
        .map_err(|e| e.to_string())?;

    profile.remove_mod(&mod_id);
    profile_manager.save_profiles(&profiles).await.map_err(|e| e.to_string())?;

    Ok(())
}
