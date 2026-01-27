#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use crate::api::client::ApiClient;
use crate::types::version::{MinecraftVersion, VersionType};

const VERSION_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

pub struct MojangClient {
    client: ApiClient,
}

impl MojangClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: ApiClient::new()?,
        })
    }

    pub async fn get_version_manifest(&self) -> Result<Vec<MinecraftVersion>> {
        let manifest: VersionManifest = self.client.get_json(VERSION_MANIFEST_URL).await?;

        let versions = manifest.versions.into_iter().map(|v| MinecraftVersion {
            id: v.id,
            version_type: match v.version_type.as_str() {
                "release" => VersionType::Release,
                "snapshot" => VersionType::Snapshot,
                "old_beta" => VersionType::OldBeta,
                "old_alpha" => VersionType::OldAlpha,
                _ => VersionType::Release,
            },
            release_time: v.release_time,
            url: Some(v.url),
        }).collect();

        Ok(versions)
    }

    pub async fn get_version_info(&self, version_url: &str) -> Result<VersionInfo> {
        let info: VersionInfo = self.client.get_json(version_url).await?;
        Ok(info)
    }
}

#[derive(Debug, Deserialize)]
struct VersionManifest {
    versions: Vec<VersionManifestEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionManifestEntry {
    id: String,
    #[serde(rename = "type")]
    version_type: String,
    release_time: String,
    url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    pub main_class: String,
    pub minecraft_arguments: Option<String>,
    pub arguments: Option<Arguments>,
    pub libraries: Vec<Library>,
    pub downloads: Downloads,
    pub asset_index: AssetIndex,
    pub assets: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Arguments {
    pub game: Vec<serde_json::Value>,
    pub jvm: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    pub downloads: Option<LibraryDownloads>,
    pub rules: Option<Vec<Rule>>,
    pub natives: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
    pub classifiers: Option<std::collections::HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub path: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: String,
    pub os: Option<OsRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Downloads {
    pub client: Download,
    pub server: Option<Download>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Download {
    pub sha1: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
    #[serde(rename = "totalSize")]
    pub total_size: u64,
}
