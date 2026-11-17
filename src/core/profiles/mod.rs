#![allow(dead_code)]

use crate::types::profile::{Profile, ProfileList};
use anyhow::Result;
use std::path::PathBuf;

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
        parse_profiles(&content)
    }

    pub async fn save_profiles(&self, profiles: &ProfileList) -> Result<()> {
        let mut value = serde_json::to_value(profiles)?;
        // Nicht lesbare Profile unverändert zurückschreiben
        if let Some(list) = value["profiles"].as_array_mut() {
            list.extend(profiles.unsupported.iter().cloned());
        }
        let content = serde_json::to_string_pretty(&value)?;

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

/// Liest `profiles.json` Profil für Profil: Ein Profil, das diese Version nicht versteht
/// (z.B. mit einem Loader aus einer neueren Version), blockiert nicht mehr alle anderen.
fn parse_profiles(content: &str) -> Result<ProfileList> {
    #[derive(serde::Deserialize)]
    struct Raw {
        #[serde(default)]
        profiles: Vec<serde_json::Value>,
        #[serde(default)]
        active_profile: Option<String>,
    }
    let raw: Raw = serde_json::from_str(content)?;
    let mut list = ProfileList {
        active_profile: raw.active_profile,
        ..ProfileList::default()
    };
    for value in raw.profiles {
        match serde_json::from_value::<Profile>(value.clone()) {
            Ok(profile) => list.profiles.push(profile),
            Err(e) => {
                tracing::warn!(
                    "Profile {} kept but not loaded (needs a newer launcher?): {}",
                    value["name"].as_str().unwrap_or("?"),
                    e
                );
                list.unsupported.push(value);
            }
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_profiles_do_not_block_the_rest() {
        let json = r#"{
            "profiles": [
                {"id": "a", "name": "Fabric", "minecraft_version": "1.21.4",
                 "loader": {"loader": "fabric", "version": "", "minecraft_version": "1.21.4"},
                 "icon_path": null, "created_at": "", "last_played": null, "mods": [],
                 "game_dir": "x", "java_args": null, "memory_mb": null},
                {"id": "b", "name": "Future", "loader": {"loader": "something-new"}}
            ],
            "active_profile": "a"
        }"#;
        let list = parse_profiles(json).unwrap();
        assert_eq!(list.profiles.len(), 1);
        assert_eq!(list.unsupported.len(), 1);
        // Beim Speichern bleibt das unbekannte Profil erhalten
        let mut value = serde_json::to_value(&list).unwrap();
        value["profiles"]
            .as_array_mut()
            .unwrap()
            .extend(list.unsupported.iter().cloned());
        assert_eq!(value["profiles"].as_array().unwrap().len(), 2);
    }
}
