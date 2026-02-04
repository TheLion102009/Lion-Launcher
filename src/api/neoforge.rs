#![allow(dead_code)]

use anyhow::{Result, bail};
use serde::Deserialize;
use crate::api::client::ApiClient;

const NEOFORGE_MAVEN_URL: &str = "https://maven.neoforged.net/releases";
const NEOFORGE_API_URL: &str = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

pub struct NeoForgeClient {
    client: ApiClient,
}

impl NeoForgeClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: ApiClient::new()?,
        })
    }

    /// Lädt alle verfügbaren NeoForge-Versionen für eine Minecraft-Version
    pub async fn get_loader_versions(&self, minecraft_version: &str) -> Result<Vec<NeoForgeVersion>> {
        let all_versions = self.get_all_versions().await?;
        
        // NeoForge verwendet ein anderes Versionsschema
        // Ab MC 1.20.1+ ist NeoForge verfügbar
        let mc_version_parts: Vec<&str> = minecraft_version.split('.').collect();
        
        if mc_version_parts.len() < 2 {
            bail!("Ungültige Minecraft-Version: {}", minecraft_version);
        }

        // Filtere Versionen nach Minecraft-Kompatibilität
        let filtered: Vec<NeoForgeVersion> = all_versions
            .into_iter()
            .filter(|v| v.is_compatible_with(minecraft_version))
            .collect();

        if filtered.is_empty() {
            bail!("Keine NeoForge-Versionen für Minecraft {} gefunden", minecraft_version);
        }

        Ok(filtered)
    }

    /// Lädt alle verfügbaren NeoForge-Versionen
    async fn get_all_versions(&self) -> Result<Vec<NeoForgeVersion>> {
        let response: NeoForgeApiResponse = self.client.get_json(NEOFORGE_API_URL).await?;
        
        let mut versions = Vec::new();
        
        for version_str in response.versions {
            // NeoForge-Versionen sind im Format "20.4.80" für MC 1.20.4
            // oder "21.0.0-beta" für MC 1.21
            if let Some(neoforge_version) = Self::parse_version(&version_str) {
                versions.push(neoforge_version);
            }
        }
        
        versions.sort_by(|a, b| b.version.cmp(&a.version));
        
        Ok(versions)
    }

    fn parse_version(version_str: &str) -> Option<NeoForgeVersion> {
        // Format: "20.4.80" -> MC 1.20.4, NeoForge 20.4.80
        let parts: Vec<&str> = version_str.split('.').collect();
        
        if parts.is_empty() {
            return None;
        }

        // Extrahiere MC-Version aus NeoForge-Version
        let mc_major = parts[0].parse::<u32>().ok()?;
        let mc_minor = if parts.len() > 1 {
            parts[1].parse::<u32>().ok()?
        } else {
            0
        };

        let mc_version = format!("1.{}.{}", mc_major, mc_minor);

        Some(NeoForgeVersion {
            version: version_str.to_string(),
            mc_version,
            is_beta: version_str.contains("beta") || version_str.contains("alpha"),
        })
    }

    /// Lädt alle unterstützten Minecraft-Versionen
    pub async fn get_supported_game_versions(&self) -> Result<Vec<String>> {
        let versions = self.get_all_versions().await?;
        
        let mut mc_versions: Vec<String> = versions
            .into_iter()
            .map(|v| v.mc_version)
            .collect();
        
        mc_versions.sort();
        mc_versions.dedup();
        mc_versions.reverse();
        
        Ok(mc_versions)
    }

    /// Generiert die Download-URL für NeoForge-Installer
    pub fn get_installer_url(&self, version: &str) -> String {
        format!(
            "{}/net/neoforged/neoforge/{}/neoforge-{}-installer.jar",
            NEOFORGE_MAVEN_URL, version, version
        )
    }
}

#[derive(Debug, Clone)]
pub struct NeoForgeVersion {
    pub version: String,
    pub mc_version: String,
    pub is_beta: bool,
}

impl NeoForgeVersion {
    fn is_compatible_with(&self, mc_version: &str) -> bool {
        self.mc_version == mc_version
    }
}

#[derive(Debug, Deserialize)]
struct NeoForgeApiResponse {
    versions: Vec<String>,
}
