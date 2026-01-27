#![allow(dead_code)]

use anyhow::Result;
use std::path::PathBuf;
use crate::types::profile::{Profile, ProfileList};

pub struct ProfileManager {
    profiles_path: PathBuf,
}

impl ProfileManager {
    pub fn new() -> Result<Self> {
        let profiles_path = crate::config::defaults::launcher_dir().join("profiles.json");
        Ok(Self { profiles_path })
    }

    pub async fn load_profiles(&self) -> Result<ProfileList> {
        if !self.profiles_path.exists() {
            return Ok(ProfileList::default());
        }

        let content = tokio::fs::read_to_string(&self.profiles_path).await?;
        let profiles: ProfileList = serde_json::from_str(&content)?;
        Ok(profiles)
    }

    pub async fn save_profiles(&self, profiles: &ProfileList) -> Result<()> {
        let content = serde_json::to_string_pretty(profiles)?;
        
        if let Some(parent) = self.profiles_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        
        tokio::fs::write(&self.profiles_path, content).await?;
        Ok(())
    }

    pub async fn create_profile(&self, profile: Profile) -> Result<ProfileList> {
        let mut profiles = self.load_profiles().await?;
        
        // Create profile directory
        tokio::fs::create_dir_all(&profile.game_dir).await?;
        tokio::fs::create_dir_all(profile.game_dir.join("mods")).await?;
        
        profiles.add_profile(profile);
        self.save_profiles(&profiles).await?;
        
        Ok(profiles)
    }

    pub async fn delete_profile(&self, profile_id: &str) -> Result<ProfileList> {
        let mut profiles = self.load_profiles().await?;
        
        if let Some(profile) = profiles.get_profile(profile_id) {
            // Optionally delete the game directory
            if profile.game_dir.exists() {
                tokio::fs::remove_dir_all(&profile.game_dir).await.ok();
            }
        }
        
        profiles.remove_profile(profile_id);
        self.save_profiles(&profiles).await?;
        
        Ok(profiles)
    }

    pub async fn update_profile(&self, profile: Profile) -> Result<ProfileList> {
        let mut profiles = self.load_profiles().await?;
        
        if let Some(existing) = profiles.get_profile_mut(&profile.id) {
            *existing = profile;
        }
        
        self.save_profiles(&profiles).await?;
        Ok(profiles)
    }
}
