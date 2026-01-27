#![allow(dead_code)]

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use serde::Deserialize;
use crate::types::profile::Profile;
use crate::core::download::DownloadManager;
use crate::config::defaults;

const MOJANG_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const RESOURCES_URL: &str = "https://resources.download.minecraft.net";

pub struct MinecraftLauncher {
    download_manager: DownloadManager,
}

#[derive(Debug, Deserialize)]
struct VersionManifest {
    versions: Vec<VersionEntry>,
}

#[derive(Debug, Deserialize)]
struct VersionEntry {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct VersionInfo {
    id: String,
    #[serde(rename = "mainClass")]
    main_class: String,
    libraries: Vec<Library>,
    downloads: GameDownloads,
    #[serde(rename = "assetIndex")]
    asset_index: AssetIndexInfo,
}

#[derive(Debug, Deserialize)]
struct Library {
    name: String,
    downloads: Option<LibraryDownloads>,
    rules: Option<Vec<Rule>>,
    natives: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct LibraryDownloads {
    artifact: Option<Artifact>,
    classifiers: Option<std::collections::HashMap<String, Artifact>>,
}

#[derive(Debug, Deserialize)]
struct Artifact {
    path: String,
    sha1: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct Rule {
    action: String,
    os: Option<OsRule>,
}

#[derive(Debug, Deserialize)]
struct OsRule {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GameDownloads {
    client: DownloadInfo,
}

#[derive(Debug, Deserialize)]
struct DownloadInfo {
    sha1: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct AssetIndexInfo {
    id: String,
    sha1: String,
    url: String,
}

#[derive(Debug, Deserialize)]
struct AssetIndex {
    objects: std::collections::HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
}

/// Konvertiert Maven-Koordinaten zu Dateipfad
fn maven_to_path(maven: &str) -> String {
    // Format: group:artifact:version -> group/artifact/version/artifact-version.jar
    let parts: Vec<&str> = maven.split(':').collect();
    if parts.len() >= 3 {
        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2];
        format!("{}/{}/{}/{}-{}.jar", group, artifact, version, artifact, version)
    } else {
        maven.to_string()
    }
}

impl MinecraftLauncher {
    pub fn new() -> Result<Self> {
        Ok(Self {
            download_manager: DownloadManager::new()?,
        })
    }

    pub async fn launch(&self, profile: &Profile, username: &str, uuid: &str, access_token: Option<&str>) -> Result<()> {
        let version = &profile.minecraft_version;
        let game_dir = Path::new(&profile.game_dir);
        let loader = &profile.loader.loader;

        tracing::info!("Preparing Minecraft {} with {:?} for {} (UUID: {})", version, loader, username, uuid);

        // Version-Info laden
        let version_info = self.get_version_info(version).await?;

        // Verzeichnisse
        let versions_dir = defaults::versions_dir();
        let libraries_dir = defaults::libraries_dir();
        let assets_dir = defaults::assets_dir();
        let natives_dir = game_dir.join("natives");

        tokio::fs::create_dir_all(&versions_dir).await?;
        tokio::fs::create_dir_all(&libraries_dir).await?;
        tokio::fs::create_dir_all(&assets_dir).await?;
        tokio::fs::create_dir_all(&natives_dir).await?;
        tokio::fs::create_dir_all(game_dir).await?;

        // Client-JAR
        let client_jar = versions_dir.join(format!("{}/{}.jar", version, version));
        if !client_jar.exists() {
            tracing::info!("Downloading client...");
            tokio::fs::create_dir_all(client_jar.parent().unwrap()).await?;
            self.download_manager
                .download_with_hash(&version_info.downloads.client.url, &client_jar, Some(&version_info.downloads.client.sha1))
                .await?;
        }

        // Libraries (Vanilla)
        tracing::info!("Checking libraries...");
        let classpath = self.download_libraries(&version_info, &libraries_dir, &natives_dir).await?;

        // Assets
        tracing::info!("Checking assets...");
        self.download_assets(&version_info.asset_index, &assets_dir).await?;

        // Mod-Loader-spezifische Konfiguration
        let (main_class, final_classpath) = match loader {
            crate::types::version::ModLoader::Fabric => {
                tracing::info!("Installing Fabric loader...");
                let (fabric_classpath, fabric_main_class) = self.install_fabric(version, &libraries_dir).await?;

                // Filter Vanilla ASM-Libraries raus (Fabric bringt eigene mit)
                let filtered_vanilla_cp: String = classpath
                    .split(':')
                    .filter(|path| !path.contains("/org/ow2/asm/"))
                    .collect::<Vec<_>>()
                    .join(":");

                tracing::info!("Filtered {} ASM entries from vanilla classpath",
                    classpath.split(':').filter(|p| p.contains("/org/ow2/asm/")).count());

                // WICHTIG: Fabric-Libraries ZUERST, dann gefilterte Vanilla-Libraries, dann Client-JAR
                let cp = format!("{}:{}:{}", fabric_classpath, filtered_vanilla_cp, client_jar.display());
                (fabric_main_class, cp)
            }
            crate::types::version::ModLoader::Quilt => {
                tracing::info!("Quilt not fully supported yet, trying Fabric-compatible mode...");
                let cp = format!("{}:{}", classpath, client_jar.display());
                (version_info.main_class.clone(), cp)
            }
            crate::types::version::ModLoader::Forge | crate::types::version::ModLoader::NeoForge => {
                tracing::warn!("Forge/NeoForge not yet fully supported, using vanilla");
                let cp = format!("{}:{}", classpath, client_jar.display());
                (version_info.main_class.clone(), cp)
            }
            crate::types::version::ModLoader::Vanilla => {
                let cp = format!("{}:{}", classpath, client_jar.display());
                (version_info.main_class.clone(), cp)
            }
        };

        // Java
        let java_path = self.find_java()?;
        let memory_mb = profile.memory_mb.unwrap_or(4096);

        let mut cmd = Command::new(&java_path);

        // JVM Args
        cmd.arg(format!("-Xmx{}M", memory_mb));
        cmd.arg(format!("-Xms{}M", memory_mb / 2));
        cmd.arg(format!("-Djava.library.path={}", natives_dir.display()));
        cmd.arg("-XX:+UseG1GC");

        // Fabric-spezifische Args
        if matches!(loader, crate::types::version::ModLoader::Fabric) {
            cmd.arg(format!("-Dfabric.gameJarPath={}", client_jar.display()));
        }

        // Classpath
        // DEBUG: Classpath ausgeben
        tracing::info!("=== CLASSPATH DEBUG ===");
        for (i, cp) in final_classpath.split(':').enumerate() {
            if cp.contains("fabric") || cp.contains("intermediary") || cp.contains("mixin") || cp.contains("asm") {
                tracing::info!("  [{}] FABRIC: {}", i, cp);
            }
        }
        tracing::info!("Classpath length: {} entries", final_classpath.split(':').count());
        tracing::info!("======================");

        // DEBUG: Classpath in Datei schreiben
        let debug_path = game_dir.join("classpath_debug.txt");
        if let Err(e) = std::fs::write(&debug_path, &final_classpath.replace(":", "\n")) {
            tracing::warn!("Failed to write classpath debug: {}", e);
        } else {
            tracing::info!("Classpath written to {:?}", debug_path);
        }

        cmd.arg("-cp").arg(&final_classpath);

        tracing::info!("Using main class: {}", main_class);
        cmd.arg(&main_class);

        // Game Args
        let token = access_token.unwrap_or("0");
        let user_type = if access_token.is_some() && token != "0" { "msa" } else { "legacy" };

        cmd.arg("--username").arg(username);
        cmd.arg("--version").arg(version);
        cmd.arg("--gameDir").arg(game_dir);
        cmd.arg("--assetsDir").arg(&assets_dir);
        cmd.arg("--assetIndex").arg(&version_info.asset_index.id);
        cmd.arg("--uuid").arg(uuid);
        cmd.arg("--accessToken").arg(token);
        cmd.arg("--userType").arg(user_type);

        tracing::info!("Launch parameters: username={}, uuid={}, userType={}, hasToken={}",
            username, uuid, user_type, access_token.is_some());

        cmd.current_dir(game_dir);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        tracing::info!("Launching Minecraft with {:?}...", loader);
        let child = cmd.spawn()?;
        tracing::info!("Started with PID: {}", child.id());

        Ok(())
    }

    /// Fabric Loader installieren und (Classpath, MainClass) zurückgeben
    async fn install_fabric(&self, mc_version: &str, libraries_dir: &Path) -> Result<(String, String)> {
        use crate::api::fabric::FabricClient;

        let fabric = FabricClient::new()?;
        let loader_versions = fabric.get_loader_versions(mc_version).await?;

        let loader = loader_versions.first()
            .ok_or_else(|| anyhow::anyhow!("No Fabric loader found for MC {}", mc_version))?;

        tracing::info!("Using Fabric loader version: {}", loader.loader.version);

        // Main-Class aus der API holen
        let main_class = loader.launcher_meta.main_class.get_client_class();
        tracing::info!("Fabric main class: {}", main_class);

        let mut classpath_entries = Vec::new();

        // Fabric Loader JAR
        let loader_maven = &loader.loader.maven;
        let loader_path = maven_to_path(loader_maven);
        let loader_url = format!("https://maven.fabricmc.net/{}", loader_path);
        let loader_dest = libraries_dir.join(&loader_path);

        if !loader_dest.exists() {
            tracing::info!("Downloading Fabric loader: {}", loader.loader.version);
            tokio::fs::create_dir_all(loader_dest.parent().unwrap()).await?;
            self.download_manager.download_with_hash(&loader_url, &loader_dest, None).await?;
        }
        classpath_entries.push(loader_dest.display().to_string());

        // Intermediary (mappings)
        let intermediary_maven = &loader.intermediary.maven;
        let intermediary_path = maven_to_path(intermediary_maven);
        let intermediary_url = format!("https://maven.fabricmc.net/{}", intermediary_path);
        let intermediary_dest = libraries_dir.join(&intermediary_path);

        if !intermediary_dest.exists() {
            tracing::info!("Downloading Fabric intermediary...");
            tokio::fs::create_dir_all(intermediary_dest.parent().unwrap()).await?;
            self.download_manager.download_with_hash(&intermediary_url, &intermediary_dest, None).await?;
        }
        classpath_entries.push(intermediary_dest.display().to_string());

        // Fabric Libraries (client + common)
        let all_libs: Vec<_> = loader.launcher_meta.libraries.client.iter()
            .chain(loader.launcher_meta.libraries.common.iter())
            .collect();

        for lib in all_libs {
            let lib_path = maven_to_path(&lib.name);

            // URL bestimmen - Fallback auf maven.fabricmc.net wenn leer
            let base_url = if lib.url.is_empty() {
                "https://maven.fabricmc.net/"
            } else {
                &lib.url
            };
            let lib_url = format!("{}{}", base_url, lib_path);
            let lib_dest = libraries_dir.join(&lib_path);

            if !lib_dest.exists() {
                tracing::info!("Downloading Fabric library: {}", lib.name);
                tokio::fs::create_dir_all(lib_dest.parent().unwrap()).await?;
                // Ignoriere Fehler bei einzelnen Libraries - manche sind optional
                if let Err(e) = self.download_manager.download_with_hash(&lib_url, &lib_dest, None).await {
                    tracing::warn!("Failed to download {}: {}, trying alternate sources...", lib.name, e);
                    // Versuche Maven Central als Fallback
                    let maven_central_url = format!("https://repo1.maven.org/maven2/{}", lib_path);
                    if let Err(e2) = self.download_manager.download_with_hash(&maven_central_url, &lib_dest, None).await {
                        tracing::warn!("Also failed from Maven Central: {}", e2);
                        continue; // Überspringe diese Library
                    }
                }
            }
            classpath_entries.push(lib_dest.display().to_string());
        }

        tracing::info!("Fabric installed with {} libraries", classpath_entries.len());
        Ok((classpath_entries.join(":"), main_class))
    }

    async fn get_version_info(&self, version: &str) -> Result<VersionInfo> {
        let manifest: VersionManifest = reqwest::get(MOJANG_MANIFEST_URL).await?.json().await?;
        let entry = manifest.versions.iter().find(|v| v.id == version)
            .ok_or_else(|| anyhow::anyhow!("Version not found: {}", version))?;
        Ok(reqwest::get(&entry.url).await?.json().await?)
    }

    async fn download_libraries(&self, info: &VersionInfo, lib_dir: &Path, natives_dir: &Path) -> Result<String> {
        let mut cp = Vec::new();
        let os = Self::get_os();

        tracing::info!("Processing {} libraries for OS: {}", info.libraries.len(), os);

        for lib in &info.libraries {
            if let Some(rules) = &lib.rules {
                if !self.check_rules(rules) {
                    tracing::debug!("Skipping {} due to rules", lib.name);
                    continue;
                }
            }

            if let Some(dl) = &lib.downloads {
                if let Some(art) = &dl.artifact {
                    let dest = lib_dir.join(&art.path);
                    if !dest.exists() {
                        tracing::info!("Downloading: {}", lib.name);
                        tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                        self.download_manager.download_with_hash(&art.url, &dest, Some(&art.sha1)).await?;
                    }
                    cp.push(dest.display().to_string());
                } else {
                    tracing::debug!("Library {} has no artifact", lib.name);
                }

                // Natives handling
                if let Some(natives) = &lib.natives {
                    if let Some(key) = natives.get(&os) {
                        if let Some(cls) = &dl.classifiers {
                            if let Some(nat) = cls.get(key) {
                                let dest = lib_dir.join(&nat.path);
                                if !dest.exists() {
                                    tracing::info!("Downloading native: {}", lib.name);
                                    tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                                    self.download_manager.download_with_hash(&nat.url, &dest, Some(&nat.sha1)).await?;
                                }
                                self.extract_native(&dest, natives_dir)?;
                            }
                        }
                    }
                }
            } else {
                tracing::debug!("Library {} has no downloads", lib.name);
            }
        }

        tracing::info!("Vanilla libraries: {} entries in classpath", cp.len());
        Ok(cp.join(":"))
    }

    async fn download_assets(&self, info: &AssetIndexInfo, assets_dir: &Path) -> Result<()> {
        let idx_dir = assets_dir.join("indexes");
        let obj_dir = assets_dir.join("objects");
        tokio::fs::create_dir_all(&idx_dir).await?;
        tokio::fs::create_dir_all(&obj_dir).await?;

        let idx_path = idx_dir.join(format!("{}.json", info.id));
        if !idx_path.exists() {
            self.download_manager.download_with_hash(&info.url, &idx_path, Some(&info.sha1)).await?;
        }

        let idx: AssetIndex = serde_json::from_str(&tokio::fs::read_to_string(&idx_path).await?)?;
        let total = idx.objects.len();
        let mut done = 0;

        for (_, asset) in &idx.objects {
            let pre = &asset.hash[..2];
            let dest = obj_dir.join(pre).join(&asset.hash);
            if !dest.exists() {
                tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                let url = format!("{}/{}/{}", RESOURCES_URL, pre, asset.hash);
                self.download_manager.download_with_hash(&url, &dest, Some(&asset.hash)).await?;
                done += 1;
                if done % 200 == 0 { tracing::info!("Assets: {}/{}", done, total); }
            }
        }
        Ok(())
    }

    fn extract_native(&self, jar: &Path, dir: &Path) -> Result<()> {
        let file = std::fs::File::open(jar)?;
        let mut archive = zip::ZipArchive::new(file)?;
        for i in 0..archive.len() {
            let mut f = archive.by_index(i)?;
            let name = f.name().to_string();
            if name.starts_with("META-INF") || name.ends_with('/') { continue; }
            if name.ends_with(".so") || name.ends_with(".dll") || name.ends_with(".dylib") {
                let dest = dir.join(Path::new(&name).file_name().unwrap());
                std::io::copy(&mut f, &mut std::fs::File::create(&dest)?)?;
            }
        }
        Ok(())
    }

    fn find_java(&self) -> Result<String> {
        if let Ok(home) = std::env::var("JAVA_HOME") {
            let p = PathBuf::from(&home).join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
            if p.exists() { return Ok(p.display().to_string()); }
        }

        let paths = if cfg!(target_os = "linux") {
            vec![
                "/usr/lib/jvm/java-21-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-17-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-21-openjdk/bin/java",
                "/usr/lib/jvm/java-17-openjdk/bin/java",
                "/usr/bin/java",
            ]
        } else {
            vec!["java"]
        };

        for p in paths {
            if Path::new(p).exists() {
                return Ok(p.to_string());
            }
        }

        if Command::new("java").arg("-version").output().is_ok() {
            return Ok("java".to_string());
        }

        bail!("Java not found! Install Java 17+")
    }

    fn get_os() -> String {
        if cfg!(target_os = "windows") { "windows" }
        else if cfg!(target_os = "macos") { "osx" }
        else { "linux" }.to_string()
    }

    fn check_rules(&self, rules: &[Rule]) -> bool {
        let os = Self::get_os();
        for r in rules {
            if let Some(o) = &r.os {
                if let Some(n) = &o.name {
                    if r.action == "allow" && n != &os { return false; }
                    if r.action == "disallow" && n == &os { return false; }
                }
            }
        }
        true
    }
}
