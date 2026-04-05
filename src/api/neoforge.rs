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
        tracing::info!("🔍 Loading NeoForge versions for Minecraft {}...", minecraft_version);

        let all_versions = self.get_all_versions_from_maven().await?;
        tracing::debug!("Found {} total NeoForge versions from Maven", all_versions.len());

        // Verwende die gleiche Filterlogik wie in core/minecraft/neoforge.rs
        let matching = Self::filter_matching_versions(&all_versions, minecraft_version);

        tracing::info!("✅ Found {} NeoForge versions for Minecraft {}", matching.len(), minecraft_version);

        if matching.is_empty() {
            bail!("Keine NeoForge-Versionen für Minecraft {} gefunden", minecraft_version);
        }

        // Konvertiere zu NeoForgeVersion Strukturen
        let mut versions: Vec<NeoForgeVersion> = matching.into_iter().map(|version_str| {
            NeoForgeVersion {
                version: version_str.clone(),
                mc_version: minecraft_version.to_string(),
                is_beta: version_str.contains("beta") || version_str.contains("alpha"),
                installer_url: format!(
                    "{}/net/neoforged/neoforge/{}/neoforge-{}-installer.jar",
                    NEOFORGE_MAVEN_URL, version_str, version_str
                ),
            }
        }).collect();

        // Sortiere nach Version (neueste zuerst)
        versions.sort_by(|a, b| Self::compare_neoforge_versions(&b.version, &a.version));

        Ok(versions)
    }

    /// Filtert NeoForge-Versionen die zur Minecraft-Version passen
    /// GLEICHE LOGIK wie in core/minecraft/neoforge.rs!
    fn filter_matching_versions(all_versions: &[String], mc_version: &str) -> Vec<String> {
        let mc_parts: Vec<&str> = mc_version.split('.').collect();

        if mc_parts.len() < 2 {
            return Vec::new();
        }

        let _major = mc_parts[0]; // "1"
        let minor = mc_parts[1]; // "21" oder "20" oder "19"
        let patch = mc_parts.get(2).unwrap_or(&"0"); // "2" oder "1" oder "0"

        let mut matching = Vec::new();

        // NeoForge verwendet unterschiedliche Schemas:
        // - Minecraft 1.20.2+ → NeoForge {minor}.{patch}.x (z.B. 21.1.219 für MC 1.21.1)
        // - Minecraft 1.20.1 → NICHT UNTERSTÜTZT (das war noch Forge)

        for version in all_versions {
            let is_match = if minor == "20" && *patch == "1" {
                // MC 1.20.1 wird nicht unterstützt
                false
            } else if minor.parse::<u32>().unwrap_or(0) >= 20 {
                // Moderne Versionen: NeoForge {minor}.{patch}.x
                let expected = if *patch == "0" {
                    format!("{}.0.", minor)
                } else {
                    format!("{}.{}.", minor, patch)
                };
                version.starts_with(&expected)
            } else {
                // Sehr alte Versionen (1.19.x und früher) - nicht unterstützt
                false
            };

            if is_match {
                matching.push(version.clone());
            }
        }

        matching
    }

    /// Lädt alle verfügbaren NeoForge-Versionen direkt von der Maven-Metadata
    async fn get_all_versions_from_maven(&self) -> Result<Vec<String>> {
        let maven_metadata_url = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

        let response = reqwest::get(maven_metadata_url).await?;
        let xml = response.text().await?;

        let mut all_versions: Vec<String> = Vec::new();

        // Parse die Maven-Metadata XML und sammle alle Versionen
        for line in xml.lines() {
            let line = line.trim();
            if line.starts_with("<version>") && line.ends_with("</version>") {
                let version = line.replace("<version>", "").replace("</version>", "");
                all_versions.push(version);
            }
        }

        if all_versions.is_empty() {
            bail!("Keine NeoForge-Versionen in Maven-Metadata gefunden");
        }

        tracing::debug!("Parsed {} versions from Maven metadata", all_versions.len());

        Ok(all_versions)
    }

    fn compare_neoforge_versions(a: &str, b: &str) -> std::cmp::Ordering {
        let parse = |v: &str| -> Vec<u32> {
            v.split(&['.', '-'][..])
                .filter_map(|s| s.parse::<u32>().ok())
                .collect()
        };

        let a_parts = parse(a);
        let b_parts = parse(b);

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

    /// Lädt alle unterstützten Minecraft-Versionen
    pub async fn get_supported_game_versions(&self) -> Result<Vec<String>> {
        let all_versions = self.get_all_versions_from_maven().await?;

        // Extrahiere MC-Versionen aus NeoForge-Versionsnummern.
        // NeoForge-Schema: {minor}.{patch}.x → MC 1.{minor}.{patch}
        // z.B. 21.1.219 → MC 1.21.1, 20.4.5 → MC 1.20.4
        let mut mc_versions_set = std::collections::HashSet::new();

        for version in &all_versions {
            let parts: Vec<&str> = version.split('.').collect();
            if parts.len() >= 2 {
                if let (Ok(minor), Ok(patch)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
                    // Nur Versionen ab minor=20 (MC 1.20.x+) sind echtes NeoForge
                    if minor >= 20 {
                        let mc_version = if patch == 0 {
                            format!("1.{}", minor)
                        } else {
                            format!("1.{}.{}", minor, patch)
                        };
                        mc_versions_set.insert(mc_version);
                    }
                }
            }
        }

        let mut mc_versions: Vec<String> = mc_versions_set.into_iter().collect();
        mc_versions.sort_by(|a, b| Self::compare_mc_versions(b, a)); // Neueste zuerst

        Ok(mc_versions)
    }

    fn compare_mc_versions(a: &str, b: &str) -> std::cmp::Ordering {
        let parse = |v: &str| -> Vec<u32> {
            v.trim_start_matches("1.")
                .split('.')
                .filter_map(|s| s.parse::<u32>().ok())
                .collect()
        };

        let a_parts = parse(a);
        let b_parts = parse(b);

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

    /// Generiert die Download-URL für NeoForge-Installer
    pub fn get_installer_url(&self, version: &str) -> String {
        format!(
            "{}/net/neoforged/neoforge/{}/neoforge-{}-installer.jar",
            NEOFORGE_MAVEN_URL, version, version
        )
    }

    /// Prüft ob NeoForge für eine MC-Version verfügbar ist (1.20.2+)
    pub fn is_available_for_version(mc_version: &str) -> bool {
        Self::compare_mc_versions(mc_version, "1.20.2") != std::cmp::Ordering::Less
    }
}

#[derive(Debug, Clone)]
pub struct NeoForgeVersion {
    pub version: String,
    pub mc_version: String,
    pub is_beta: bool,
    pub installer_url: String,
}


#[derive(Debug, Deserialize)]
struct NeoForgeApiResponse {
    versions: Vec<String>,
}
