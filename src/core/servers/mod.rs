use crate::types::server_profile::{ServerProfile, ServerProfileList};
use anyhow::Result;
use std::path::PathBuf;

pub struct ServerProfileManager {
    profiles_path: PathBuf,
}

impl ServerProfileManager {
    pub fn new() -> Result<Self> {
        let profiles_path = crate::config::defaults::server_profiles_file();
        Ok(Self { profiles_path })
    }

    pub async fn load_profiles(&self) -> Result<ServerProfileList> {
        if !self.profiles_path.exists() {
            return Ok(ServerProfileList::default());
        }

        let content = tokio::fs::read_to_string(&self.profiles_path).await?;
        let profiles: ServerProfileList = serde_json::from_str(&content)?;
        Ok(profiles)
    }

    pub async fn save_profiles(&self, profiles: &ServerProfileList) -> Result<()> {
        if let Some(parent) = self.profiles_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let content = serde_json::to_string_pretty(profiles)?;
        tokio::fs::write(&self.profiles_path, content).await?;
        Ok(())
    }

    pub async fn create_profile(&self, profile: ServerProfile) -> Result<ServerProfileList> {
        let mut profiles = self.load_profiles().await?;
        profiles.add_profile(profile);
        self.save_profiles(&profiles).await?;
        Ok(profiles)
    }

    pub async fn delete_profile(&self, server_id: &str) -> Result<ServerProfileList> {
        let mut profiles = self.load_profiles().await?;

        if let Some(profile) = profiles.get_profile(server_id) {
            if profile.server_dir.exists() {
                tokio::fs::remove_dir_all(&profile.server_dir).await.ok();
            }
        }

        profiles.remove_profile(server_id);
        self.save_profiles(&profiles).await?;
        Ok(profiles)
    }
}



