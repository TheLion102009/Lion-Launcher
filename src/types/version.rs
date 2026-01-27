#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftVersion {
    pub id: String,
    pub version_type: VersionType,
    pub release_time: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VersionType {
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModLoader {
    Vanilla,
    Fabric,
    Forge,
    NeoForge,
    Quilt,
}

impl ModLoader {
    pub fn as_str(&self) -> &str {
        match self {
            ModLoader::Vanilla => "vanilla",
            ModLoader::Fabric => "fabric",
            ModLoader::Forge => "forge",
            ModLoader::NeoForge => "neoforge",
            ModLoader::Quilt => "quilt",
        }
    }

    pub fn supports_mods(&self) -> bool {
        !matches!(self, ModLoader::Vanilla)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderVersion {
    pub loader: ModLoader,
    pub version: String,
    pub minecraft_version: String,
}

impl std::fmt::Display for ModLoader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
