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

    /// Lädt alle verfügbaren Quilt-Loader-Versionen für eine Minecraft-Version
    pub async fn get_loader_versions(&self, minecraft_version: &str) -> Result<Vec<QuiltLoaderVersion>> {
        let url = format!("{}/versions/loader/{}", QUILT_META_URL, minecraft_version);
        let versions: Vec<QuiltLoaderVersion> = self.client.get_json(&url).await?;
        Ok(versions)
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
