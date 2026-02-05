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
            if let Some(neoforge_version) = Self::parse_version(&version_str) {
                versions.push(neoforge_version);
            }
        }
        
        // Sortiere nach Version (neueste zuerst)
        versions.sort_by(|a, b| Self::compare_neoforge_versions(&b.version, &a.version));
        
        Ok(versions)
    }

    fn parse_version(version_str: &str) -> Option<NeoForgeVersion> {
        // NeoForge Versionsschema:
        // - Für MC 1.20.1: "20.1.x" Format
        // - Für MC 1.20.2+: "20.2.x", "20.3.x", "20.4.x" etc.
        // - Für MC 1.21+: "21.0.x", "21.1.x" etc.
        
        // Entferne Build-Suffixes
        let clean_version = version_str
            .split('-')
            .next()
            .unwrap_or(version_str);

        let parts: Vec<&str> = clean_version.split('.').collect();
        
        if parts.is_empty() {
            return None;
        }

        // Parse major version (entspricht MC major version nach 1.)
        let major = parts[0].parse::<u32>().ok()?;
        
        // Parse minor version (entspricht MC minor version)
        let minor = if parts.len() > 1 {
            parts[1].parse::<u32>().ok()?
        } else {
            0
        };

        // Bestimme die Minecraft-Version
        let mc_version = Self::get_minecraft_version_for_neoforge(major, minor);

        Some(NeoForgeVersion {
            version: version_str.to_string(),
            mc_version,
            is_beta: version_str.contains("beta") || version_str.contains("alpha"),
            installer_url: format!(
                "{}/net/neoforged/neoforge/{}/neoforge-{}-installer.jar",
                NEOFORGE_MAVEN_URL, version_str, version_str
            ),
        })
    }

    /// Bestimmt die Minecraft-Version basierend auf der NeoForge-Version
    fn get_minecraft_version_for_neoforge(major: u32, minor: u32) -> String {
        // NeoForge Mapping:
        // 20.1.x -> 1.20.1
        // 20.2.x -> 1.20.2
        // 20.3.x -> 1.20.3
        // 20.4.x -> 1.20.4
        // 20.5.x -> 1.20.5
        // 20.6.x -> 1.20.6
        // 21.0.x -> 1.21.0 / 1.21
        // 21.1.x -> 1.21.1
        // 21.2.x -> 1.21.2
        // 21.3.x -> 1.21.3
        
        format!("1.{}.{}", major, minor)
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
        let versions = self.get_all_versions().await?;
        
        let mut mc_versions: Vec<String> = versions
            .into_iter()
            .map(|v| v.mc_version)
            .collect();
        
        mc_versions.dedup();
        
        // Sortiere nach Version
        mc_versions.sort_by(|a, b| {
            Self::compare_mc_versions(b, a) // Neueste zuerst
        });
        
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

    /// Prüft ob NeoForge für eine MC-Version verfügbar ist (1.20.1+)
    pub fn is_available_for_version(mc_version: &str) -> bool {
        Self::compare_mc_versions(mc_version, "1.20.1") != std::cmp::Ordering::Less
    }
}

#[derive(Debug, Clone)]
pub struct NeoForgeVersion {
    pub version: String,
    pub mc_version: String,
    pub is_beta: bool,
    pub installer_url: String,
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
