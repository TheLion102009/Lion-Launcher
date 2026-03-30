#![allow(dead_code)]

use anyhow::Result;
use serde::Deserialize;
use crate::api::client::ApiClient;

const QUILT_META_URL: &str = "https://meta.quiltmc.org/v3";

pub struct QuiltClient {
    client: ApiClient,
}

impl QuiltClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: ApiClient::new()?,
        })
    }

    /// Lädt alle verfügbaren Quilt-Loader-Versionen für eine Minecraft-Version.
    /// Falls die Version nicht direkt unterstützt wird, wird automatisch auf die
    /// neueste unterstützte Version zurückgefallen (wie der Modrinth-Launcher).
    pub async fn get_loader_versions(&self, minecraft_version: &str) -> Result<Vec<QuiltLoaderVersion>> {
        match self.try_get_loader_versions(minecraft_version).await {
            Ok(versions) => Ok(versions),
            Err(_) => {
                // Fallback: neueste offiziell unterstützte Quilt-Game-Version verwenden
                tracing::warn!(
                    "Quilt unterstützt MC {} nicht direkt – suche neueste kompatible Version...",
                    minecraft_version
                );
                let fallback_version = self.get_latest_supported_game_version().await?;
                tracing::info!(
                    "Quilt-Fallback: Verwende Loader für MC {} (für Launch von MC {})",
                    fallback_version, minecraft_version
                );
                let versions = self.try_get_loader_versions(&fallback_version).await
                    .map_err(|e| anyhow::anyhow!(
                        "Quilt Fallback für MC {} fehlgeschlagen (Fallback-Version {}): {}",
                        minecraft_version, fallback_version, e
                    ))?;
                Ok(versions)
            }
        }
    }

    /// Interner Versuch, Loader-Versionen von der API zu laden (ohne Fallback).
    async fn try_get_loader_versions(&self, minecraft_version: &str) -> Result<Vec<QuiltLoaderVersion>> {
        let url = format!("{}/versions/loader/{}", QUILT_META_URL, minecraft_version);
        let response = self.client.get(&url).await?;

        let status = response.status();
        let text = response.text().await
            .map_err(|e| anyhow::anyhow!("Quilt API Antwort konnte nicht gelesen werden: {}", e))?;

        if status.as_u16() >= 400 {
            anyhow::bail!(
                "Quilt API HTTP {} für MC {}: {}",
                status.as_u16(), minecraft_version,
                text.chars().take(120).collect::<String>()
            );
        }

        let value: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Ungültiges JSON von Quilt API: {}", e))?;

        if let Some(obj) = value.as_object() {
            let code = obj.get("code").and_then(|v| v.as_str()).unwrap_or("unknown");
            anyhow::bail!("Quilt API Fehler für MC {}: code={}", minecraft_version, code);
        }

        if value.is_array() {
            let versions: Vec<QuiltLoaderVersion> = serde_json::from_value(value)
                .map_err(|e| anyhow::anyhow!("Quilt-Versionen konnten nicht geparst werden: {}", e))?;
            if versions.is_empty() {
                anyhow::bail!("Leere Quilt-Versionsliste für MC {}", minecraft_version);
            }
            Ok(versions)
        } else {
            anyhow::bail!(
                "Unerwartetes JSON-Format von Quilt API für MC {}",
                minecraft_version
            )
        }
    }

    /// Gibt die neueste stabile MC-Version zurück, für die Quilt verfügbar ist.
    async fn get_latest_supported_game_version(&self) -> Result<String> {
        let url = format!("{}/versions/game", QUILT_META_URL);
        let versions: Vec<QuiltGameVersion> = self.client.get_json(&url).await?;

        // Bevorzuge stabile Releases, dann neueste überhaupt
        let version = versions.iter()
            .find(|v| v.stable)
            .or_else(|| versions.first())
            .ok_or_else(|| anyhow::anyhow!("Keine unterstützten Quilt-Game-Versionen gefunden"))?;

        Ok(version.version.clone())
    }

    /// Lädt alle Minecraft-Versionen mit Quilt-Support
    pub async fn get_game_versions(&self) -> Result<Vec<QuiltGameVersion>> {
        let url = format!("{}/versions/game", QUILT_META_URL);
        let versions: Vec<QuiltGameVersion> = self.client.get_json(&url).await?;
        Ok(versions)
    }

    /// Lädt alle verfügbaren Quilt-Loader-Versionen (ohne MC-Version)
    pub async fn get_all_loader_versions(&self) -> Result<Vec<QuiltLoaderInfo>> {
        let url = format!("{}/versions/loader", QUILT_META_URL);
        let versions: Vec<QuiltLoaderInfo> = self.client.get_json(&url).await?;
        Ok(versions)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltLoaderVersion {
    pub loader: QuiltLoaderInfo,
    pub hashed: QuiltHashedInfo,
    pub intermediary: QuiltIntermediaryInfo,
    #[serde(rename = "launcherMeta")]
    pub launcher_meta: QuiltLauncherMeta,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltLoaderInfo {
    pub separator: String,
    pub build: i32,
    pub maven: String,
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltHashedInfo {
    pub maven: String,
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltIntermediaryInfo {
    pub maven: String,
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltLauncherMeta {
    pub version: i32,
    pub libraries: QuiltLibraries,
    #[serde(rename = "mainClass")]
    pub main_class: QuiltMainClass,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum QuiltMainClass {
    Simple(String),
    Map(std::collections::HashMap<String, String>),
}

impl QuiltMainClass {
    pub fn get_client_class(&self) -> String {
        match self {
            QuiltMainClass::Simple(s) => s.clone(),
            QuiltMainClass::Map(m) => m.get("client")
                .or_else(|| m.values().next())
                .cloned()
                .unwrap_or_else(|| "org.quiltmc.loader.impl.launch.knot.KnotClient".to_string()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltLibraries {
    pub client: Vec<QuiltLibrary>,
    pub common: Vec<QuiltLibrary>,
    pub server: Vec<QuiltLibrary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltLibrary {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltGameVersion {
    pub version: String,
    pub stable: bool,
}
