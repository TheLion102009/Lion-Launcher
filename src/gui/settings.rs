use crate::config::schema::LauncherConfig;
use crate::types::version::MinecraftVersion;

#[tauri::command]
pub async fn get_config() -> Result<LauncherConfig, String> {
    let config_path = crate::config::defaults::launcher_dir().join("config.json");
    
    if !config_path.exists() {
        return Ok(LauncherConfig::default());
    }

    let content = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|e| e.to_string())?;
    
    let config: LauncherConfig = serde_json::from_str(&content)
        .map_err(|e| e.to_string())?;
    
    Ok(config)
}

#[tauri::command]
pub async fn save_config(config: LauncherConfig) -> Result<(), String> {
    let config_path = crate::config::defaults::launcher_dir().join("config.json");
    
    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    let content = serde_json::to_string_pretty(&config)
        .map_err(|e| e.to_string())?;
    
    tokio::fs::write(&config_path, content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_minecraft_versions() -> Result<Vec<MinecraftVersion>, String> {
    let client = crate::api::mojang::MojangClient::new()
        .map_err(|e| e.to_string())?;
    
    client.get_version_manifest()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_fabric_versions(minecraft_version: String) -> Result<Vec<String>, String> {
    let client = crate::api::fabric::FabricClient::new()
        .map_err(|e| e.to_string())?;
    
    let versions = client.get_loader_versions(&minecraft_version)
        .await
        .map_err(|e| e.to_string())?;
    
    Ok(versions.into_iter().map(|v| v.loader.version).collect())
}

#[tauri::command]
pub async fn get_quilt_versions(minecraft_version: String) -> Result<Vec<String>, String> {
    let client = crate::api::quilt::QuiltClient::new()
        .map_err(|e| e.to_string())?;

    let versions = client.get_loader_versions(&minecraft_version)
        .await
        .map_err(|e| e.to_string())?;

    Ok(versions.into_iter().map(|v| v.loader.version).collect())
}

#[tauri::command]
pub async fn get_forge_versions(minecraft_version: String) -> Result<Vec<String>, String> {
    let client = crate::api::forge::ForgeClient::new()
        .map_err(|e| e.to_string())?;

    let versions = client.get_loader_versions(&minecraft_version)
        .await
        .map_err(|e| e.to_string())?;

    // ForgeVersion verwendet "forge_version" nicht "version"!
    Ok(versions.into_iter().map(|v| v.forge_version).collect())
}

#[tauri::command]
pub async fn get_neoforge_versions(minecraft_version: String) -> Result<Vec<String>, String> {
    tracing::info!("🔍 GUI: Loading NeoForge versions for MC {}", minecraft_version);

    let client = crate::api::neoforge::NeoForgeClient::new()
        .map_err(|e| {
            tracing::error!("❌ Failed to create NeoForge client: {}", e);
            e.to_string()
        })?;

    let versions = client.get_loader_versions(&minecraft_version)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to load NeoForge versions: {}", e);
            e.to_string()
        })?;

    let version_strings: Vec<String> = versions.into_iter().map(|v| v.version).collect();

    tracing::info!("✅ GUI: Loaded {} NeoForge versions for MC {}", version_strings.len(), minecraft_version);
    if !version_strings.is_empty() {
        tracing::debug!("   First 3 versions: {:?}", version_strings.iter().take(3).collect::<Vec<_>>());
    }

    Ok(version_strings)
}

#[tauri::command]
pub async fn get_system_memory() -> Result<u64, String> {
    use sysinfo::System;

    let mut sys = System::new_all();
    sys.refresh_memory();

    // Gib den Gesamt-RAM in MB zurück
    let total_memory_mb = sys.total_memory() / 1024 / 1024;

    tracing::debug!("System total memory: {} MB", total_memory_mb);

    Ok(total_memory_mb)
}

#[tauri::command]
pub async fn initialize_launcher() -> Result<(), String> {
    crate::core::fs::ensure_launcher_dirs()
        .await
        .map_err(|e| e.to_string())
}
