use serde::{Deserialize, Serialize};
use std::path::PathBuf;

fn default_server_memory_mb() -> u32 {
    4096
}

fn default_server_port() -> u16 {
    25565
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServerMode {
    Plugins,
    Modded,
}

impl ServerMode {
    pub fn from_str(value: &str) -> Self {
        match value.trim().to_lowercase().as_str() {
            "modded" => Self::Modded,
            _ => Self::Plugins,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub mode: ServerMode,
    pub software: String,
    pub minecraft_version: String,
    pub software_version: String,
    pub created_at: String,
    pub updated_at: String,
    pub server_dir: PathBuf,
    #[serde(default = "default_server_port")]
    pub port: u16,
    #[serde(default = "default_server_memory_mb")]
    pub memory_mb: u32,
    #[serde(default)]
    pub java_args: Vec<String>,
    #[serde(default)]
    pub auto_restart: bool,
}

impl ServerProfile {
    pub fn new(
        name: String,
        mode: ServerMode,
        software: String,
        minecraft_version: String,
        software_version: String,
        port: u16,
    ) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let server_dir = crate::config::defaults::servers_dir().join(&id);

        Self {
            id,
            name,
            mode,
            software,
            minecraft_version,
            software_version,
            created_at: now.clone(),
            updated_at: now,
            server_dir,
            port: if port == 0 { default_server_port() } else { port },
            memory_mb: default_server_memory_mb(),
            java_args: crate::config::defaults::default_java_args(),
            auto_restart: false,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = chrono::Utc::now().to_rfc3339();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerProfileList {
    pub profiles: Vec<ServerProfile>,
}

impl ServerProfileList {
    pub fn add_profile(&mut self, profile: ServerProfile) {
        self.profiles.push(profile);
    }

    pub fn remove_profile(&mut self, server_id: &str) {
        self.profiles.retain(|p| p.id != server_id);
    }

    pub fn get_profile(&self, server_id: &str) -> Option<&ServerProfile> {
        self.profiles.iter().find(|p| p.id == server_id)
    }

    pub fn get_profile_mut(&mut self, server_id: &str) -> Option<&mut ServerProfile> {
        self.profiles.iter_mut().find(|p| p.id == server_id)
    }
}

