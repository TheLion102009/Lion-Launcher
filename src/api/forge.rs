#![allow(dead_code)]

use anyhow::{Result, bail};
use serde::Deserialize;
use crate::api::client::ApiClient;

const FORGE_MAVEN_URL: &str = "https://maven.minecraftforge.net";
const FORGE_META_URL: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";

pub struct ForgeClient {
    client: ApiClient,
}

impl ForgeClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: ApiClient::new()?,
        })
    }

    /// Lädt alle verfügbaren Forge-Versionen
    pub async fn get_loader_versions(&self, minecraft_version: &str) -> Result<Vec<ForgeVersion>> {
        let versions = self.get_all_versions().await?;
        
        // Filtere nach Minecraft-Version
        let filtered: Vec<ForgeVersion> = versions
            .into_iter()
            .filter(|v| v.mc_version == minecraft_version)
            .collect();

        if filtered.is_empty() {
            bail!("Keine Forge-Versionen für Minecraft {} gefunden", minecraft_version);
        }

        Ok(filtered)
    }

    /// Lädt alle Minecraft-Versionen mit Forge-Support
    pub async fn get_supported_game_versions(&self) -> Result<Vec<String>> {
        let versions = self.get_all_versions().await?;
        
        let mut mc_versions: Vec<String> = versions
            .into_iter()
            .map(|v| v.mc_version)
            .collect();
        
        mc_versions.sort();
        mc_versions.dedup();
        mc_versions.reverse(); // Neueste zuerst
        
        Ok(mc_versions)
    }

    async fn get_all_versions(&self) -> Result<Vec<ForgeVersion>> {
        let data: ForgeMavenMetadata = self.client.get_json(FORGE_META_URL).await?;
        
        let mut versions = Vec::new();
        
        for (mc_version, forge_versions) in data.versions {
            for forge_version in forge_versions {
                versions.push(ForgeVersion {
                    mc_version: mc_version.clone(),
                    forge_version: forge_version.clone(),
                    full_version: format!("{}-{}", mc_version, forge_version),
                    recommended: false, // Forge Meta enthält keine Empfehlungen mehr
                });
            }
        }
        
        Ok(versions)
    }

    /// Generiert die Download-URL für Forge-Installer
    pub fn get_installer_url(&self, mc_version: &str, forge_version: &str) -> String {
        format!(
            "{}/net/minecraftforge/forge/{}-{}/forge-{}-{}-installer.jar",
            FORGE_MAVEN_URL, mc_version, forge_version, mc_version, forge_version
        )
    }

    /// Generiert die Download-URL für das Forge-Universal-JAR
    pub fn get_universal_url(&self, mc_version: &str, forge_version: &str) -> String {
        format!(
            "{}/net/minecraftforge/forge/{}-{}/forge-{}-{}-universal.jar",
            FORGE_MAVEN_URL, mc_version, forge_version, mc_version, forge_version
        )
    }
}

#[derive(Debug, Clone)]
pub struct ForgeVersion {
    pub mc_version: String,
    pub forge_version: String,
    pub full_version: String,
    pub recommended: bool,
}

#[derive(Debug, Deserialize)]
struct ForgeMavenMetadata {
    #[serde(flatten)]
    versions: std::collections::HashMap<String, Vec<String>>,
}
