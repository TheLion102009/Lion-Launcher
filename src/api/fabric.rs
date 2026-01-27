#![allow(dead_code)]

use anyhow::Result;
use serde::Deserialize;
use crate::api::client::ApiClient;

const FABRIC_META_URL: &str = "https://meta.fabricmc.net/v2";

pub struct FabricClient {
    client: ApiClient,
}

impl FabricClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: ApiClient::new()?,
        })
    }

    pub async fn get_loader_versions(&self, minecraft_version: &str) -> Result<Vec<FabricLoaderVersion>> {
        let url = format!("{}/versions/loader/{}", FABRIC_META_URL, minecraft_version);
        let versions: Vec<FabricLoaderVersion> = self.client.get_json(&url).await?;
        Ok(versions)
    }

    pub async fn get_game_versions(&self) -> Result<Vec<FabricGameVersion>> {
        let url = format!("{}/versions/game", FABRIC_META_URL);
        let versions: Vec<FabricGameVersion> = self.client.get_json(&url).await?;
        Ok(versions)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLoaderVersion {
    pub loader: LoaderInfo,
    pub intermediary: IntermediaryInfo,
    #[serde(rename = "launcherMeta")]
    pub launcher_meta: LauncherMeta,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoaderInfo {
    pub separator: String,
    pub build: i32,
    pub maven: String,
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IntermediaryInfo {
    pub maven: String,
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LauncherMeta {
    pub version: i32,
    pub libraries: Libraries,
    #[serde(rename = "mainClass")]
    pub main_class: MainClass,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum MainClass {
    Simple(String),
    Map(std::collections::HashMap<String, String>),
}

impl MainClass {
    pub fn get_client_class(&self) -> String {
        match self {
            MainClass::Simple(s) => s.clone(),
            MainClass::Map(m) => m.get("client")
                .or_else(|| m.values().next())
                .cloned()
                .unwrap_or_else(|| "net.fabricmc.loader.impl.launch.knot.KnotClient".to_string()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Libraries {
    pub client: Vec<FabricLibrary>,
    pub common: Vec<FabricLibrary>,
    pub server: Vec<FabricLibrary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLibrary {
    pub name: String,
    #[serde(default = "default_maven_url")]
    pub url: String,
}

fn default_maven_url() -> String {
    "https://maven.fabricmc.net/".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricGameVersion {
    pub version: String,
    pub stable: bool,
}
