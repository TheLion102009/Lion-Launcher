#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::types::version::{ModLoader, LoaderVersion};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub minecraft_version: String,
    pub loader: LoaderVersion,
    pub icon_path: Option<PathBuf>,
    pub created_at: String,
    pub last_played: Option<String>,
    pub mods: Vec<String>, // Mod IDs
    pub game_dir: PathBuf,
    pub java_args: Option<Vec<String>>,
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub settings_sync: bool, // Sync MC settings (options.txt) with global settings
}

impl Profile {
    pub fn new(
        name: String,
        minecraft_version: String,
        loader: ModLoader,
        loader_version: String,
    ) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();
        let game_dir = crate::config::defaults::profiles_dir().join(&id);

        Self {
            id,
            name,
            minecraft_version: minecraft_version.clone(),
            loader: LoaderVersion {
                loader,
                version: loader_version,
                minecraft_version,
            },
            icon_path: None,
            created_at,
            last_played: None,
            mods: Vec::new(),
            game_dir,
            java_args: None,
            memory_mb: None,
            settings_sync: true, // Standardmäßig aktiviert
        }
    }

    pub fn update_last_played(&mut self) {
        self.last_played = Some(chrono::Utc::now().to_rfc3339());
    }

    pub fn add_mod(&mut self, mod_id: String) {
        if !self.mods.contains(&mod_id) {
            self.mods.push(mod_id);
        }
    }

    pub fn remove_mod(&mut self, mod_id: &str) {
        self.mods.retain(|id| id != mod_id);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileList {
    pub profiles: Vec<Profile>,
    pub active_profile: Option<String>,
}

impl ProfileList {
    pub fn new() -> Self {
        Self {
            profiles: Vec::new(),
            active_profile: None,
        }
    }

    pub fn add_profile(&mut self, profile: Profile) {
        self.profiles.push(profile);
    }

    pub fn remove_profile(&mut self, profile_id: &str) {
        self.profiles.retain(|p| p.id != profile_id);
        if self.active_profile.as_deref() == Some(profile_id) {
            self.active_profile = None;
        }
    }

    pub fn get_profile(&self, profile_id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == profile_id)
    }

    pub fn get_profile_mut(&mut self, profile_id: &str) -> Option<&mut Profile> {
        self.profiles.iter_mut().find(|p| p.id == profile_id)
    }

    pub fn get_active_profile(&self) -> Option<&Profile> {
        self.active_profile.as_ref()
            .and_then(|id| self.get_profile(id))
    }
}

impl Default for ProfileList {
    fn default() -> Self {
        Self::new()
    }
}
