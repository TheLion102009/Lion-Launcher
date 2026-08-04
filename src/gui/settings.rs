use crate::config::schema::LauncherConfig;
use crate::types::version::MinecraftVersion;
use std::collections::BTreeSet;

#[derive(serde::Serialize)]
pub struct JavaRuntimeOption {
    pub value: String,
    pub label: String,
    pub major: u32,
    pub installed: bool,
    pub path: Option<String>,
}

fn java_bin_name() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

fn managed_java_bin_for_major(major: u32) -> std::path::PathBuf {
    crate::config::defaults::java_dir()
        .join(format!("java-{}", major))
        .join("bin")
        .join(java_bin_name())
}

async fn detect_latest_stable_jdk_major() -> u32 {
    #[derive(serde::Deserialize)]
    struct AdoptiumInfo {
        most_recent_feature_release: Option<u32>,
        available_releases: Option<Vec<u32>>,
    }

    let response = reqwest::Client::new()
        .get("https://api.adoptium.net/v3/info/available_releases")
        .send()
        .await;

    if let Ok(resp) = response {
        if let Ok(resp) = resp.error_for_status() {
            if let Ok(info) = resp.json::<AdoptiumInfo>().await {
                if let Some(v) = info.most_recent_feature_release {
                    return v.max(17);
                }
                if let Some(list) = info.available_releases {
                    if let Some(v) = list.into_iter().max() {
                        return v.max(17);
                    }
                }
            }
        }
    }

    21
}

#[tauri::command]
pub async fn get_latest_stable_jdk_major() -> Result<u32, String> {
    Ok(detect_latest_stable_jdk_major().await)
}

#[tauri::command]
pub async fn get_java_runtime_options() -> Result<Vec<JavaRuntimeOption>, String> {
    let latest_stable = detect_latest_stable_jdk_major().await;
    let mut majors = BTreeSet::new();
    majors.insert(8);
    majors.insert(11);
    majors.insert(17);
    majors.insert(21);
    majors.insert(latest_stable);

    let mut options = Vec::new();
    options.push(JavaRuntimeOption {
        value: "auto".to_string(),
        label: format!("Latest stable JDK ({}) - automatic", latest_stable),
        major: latest_stable,
        installed: managed_java_bin_for_major(latest_stable).exists(),
        path: None,
    });

    for major in majors.into_iter().rev() {
        let managed = managed_java_bin_for_major(major);
        let installed = managed.exists();
        options.push(JavaRuntimeOption {
            value: major.to_string(),
            label: if installed {
                format!("Java {} (installed)", major)
            } else {
                format!("Java {} (download on save)", major)
            },
            major,
            installed,
            path: installed.then(|| managed.display().to_string()),
        });
    }

    // Add current PATH java (if any) as extra selectable runtime.
    let path_java = if cfg!(windows) { "java.exe" } else { "java" };
    if tokio::process::Command::new(path_java)
        .arg("-version")
        .output()
        .await
        .is_ok()
    {
        let major = crate::core::minecraft::detect_java_runtime_major(path_java).await;
        if major > 0 {
            options.push(JavaRuntimeOption {
                value: format!("system-{}", major),
                label: format!("System Java {} (PATH)", major),
                major,
                installed: true,
                path: Some(path_java.to_string()),
            });
        }
    }

    Ok(options)
}

#[tauri::command]
pub async fn ensure_java_runtime_for_settings(major: u32) -> Result<String, String> {
    let required = major.max(8);
    crate::core::minecraft::ensure_java_runtime(required, None).await
}

#[tauri::command]
pub async fn get_config() -> Result<LauncherConfig, String> {
    let config_path = crate::config::defaults::launcher_dir().join("config.json");

    if !config_path.exists() {
        return Ok(LauncherConfig::default());
    }

    let content = tokio::fs::read_to_string(&config_path)
        .await
        .map_err(|e| e.to_string())?;

    let config: LauncherConfig = serde_json::from_str(&content).map_err(|e| e.to_string())?;

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

    let content = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;

    tokio::fs::write(&config_path, content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_minecraft_versions() -> Result<Vec<MinecraftVersion>, String> {
    let client = crate::api::mojang::MojangClient::new().map_err(|e| e.to_string())?;

    client
        .get_version_manifest()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_fabric_versions(minecraft_version: String) -> Result<Vec<String>, String> {
    let client = crate::api::fabric::FabricClient::new().map_err(|e| e.to_string())?;

    let versions = client
        .get_loader_versions(&minecraft_version)
        .await
        .map_err(|e| e.to_string())?;

    Ok(versions.into_iter().map(|v| v.loader.version).collect())
}

#[tauri::command]
pub async fn get_quilt_versions(minecraft_version: String) -> Result<Vec<String>, String> {
    let client = crate::api::quilt::QuiltClient::new().map_err(|e| e.to_string())?;

    // Versuche Loader-Versionen für die gewünschte MC-Version zu laden.
    // Die Methode hat bereits einen internen Fallback auf die neueste unterstützte Version.
    match client.get_loader_versions(&minecraft_version).await {
        Ok(versions) if !versions.is_empty() => {
            return Ok(versions.into_iter().map(|v| v.loader.version).collect());
        }
        _ => {}
    }

    // Zweiter Fallback: Alle Loader-Versionen laden (unabhängig von MC-Version).
    // Das stellt sicher, dass auch bei einer sehr neuen/unbekannten MC-Version
    // immer Loader-Versionen angezeigt werden (Quilt unterstützt viele MC-Versionen rückwärtskompatibel).
    tracing::warn!(
        "Quilt-Fallback (global): Lade alle Loader-Versionen für MC {} – direkte Abfrage gescheitert",
        minecraft_version
    );
    let all_versions = client.get_all_loader_versions()
        .await
        .map_err(|e| format!("Quilt Loader-Versionen konnten nicht geladen werden (auch globaler Fallback fehlgeschlagen): {}", e))?;

    if all_versions.is_empty() {
        return Err("Keine Quilt Loader-Versionen gefunden".to_string());
    }

    Ok(all_versions.into_iter().map(|v| v.version).collect())
}

#[tauri::command]
pub async fn get_forge_versions(minecraft_version: String) -> Result<Vec<String>, String> {
    let client = crate::api::forge::ForgeClient::new().map_err(|e| e.to_string())?;

    let versions = client
        .get_loader_versions(&minecraft_version)
        .await
        .map_err(|e| e.to_string())?;

    // ForgeVersion verwendet "forge_version" nicht "version"!
    Ok(versions.into_iter().map(|v| v.forge_version).collect())
}

/// Gibt alle MC-Versionen zurück für die Forge verfügbar ist
#[tauri::command]
pub async fn get_forge_supported_mc_versions() -> Result<Vec<String>, String> {
    let client = crate::api::forge::ForgeClient::new().map_err(|e| e.to_string())?;

    client
        .get_supported_game_versions()
        .await
        .map_err(|e| e.to_string())
}

/// Gibt alle MC-Versionen zurück für die Fabric verfügbar ist
#[tauri::command]
pub async fn get_fabric_supported_mc_versions() -> Result<Vec<String>, String> {
    let client = crate::api::fabric::FabricClient::new().map_err(|e| e.to_string())?;

    let versions = client
        .get_game_versions()
        .await
        .map_err(|e| e.to_string())?;

    Ok(versions.into_iter().map(|v| v.version).collect())
}

/// Gibt alle MC-Versionen zurück für die Quilt verfügbar ist
#[tauri::command]
pub async fn get_quilt_supported_mc_versions() -> Result<Vec<String>, String> {
    let client = crate::api::quilt::QuiltClient::new().map_err(|e| e.to_string())?;

    let versions = client
        .get_game_versions()
        .await
        .map_err(|e| e.to_string())?;

    Ok(versions.into_iter().map(|v| v.version).collect())
}

/// Gibt alle MC-Versionen zurück für die NeoForge verfügbar ist
#[tauri::command]
pub async fn get_neoforge_supported_mc_versions() -> Result<Vec<String>, String> {
    let client = crate::api::neoforge::NeoForgeClient::new().map_err(|e| e.to_string())?;

    client
        .get_supported_game_versions()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_neoforge_versions(minecraft_version: String) -> Result<Vec<String>, String> {
    tracing::info!(
        "🔍 GUI: Loading NeoForge versions for MC {}",
        minecraft_version
    );

    let client = crate::api::neoforge::NeoForgeClient::new().map_err(|e| {
        tracing::error!("❌ Failed to create NeoForge client: {}", e);
        e.to_string()
    })?;

    let versions = client
        .get_loader_versions(&minecraft_version)
        .await
        .map_err(|e| {
            tracing::error!("❌ Failed to load NeoForge versions: {}", e);
            e.to_string()
        })?;

    let version_strings: Vec<String> = versions.into_iter().map(|v| v.version).collect();

    tracing::info!(
        "✅ GUI: Loaded {} NeoForge versions for MC {}",
        version_strings.len(),
        minecraft_version
    );
    if !version_strings.is_empty() {
        tracing::debug!(
            "   First 3 versions: {:?}",
            version_strings.iter().take(3).collect::<Vec<_>>()
        );
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
