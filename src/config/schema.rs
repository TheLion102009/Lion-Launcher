use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    pub version: String,
    pub launcher_dir: PathBuf,
    pub game_settings: GameSettings,
    pub mod_sources: ModSources,
    pub appearance: AppearanceSettings,
    #[serde(default)]
    pub privacy: PrivacySettings,
}

/// Funktionen, die ohne direkte Aktion Verbindungen ins Internet aufbauen (siehe PRIVACY.md).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySettings {
    /// Beim Start und regelmäßig bei GitHub nach Launcher-Updates suchen
    #[serde(default = "enabled")]
    pub auto_update_check: bool,
    /// P2P-Freunde-System (Online-Status, Chat, geteilte Instanzen) starten
    #[serde(default = "enabled")]
    pub friends_enabled: bool,
}

fn enabled() -> bool {
    true
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            auto_update_check: true,
            friends_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSettings {
    pub memory_mb: u32,
    pub java_path: Option<PathBuf>,
    pub java_args: Vec<String>,
    pub fullscreen: bool,
    pub resolution: Resolution,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModSources {
    pub modrinth_enabled: bool,
    pub curseforge_enabled: bool,
    pub curseforge_api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceSettings {
    pub theme: String,
    pub language: String,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            launcher_dir: crate::config::defaults::launcher_dir(),
            game_settings: GameSettings::default(),
            mod_sources: ModSources::default(),
            appearance: AppearanceSettings::default(),
            privacy: PrivacySettings::default(),
        }
    }
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            memory_mb: crate::config::defaults::default_memory_mb(),
            java_path: None,
            java_args: crate::config::defaults::default_java_args(),
            fullscreen: false,
            resolution: Resolution {
                width: 1280,
                height: 720,
            },
        }
    }
}

impl Default for ModSources {
    fn default() -> Self {
        Self {
            modrinth_enabled: true,
            curseforge_enabled: true,
            curseforge_api_key: None,
        }
    }
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            language: "en".to_string(),
        }
    }
}
