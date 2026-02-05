#![allow(dead_code)]

use anyhow::Result;
use crate::api::{forge, neoforge};

/// Einheitliche Forge/NeoForge-Kompatibilitätsschicht
/// 
/// Diese Schicht entscheidet automatisch, ob Forge oder NeoForge
/// für eine bestimmte Minecraft-Version verwendet werden soll.
/// 
/// Regel:
/// - MC < 1.20.1: Nur Forge verfügbar
/// - MC >= 1.20.1: Beide verfügbar, aber NeoForge bevorzugt für neuere Versionen
/// - MC >= 1.21.0: NeoForge stark empfohlen (Forge läuft aus)
pub struct ForgeCompatClient {
    forge: forge::ForgeClient,
    neoforge: neoforge::NeoForgeClient,
}

impl ForgeCompatClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            forge: forge::ForgeClient::new()?,
            neoforge: neoforge::NeoForgeClient::new()?,
        })
    }

    /// Lädt alle verfügbaren Forge-kompatiblen Versionen für eine MC-Version
    /// Kombiniert Forge und NeoForge basierend auf Verfügbarkeit
    pub async fn get_all_compatible_versions(
        &self,
        minecraft_version: &str,
    ) -> Result<ForgeCompatVersions> {
        let mut forge_versions = Vec::new();
        let mut neoforge_versions = Vec::new();

        // Versuche Forge-Versionen zu laden
        if let Ok(versions) = self.forge.get_loader_versions(minecraft_version).await {
            forge_versions = versions;
        }

        // Versuche NeoForge-Versionen zu laden (nur für MC 1.20.1+)
        if neoforge::NeoForgeClient::is_available_for_version(minecraft_version) {
            if let Ok(versions) = self.neoforge.get_loader_versions(minecraft_version).await {
                neoforge_versions = versions;
            }
        }

        Ok(ForgeCompatVersions {
            minecraft_version: minecraft_version.to_string(),
            forge_versions,
            neoforge_versions,
            recommended_loader: Self::get_recommended_loader(minecraft_version),
        })
    }

    /// Gibt den empfohlenen Loader für eine MC-Version zurück
    pub fn get_recommended_loader(minecraft_version: &str) -> LoaderType {
        if Self::compare_versions(minecraft_version, "1.21.0") != std::cmp::Ordering::Less {
            // MC 1.21+: NeoForge stark empfohlen
            LoaderType::NeoForge
        } else if Self::compare_versions(minecraft_version, "1.20.1") != std::cmp::Ordering::Less {
            // MC 1.20.1-1.20.x: Beide verfügbar, NeoForge leicht bevorzugt
            LoaderType::NeoForge
        } else {
            // MC < 1.20.1: Nur Forge verfügbar
            LoaderType::Forge
        }
    }

    /// Prüft ob ein Loader für eine MC-Version verfügbar ist
    pub fn is_loader_available(loader: LoaderType, minecraft_version: &str) -> bool {
        match loader {
            LoaderType::Forge => {
                // Forge ist für die meisten Versionen verfügbar (ab MC 1.5.2)
                Self::compare_versions(minecraft_version, "1.5.2") != std::cmp::Ordering::Less
            }
            LoaderType::NeoForge => {
                // NeoForge ist ab MC 1.20.1 verfügbar
                neoforge::NeoForgeClient::is_available_for_version(minecraft_version)
            }
        }
    }

    /// Lädt alle unterstützten Minecraft-Versionen (kombiniert Forge + NeoForge)
    pub async fn get_all_supported_versions(&self) -> Result<Vec<String>> {
        let mut versions = Vec::new();

        // Lade Forge-Versionen
        if let Ok(forge_versions) = self.forge.get_supported_game_versions().await {
            versions.extend(forge_versions);
        }

        // Lade NeoForge-Versionen
        if let Ok(neoforge_versions) = self.neoforge.get_supported_game_versions().await {
            versions.extend(neoforge_versions);
        }

        // Dedupliziere und sortiere
        versions.sort_by(|a, b| Self::compare_versions(b, a));
        versions.dedup();

        Ok(versions)
    }

    /// Prüft ob Forge-Mods mit NeoForge kompatibel sind
    pub fn are_forge_mods_compatible_with_neoforge(minecraft_version: &str) -> bool {
        // NeoForge ist zu einem großen Teil rückwärtskompatibel mit Forge-Mods
        // Ab MC 1.20.1+ ist die Kompatibilität sehr hoch
        // Ab MC 1.21+ kann es Kompatibilitätsprobleme geben
        
        if Self::compare_versions(minecraft_version, "1.21.0") != std::cmp::Ordering::Less {
            // MC 1.21+: Teilweise kompatibel (Mods müssen getestet werden)
            false
        } else if Self::compare_versions(minecraft_version, "1.20.1") != std::cmp::Ordering::Less {
            // MC 1.20.1-1.20.x: Sehr gute Kompatibilität
            true
        } else {
            // MC < 1.20.1: NeoForge nicht verfügbar
            false
        }
    }

    /// Gibt Hinweise zur Migration von Forge zu NeoForge
    pub fn get_migration_info(minecraft_version: &str) -> MigrationInfo {
        if !neoforge::NeoForgeClient::is_available_for_version(minecraft_version) {
            return MigrationInfo {
                can_migrate: false,
                recommendation: "NeoForge ist für diese Minecraft-Version nicht verfügbar.".to_string(),
                compatibility_notes: vec![],
            };
        }

        let can_migrate = true;
        let recommendation = if Self::compare_versions(minecraft_version, "1.21.0") != std::cmp::Ordering::Less {
            "Für Minecraft 1.21+ wird NeoForge dringend empfohlen, da Forge hier weniger aktiv entwickelt wird.".to_string()
        } else {
            "NeoForge ist eine modernere Alternative zu Forge mit verbesserter Performance und aktiver Entwicklung.".to_string()
        };

        let compatibility_notes = vec![
            "Die meisten Forge-Mods funktionieren auch mit NeoForge.".to_string(),
            "Einige Mods benötigen möglicherweise NeoForge-spezifische Versionen.".to_string(),
            "Prüfe die Mod-Beschreibungen auf NeoForge-Kompatibilität.".to_string(),
        ];

        MigrationInfo {
            can_migrate,
            recommendation,
            compatibility_notes,
        }
    }

    fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
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
}

#[derive(Debug, Clone)]
pub struct ForgeCompatVersions {
    pub minecraft_version: String,
    pub forge_versions: Vec<forge::ForgeVersion>,
    pub neoforge_versions: Vec<neoforge::NeoForgeVersion>,
    pub recommended_loader: LoaderType,
}

impl ForgeCompatVersions {
    /// Gibt alle verfügbaren Versionen als einheitliche Liste zurück
    pub fn get_all_versions(&self) -> Vec<UnifiedLoaderVersion> {
        let mut versions = Vec::new();

        for forge in &self.forge_versions {
            versions.push(UnifiedLoaderVersion {
                loader_type: LoaderType::Forge,
                version: forge.forge_version.clone(),
                full_version: forge.full_version.clone(),
                minecraft_version: forge.mc_version.clone(),
                recommended: forge.recommended,
                is_beta: false,
                installer_url: forge.installer_url.clone(),
            });
        }

        for neoforge in &self.neoforge_versions {
            versions.push(UnifiedLoaderVersion {
                loader_type: LoaderType::NeoForge,
                version: neoforge.version.clone(),
                full_version: format!("neoforge-{}", neoforge.version),
                minecraft_version: neoforge.mc_version.clone(),
                recommended: false, // NeoForge hat keine "recommended" Kennzeichnung
                is_beta: neoforge.is_beta,
                installer_url: neoforge.installer_url.clone(),
            });
        }

        versions
    }

    /// Gibt die empfohlene Version zurück
    pub fn get_recommended_version(&self) -> Option<UnifiedLoaderVersion> {
        let all_versions = self.get_all_versions();

        // Bevorzuge den empfohlenen Loader
        match self.recommended_loader {
            LoaderType::NeoForge => {
                // Suche die neueste stabile NeoForge-Version
                all_versions.iter()
                    .filter(|v| v.loader_type == LoaderType::NeoForge && !v.is_beta)
                    .next()
                    .cloned()
            }
            LoaderType::Forge => {
                // Suche die empfohlene Forge-Version oder die neueste
                all_versions.iter()
                    .filter(|v| v.loader_type == LoaderType::Forge)
                    .find(|v| v.recommended)
                    .or_else(|| all_versions.iter().filter(|v| v.loader_type == LoaderType::Forge).next())
                    .cloned()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoaderType {
    Forge,
    NeoForge,
}

impl std::fmt::Display for LoaderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoaderType::Forge => write!(f, "Forge"),
            LoaderType::NeoForge => write!(f, "NeoForge"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnifiedLoaderVersion {
    pub loader_type: LoaderType,
    pub version: String,
    pub full_version: String,
    pub minecraft_version: String,
    pub recommended: bool,
    pub is_beta: bool,
    pub installer_url: String,
}

impl UnifiedLoaderVersion {
    pub fn display_name(&self) -> String {
        let suffix = if self.recommended {
            " (empfohlen)"
        } else if self.is_beta {
            " (beta)"
        } else {
            ""
        };
        format!("{} {}{}", self.loader_type, self.version, suffix)
    }
}

#[derive(Debug, Clone)]
pub struct MigrationInfo {
    pub can_migrate: bool,
    pub recommendation: String,
    pub compatibility_notes: Vec<String>,
}
