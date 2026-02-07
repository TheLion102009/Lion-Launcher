#![allow(dead_code)]

use anyhow::{Result, bail};
use serde::Deserialize;
use crate::api::client::ApiClient;

const FORGE_MAVEN_URL: &str = "https://maven.minecraftforge.net";
const FORGE_META_URL: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";
const FORGE_PROMOTIONS_URL: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";

pub struct ForgeClient {
    client: ApiClient,
}

impl ForgeClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: ApiClient::new()?,
        })
    }

    /// Lädt alle verfügbaren Forge-Versionen für eine Minecraft-Version
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
        
        mc_versions.sort_by(|a, b| {
            Self::compare_version_strings(b, a) // Neueste zuerst
        });
        mc_versions.dedup();

        Ok(mc_versions)
    }

    async fn get_all_versions(&self) -> Result<Vec<ForgeVersion>> {
        // Versuche zuerst die neue API
        if let Ok(versions) = self.get_versions_from_new_api().await {
            return Ok(versions);
        }

        // Fallback auf alte Methode
        self.get_versions_from_legacy_api().await
    }

    /// Neue API Methode (für MC 1.13+)
    async fn get_versions_from_new_api(&self) -> Result<Vec<ForgeVersion>> {
        let data: ForgeMavenMetadata = self.client.get_json(FORGE_META_URL).await?;
        let promotions = self.get_promotions().await.ok();

        let mut versions = Vec::new();
        
        for (mc_version, forge_versions) in data.versions {
            for raw_forge_version in forge_versions {
                // Die Forge-Version in maven-metadata.json kann im Format "1.11.2-13.20.0.2201"
                // oder nur "47.3.0" sein. Wir müssen das MC-Prefix entfernen wenn vorhanden.
                let forge_version = if raw_forge_version.starts_with(&format!("{}-", mc_version)) {
                    // Format: "1.11.2-13.20.0.2201" -> "13.20.0.2201"
                    raw_forge_version.strip_prefix(&format!("{}-", mc_version))
                        .unwrap_or(&raw_forge_version)
                        .to_string()
                } else {
                    // Format: "47.3.0" -> "47.3.0"
                    raw_forge_version.clone()
                };

                let full_version = format!("{}-{}", mc_version, forge_version);
                let recommended = promotions.as_ref()
                    .and_then(|p| p.promos.get(&format!("{}-recommended", mc_version)))
                    .map(|v| {
                        // Vergleiche auch mit raw_forge_version falls das in promotions steht
                        v == &forge_version || v == &raw_forge_version
                    })
                    .unwrap_or(false);

                let installer_url = self.get_installer_url(&mc_version, &forge_version);

                versions.push(ForgeVersion {
                    mc_version: mc_version.clone(),
                    forge_version: forge_version.clone(),
                    full_version,
                    recommended,
                    installer_url,
                });
            }
        }

        Ok(versions)
    }

    /// Legacy API für ältere MC Versionen
    async fn get_versions_from_legacy_api(&self) -> Result<Vec<ForgeVersion>> {
        let promotions = self.get_promotions().await?;
        let mut versions = Vec::new();

        for (key, forge_version) in promotions.promos {
            let is_recommended = key.ends_with("-recommended");

            if let Some(mc_version) = key.strip_suffix("-recommended")
                .or_else(|| key.strip_suffix("-latest"))
            {
                let full_version = format!("{}-{}", mc_version, forge_version);
                let installer_url = self.get_installer_url(mc_version, &forge_version);

                versions.push(ForgeVersion {
                    mc_version: mc_version.to_string(),
                    forge_version,
                    full_version,
                    recommended: is_recommended,
                    installer_url,
                });
            }
        }
        
        Ok(versions)
    }

    async fn get_promotions(&self) -> Result<ForgePromotions> {
        self.client.get_json(FORGE_PROMOTIONS_URL).await
    }

    /// Generiert die Download-URL für Forge-Installer
    pub fn get_installer_url(&self, mc_version: &str, forge_version: &str) -> String {
        format!(
            "{}/net/minecraftforge/forge/{}-{}/forge-{}-{}-installer.jar",
            FORGE_MAVEN_URL, mc_version, forge_version, mc_version, forge_version
        )
    }

    /// Generiert die Download-URL für das Forge-Universal-JAR (ältere Versionen)
    pub fn get_universal_url(&self, mc_version: &str, forge_version: &str) -> String {
        format!(
            "{}/net/minecraftforge/forge/{}-{}/forge-{}-{}-universal.jar",
            FORGE_MAVEN_URL, mc_version, forge_version, mc_version, forge_version
        )
    }

    /// Prüft ob eine Minecraft-Version NeoForge verwenden sollte (MC 1.20.1+)
    pub fn should_use_neoforge(mc_version: &str) -> bool {
        Self::is_version_at_least(mc_version, "1.20.1")
    }

    /// Vergleicht Versionsstrings (z.B. "1.20.1" >= "1.20.0")
    fn is_version_at_least(version: &str, minimum: &str) -> bool {
        Self::compare_version_strings(version, minimum) != std::cmp::Ordering::Less
    }

    fn compare_version_strings(a: &str, b: &str) -> std::cmp::Ordering {
        let parse_version = |v: &str| -> Vec<u32> {
            v.split('.')
                .filter_map(|s| s.parse::<u32>().ok())
                .collect()
        };

        let a_parts = parse_version(a);
        let b_parts = parse_version(b);

        for i in 0..a_parts.len().max(b_parts.len()) {
            let a_part = a_parts.get(i).copied().unwrap_or(0);
            let b_part = b_parts.get(i).copied().unwrap_or(0);

            match a_part.cmp(&b_part) {
                std::cmp::Ordering::Equal => continue,
                other => return other,
            }
        }

        std::cmp::Ordering::Equal
    }
}

#[derive(Debug, Clone)]
pub struct ForgeVersion {
    pub mc_version: String,
    pub forge_version: String,
    pub full_version: String,
    pub recommended: bool,
    pub installer_url: String,
}

#[derive(Debug, Deserialize)]
struct ForgeMavenMetadata {
    #[serde(flatten)]
    versions: std::collections::HashMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct ForgePromotions {
    promos: std::collections::HashMap<String, String>,
}

