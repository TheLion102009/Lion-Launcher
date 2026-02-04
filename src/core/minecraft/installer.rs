#![allow(dead_code)]

use anyhow::{Result, bail};
use serde::Deserialize;
use std::path::Path;
use crate::core::download::DownloadManager;

/// Forge/NeoForge Installer Handler
pub struct ForgeInstaller {
    download_manager: DownloadManager,
}

impl ForgeInstaller {
    pub fn new() -> Result<Self> {
        Ok(Self {
            download_manager: DownloadManager::new()?,
        })
    }

    /// Installiert Forge aus einem Installer-JAR
    pub async fn install_forge(
        &self,
        installer_jar: &Path,
        libraries_dir: &Path,
        _mc_version: &str,
    ) -> Result<ForgeInstallation> {
        tracing::info!("Processing Forge installer: {:?}", installer_jar);

        // Entpacke install_profile.json und version.json aus dem Installer
        let profile = self.extract_install_profile(installer_jar)?;

        // Lade alle Libraries aus dem Profil
        let mut classpath_entries = Vec::new();

        for lib in &profile.version_info.libraries {
            let lib_path = Self::maven_to_path(&lib.name);
            let lib_dest = libraries_dir.join(&lib_path);

            if !lib_dest.exists() {
                if let Some(downloads) = &lib.downloads {
                    if let Some(artifact) = &downloads.artifact {
                        tracing::info!("Downloading Forge library: {}", lib.name);
                        tokio::fs::create_dir_all(lib_dest.parent().unwrap()).await?;
                        self.download_manager
                            .download_with_hash(&artifact.url, &lib_dest, Some(&artifact.sha1))
                            .await?;
                    }
                } else {
                    // Versuche Standard-Maven-URLs
                    let maven_urls = vec![
                        format!("https://maven.minecraftforge.net/{}", lib_path),
                        format!("https://repo1.maven.org/maven2/{}", lib_path),
                    ];

                    let mut success = false;
                    for url in maven_urls {
                        if let Ok(_) = self.download_manager.download_with_hash(&url, &lib_dest, None).await {
                            success = true;
                            break;
                        }
                    }

                    if !success {
                        tracing::warn!("Failed to download library: {}", lib.name);
                        continue;
                    }
                }
            }

            classpath_entries.push(lib_dest.display().to_string());
        }

        Ok(ForgeInstallation {
            main_class: profile.version_info.main_class,
            classpath: classpath_entries.join(":"),
            minecraft_arguments: profile.version_info.minecraft_arguments,
        })
    }

    fn extract_install_profile(&self, installer_jar: &Path) -> Result<ForgeInstallProfile> {
        let file = std::fs::File::open(installer_jar)?;
        let mut archive = zip::ZipArchive::new(file)?;

        // Versuche zuerst install_profile.json zu lesen
        let profile_data = if let Ok(mut entry) = archive.by_name("install_profile.json") {
            let mut data = String::new();
            std::io::Read::read_to_string(&mut entry, &mut data)?;
            drop(entry); // Explizit droppen um den Borrow zu beenden
            Some(data)
        } else {
            None
        };

        // Wenn install_profile.json nicht gefunden wurde, versuche version.json
        let profile_data = if let Some(data) = profile_data {
            data
        } else if let Ok(mut entry) = archive.by_name("version.json") {
            let mut data = String::new();
            std::io::Read::read_to_string(&mut entry, &mut data)?;
            data
        } else {
            bail!("No install profile found in installer JAR");
        };

        // Parse das Profil
        if let Ok(profile) = serde_json::from_str::<ForgeInstallProfileV2>(&profile_data) {
            return Ok(ForgeInstallProfile {
                version_info: profile.version,
            });
        }

        // Fallback: Direktes VersionInfo
        let version_info: ForgeVersionInfo = serde_json::from_str(&profile_data)?;
        Ok(ForgeInstallProfile { version_info })
    }

    fn maven_to_path(maven: &str) -> String {
        let parts: Vec<&str> = maven.split(':').collect();
        if parts.len() >= 3 {
            let group = parts[0].replace('.', "/");
            let artifact = parts[1];
            let version = parts[2];
            let classifier = if parts.len() > 3 { format!("-{}", parts[3]) } else { String::new() };
            format!("{}/{}/{}/{}-{}{}.jar", group, artifact, version, artifact, version, classifier)
        } else {
            maven.to_string()
        }
    }
}

#[derive(Debug, Deserialize)]
struct ForgeInstallProfileV2 {
    version: ForgeVersionInfo,
}

#[derive(Debug, Deserialize)]
struct ForgeInstallProfile {
    #[serde(rename = "versionInfo")]
    version_info: ForgeVersionInfo,
}

#[derive(Debug, Deserialize)]
struct ForgeVersionInfo {
    #[serde(rename = "mainClass")]
    main_class: String,
    libraries: Vec<ForgeLibrary>,
    #[serde(rename = "minecraftArguments", default)]
    minecraft_arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ForgeLibrary {
    name: String,
    downloads: Option<ForgeLibraryDownloads>,
}

#[derive(Debug, Deserialize)]
struct ForgeLibraryDownloads {
    artifact: Option<ForgeArtifact>,
}

#[derive(Debug, Deserialize)]
struct ForgeArtifact {
    url: String,
    sha1: String,
}

pub struct ForgeInstallation {
    pub main_class: String,
    pub classpath: String,
    pub minecraft_arguments: Option<String>,
}
