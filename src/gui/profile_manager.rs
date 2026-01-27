use crate::core::profiles::ProfileManager;
use crate::types::profile::{Profile, ProfileList};
use crate::types::version::ModLoader;

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
    .map_err(|e| e.to_string())
}
