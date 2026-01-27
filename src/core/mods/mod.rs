#![allow(dead_code)]

use anyhow::Result;
use std::path::Path;
use crate::types::mod_info::{ModInfo, ModVersion, ModSearchQuery};
use crate::api::{modrinth::ModrinthClient, curseforge::CurseForgeClient};
use crate::core::download::DownloadManager;

pub struct ModManager {
    modrinth: ModrinthClient,
    curseforge: CurseForgeClient,
    download_manager: DownloadManager,
}

impl ModManager {
    pub fn new(curseforge_api_key: Option<String>) -> Result<Self> {
        Ok(Self {
            modrinth: ModrinthClient::new()?,
            curseforge: CurseForgeClient::new(curseforge_api_key)?,
            download_manager: DownloadManager::new()?,
        })
    }

    pub async fn search_mods(&self, query: &ModSearchQuery, use_modrinth: bool, use_curseforge: bool) -> Result<Vec<ModInfo>> {
        let mut all_mods = Vec::new();

        if use_modrinth {
            match self.modrinth.search_mods(query).await {
                Ok(mods) => all_mods.extend(mods),
                Err(e) => tracing::warn!("Modrinth search failed: {}", e),
            }
        }

        if use_curseforge {
            match self.curseforge.search_mods(query).await {
                Ok(mods) => all_mods.extend(mods),
                Err(e) => tracing::warn!("CurseForge search failed: {}", e),
            }
        }

        Ok(all_mods)
    }

    pub async fn get_mod_versions(&self, mod_info: &ModInfo) -> Result<Vec<ModVersion>> {
        match mod_info.source {
            crate::types::mod_info::ModSource::Modrinth => {
                self.modrinth.get_versions(&mod_info.id).await
            }
            crate::types::mod_info::ModSource::CurseForge => {
                // CurseForge version fetching would go here
                Ok(Vec::new())
            }
        }
    }

    pub async fn get_mod_versions_raw(&self, mod_id: &str, source: crate::types::mod_info::ModSource) -> Result<Vec<ModVersion>> {
        match source {
            crate::types::mod_info::ModSource::Modrinth => {
                self.modrinth.get_versions(mod_id).await
            }
            crate::types::mod_info::ModSource::CurseForge => {
                Ok(Vec::new())
            }
        }
    }

    pub async fn download_mod(
        &self,
        mod_version: &ModVersion,
        mods_dir: &Path,
    ) -> Result<()> {
        // Finde primary file, oder nimm das erste file
        let file = mod_version.files.iter().find(|f| f.primary)
            .or_else(|| mod_version.files.first());

        if let Some(file) = file {
            let dest = mods_dir.join(&file.filename);
            
            tracing::info!("Downloading mod file: {} to {:?}", file.filename, dest);
            tracing::info!("Download URL: {}", file.url);

            self.download_manager
                .download_with_hash(&file.url, &dest, file.hashes.sha1.as_deref())
                .await?;

            tracing::info!("✅ Mod file downloaded successfully: {:?}", dest);
        } else {
            tracing::warn!("No files found for mod version!");
            anyhow::bail!("No downloadable files found for this mod version");
        }

        Ok(())
    }

    pub async fn install_mod(
        &self,
        mod_id: &str,
        version_id: &str,
        mods_dir: &Path,
        source: crate::types::mod_info::ModSource,
    ) -> Result<()> {
        let versions = match source {
            crate::types::mod_info::ModSource::Modrinth => {
                self.modrinth.get_versions(mod_id).await?
            }
            crate::types::mod_info::ModSource::CurseForge => {
                Vec::new()
            }
        };

        if let Some(version) = versions.iter().find(|v| v.id == version_id) {
            self.download_mod(version, mods_dir).await?;
        }

        Ok(())
    }

    pub async fn uninstall_mod(&self, mod_filename: &str, mods_dir: &Path) -> Result<()> {
        let mod_path = mods_dir.join(mod_filename);
        if mod_path.exists() {
            tokio::fs::remove_file(mod_path).await?;
        }
        Ok(())
    }
}
