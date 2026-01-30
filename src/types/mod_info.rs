#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModInfo {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub author: String,
    pub downloads: u64,
    pub categories: Vec<String>,
    pub source: ModSource,
    pub versions: Vec<String>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub project_url: String,
    pub updated_at: String,
    #[serde(default)]
    pub client_side: Option<String>,
    #[serde(default)]
    pub server_side: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModSource {
    Modrinth,
    CurseForge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModVersion {
    pub id: String,
    pub mod_id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<ModFile>,
    pub dependencies: Vec<ModDependency>,
    pub published: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModFile {
    pub url: String,
    pub filename: String,
    pub primary: bool,
    pub size: u64,
    pub hashes: FileHashes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHashes {
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModDependency {
    pub mod_id: String,
    pub dependency_type: DependencyType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyType {
    Required,
    Optional,
    Incompatible,
    Embedded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModSearchQuery {
    pub query: String,
    pub game_version: Option<String>,
    pub loader: Option<String>,
    pub categories: Vec<String>,
    pub offset: u32,
    pub limit: u32,
    pub sort_by: SortOption,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortOption {
    Relevance,
    Downloads,
    Updated,
    Newest,
}

impl Default for ModSearchQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            game_version: None,
            loader: None,
            categories: Vec::new(),
            offset: 0,
            limit: 20,
            sort_by: SortOption::Relevance,
        }
    }
}
