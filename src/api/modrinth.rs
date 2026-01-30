#![allow(dead_code)]

use anyhow::Result;
use serde::Deserialize;
use crate::api::client::ApiClient;
use crate::types::mod_info::{ModInfo, ModVersion, ModSource, ModSearchQuery, ModFile, FileHashes, ModDependency, DependencyType};

const MODRINTH_API_BASE: &str = "https://api.modrinth.com/v2";

pub struct ModrinthClient {
    client: ApiClient,
}

impl ModrinthClient {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: ApiClient::new()?,
        })
    }

    pub async fn search_mods(&self, query: &ModSearchQuery) -> Result<Vec<ModInfo>> {
        // Sortierung für Modrinth API
        let index = match query.sort_by {
            crate::types::mod_info::SortOption::Downloads => "downloads",
            crate::types::mod_info::SortOption::Updated => "updated",
            crate::types::mod_info::SortOption::Newest => "newest",
            crate::types::mod_info::SortOption::Relevance => "relevance",
        };

        let mut url = format!(
            "{}/search?query={}&limit={}&offset={}&index={}",
            MODRINTH_API_BASE,
            urlencoding::encode(&query.query),
            query.limit,
            query.offset,
            index
        );

        // Facets für Filter
        let mut facets: Vec<String> = Vec::new();

        if let Some(version) = &query.game_version {
            if !version.is_empty() {
                facets.push(format!("[\"versions:{}\"]", version));
            }
        }

        if let Some(loader) = &query.loader {
            if !loader.is_empty() {
                facets.push(format!("[\"categories:{}\"]", loader));
            }
        }

        // Categories hinzufügen (z.B. technology, adventure, etc.)
        for category in &query.categories {
            if !category.is_empty() {
                facets.push(format!("[\"categories:{}\"]", category));
            }
        }

        // Nur Mods (keine Modpacks etc.)
        facets.push("[\"project_type:mod\"]".to_string());

        if !facets.is_empty() {
            url.push_str(&format!("&facets=[{}]", facets.join(",")));
        }

        let response: ModrinthSearchResponse = self.client.get_json(&url).await?;

        let mods = response.hits.into_iter().map(|hit| ModInfo {
            id: hit.project_id,
            slug: hit.slug.clone(),
            name: hit.title,
            description: hit.description,
            icon_url: Some(hit.icon_url),
            author: hit.author,
            downloads: hit.downloads as u64,
            categories: hit.categories,
            source: ModSource::Modrinth,
            versions: hit.versions.clone(),
            game_versions: hit.versions,
            loaders: vec![],
            project_url: format!("https://modrinth.com/mod/{}", hit.slug),
            updated_at: hit.date_modified,
            client_side: hit.client_side,
            server_side: hit.server_side,
        }).collect();

        Ok(mods)
    }

    pub async fn get_mod(&self, mod_id: &str) -> Result<ModInfo> {
        let url = format!("{}/project/{}", MODRINTH_API_BASE, mod_id);
        let project: ModrinthProject = self.client.get_json(&url).await?;

        Ok(ModInfo {
            id: project.id,
            slug: project.slug.clone(),
            name: project.title,
            description: project.description,
            icon_url: project.icon_url,
            author: project.team.unwrap_or_default(),
            downloads: project.downloads as u64,
            categories: project.categories,
            source: ModSource::Modrinth,
            versions: project.versions,
            game_versions: project.game_versions,
            loaders: project.loaders,
            project_url: format!("https://modrinth.com/mod/{}", project.slug),
            updated_at: project.updated,
            client_side: project.client_side,
            server_side: project.server_side,
        })
    }

    pub async fn get_versions(&self, mod_id: &str) -> Result<Vec<ModVersion>> {
        let url = format!("{}/project/{}/version", MODRINTH_API_BASE, mod_id);
        let versions: Vec<ModrinthVersion> = self.client.get_json(&url).await?;

        let mod_versions = versions.into_iter().map(|v| ModVersion {
            id: v.id.clone(),
            mod_id: v.project_id,
            name: v.name,
            version_number: v.version_number,
            game_versions: v.game_versions,
            loaders: v.loaders,
            files: v.files.into_iter().map(|f| ModFile {
                url: f.url,
                filename: f.filename,
                primary: f.primary,
                size: f.size as u64,
                hashes: FileHashes {
                    sha1: f.hashes.sha1,
                    sha512: f.hashes.sha512,
                },
            }).collect(),
            dependencies: v.dependencies.into_iter().map(|d| ModDependency {
                mod_id: d.project_id.unwrap_or_default(),
                dependency_type: match d.dependency_type.as_str() {
                    "required" => DependencyType::Required,
                    "optional" => DependencyType::Optional,
                    "incompatible" => DependencyType::Incompatible,
                    "embedded" => DependencyType::Embedded,
                    _ => DependencyType::Optional,
                },
            }).collect(),
            published: v.date_published,
        }).collect();

        Ok(mod_versions)
    }
}

#[derive(Debug, Deserialize)]
struct ModrinthSearchResponse {
    hits: Vec<ModrinthSearchHit>,
}

#[derive(Debug, Deserialize)]
struct ModrinthSearchHit {
    project_id: String,
    slug: String,
    title: String,
    description: String,
    icon_url: String,
    author: String,
    downloads: i64,
    categories: Vec<String>,
    versions: Vec<String>,
    date_modified: String,
    #[serde(default)]
    client_side: Option<String>,
    #[serde(default)]
    server_side: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthProject {
    id: String,
    slug: String,
    title: String,
    description: String,
    icon_url: Option<String>,
    team: Option<String>,
    downloads: i64,
    categories: Vec<String>,
    versions: Vec<String>,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    updated: String,
    #[serde(default)]
    client_side: Option<String>,
    #[serde(default)]
    server_side: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthVersion {
    id: String,
    project_id: String,
    name: String,
    version_number: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    files: Vec<ModrinthFile>,
    dependencies: Vec<ModrinthDependency>,
    date_published: String,
}

#[derive(Debug, Deserialize)]
struct ModrinthFile {
    url: String,
    filename: String,
    primary: bool,
    size: i64,
    hashes: ModrinthHashes,
}

#[derive(Debug, Deserialize)]
struct ModrinthHashes {
    sha1: Option<String>,
    sha512: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModrinthDependency {
    project_id: Option<String>,
    dependency_type: String,
}
