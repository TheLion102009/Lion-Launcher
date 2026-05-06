use serde::Deserialize;
use crate::core::mods::ModManager;
use crate::types::mod_info::{ModInfo, ModVersion, ModSearchQuery, SortOption};

// Re-export ModrinthCategory für Frontend
pub use crate::api::modrinth::ModrinthCategory;

// Import für get_modrinth_categories
use crate::api::modrinth::ModrinthClient;

// ==================== CATEGORIES ====================

#[tauri::command]
pub async fn get_modrinth_categories() -> Result<Vec<ModrinthCategory>, String> {
    let client = ModrinthClient::new().map_err(|e| e.to_string())?;
    let categories = client.get_categories().await.map_err(|e| e.to_string())?;
    Ok(categories)
}

// ==================== MODS ====================

#[tauri::command]
pub async fn search_mods(
    query: String,
    game_version: Option<String>,
    loader: Option<String>,
    categories: Option<Vec<String>>,
    sort_by: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<Vec<ModInfo>, String> {
    let search_query = ModSearchQuery {
        query,
        game_version,
        loader,
        categories: categories.unwrap_or_default(),
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

    manager.get_mod_versions_raw(&mod_id, mod_source).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_mod_info(mod_id: String, source: String) -> Result<ModInfo, String> {
    let client = ModrinthClient::new().map_err(|e| e.to_string())?;

    match source.as_str() {
        "modrinth" => {
            client.get_mod(&mod_id).await.map_err(|e| e.to_string())
        }
        "curseforge" => {
            Err("CurseForge not yet implemented".to_string())
        }
        _ => Err("Invalid source".to_string()),
    }
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
    let all_versions = manager.get_mod_versions_raw(&mod_id, mod_source)
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("Found {} versions for mod {}", all_versions.len(), mod_id);

    // Finde die passende Version für unser Profil (MC-Version + Loader)
    let matching_version = if let Some(vid) = version_id {
        // Spezifische Version wurde angegeben
        all_versions.iter().find(|v| v.id == vid)
    } else {
        // Finde passende Version für MC-Version und Loader
        let mut found = all_versions.iter().find(|v| {
            let has_mc_version = v.game_versions.iter().any(|gv| gv == &mc_version);
            let has_loader = v.loaders.iter().any(|l| l.to_lowercase() == loader);

            if has_mc_version && has_loader {
                tracing::info!("Found matching version: {} (mc: {:?}, loaders: {:?})",
                    v.version_number, v.game_versions, v.loaders);
                true
            } else {
                false
            }
        });

        // Quilt Fallback: Wenn keine Quilt-Version gefunden, versuche Fabric (Quilt ist Fabric-kompatibel)
        if found.is_none() && loader == "quilt" {
            tracing::info!("No Quilt version found, trying Fabric as fallback...");
            found = all_versions.iter().find(|v| {
                let has_mc_version = v.game_versions.iter().any(|gv| gv == &mc_version);
                let has_fabric = v.loaders.iter().any(|l| l.to_lowercase() == "fabric");

                if has_mc_version && has_fabric {
                    tracing::info!("Found Fabric version as fallback: {} (mc: {:?}, loaders: {:?})",
                        v.version_number, v.game_versions, v.loaders);
                    true
                } else {
                    false
                }
            });
        }

        found
    };

    let version = matching_version
        .ok_or_else(|| format!(
            "Keine passende Mod-Version gefunden für Minecraft {} mit {}. \
             Diese Mod unterstützt möglicherweise nicht deine Kombination.",
            mc_version, loader
        ))?;

    // Warnung wenn die gewählte Version nicht exakt zur MC-Version passt
    if !version.game_versions.iter().any(|gv| gv == &mc_version) {
        let supported_versions = version.game_versions.join(", ");
        let mod_display_name = mod_name.as_deref().unwrap_or(mod_id.as_str());
        tracing::warn!(
            "⚠️  VERSION MISMATCH: Mod '{}' v{} ist für {} gedacht, aber dein Profil verwendet {}! \
             Dies kann zu Crashes führen.",
            mod_display_name, version.version_number, supported_versions, mc_version
        );
    }

    tracing::info!("Installing version: {} ({})", version.version_number, version.id);

    // Prüfe ob bereits eine Version dieser Mod installiert ist und entferne sie
    if let Ok(mut entries) = tokio::fs::read_dir(&mods_dir).await {
        let modinfos_dir = profile.game_dir.join("modinfos");
        
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("jar") {
                // Prüfe ob dies die gleiche Mod ist (über Metadaten)
                let filename = path.file_name().unwrap().to_str().unwrap();
                let meta_filename = filename.replace(".jar", ".json");
                let meta_path = modinfos_dir.join(&meta_filename);
                
                if let Ok(meta_content) = tokio::fs::read_to_string(&meta_path).await {
                    if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&meta_content) {
                        if let Some(existing_mod_id) = meta.get("mod_id").and_then(|v| v.as_str()) {
                            if existing_mod_id == mod_id {
                                // Gleiche Mod gefunden - lösche alte Version
                                tracing::info!("🗑️  Removing old version: {}", filename);
                                if let Err(e) = tokio::fs::remove_file(&path).await {
                                    tracing::warn!("Failed to remove old mod file: {}", e);
                                }
                                // Lösche auch alte Metadaten
                                let _ = tokio::fs::remove_file(&meta_path).await;
                            }
                        }
                    }
                }
            }
        }
    }

    manager.download_mod(version, &mods_dir)
        .await
        .map_err(|e| e.to_string())?;

    // Speichere Metadaten in separatem modinfos/ Ordner
    let primary_file = version.files.iter().find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| "No files in version".to_string())?;

    // Erstelle modinfos/ Ordner im Profil-Verzeichnis
    let modinfos_dir = profile.game_dir.join("modinfos");
    tokio::fs::create_dir_all(&modinfos_dir).await.map_err(|e| e.to_string())?;

    // Speichere Metadaten mit gleichem Dateinamen aber in modinfos/
    let jar_filename = &primary_file.filename;
    let meta_filename = jar_filename.replace(".jar", ".json");
    let meta_path = modinfos_dir.join(&meta_filename);

    let metadata = serde_json::json!({
        "mod_id": mod_id,
        "mod_name": mod_name,
        "icon_url": icon_url,
        "version": version.version_number,
        "source": source,
        "filename": jar_filename,
    });

    if let Err(e) = tokio::fs::write(&meta_path, serde_json::to_string_pretty(&metadata).unwrap()).await {
        tracing::warn!("Failed to write metadata file to {:?}: {}", meta_path, e);
        // Nicht kritisch, fahre fort
    } else {
        tracing::info!("✅ Saved metadata to {:?}", meta_path);
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

// ==================== RESOURCE PACKS ====================

#[tauri::command]
pub async fn search_resourcepacks(
    query: String,
    game_version: Option<String>,
    categories: Option<Vec<String>>,
    sort_by: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<Vec<ModInfo>, String> {
    // Modrinth API: Resource Packs haben project_type=resourcepack
    let client = reqwest::Client::new();
    let url = "https://api.modrinth.com/v2/search";

    let sort = match sort_by.as_deref() {
        Some("downloads") => "downloads",
        Some("updated") => "updated",
        Some("newest") => "newest",
        _ => "relevance",
    };

    let mut facets = vec![r#"["project_type:resourcepack"]"#.to_string()];

    if let Some(version) = game_version {
        facets.push(format!(r#"["versions:{}"]"#, version));
    }

    // Categories hinzufügen
    if let Some(cats) = categories {
        for cat in cats {
            if !cat.is_empty() {
                facets.push(format!(r#"["categories:{}"]"#, cat));
            }
        }
    }

    let facets_str = format!("[{}]", facets.join(","));

    let response = client
        .get(url)
        .query(&[
            ("query", query.as_str()),
            ("facets", &facets_str),
            ("index", sort),
            ("offset", &offset.unwrap_or(0).to_string()),
            ("limit", &limit.unwrap_or(20).to_string()),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    #[derive(Deserialize)]
    struct SearchResponse {
        hits: Vec<SearchHit>,
    }

    #[derive(Deserialize)]
    struct SearchHit {
        project_id: String,
        slug: String,
        title: String,
        description: String,
        icon_url: Option<String>,
        author: String,
        downloads: u64,
        categories: Vec<String>,
        versions: Vec<String>,
        date_modified: String,
    }

    let result: SearchResponse = response.json().await.map_err(|e| e.to_string())?;

    Ok(result.hits.into_iter().map(|hit| {
        let slug = hit.slug.clone();
        ModInfo {
            id: hit.project_id,
            slug: hit.slug,
            name: hit.title,
            description: hit.description,
            body: None,
            icon_url: hit.icon_url,
            author: hit.author,
            downloads: hit.downloads,
            followers: None,
            categories: hit.categories,
            source: crate::types::mod_info::ModSource::Modrinth,
            versions: hit.versions,
            game_versions: vec![],
            loaders: vec![],
            project_url: format!("https://modrinth.com/resourcepack/{}", slug),
            updated_at: hit.date_modified,
            client_side: None,
            server_side: None,
            source_url: None,
            issues_url: None,
            wiki_url: None,
            discord_url: None,
            gallery: vec![],
        }
    }).collect())
}

#[tauri::command]
pub async fn install_resourcepack(
    profile_id: String,
    pack_id: String,
    version_id: Option<String>,
) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let rp_dir = profile.game_dir.join("resourcepacks");
    tokio::fs::create_dir_all(&rp_dir).await.map_err(|e| e.to_string())?;

    let mc_version = profile.minecraft_version.clone();

    tracing::info!("Installing resource pack {} for {} to {:?}", pack_id, mc_version, rp_dir);

    // Hole Versionen von Modrinth
    let client = reqwest::Client::new();
    let url = format!("https://api.modrinth.com/v2/project/{}/version", pack_id);

    let response = client.get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    #[derive(Deserialize)]
    struct Version {
        id: String,
        #[allow(dead_code)]
        name: String,
        version_number: String,
        game_versions: Vec<String>,
        files: Vec<File>,
    }

    #[derive(Deserialize)]
    struct File {
        url: String,
        filename: String,
        primary: bool,
        #[allow(dead_code)]
        size: u64,
    }

    let versions: Vec<Version> = response.json().await.map_err(|e| e.to_string())?;

    // Finde passende Version
    let version = if let Some(vid) = version_id {
        versions.iter().find(|v| v.id == vid)
    } else {
        versions.iter().find(|v| v.game_versions.iter().any(|gv| gv == &mc_version))
    }.ok_or_else(|| format!("Keine passende Resource Pack Version für MC {} gefunden", mc_version))?;

    tracing::info!("Installing version: {} ({})", version.version_number, version.id);

    // Lade Datei herunter
    let file = version.files.iter().find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| "No files in version".to_string())?;

    let target_path = rp_dir.join(&file.filename);

    tracing::info!("Downloading from {} to {:?}", file.url, target_path);

    let response = client.get(&file.url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    tokio::fs::write(&target_path, &bytes).await.map_err(|e| e.to_string())?;

    tracing::info!("Resource pack installed successfully to {:?}", target_path);

    // META-INF Entfernung deaktiviert - kann Probleme mit eingebetteten Assets verursachen
    // if file.filename.ends_with(".zip") {
    //     if let Err(e) = remove_meta_inf_from_zip(&target_path).await {
    //         tracing::warn!("Failed to remove META-INF from {}: {}", file.filename, e);
    //     }
    // }

    Ok(())
}

// ==================== SHADER PACKS ====================

#[tauri::command]
pub async fn search_shaderpacks(
    query: String,
    game_version: Option<String>,
    categories: Option<Vec<String>>,
    sort_by: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<Vec<ModInfo>, String> {
    let client = reqwest::Client::new();
    let url = "https://api.modrinth.com/v2/search";

    let sort = match sort_by.as_deref() {
        Some("downloads") => "downloads",
        Some("updated") => "updated",
        Some("newest") => "newest",
        _ => "relevance",
    };

    let mut facets = vec![r#"["project_type:shader"]"#.to_string()];

    if let Some(version) = game_version {
        facets.push(format!(r#"["versions:{}"]"#, version));
    }

    // Categories hinzufügen
    if let Some(cats) = categories {
        for cat in cats {
            if !cat.is_empty() {
                facets.push(format!(r#"["categories:{}"]"#, cat));
            }
        }
    }

    let facets_str = format!("[{}]", facets.join(","));

    let response = client
        .get(url)
        .query(&[
            ("query", query.as_str()),
            ("facets", &facets_str),
            ("index", sort),
            ("offset", &offset.unwrap_or(0).to_string()),
            ("limit", &limit.unwrap_or(20).to_string()),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    #[derive(Deserialize)]
    struct SearchResponse {
        hits: Vec<SearchHit>,
    }

    #[derive(Deserialize)]
    struct SearchHit {
        project_id: String,
        slug: String,
        title: String,
        description: String,
        icon_url: Option<String>,
        author: String,
        downloads: u64,
        categories: Vec<String>,
        versions: Vec<String>,
        date_modified: String,
    }

    let result: SearchResponse = response.json().await.map_err(|e| e.to_string())?;

    Ok(result.hits.into_iter().map(|hit| {
        let slug = hit.slug.clone();
        ModInfo {
            id: hit.project_id,
            slug: hit.slug,
            name: hit.title,
            description: hit.description,
            body: None,
            icon_url: hit.icon_url,
            author: hit.author,
            downloads: hit.downloads,
            followers: None,
            categories: hit.categories,
            source: crate::types::mod_info::ModSource::Modrinth,
            versions: hit.versions,
            game_versions: vec![],
            loaders: vec![],
            project_url: format!("https://modrinth.com/shader/{}", slug),
            updated_at: hit.date_modified,
            client_side: None,
            server_side: None,
            source_url: None,
            issues_url: None,
            wiki_url: None,
            discord_url: None,
            gallery: vec![],
        }
    }).collect())
}

#[tauri::command]
pub async fn install_shaderpack(
    profile_id: String,
    pack_id: String,
    version_id: Option<String>,
) -> Result<(), String> {
    use crate::core::profiles::ProfileManager;

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = profile_manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = profiles.get_profile(&profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;

    let shader_dir = profile.game_dir.join("shaderpacks");
    tokio::fs::create_dir_all(&shader_dir).await.map_err(|e| e.to_string())?;

    let mc_version = profile.minecraft_version.clone();

    tracing::info!("Installing shader pack {} for {} to {:?}", pack_id, mc_version, shader_dir);

    let client = reqwest::Client::new();
    let url = format!("https://api.modrinth.com/v2/project/{}/version", pack_id);

    let response = client.get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    #[derive(Deserialize)]
    struct Version {
        id: String,
        #[allow(dead_code)]
        version_number: String,
        game_versions: Vec<String>,
        files: Vec<File>,
    }

    #[derive(Deserialize)]
    struct File {
        url: String,
        filename: String,
        primary: bool,
    }

    let versions: Vec<Version> = response.json().await.map_err(|e| e.to_string())?;

    let version = if let Some(vid) = version_id {
        versions.iter().find(|v| v.id == vid)
    } else {
        versions.iter().find(|v| v.game_versions.iter().any(|gv| gv == &mc_version))
            .or_else(|| versions.first()) // Shader sind oft version-unabhängig
    }.ok_or_else(|| "Keine passende Shader Version gefunden".to_string())?;

    let file = version.files.iter().find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| "No files in version".to_string())?;

    let target_path = shader_dir.join(&file.filename);

    let response = client.get(&file.url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    tokio::fs::write(&target_path, &bytes).await.map_err(|e| e.to_string())?;

    tracing::info!("Shader pack installed successfully to {:?}", target_path);

    // META-INF Entfernung deaktiviert - kann Probleme mit eingebetteten Assets verursachen
    // if file.filename.ends_with(".zip") {
    //     if let Err(e) = remove_meta_inf_from_zip(&target_path).await {
    //         tracing::warn!("Failed to remove META-INF from {}: {}", file.filename, e);
    //     }
    // }

    Ok(())
}

// ==================== MODPACKS ====================

/// Installiert ein Modrinth Modpack (.mrpack Format):
/// 1. Holt Projekt-Icon + Versionen von Modrinth
/// 2. Lädt .mrpack herunter
/// 3. Liest modrinth.index.json
/// 4. Erstellt neues Profil mit Modpack-Icon
/// 5. Lädt alle Dateien aus dem Manifest herunter (mods, configs, ...)
/// 6. Kopiert ALLE Overrides (overrides/ + client-overrides/ + server-overrides/)
///    inkl. Unterordner wie config/, saves/, resourcepacks/, options.txt usw.
#[tauri::command]
pub async fn install_modpack(
    pack_id: String,
    pack_name: String,
    version_id: Option<String>,
) -> Result<serde_json::Value, String> {
    use std::io::Read;
    use base64::Engine as _;
    use crate::core::profiles::ProfileManager;
    use crate::types::profile::Profile;
    use crate::types::version::ModLoader;

    tracing::info!("🎮 Installing modpack: {} ({})", pack_name, pack_id);

    let client = reqwest::Client::builder()
        .user_agent("LionLauncher/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    // ── 1a. Projekt-Info holen (für Icon-URL) ────────────────────────────────
    #[derive(serde::Deserialize)]
    struct ProjectInfo {
        icon_url: Option<String>,
    }

    let project_url = format!("https://api.modrinth.com/v2/project/{}", pack_id);
    let icon_data_url: Option<String> = match client.get(&project_url).send().await {
        Ok(resp) => {
            match resp.json::<ProjectInfo>().await {
                Ok(info) => {
                    if let Some(icon_url) = info.icon_url {
                        tracing::info!("🖼️ Fetching modpack icon: {}", icon_url);
                        match client.get(&icon_url).send().await {
                            Ok(img_resp) => {
                                // Content-Type für korrekten Mime-Type
                                let mime = img_resp.headers()
                                    .get("content-type")
                                    .and_then(|v| v.to_str().ok())
                                    .unwrap_or("image/png")
                                    .split(';').next()
                                    .unwrap_or("image/png")
                                    .to_string();
                                match img_resp.bytes().await {
                                    Ok(img_bytes) => {
                                        let b64 = base64::engine::general_purpose::STANDARD.encode(&img_bytes);
                                        Some(format!("data:{};base64,{}", mime, b64))
                                    }
                                    Err(e) => { tracing::warn!("Icon bytes failed: {}", e); None }
                                }
                            }
                            Err(e) => { tracing::warn!("Icon fetch failed: {}", e); None }
                        }
                    } else { None }
                }
                Err(e) => { tracing::warn!("Project info parse failed: {}", e); None }
            }
        }
        Err(e) => { tracing::warn!("Project info fetch failed: {}", e); None }
    };

    // ── 1b. Versionen holen ──────────────────────────────────────────────────
    #[derive(serde::Deserialize)]
    struct MrpackFile {
        url: String,
        filename: String,
        primary: bool,
        #[allow(dead_code)]
        size: u64,
    }

    #[derive(serde::Deserialize)]
    struct MrpackVersion {
        id: String,
        #[allow(dead_code)]
        name: String,
        #[allow(dead_code)]
        version_number: String,
        #[allow(dead_code)]
        game_versions: Vec<String>,
        #[allow(dead_code)]
        loaders: Vec<String>,
        files: Vec<MrpackFile>,
    }

    let versions_url = format!("https://api.modrinth.com/v2/project/{}/version", pack_id);
    let versions_resp = client.get(&versions_url).send().await.map_err(|e| e.to_string())?;
    let versions: Vec<MrpackVersion> = versions_resp.json().await.map_err(|e| e.to_string())?;

    let version = if let Some(vid) = version_id {
        versions.iter().find(|v| v.id == vid)
    } else {
        versions.first()
    }.ok_or_else(|| "Keine Modpack-Version gefunden".to_string())?;

    let mrpack_file = version.files.iter().find(|f| f.filename.ends_with(".mrpack") && f.primary)
        .or_else(|| version.files.iter().find(|f| f.filename.ends_with(".mrpack")))
        .or_else(|| version.files.first())
        .ok_or_else(|| "Keine .mrpack Datei in dieser Version gefunden".to_string())?;

    // ── 2. .mrpack herunterladen in temp-Datei ──────────────────────────────
    let temp_dir = std::env::temp_dir().join(format!("lion_modpack_{}", uuid::Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await.map_err(|e| e.to_string())?;
    let mrpack_path = temp_dir.join(&mrpack_file.filename);

    tracing::info!("📥 Downloading mrpack from: {}", mrpack_file.url);

    let resp = client.get(&mrpack_file.url).send().await.map_err(|e| e.to_string())?;
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    tokio::fs::write(&mrpack_path, &bytes).await.map_err(|e| e.to_string())?;

    tracing::info!("✅ mrpack downloaded: {} bytes", bytes.len());

    // ── 3. modrinth.index.json lesen ────────────────────────────────────────
    #[derive(serde::Deserialize)]
    struct IndexFile {
        path: String,
        hashes: IndexHashes,
        #[allow(dead_code)]
        env: Option<serde_json::Value>,
        downloads: Vec<String>,
        #[serde(rename = "fileSize")]
        #[allow(dead_code)]
        file_size: Option<u64>,
    }

    #[derive(serde::Deserialize)]
    struct IndexHashes {
        sha1: Option<String>,
        #[allow(dead_code)]
        sha512: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct ModrinthIndex {
        #[allow(dead_code)]
        name: String,
        #[allow(dead_code)]
        summary: Option<String>,
        files: Vec<IndexFile>,
        dependencies: std::collections::HashMap<String, String>,
    }

    let zip_file = std::fs::File::open(&mrpack_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| e.to_string())?;

    let index_json = {
        let mut index_file = archive.by_name("modrinth.index.json")
            .map_err(|_| "modrinth.index.json nicht im Modpack gefunden".to_string())?;
        let mut content = String::new();
        index_file.read_to_string(&mut content).map_err(|e| e.to_string())?;
        content
    };

    let index: ModrinthIndex = serde_json::from_str(&index_json).map_err(|e| e.to_string())?;

    let mc_version = index.dependencies.get("minecraft")
        .cloned()
        .ok_or_else(|| "Minecraft-Version nicht im Modpack angegeben".to_string())?;

    let (loader, loader_version) = if let Some(v) = index.dependencies.get("fabric-loader") {
        (ModLoader::Fabric, v.clone())
    } else if let Some(v) = index.dependencies.get("neoforge") {
        (ModLoader::NeoForge, v.clone())
    } else if let Some(v) = index.dependencies.get("forge") {
        (ModLoader::Forge, v.clone())
    } else if let Some(v) = index.dependencies.get("quilt-loader") {
        (ModLoader::Quilt, v.clone())
    } else {
        (ModLoader::Vanilla, String::new())
    };

    tracing::info!("Modpack: {} – MC {} {:?} {}", pack_name, mc_version, loader, loader_version);

    // ── 4. Profil erstellen (mit Modpack-Icon) ──────────────────────────────
    let mut profile = Profile::new(pack_name.clone(), mc_version.clone(), loader, loader_version);

    // Modpack-Icon als Profil-Icon setzen (als data-URL in icon_path)
    if let Some(ref data_url) = icon_data_url {
        profile.icon_path = Some(std::path::PathBuf::from(data_url.clone()));
        tracing::info!("✅ Modpack icon set as profile icon");
    }

    let profile_dir = profile.game_dir.clone();
    let profile_id = profile.id.clone();

    let profile_manager = ProfileManager::new().map_err(|e| e.to_string())?;
    profile_manager.create_profile(profile).await.map_err(|e| e.to_string())?;

    // ── 5. Manifest-Dateien herunterladen (Mods + alle anderen Pfade) ────────
    // Das Manifest kann nicht nur mods/ enthalten, sondern auch config/, saves/, etc.
    let mods_dir = profile_dir.join("mods");
    tokio::fs::create_dir_all(&mods_dir).await.map_err(|e| e.to_string())?;

    let total = index.files.len();
    tracing::info!("📦 Downloading {} manifest files...", total);

    for (i, file) in index.files.iter().enumerate() {
        if let Some(download_url) = file.downloads.first() {
            // Normalisiere Pfad (Windows-Backslashes → Forward Slashes)
            let normalized_path = file.path.replace('\\', "/");

            // Ziel: immer relativ zum profile_dir (game directory)
            let target_path = profile_dir.join(&normalized_path);

            // Stelle sicher dass alle Parent-Ordner existieren
            if let Some(parent) = target_path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    tracing::warn!("Could not create dir {:?}: {}", parent, e);
                }
            }

            tracing::info!("[{}/{}] Downloading: {}", i + 1, total, normalized_path);

            let resp = client.get(download_url).send().await;
            match resp {
                Ok(r) => {
                    match r.bytes().await {
                        Ok(file_bytes) => {
                            if let Err(e) = tokio::fs::write(&target_path, &file_bytes).await {
                                tracing::warn!("Failed to write {}: {}", normalized_path, e);
                            } else if let Some(expected_sha1) = &file.hashes.sha1 {
                                use sha1::Digest;
                                let hash = sha1::Sha1::digest(&file_bytes);
                                let actual = hex::encode(hash);
                                if &actual != expected_sha1 {
                                    tracing::warn!("⚠️ SHA1 mismatch for {}", normalized_path);
                                }
                            }
                        }
                        Err(e) => tracing::warn!("Failed to read bytes for {}: {}", normalized_path, e),
                    }
                }
                Err(e) => tracing::warn!("Failed to download {}: {}", normalized_path, e),
            }
        }
    }

    // ── 6. Overrides kopieren (ALLE Typen + ALLE Unterordner) ───────────────
    // overrides/          → alles (config/, mods/, options.txt, ...)
    // client-overrides/   → client-seitige Dateien
    // server-overrides/   → serverseitige Dateien (auch relevant für configs)
    //
    // Alle Pfad-Komponenten bleiben erhalten:
    //   overrides/config/sodium/sodium-options.json → profile_dir/config/sodium/sodium-options.json
    //   overrides/options.txt                       → profile_dir/options.txt
    //   overrides/resourcepacks/MyPack.zip          → profile_dir/resourcepacks/MyPack.zip
    let zip_file2 = std::fs::File::open(&mrpack_path).map_err(|e| e.to_string())?;
    let mut archive2 = zip::ZipArchive::new(zip_file2).map_err(|e| e.to_string())?;

    // Alle drei Override-Typen unterstützen
    let override_prefixes: &[&str] = &["overrides/", "client-overrides/", "server-overrides/"];
    let mut overrides_copied = 0;

    for i in 0..archive2.len() {
        let mut entry = archive2.by_index(i).map_err(|e| e.to_string())?;
        let raw_name = entry.name().to_string();

        // Normalisiere Backslashes
        let entry_name = raw_name.replace('\\', "/");

        // Ordner-Einträge überspringen (werden implizit durch create_dir_all erstellt)
        if entry_name.ends_with('/') {
            continue;
        }

        // Suche passenden Override-Prefix
        let matched_prefix = override_prefixes.iter().find(|&&prefix| entry_name.starts_with(prefix));

        if let Some(prefix) = matched_prefix {
            // Relative Pfadkomponente nach dem Prefix
            let rel = &entry_name[prefix.len()..];

            // Ziel: profile_dir/<rel>
            // Beispiele:
            //   overrides/config/mod.json       → profile_dir/config/mod.json
            //   overrides/options.txt           → profile_dir/options.txt
            //   client-overrides/resourcepacks/ → profile_dir/resourcepacks/
            let target = profile_dir.join(rel);

            // Erstelle alle Parent-Verzeichnisse (inkl. tief verschachtelte config-Ordner)
            if let Some(parent) = target.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    tracing::warn!("Failed to create override dir {:?}: {}", parent, e);
                    continue;
                }
            }

            let mut content = Vec::new();
            if let Err(e) = entry.read_to_end(&mut content) {
                tracing::warn!("Failed to read override entry {}: {}", rel, e);
                continue;
            }

            match std::fs::write(&target, &content) {
                Ok(_) => {
                    tracing::debug!("Override: {} → {:?}", rel, target);
                    overrides_copied += 1;
                }
                Err(e) => tracing::warn!("Override write failed for {}: {}", rel, e),
            }
        }
    }

    tracing::info!("✅ Overrides kopiert: {} Dateien", overrides_copied);

    // ── 7. Temp-Ordner aufräumen ────────────────────────────────────────────
    tokio::fs::remove_dir_all(&temp_dir).await.ok();

    tracing::info!("🎉 Modpack '{}' erfolgreich installiert! Profil-ID: {}", pack_name, profile_id);

    Ok(serde_json::json!({
        "success": true,
        "profile_id": profile_id,
        "profile_name": pack_name,
        "minecraft_version": mc_version,
        "mods_downloaded": total,
        "overrides_copied": overrides_copied,
        "has_icon": icon_data_url.is_some(),
    }))
}

#[tauri::command]
pub async fn search_modpacks(
    query: String,
    game_version: Option<String>,
    loader: Option<String>,
    categories: Option<Vec<String>>,
    sort_by: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<Vec<ModInfo>, String> {
    let client = reqwest::Client::new();
    let url = "https://api.modrinth.com/v2/search";

    let sort = match sort_by.as_deref() {
        Some("downloads") => "downloads",
        Some("updated") => "updated",
        Some("newest") => "newest",
        _ => "relevance",
    };

    let mut facets = vec![r#"["project_type:modpack"]"#.to_string()];

    if let Some(version) = game_version {
        facets.push(format!(r#"["versions:{}"]"#, version));
    }
    
    if let Some(l) = loader {
        facets.push(format!(r#"["categories:{}"]"#, l));
    }

    // Categories hinzufügen
    if let Some(cats) = categories {
        for cat in cats {
            if !cat.is_empty() {
                facets.push(format!(r#"["categories:{}"]"#, cat));
            }
        }
    }

    let facets_str = format!("[{}]", facets.join(","));

    let response = client
        .get(url)
        .query(&[
            ("query", query.as_str()),
            ("facets", &facets_str),
            ("index", sort),
            ("offset", &offset.unwrap_or(0).to_string()),
            ("limit", &limit.unwrap_or(20).to_string()),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;

    #[derive(Deserialize)]
    struct SearchResponse {
        hits: Vec<SearchHit>,
    }

    #[derive(Deserialize)]
    struct SearchHit {
        project_id: String,
        slug: String,
        title: String,
        description: String,
        icon_url: Option<String>,
        author: String,
        downloads: u64,
        categories: Vec<String>,
        versions: Vec<String>,
        date_modified: String,
    }

    let result: SearchResponse = response.json().await.map_err(|e| e.to_string())?;

    Ok(result.hits.into_iter().map(|hit| {
        let slug = hit.slug.clone();
        ModInfo {
            id: hit.project_id,
            slug: hit.slug,
            name: hit.title,
            description: hit.description,
            body: None,
            icon_url: hit.icon_url,
            author: hit.author,
            downloads: hit.downloads,
            followers: None,
            categories: hit.categories,
            source: crate::types::mod_info::ModSource::Modrinth,
            versions: hit.versions,
            game_versions: vec![],
            loaders: vec![],
            project_url: format!("https://modrinth.com/modpack/{}", slug),
            updated_at: hit.date_modified,
            client_side: None,
            server_side: None,
            source_url: None,
            issues_url: None,
            wiki_url: None,
            discord_url: None,
            gallery: vec![],
        }
    }).collect())
}

/// Entfernt nur Signatur-Dateien aus einer ZIP-Datei (Resource Pack, Shader Pack, etc.)
#[allow(dead_code)]
async fn remove_meta_inf_from_zip(zip_path: &std::path::Path) -> Result<(), String> {
    use std::io::{Read, Write};
    use zip::write::FileOptions;

    // Lese die originale ZIP
    let zip_file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| e.to_string())?;

    // Erstelle temporäre Datei
    let temp_path = zip_path.with_extension("zip.tmp");
    let temp_file = std::fs::File::create(&temp_path).map_err(|e| e.to_string())?;
    let mut zip_writer = zip::ZipWriter::new(temp_file);

    let mut removed_count = 0;

    // Kopiere alle Dateien, aber überspringe nur Signatur-Dateien
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_string();

        // Überspringe NUR Signatur-Dateien (.SF, .DSA, .RSA, .EC)
        // Behalte aber andere META-INF Dateien (z.B. MANIFEST.MF, nested JARs)
        let should_skip = name.starts_with("META-INF/") && (
            name.ends_with(".SF") ||   // Signature File
            name.ends_with(".DSA") ||  // Digital Signature
            name.ends_with(".RSA") ||  // RSA Signature
            name.ends_with(".EC")      // Elliptic Curve Signature
        );

        if should_skip {
            tracing::debug!("Removing signature file: {}", name);
            removed_count += 1;
            continue;
        }

        // Kopiere andere Dateien
        let options = FileOptions::default()
            .compression_method(file.compression())
            .unix_permissions(file.unix_mode().unwrap_or(0o755));

        if name.ends_with('/') {
            // Ordner
            zip_writer.add_directory(&name, options).map_err(|e| e.to_string())?;
        } else {
            // Datei
            zip_writer.start_file(&name, options).map_err(|e| e.to_string())?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
            zip_writer.write_all(&buffer).map_err(|e| e.to_string())?;
        }
    }

    zip_writer.finish().map_err(|e| e.to_string())?;
    drop(archive);

    if removed_count > 0 {
        tracing::info!("Removed {} signature files from ZIP", removed_count);
    }

    // Ersetze originale Datei mit bereinigter Version
    tokio::fs::remove_file(zip_path).await.map_err(|e| e.to_string())?;
    tokio::fs::rename(&temp_path, zip_path).await.map_err(|e| e.to_string())?;

    Ok(())
}
