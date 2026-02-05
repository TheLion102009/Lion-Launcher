#![allow(dead_code)]

use anyhow::{Result, bail};
use crate::api::{fabric, forge, neoforge, quilt, forge_compat};
use crate::types::version::ModLoader;
use serde::{Deserialize, Serialize};

/// Vereinheitlichte Schnittstelle für alle Mod-Loader
pub struct LoaderManager {
    fabric: fabric::FabricClient,
    forge: forge::ForgeClient,
    neoforge: neoforge::NeoForgeClient,
    forge_compat: forge_compat::ForgeCompatClient,
    quilt: quilt::QuiltClient,
}

impl LoaderManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            fabric: fabric::FabricClient::new()?,
            forge: forge::ForgeClient::new()?,
            neoforge: neoforge::NeoForgeClient::new()?,
            forge_compat: forge_compat::ForgeCompatClient::new()?,
            quilt: quilt::QuiltClient::new()?,
        })
    }

    /// Lädt alle verfügbaren Loader-Versionen für eine bestimmte Minecraft-Version
    pub async fn get_loader_versions(
        &self,
        loader: ModLoader,
        minecraft_version: &str,
    ) -> Result<Vec<LoaderVersionInfo>> {
        match loader {
            ModLoader::Vanilla => Ok(vec![LoaderVersionInfo {
                id: "vanilla".to_string(),
                version: minecraft_version.to_string(),
                stable: true,
                loader_type: ModLoader::Vanilla,
            }]),
            ModLoader::Fabric => {
                let versions = self.fabric.get_loader_versions(minecraft_version).await?;
                Ok(versions
                    .into_iter()
                    .map(|v| LoaderVersionInfo {
                        id: format!("fabric-{}", v.loader.version),
                        version: v.loader.version,
                        stable: v.loader.stable,
                        loader_type: ModLoader::Fabric,
                    })
                    .collect())
            }
            ModLoader::Forge => {
                let versions = self.forge.get_loader_versions(minecraft_version).await?;
                Ok(versions
                    .into_iter()
                    .map(|v| LoaderVersionInfo {
                        id: format!("forge-{}", v.forge_version),
                        version: v.forge_version,
                        stable: !v.full_version.contains("beta") && !v.full_version.contains("alpha"),
                        loader_type: ModLoader::Forge,
                    })
                    .collect())
            }
            ModLoader::NeoForge => {
                let versions = self.neoforge.get_loader_versions(minecraft_version).await?;
                Ok(versions
                    .into_iter()
                    .map(|v| LoaderVersionInfo {
                        id: format!("neoforge-{}", v.version),
                        version: v.version,
                        stable: !v.is_beta,
                        loader_type: ModLoader::NeoForge,
                    })
                    .collect())
            }
            ModLoader::Quilt => {
                let versions = self.quilt.get_loader_versions(minecraft_version).await?;
                Ok(versions
                    .into_iter()
                    .map(|v| LoaderVersionInfo {
                        id: format!("quilt-{}", v.loader.version),
                        version: v.loader.version,
                        stable: true, // Quilt hat keine explizite stable-Flag für Loader
                        loader_type: ModLoader::Quilt,
                    })
                    .collect())
            }
        }
    }

    /// Lädt alle kompatiblen Forge/NeoForge-Versionen für eine MC-Version
    pub async fn get_forge_compatible_versions(
        &self,
        minecraft_version: &str,
    ) -> Result<forge_compat::ForgeCompatVersions> {
        self.forge_compat.get_all_compatible_versions(minecraft_version).await
    }

    /// Gibt den empfohlenen Loader für eine MC-Version zurück (Forge vs NeoForge)
    pub fn get_recommended_forge_loader(&self, minecraft_version: &str) -> forge_compat::LoaderType {
        forge_compat::ForgeCompatClient::get_recommended_loader(minecraft_version)
    }

    /// Prüft ob Forge-Mods mit NeoForge kompatibel sind
    pub fn are_forge_mods_compatible_with_neoforge(&self, minecraft_version: &str) -> bool {
        forge_compat::ForgeCompatClient::are_forge_mods_compatible_with_neoforge(minecraft_version)
    }

    /// Gibt Migrations-Informationen von Forge zu NeoForge
    pub fn get_forge_migration_info(&self, minecraft_version: &str) -> forge_compat::MigrationInfo {
        forge_compat::ForgeCompatClient::get_migration_info(minecraft_version)
    }

    /// Lädt alle unterstützten Minecraft-Versionen für einen Loader
    pub async fn get_supported_game_versions(&self, loader: ModLoader) -> Result<Vec<String>> {
        match loader {
            ModLoader::Vanilla => {
                // Verwende Mojang API für Vanilla-Versionen
                bail!("Vanilla-Versionen sollten über die Mojang API geladen werden")
            }
            ModLoader::Fabric => {
                let versions = self.fabric.get_game_versions().await?;
                Ok(versions.into_iter().map(|v| v.version).collect())
            }
            ModLoader::Forge => {
                self.forge.get_supported_game_versions().await
            }
            ModLoader::NeoForge => {
                self.neoforge.get_supported_game_versions().await
            }
            ModLoader::Quilt => {
                let versions = self.quilt.get_game_versions().await?;
                Ok(versions.into_iter().map(|v| v.version).collect())
            }
        }
    }

    /// Lädt alle Minecraft-Versionen mit Forge oder NeoForge Support
    pub async fn get_all_forge_compatible_game_versions(&self) -> Result<Vec<String>> {
        self.forge_compat.get_all_supported_versions().await
    }

    /// Prüft, ob eine bestimmte Minecraft-Version von einem Loader unterstützt wird
    pub async fn is_version_supported(
        &self,
        loader: ModLoader,
        minecraft_version: &str,
    ) -> Result<bool> {
        let versions = self.get_supported_game_versions(loader).await?;
        Ok(versions.contains(&minecraft_version.to_string()))
    }

    /// Gibt die empfohlene Loader-Version für eine Minecraft-Version zurück
    pub async fn get_recommended_version(
        &self,
        loader: ModLoader,
        minecraft_version: &str,
    ) -> Result<Option<LoaderVersionInfo>> {
        let versions = self.get_loader_versions(loader, minecraft_version).await?;

        // Suche nach der neuesten stabilen Version
        Ok(versions
            .into_iter()
            .filter(|v| v.stable)
            .next())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderVersionInfo {
    pub id: String,
    pub version: String,
    pub stable: bool,
    pub loader_type: ModLoader,
}

impl LoaderVersionInfo {
    pub fn display_name(&self) -> String {
        format!("{} {}", self.loader_type.as_str(), self.version)
    }
}
