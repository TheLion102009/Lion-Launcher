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

            // META-INF Entfernung deaktiviert - nested JARs sind wichtiger als Signatur-Konflikte
            // Die meisten Mods funktionieren auch mit Signaturen
            // if let Err(e) = remove_meta_inf(&dest).await {
            //     tracing::warn!("Failed to remove META-INF from {}: {}", file.filename, e);
            // }
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

/// Entfernt nur Signatur-Dateien aus META-INF, behält aber nested JARs und Manifests
async fn remove_meta_inf(jar_path: &Path) -> Result<()> {
    use std::io::{Read, Write};
    use zip::write::FileOptions;

    // Lese die originale JAR
    let jar_file = std::fs::File::open(jar_path)?;
    let mut archive = zip::ZipArchive::new(jar_file)?;

    // Erstelle temporäre Datei
    let temp_path = jar_path.with_extension("jar.tmp");
    let temp_file = std::fs::File::create(&temp_path)?;
    let mut zip_writer = zip::ZipWriter::new(temp_file);

    let mut removed_count = 0;
    let mut kept_count = 0;

    // Kopiere alle Dateien, aber überspringe nur Signatur-Dateien
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().to_string();

        // Überspringe NUR Signatur-Dateien, aber behalte:
        // - META-INF/jars/ (nested JARs für Fabric API Modules)
        // - META-INF/MANIFEST.MF (Mod-Metadaten)
        // - META-INF/mods.toml (Forge Mods)
        // - META-INF/neoforge.mods.toml (NeoForge Mods)
        let should_skip = name.starts_with("META-INF/") && (
            name.ends_with(".SF") ||   // Signature File
            name.ends_with(".DSA") ||  // Digital Signature
            name.ends_with(".RSA") ||  // RSA Signature
            name.ends_with(".EC")      // Elliptic Curve Signature
        );

        if should_skip {
            tracing::debug!("Removing signature file: {}", name);
            removed_count += 1;
            continue;
        }

        if name.starts_with("META-INF/jars/") {
            tracing::debug!("Keeping nested JAR: {}", name);
            kept_count += 1;
        }

        // Kopiere alle anderen Dateien
        let options = FileOptions::default()
            .compression_method(file.compression())
            .unix_permissions(file.unix_mode().unwrap_or(0o755));

        if name.ends_with('/') {
            // Ordner
            zip_writer.add_directory(&name, options)?;
        } else {
            // Datei
            zip_writer.start_file(&name, options)?;
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            zip_writer.write_all(&buffer)?;
        }
    }

    zip_writer.finish()?;
    drop(archive);

    if removed_count > 0 {
        tracing::info!("Removed {} signature files, kept {} nested JARs", removed_count, kept_count);
    }

    // Ersetze originale Datei mit bereinigter Version
    tokio::fs::remove_file(jar_path).await?;
    tokio::fs::rename(&temp_path, jar_path).await?;

    Ok(())
}

