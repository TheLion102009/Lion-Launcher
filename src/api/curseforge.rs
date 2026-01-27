#![allow(dead_code)]

use anyhow::{Result, bail};
use serde::Deserialize;
use crate::types::mod_info::{ModInfo, ModSource, ModSearchQuery};

const CURSEFORGE_API_BASE: &str = "https://api.curseforge.com/v1";
const MINECRAFT_GAME_ID: i32 = 432;

pub struct CurseForgeClient {
    client: reqwest::Client,
    api_key: Option<String>,
}

impl CurseForgeClient {
    pub fn new(api_key: Option<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self { client, api_key })
    }

    fn check_api_key(&self) -> Result<&String> {
        self.api_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("CurseForge API key not configured"))
    }

    pub async fn search_mods(&self, query: &ModSearchQuery) -> Result<Vec<ModInfo>> {
        let api_key = self.check_api_key()?;
        
        let mut url = format!(
            "{}/mods/search?gameId={}&searchFilter={}&pageSize={}&index={}",
            CURSEFORGE_API_BASE,
            MINECRAFT_GAME_ID,
            urlencoding::encode(&query.query),
            query.limit,
            query.offset
        );

        if let Some(version) = &query.game_version {
            url.push_str(&format!("&gameVersion={}", version));
        }

        let response = self.client
            .get(&url)
            .header("x-api-key", api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            bail!("CurseForge API request failed: {}", response.status());
        }

        let cf_response: CurseForgeResponse<Vec<CurseForgeMod>> = response.json().await?;

        let mods = cf_response.data.into_iter().map(|cf_mod| ModInfo {
            id: cf_mod.id.to_string(),
            slug: cf_mod.slug,
            name: cf_mod.name,
            description: cf_mod.summary,
            icon_url: cf_mod.logo.map(|l| l.url),
            author: cf_mod.authors.first()
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
            downloads: cf_mod.download_count as u64,
            categories: cf_mod.categories.into_iter()
                .map(|c| c.name)
                .collect(),
            source: ModSource::CurseForge,
            versions: vec![],
            game_versions: cf_mod.latest_files_indexes.into_iter()
                .map(|f| f.game_version)
                .collect(),
            loaders: vec![],
            project_url: cf_mod.links.website_url,
            updated_at: cf_mod.date_modified,
        }).collect();

        Ok(mods)
    }

    pub async fn get_mod(&self, mod_id: &str) -> Result<ModInfo> {
        let api_key = self.check_api_key()?;
        let url = format!("{}/mods/{}", CURSEFORGE_API_BASE, mod_id);

        let response = self.client
            .get(&url)
            .header("x-api-key", api_key)
            .send()
            .await?;

        let cf_response: CurseForgeResponse<CurseForgeMod> = response.json().await?;
        let cf_mod = cf_response.data;

        Ok(ModInfo {
            id: cf_mod.id.to_string(),
            slug: cf_mod.slug,
            name: cf_mod.name,
            description: cf_mod.summary,
            icon_url: cf_mod.logo.map(|l| l.url),
            author: cf_mod.authors.first()
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
            downloads: cf_mod.download_count as u64,
            categories: cf_mod.categories.into_iter()
                .map(|c| c.name)
                .collect(),
            source: ModSource::CurseForge,
            versions: vec![],
            game_versions: cf_mod.latest_files_indexes.into_iter()
                .map(|f| f.game_version)
                .collect(),
            loaders: vec![],
            project_url: cf_mod.links.website_url,
            updated_at: cf_mod.date_modified,
        })
    }
}

#[derive(Debug, Deserialize)]
struct CurseForgeResponse<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeMod {
    id: i32,
    slug: String,
    name: String,
    summary: String,
    logo: Option<CurseForgeLogo>,
    authors: Vec<CurseForgeAuthor>,
    download_count: i64,
    categories: Vec<CurseForgeCategory>,
    latest_files_indexes: Vec<CurseForgeFileIndex>,
    links: CurseForgeLinks,
    date_modified: String,
}

#[derive(Debug, Deserialize)]
struct CurseForgeLogo {
    url: String,
}

#[derive(Debug, Deserialize)]
struct CurseForgeAuthor {
    name: String,
}

#[derive(Debug, Deserialize)]
struct CurseForgeCategory {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeFileIndex {
    game_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeLinks {
    website_url: String,
}
