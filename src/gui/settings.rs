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
pub async fn initialize_launcher() -> Result<(), String> {
    crate::core::fs::ensure_launcher_dirs()
        .await
        .map_err(|e| e.to_string())
}
