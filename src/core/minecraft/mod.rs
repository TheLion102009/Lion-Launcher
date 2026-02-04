#![allow(dead_code)]

mod installer;

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

/// Ergebnis einer NeoForge/Forge-Installation
struct ForgeInstallResult {
    /// Main-Class zum Starten
    main_class: String,
    /// Classpath-Einträge (normale JARs)
    classpath: Vec<String>,
    /// Modulpfad-Einträge (für Java Module System)
    module_path: Vec<String>,
    /// JVM-Argumente (--add-opens, etc.)
    jvm_args: Vec<String>,
    /// Game-Argumente (--fml.*, etc.)
    game_args: Vec<String>,
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

        // NeoForge/Forge verwendet einen speziellen Launch-Mechanismus
        if matches!(loader, crate::types::version::ModLoader::NeoForge | crate::types::version::ModLoader::Forge) {
            return self.launch_neoforge_or_forge(
                profile,
                &version_info,
                &client_jar,
                &classpath,
                &libraries_dir,
                &assets_dir,
                &natives_dir,
                game_dir,
                username,
                uuid,
                access_token
            ).await;
        }

        // Mod-Loader-spezifische Konfiguration für Fabric/Quilt/Vanilla
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

                let cp = format!("{}:{}:{}", fabric_classpath, filtered_vanilla_cp, client_jar.display());
                (fabric_main_class, cp)
            }
            crate::types::version::ModLoader::Quilt => {
                tracing::info!("Installing Quilt loader...");
                let (quilt_classpath, quilt_main_class) = self.install_quilt(version, &libraries_dir).await?;

                let filtered_vanilla_cp: String = classpath
                    .split(':')
                    .filter(|path| !path.contains("/org/ow2/asm/"))
                    .collect::<Vec<_>>()
                    .join(":");

                let cp = format!("{}:{}:{}", quilt_classpath, filtered_vanilla_cp, client_jar.display());
                (quilt_main_class, cp)
            }
            crate::types::version::ModLoader::Vanilla => {
                let cp = format!("{}:{}", classpath, client_jar.display());
                (version_info.main_class.clone(), cp)
            }
            _ => unreachable!()
        };

        // Standard-Launch für Fabric/Quilt/Vanilla
        self.launch_standard(
            profile,
            &main_class,
            &final_classpath,
            &client_jar,
            &assets_dir,
            &natives_dir,
            game_dir,
            &version_info,
            username,
            uuid,
            access_token
        ).await
    }

    /// Launch für NeoForge und Forge mit korrektem Modulpfad
    async fn launch_neoforge_or_forge(
        &self,
        profile: &Profile,
        version_info: &VersionInfo,
        client_jar: &Path,
        vanilla_classpath: &str,
        libraries_dir: &Path,
        assets_dir: &Path,
        natives_dir: &Path,
        game_dir: &Path,
        username: &str,
        uuid: &str,
        access_token: Option<&str>,
    ) -> Result<()> {
        let version = &profile.minecraft_version;
        let loader = &profile.loader.loader;
        let is_neoforge = matches!(loader, crate::types::version::ModLoader::NeoForge);

        tracing::info!("=== {} Launch ===", if is_neoforge { "NeoForge" } else { "Forge" });

        // Loader-Version auflösen
        let loader_version = if profile.loader.version == "latest" || profile.loader.version.is_empty() {
            if is_neoforge {
                self.resolve_latest_neoforge_version(version).await?
            } else {
                self.resolve_latest_forge_version(version).await?
            }
        } else {
            profile.loader.version.clone()
        };

        tracing::info!("Using loader version: {}", loader_version);

        // Installiere Loader und hole die Konfiguration
        let install_result = if is_neoforge {
            self.install_neoforge_complete(&loader_version, libraries_dir, client_jar).await?
        } else {
            self.install_forge_complete(version, &loader_version, libraries_dir, client_jar).await?
        };

        let java_path = self.find_java()?;
        let memory_mb = profile.memory_mb.unwrap_or(4096);

        let mut cmd = Command::new(&java_path);

        // Basis JVM-Argumente
        cmd.arg(format!("-Xmx{}M", memory_mb));
        cmd.arg(format!("-Xms{}M", memory_mb / 2));
        cmd.arg(format!("-Djava.library.path={}", natives_dir.display()));
        cmd.arg("-XX:+UseG1GC");
        cmd.arg("-XX:+UnlockExperimentalVMOptions");
        cmd.arg("-XX:G1NewSizePercent=20");
        cmd.arg("-XX:G1ReservePercent=20");
        cmd.arg("-XX:MaxGCPauseMillis=50");
        cmd.arg("-XX:G1HeapRegionSize=32M");

        // Module System Argumente - KRITISCH für NeoForge/Forge
        for arg in &install_result.jvm_args {
            cmd.arg(arg);
        }

        // Wenn es einen Modulpfad gibt, verwende -p
        if !install_result.module_path.is_empty() {
            tracing::info!("Using module path with {} entries", install_result.module_path.len());
            cmd.arg("-p").arg(install_result.module_path.join(":"));
            // Lade alle Module aus dem Modulpfad
            cmd.arg("--add-modules").arg("ALL-MODULE-PATH");
        }

        // Classpath (enthält nicht-modulare JARs + Minecraft Client JAR)
        // KRITISCH: Minecraft Client JAR MUSS am ANFANG des Classpaths stehen!
        // NeoForge's ModuleClassLoader sucht zuerst am Anfang des Classpaths
        let combined_classpath = if install_result.classpath.is_empty() {
            format!("{}:{}", client_jar.display(), vanilla_classpath)
        } else {
            // Minecraft Client JAR ZUERST, dann NeoForge-Libraries, dann Vanilla-Libraries
            let combined = format!("{}:{}:{}", client_jar.display(), install_result.classpath.join(":"), vanilla_classpath);
            Self::deduplicate_classpath(&combined)
        };

        cmd.arg("-cp").arg(&combined_classpath);

        // Debug: Classpath speichern
        let debug_path = game_dir.join("classpath_debug.txt");
        std::fs::write(&debug_path, combined_classpath.replace(":", "\n")).ok();

        // Main Class
        tracing::info!("Main class: {}", install_result.main_class);
        cmd.arg(&install_result.main_class);

        // KRITISCH: Game Arguments für NeoForge (--fml.*, --launchTarget, etc.)
        // Diese müssen NACH der Main-Class kommen!
        for arg in &install_result.game_args {
            cmd.arg(arg);
        }

        // launchTarget für Forge/NeoForge
        if install_result.main_class.contains("BootstrapLauncher") || install_result.main_class.contains("modlauncher") {
            // Nur hinzufügen wenn nicht bereits in game_args
            if !install_result.game_args.iter().any(|a| a.contains("launchTarget")) {
                cmd.arg("--launchTarget").arg("forgeclient");
            }
        }

        let token = access_token.unwrap_or("0");
        let user_type = if access_token.is_some() && token != "0" { "msa" } else { "legacy" };

        cmd.arg("--username").arg(username);
        cmd.arg("--version").arg(version);
        cmd.arg("--gameDir").arg(game_dir);
        cmd.arg("--assetsDir").arg(assets_dir);
        cmd.arg("--assetIndex").arg(&version_info.asset_index.id);
        cmd.arg("--uuid").arg(uuid);
        cmd.arg("--accessToken").arg(token);
        cmd.arg("--userType").arg(user_type);

        cmd.current_dir(game_dir);
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());

        tracing::info!("Launching {} {}...", if is_neoforge { "NeoForge" } else { "Forge" }, loader_version);

        let mut child = cmd.spawn()?;
        let pid = child.id();
        tracing::info!("Started with PID: {}", pid);

        tokio::spawn(async move {
            match child.wait() {
                Ok(status) => {
                    if status.success() {
                        tracing::info!("Minecraft (PID {}) exited successfully", pid);
                    } else {
                        tracing::warn!("Minecraft (PID {}) exited with status: {}", pid, status);
                    }
                }
                Err(e) => tracing::error!("Error waiting for Minecraft: {}", e),
            }
        });

        Ok(())
    }

    /// Standard-Launch für Fabric/Quilt/Vanilla
    async fn launch_standard(
        &self,
        profile: &Profile,
        main_class: &str,
        classpath: &str,
        client_jar: &Path,
        assets_dir: &Path,
        natives_dir: &Path,
        game_dir: &Path,
        version_info: &VersionInfo,
        username: &str,
        uuid: &str,
        access_token: Option<&str>,
    ) -> Result<()> {
        let java_path = self.find_java()?;
        let memory_mb = profile.memory_mb.unwrap_or(4096);
        let loader = &profile.loader.loader;

        let mut cmd = Command::new(&java_path);

        cmd.arg(format!("-Xmx{}M", memory_mb));
        cmd.arg(format!("-Xms{}M", memory_mb / 2));
        cmd.arg(format!("-Djava.library.path={}", natives_dir.display()));
        cmd.arg("-XX:+UseG1GC");

        // Loader-spezifische JVM-Args
        match loader {
            crate::types::version::ModLoader::Fabric => {
                cmd.arg(format!("-Dfabric.gameJarPath={}", client_jar.display()));
            }
            crate::types::version::ModLoader::Quilt => {
                cmd.arg(format!("-Dquilt.gameJarPath={}", client_jar.display()));
            }
            _ => {}
        }

        cmd.arg("-cp").arg(classpath);
        cmd.arg(main_class);

        let token = access_token.unwrap_or("0");
        let user_type = if access_token.is_some() && token != "0" { "msa" } else { "legacy" };

        cmd.arg("--username").arg(username);
        cmd.arg("--version").arg(&profile.minecraft_version);
        cmd.arg("--gameDir").arg(game_dir);
        cmd.arg("--assetsDir").arg(assets_dir);
        cmd.arg("--assetIndex").arg(&version_info.asset_index.id);
        cmd.arg("--uuid").arg(uuid);
        cmd.arg("--accessToken").arg(token);
        cmd.arg("--userType").arg(user_type);

        cmd.current_dir(game_dir);
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());

        tracing::info!("Launching Minecraft...");
        let mut child = cmd.spawn()?;
        let pid = child.id();

        tokio::spawn(async move {
            match child.wait() {
                Ok(status) => {
                    if status.success() {
                        tracing::info!("Minecraft (PID {}) exited successfully", pid);
                    } else {
                        tracing::warn!("Minecraft (PID {}) exited with status: {}", pid, status);
                    }
                }
                Err(e) => tracing::error!("Error waiting for Minecraft: {}", e),
            }
        });

        Ok(())
    }

    /// Entfernt doppelte Einträge aus dem Classpath
    fn deduplicate_classpath(classpath: &str) -> String {
        use std::collections::HashSet;

        let mut seen = HashSet::new();
        let mut unique_entries = Vec::new();

        for entry in classpath.split(':') {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }

            // Verwende den Dateinamen als Schlüssel für die Deduplizierung
            // So werden z.B. /path/a/lib.jar und /path/b/lib.jar als Duplikat erkannt
            let key = std::path::Path::new(entry)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.to_string());

            if seen.insert(key) {
                unique_entries.push(entry.to_string());
            } else {
                tracing::debug!("Removing duplicate classpath entry: {}", entry);
            }
        }

        let result = unique_entries.join(":");
        tracing::info!("Classpath deduplicated: {} -> {} entries",
            classpath.split(':').count(),
            unique_entries.len()
        );

        result
    }

    /// Löst die neueste NeoForge-Version für eine Minecraft-Version auf
    async fn resolve_latest_neoforge_version(&self, mc_version: &str) -> Result<String> {
        use crate::api::neoforge::NeoForgeClient;

        let client = NeoForgeClient::new()?;
        let versions = client.get_loader_versions(mc_version).await?;

        // Nehme die erste (neueste) Version
        let version = versions.first()
            .ok_or_else(|| anyhow::anyhow!("No NeoForge version found for MC {}", mc_version))?;

        Ok(version.version.clone())
    }

    /// Löst die neueste Forge-Version für eine Minecraft-Version auf
    async fn resolve_latest_forge_version(&self, mc_version: &str) -> Result<String> {
        use crate::api::forge::ForgeClient;

        let client = ForgeClient::new()?;
        let versions = client.get_loader_versions(mc_version).await?;

        // Nehme die erste (neueste) Version
        let version = versions.first()
            .ok_or_else(|| anyhow::anyhow!("No Forge version found for MC {}", mc_version))?;

        Ok(version.forge_version.clone())
    }

    /// Installiert NeoForge vollständig und gibt das Ergebnis zurück
    async fn install_neoforge_complete(&self, neoforge_version: &str, libraries_dir: &Path, client_jar: &Path) -> Result<ForgeInstallResult> {
        use crate::api::neoforge::NeoForgeClient;
        use std::io::Read;

        let neoforge_client = NeoForgeClient::new()?;
        tracing::info!("Installing NeoForge {} (complete)", neoforge_version);

        // NeoForge-Installer herunterladen
        let installer_url = neoforge_client.get_installer_url(neoforge_version);
        let installer_path = libraries_dir.join(format!("neoforge-{}-installer.jar", neoforge_version));

        if installer_path.exists() && !Self::is_valid_zip(&installer_path) {
            tokio::fs::remove_file(&installer_path).await.ok();
        }

        if !installer_path.exists() {
            tracing::info!("Downloading NeoForge installer from: {}", installer_url);
            tokio::fs::create_dir_all(installer_path.parent().unwrap()).await?;
            self.download_manager.download_with_hash(&installer_url, &installer_path, None).await?;

            if !Self::is_valid_zip(&installer_path) {
                tokio::fs::remove_file(&installer_path).await.ok();
                bail!("Downloaded NeoForge installer is corrupted");
            }
        }

        // Extrahiere version.json aus dem Installer
        let (version_json, jvm_args_json, jars_data) = {
            let file = std::fs::File::open(&installer_path)?;
            let mut archive = zip::ZipArchive::new(file)?;

            let version_json = {
                let mut entry = archive.by_name("version.json")
                    .map_err(|_| anyhow::anyhow!("version.json not found in installer"))?;
                let mut data = String::new();
                entry.read_to_string(&mut data)?;
                data
            };

            // Versuche install_profile.json zu lesen (enthält JVM args)
            let jvm_args_json = {
                if let Ok(mut entry) = archive.by_name("install_profile.json") {
                    let mut data = String::new();
                    entry.read_to_string(&mut data)?;
                    Some(data)
                } else {
                    None
                }
            };

            // Sammle JARs aus maven/
            let mut jars_data: Vec<(PathBuf, Vec<u8>)> = Vec::new();
            let mut jar_names: Vec<(String, PathBuf)> = Vec::new();

            for i in 0..archive.len() {
                if let Ok(entry) = archive.by_index(i) {
                    let name = entry.name().to_string();
                    if name.starts_with("maven/") && name.ends_with(".jar") {
                        let rel_path = name.strip_prefix("maven/").unwrap();
                        let dest = libraries_dir.join(rel_path);
                        if !dest.exists() {
                            jar_names.push((name, dest));
                        }
                    }
                }
            }

            for (name, dest) in jar_names {
                if let Ok(mut entry) = archive.by_name(&name) {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data)?;
                    jars_data.push((dest, data));
                }
            }

            (version_json, jvm_args_json, jars_data)
        };

        // Parse version.json
        #[derive(serde::Deserialize)]
        struct NeoForgeVersion {
            id: Option<String>,
            #[serde(rename = "mainClass")]
            main_class: String,
            #[serde(rename = "inheritsFrom")]
            inherits_from: Option<String>,
            libraries: Vec<NeoForgeLib>,
            arguments: Option<NeoForgeArguments>,
        }

        #[derive(serde::Deserialize)]
        struct NeoForgeArguments {
            jvm: Option<Vec<serde_json::Value>>,
            game: Option<Vec<serde_json::Value>>,
        }

        #[derive(serde::Deserialize)]
        struct NeoForgeLib {
            name: String,
            downloads: Option<NeoForgeDownloads>,
        }

        #[derive(serde::Deserialize)]
        struct NeoForgeDownloads {
            artifact: Option<NeoForgeArtifact>,
        }

        #[derive(serde::Deserialize)]
        struct NeoForgeArtifact {
            url: String,
            path: String,
            sha1: Option<String>,
        }

        let version: NeoForgeVersion = serde_json::from_str(&version_json)?;
        tracing::info!("NeoForge main class: {}", version.main_class);
        tracing::info!("NeoForge has {} libraries", version.libraries.len());

        // Extrahiere JVM-Argumente aus version.json
        let mut jvm_args = Vec::new();

        if let Some(args) = &version.arguments {
            if let Some(jvm) = &args.jvm {
                for arg in jvm {
                    if let Some(s) = arg.as_str() {
                        // Ersetze Platzhalter
                        let processed = s
                            .replace("${library_directory}", &libraries_dir.display().to_string())
                            .replace("${classpath_separator}", ":")
                            .replace("${version_name}", neoforge_version);
                        jvm_args.push(processed);
                    }
                }
            }
        }

        // Standard JVM-Args für NeoForge wenn keine vorhanden
        if jvm_args.is_empty() || !jvm_args.iter().any(|a| a.starts_with("--add-opens")) {
            jvm_args.extend(vec![
                "--add-opens=java.base/java.util.jar=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.lang.invoke=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.nio.file=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.net=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.io=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.lang=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.util=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/sun.nio.ch=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/jdk.internal.loader=ALL-UNNAMED".to_string(),
                "--add-exports=java.base/sun.security.util=ALL-UNNAMED".to_string(),
                format!("-Dminecraft.client.jar={}", client_jar.display()),
                format!("-DlibraryDirectory={}", libraries_dir.display()),
                "-DignoreList=bootstraplauncher,securejarhandler,asm-commons,asm-util,asm-analysis,asm-tree,asm,client-extra,fmlcore,javafmllanguage,lowcodelanguage,mclanguage,forge-,neoforge-".to_string(),
                "-Dfml.earlyprogresswindow=false".to_string(),
                // KRITISCH: Diese Properties teilen NeoForge mit, wo das Minecraft JAR ist
                format!("-DlegacyClassPath={}", client_jar.display()),
                // KRITISCH: Game Layer Libraries - das ist der offizielle Weg für NeoForge
                format!("-Dfml.gameLayerLibraries={}", client_jar.display()),
            ]);
        }

        // KRITISCH: Extrahiere NeoForge/MC Version-Infos aus der version.json
        // Verwende inheritsFrom wenn vorhanden (das ist die MC-Version)
        let mc_version = version.inherits_from.clone().unwrap_or_else(|| {
            // Fallback: Parse aus NeoForge-Version
            // NeoForge-Version ist im Format "21.1.77" wobei "21" = MC 1.21, "1" = Minor
            let neoforge_parts: Vec<&str> = neoforge_version.split('.').collect();
            let mc_major = neoforge_parts.get(0).unwrap_or(&"21");
            let mc_minor = neoforge_parts.get(1).unwrap_or(&"1");
            format!("1.{}.{}", mc_major, mc_minor)
        });

        tracing::info!("Detected MC version from version.json: {}", mc_version);

        // Finde FML und NeoForm Versionen aus den Libraries
        let mut fml_version = String::new();
        let mut neoform_version = String::new();

        for lib in &version.libraries {
            if lib.name.contains("fmlcore") {
                // Format: net.neoforged.fancymodloader:fmlcore:VERSION
                if let Some(v) = lib.name.split(':').last() {
                    fml_version = v.to_string();
                }
            }
            if lib.name.contains("neoform") {
                if let Some(v) = lib.name.split(':').last() {
                    neoform_version = v.to_string();
                }
            }
        }

        // Fallback-Werte wenn nicht gefunden
        if fml_version.is_empty() {
            fml_version = neoforge_version.to_string();
        }
        if neoform_version.is_empty() {
            neoform_version = format!("{}-{}", mc_version, "20240808.144430"); // Default NeoForm
        }

        tracing::info!("NeoForge versions: mc={}, neoforge={}, fml={}, neoform={}",
            mc_version, neoforge_version, fml_version, neoform_version);

        // KRITISCH: Diese Game-Argumente sind PFLICHT für NeoForge
        // Sie müssen als Programm-Argumente übergeben werden, NICHT als JVM-Argumente!
        let mut game_args = vec![
            "--launchTarget".to_string(), "forgeclient".to_string(),
            "--fml.fmlVersion".to_string(), fml_version.clone(),
            "--fml.mcVersion".to_string(), mc_version.clone(),
            "--fml.neoForgeVersion".to_string(), neoforge_version.to_string(),
            "--fml.neoFormVersion".to_string(), neoform_version.clone(),
            // KRITISCH: Registriert das Minecraft JAR als Game Layer
            "--gameJar".to_string(), client_jar.display().to_string(),
        ];

        tracing::info!("NeoForge game args: {:?}", game_args);

        // Extrahiere JARs
        for (dest, data) in jars_data {
            tracing::info!("Extracting: {:?}", dest);
            tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
            tokio::fs::write(&dest, data).await?;
        }

        // Lade fehlende Libraries
        let mut classpath = Vec::new();
        let mut module_path = Vec::new();

        for lib in &version.libraries {
            let lib_path = if let Some(downloads) = &lib.downloads {
                if let Some(artifact) = &downloads.artifact {
                    let dest = libraries_dir.join(&artifact.path);

                    if !dest.exists() && !artifact.url.is_empty() {
                        tracing::info!("Downloading: {}", lib.name);
                        tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                        self.download_manager.download_with_hash(&artifact.url, &dest, artifact.sha1.as_deref()).await.ok();
                    }
                    dest
                } else {
                    continue;
                }
            } else {
                let maven_path = Self::maven_to_path(&lib.name);
                let dest = libraries_dir.join(&maven_path);

                if !dest.exists() {
                    for url in &[
                        format!("https://maven.neoforged.net/releases/{}", maven_path),
                        format!("https://maven.minecraftforge.net/{}", maven_path),
                        format!("https://repo1.maven.org/maven2/{}", maven_path),
                        format!("https://libraries.minecraft.net/{}", maven_path),
                    ] {
                        if self.download_manager.download_with_hash(url, &dest, None).await.is_ok() {
                            tracing::info!("Downloaded {} from {}", lib.name, url);
                            break;
                        }
                    }
                }
                dest
            };

            if lib_path.exists() {
                let path_str = lib_path.display().to_string();

                // Bestimmte Libraries gehören in den Modulpfad (für Java Module System)
                // Diese sind kritisch für den BootstrapLauncher
                if lib.name.contains("bootstraplauncher") ||
                   lib.name.contains("securejarhandler") ||
                   lib.name.contains("jarjar") ||  // KRITISCH: JarJar für Mod-Isolation
                   lib.name.contains("asm") {
                    module_path.push(path_str);
                } else {
                    classpath.push(path_str);
                }
            }
        }

        // KRITISCH: NeoForge Universal JAR - enthält die NeoForge Core-Mod
        let neoforge_universal_path = libraries_dir.join(format!("net/neoforged/neoforge/{}/neoforge-{}-universal.jar", neoforge_version, neoforge_version));
        if !neoforge_universal_path.exists() {
            tracing::info!("Downloading NeoForge universal JAR");
            tokio::fs::create_dir_all(neoforge_universal_path.parent().unwrap()).await.ok();

            let neoforge_universal_url = format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{}/neoforge-{}-universal.jar", neoforge_version, neoforge_version);
            if let Err(e) = self.download_manager.download_with_hash(&neoforge_universal_url, &neoforge_universal_path, None).await {
                tracing::error!("Failed to download NeoForge universal: {}", e);
            } else {
                tracing::info!("Successfully downloaded NeoForge universal JAR");
                // Füge es zum Classpath hinzu (nicht Modulpfad)
                classpath.push(neoforge_universal_path.display().to_string());
            }
        } else {
            // Sicherstellen dass es im Classpath ist
            if !classpath.iter().any(|p| p.contains("neoforge") && p.contains("universal")) {
                tracing::info!("Adding existing NeoForge universal to classpath");
                classpath.push(neoforge_universal_path.display().to_string());
            }
        }

        // KRITISCH: JarJarFileSystems ist oft nicht in der version.json, aber wird benötigt!
        // Wir müssen es manuell herunterladen, da JarJarSelector/JarJarMetadata es zur Laufzeit brauchen
        let jarjar_filesystems_path = libraries_dir.join("net/neoforged/JarJarFileSystems/0.4.1/JarJarFileSystems-0.4.1.jar");
        if !jarjar_filesystems_path.exists() {
            tracing::info!("Downloading critical missing library: JarJarFileSystems");
            tokio::fs::create_dir_all(jarjar_filesystems_path.parent().unwrap()).await.ok();

            let jarjar_url = "https://maven.neoforged.net/releases/net/neoforged/JarJarFileSystems/0.4.1/JarJarFileSystems-0.4.1.jar";
            if let Err(e) = self.download_manager.download_with_hash(jarjar_url, &jarjar_filesystems_path, None).await {
                tracing::error!("Failed to download JarJarFileSystems: {}", e);
            } else {
                tracing::info!("Successfully downloaded JarJarFileSystems");
                // Füge es zum Modulpfad hinzu
                module_path.push(jarjar_filesystems_path.display().to_string());
            }
        } else {
            // Sicherstellen dass es im Modulpfad ist wenn es existiert
            if !module_path.iter().any(|p| p.contains("JarJarFileSystems")) {
                tracing::info!("Adding existing JarJarFileSystems to module path");
                module_path.push(jarjar_filesystems_path.display().to_string());
            }
        }

        tracing::info!("NeoForge complete: {} classpath, {} module path, {} jvm args, {} game args",
            classpath.len(), module_path.len(), jvm_args.len(), game_args.len());

        Ok(ForgeInstallResult {
            main_class: version.main_class,
            classpath,
            module_path,
            jvm_args,
            game_args,
        })
    }

    /// Installiert Forge vollständig und gibt das Ergebnis zurück
    async fn install_forge_complete(&self, mc_version: &str, forge_version: &str, libraries_dir: &Path, client_jar: &Path) -> Result<ForgeInstallResult> {
        use crate::api::forge::ForgeClient;
        use std::io::Read;

        let forge_client = ForgeClient::new()?;
        tracing::info!("Installing Forge {}-{} (complete)", mc_version, forge_version);

        let installer_url = forge_client.get_installer_url(mc_version, forge_version);
        let installer_path = libraries_dir.join(format!("forge-{}-{}-installer.jar", mc_version, forge_version));

        if installer_path.exists() && !Self::is_valid_zip(&installer_path) {
            tokio::fs::remove_file(&installer_path).await.ok();
        }

        if !installer_path.exists() {
            tracing::info!("Downloading Forge installer from: {}", installer_url);
            tokio::fs::create_dir_all(installer_path.parent().unwrap()).await?;
            self.download_manager.download_with_hash(&installer_url, &installer_path, None).await?;
        }

        // Extrahiere version.json
        let (version_json, jars_data) = {
            let file = std::fs::File::open(&installer_path)?;
            let mut archive = zip::ZipArchive::new(file)?;

            let version_json = {
                if let Ok(mut entry) = archive.by_name("version.json") {
                    let mut data = String::new();
                    entry.read_to_string(&mut data)?;
                    Some(data)
                } else {
                    None
                }
            };

            let mut jars_data: Vec<(PathBuf, Vec<u8>)> = Vec::new();
            let mut jar_names: Vec<(String, PathBuf)> = Vec::new();

            for i in 0..archive.len() {
                if let Ok(entry) = archive.by_index(i) {
                    let name = entry.name().to_string();
                    if name.starts_with("maven/") && name.ends_with(".jar") {
                        let rel_path = name.strip_prefix("maven/").unwrap();
                        let dest = libraries_dir.join(rel_path);
                        if !dest.exists() {
                            jar_names.push((name, dest));
                        }
                    }
                }
            }

            for (name, dest) in jar_names {
                if let Ok(mut entry) = archive.by_name(&name) {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data)?;
                    jars_data.push((dest, data));
                }
            }

            (version_json, jars_data)
        };

        let version_json = version_json.ok_or_else(|| anyhow::anyhow!("version.json not found"))?;

        #[derive(serde::Deserialize)]
        struct ForgeVersion {
            #[serde(rename = "mainClass")]
            main_class: String,
            libraries: Vec<ForgeLib>,
            arguments: Option<ForgeArguments>,
        }

        #[derive(serde::Deserialize)]
        struct ForgeArguments {
            jvm: Option<Vec<serde_json::Value>>,
        }

        #[derive(serde::Deserialize)]
        struct ForgeLib {
            name: String,
            downloads: Option<ForgeDownloads>,
        }

        #[derive(serde::Deserialize)]
        struct ForgeDownloads {
            artifact: Option<ForgeArtifact>,
        }

        #[derive(serde::Deserialize)]
        struct ForgeArtifact {
            url: String,
            path: String,
            sha1: Option<String>,
        }

        let version: ForgeVersion = serde_json::from_str(&version_json)?;
        tracing::info!("Forge main class: {}", version.main_class);

        // JVM args
        let mut jvm_args = Vec::new();

        if let Some(args) = &version.arguments {
            if let Some(jvm) = &args.jvm {
                for arg in jvm {
                    if let Some(s) = arg.as_str() {
                        let processed = s
                            .replace("${library_directory}", &libraries_dir.display().to_string())
                            .replace("${classpath_separator}", ":")
                            .replace("${version_name}", forge_version);
                        jvm_args.push(processed);
                    }
                }
            }
        }

        if jvm_args.is_empty() {
            jvm_args.extend(vec![
                "--add-opens=java.base/java.util.jar=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.lang.invoke=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.nio.file=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.io=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.lang=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/sun.nio.ch=ALL-UNNAMED".to_string(),
                "--add-exports=java.base/sun.security.util=ALL-UNNAMED".to_string(),
                format!("-Dminecraft.client.jar={}", client_jar.display()),
                format!("-DlibraryDirectory={}", libraries_dir.display()),
                "-DignoreList=bootstraplauncher,securejarhandler,asm-commons,asm-util,asm-analysis,asm-tree,asm,client-extra,fmlcore,javafmllanguage,lowcodelanguage,mclanguage,forge-".to_string(),
                "-Dfml.earlyprogresswindow=false".to_string(),
            ]);
        }

        // Extrahiere JARs
        for (dest, data) in jars_data {
            tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
            tokio::fs::write(&dest, data).await?;
        }

        // Lade Libraries
        let mut classpath = Vec::new();
        let mut module_path = Vec::new();

        for lib in &version.libraries {
            let lib_path = if let Some(downloads) = &lib.downloads {
                if let Some(artifact) = &downloads.artifact {
                    let dest = libraries_dir.join(&artifact.path);
                    if !dest.exists() && !artifact.url.is_empty() {
                        tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                        self.download_manager.download_with_hash(&artifact.url, &dest, artifact.sha1.as_deref()).await.ok();
                    }
                    dest
                } else {
                    continue;
                }
            } else {
                let maven_path = Self::maven_to_path(&lib.name);
                let dest = libraries_dir.join(&maven_path);
                if !dest.exists() {
                    for url in &[
                        format!("https://maven.minecraftforge.net/{}", maven_path),
                        format!("https://repo1.maven.org/maven2/{}", maven_path),
                        format!("https://libraries.minecraft.net/{}", maven_path),
                    ] {
                        if self.download_manager.download_with_hash(url, &dest, None).await.is_ok() {
                            break;
                        }
                    }
                }
                dest
            };

            if lib_path.exists() {
                let path_str = lib_path.display().to_string();
                if lib.name.contains("bootstraplauncher") ||
                   lib.name.contains("securejarhandler") ||
                   lib.name.contains("jarjar") {
                    module_path.push(path_str);
                } else {
                    classpath.push(path_str);
                }
            }
        }

        // Forge verwendet auch Game-Args für --launchTarget
        let game_args = vec![
            "--launchTarget".to_string(),
            "forgeclient".to_string(),
        ];

        Ok(ForgeInstallResult {
            main_class: version.main_class,
            classpath,
            module_path,
            jvm_args,
            game_args,
        })
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

    /// Quilt Loader installieren und (Classpath, MainClass) zurückgeben
    async fn install_quilt(&self, mc_version: &str, libraries_dir: &Path) -> Result<(String, String)> {
        use crate::api::quilt::QuiltClient;

        let quilt = QuiltClient::new()?;
        let loader_versions = quilt.get_loader_versions(mc_version).await?;

        let loader = loader_versions.first()
            .ok_or_else(|| anyhow::anyhow!("No Quilt loader found for MC {}", mc_version))?;

        tracing::info!("Using Quilt loader version: {}", loader.loader.version);

        // Main-Class aus der API holen
        let main_class = loader.launcher_meta.main_class.get_client_class();
        tracing::info!("Quilt main class: {}", main_class);

        let mut classpath_entries = Vec::new();

        // Quilt Loader JAR
        let loader_maven = &loader.loader.maven;
        let loader_path = maven_to_path(loader_maven);
        let loader_url = format!("https://maven.quiltmc.org/repository/release/{}", loader_path);
        let loader_dest = libraries_dir.join(&loader_path);

        if !loader_dest.exists() {
            tracing::info!("Downloading Quilt loader: {}", loader.loader.version);
            tokio::fs::create_dir_all(loader_dest.parent().unwrap()).await?;
            self.download_manager.download_with_hash(&loader_url, &loader_dest, None).await?;
        }
        classpath_entries.push(loader_dest.display().to_string());

        // Hashed (Quilt mappings)
        let hashed_maven = &loader.hashed.maven;
        let hashed_path = maven_to_path(hashed_maven);
        let hashed_url = format!("https://maven.quiltmc.org/repository/release/{}", hashed_path);
        let hashed_dest = libraries_dir.join(&hashed_path);

        if !hashed_dest.exists() {
            tracing::info!("Downloading Quilt hashed...");
            tokio::fs::create_dir_all(hashed_dest.parent().unwrap()).await?;
            self.download_manager.download_with_hash(&hashed_url, &hashed_dest, None).await?;
        }
        classpath_entries.push(hashed_dest.display().to_string());

        // Intermediary
        let intermediary_maven = &loader.intermediary.maven;
        let intermediary_path = maven_to_path(intermediary_maven);
        let intermediary_url = format!("https://maven.quiltmc.org/repository/release/{}", intermediary_path);
        let intermediary_dest = libraries_dir.join(&intermediary_path);

        if !intermediary_dest.exists() {
            tracing::info!("Downloading Quilt intermediary...");
            tokio::fs::create_dir_all(intermediary_dest.parent().unwrap()).await?;
            self.download_manager.download_with_hash(&intermediary_url, &intermediary_dest, None).await?;
        }
        classpath_entries.push(intermediary_dest.display().to_string());

        // Quilt Libraries (client + common)
        let all_libs: Vec<_> = loader.launcher_meta.libraries.client.iter()
            .chain(loader.launcher_meta.libraries.common.iter())
            .collect();

        for lib in all_libs {
            let lib_path = maven_to_path(&lib.name);
            let lib_url = format!("{}{}", lib.url, lib_path);
            let lib_dest = libraries_dir.join(&lib_path);

            if !lib_dest.exists() {
                tracing::info!("Downloading Quilt library: {}", lib.name);
                tokio::fs::create_dir_all(lib_dest.parent().unwrap()).await?;
                if let Err(e) = self.download_manager.download_with_hash(&lib_url, &lib_dest, None).await {
                    tracing::warn!("Failed to download {}: {}, trying alternate sources...", lib.name, e);
                    let maven_central_url = format!("https://repo1.maven.org/maven2/{}", lib_path);
                    if let Err(e2) = self.download_manager.download_with_hash(&maven_central_url, &lib_dest, None).await {
                        tracing::warn!("Also failed from Maven Central: {}", e2);
                        continue;
                    }
                }
            }
            classpath_entries.push(lib_dest.display().to_string());
        }

        tracing::info!("Quilt installed with {} libraries", classpath_entries.len());
        Ok((classpath_entries.join(":"), main_class))
    }

    /// Forge Loader installieren und (Classpath, MainClass) zurückgeben
    async fn install_forge(&self, mc_version: &str, forge_version: &str, libraries_dir: &Path) -> Result<(String, String)> {
        use crate::api::forge::ForgeClient;

        let forge_client = ForgeClient::new()?;

        tracing::info!("Installing Forge {}-{}", mc_version, forge_version);

        // Forge-Installer herunterladen
        let installer_url = forge_client.get_installer_url(mc_version, forge_version);
        let installer_path = libraries_dir.join(format!("forge-{}-{}-installer.jar", mc_version, forge_version));

        // Prüfe ob existierende Datei gültig ist
        if installer_path.exists() {
            if !Self::is_valid_zip(&installer_path) {
                tracing::warn!("Existing Forge installer is corrupted, re-downloading...");
                tokio::fs::remove_file(&installer_path).await.ok();
            }
        }

        if !installer_path.exists() {
            tracing::info!("Downloading Forge installer from: {}", installer_url);
            tokio::fs::create_dir_all(installer_path.parent().unwrap()).await?;

            // Versuche den Download
            if let Err(e) = self.download_manager.download_with_hash(&installer_url, &installer_path, None).await {
                tracing::error!("Failed to download Forge installer: {}", e);
                tracing::warn!("Forge/NeoForge support is currently limited. Please try Fabric or Quilt for best results.");
                bail!("Forge installer not available. Try Fabric instead, which has better mod compatibility.");
            }

            // Validiere das heruntergeladene JAR
            if !Self::is_valid_zip(&installer_path) {
                tokio::fs::remove_file(&installer_path).await.ok();
                bail!("Downloaded Forge installer is corrupted. Please try again or use a different version.");
            }
        }

        // Extrahiere Libraries direkt aus dem Installer JAR (gleiche Methode wie NeoForge)
        let (classpath_entries, main_class) = self.extract_forge_libraries(&installer_path, libraries_dir, mc_version).await?;

        if !classpath_entries.is_empty() {
            tracing::info!("Forge installed successfully with {} libraries", classpath_entries.len());
            return Ok((classpath_entries.join(":"), main_class));
        }

        // Fallback: Versuche den Installer
        let installer = crate::core::minecraft::installer::ForgeInstaller::new()?;
        match installer.install_forge(&installer_path, libraries_dir, mc_version).await {
            Ok(installation) => {
                tracing::info!("Forge installed successfully via installer");
                return Ok((installation.classpath, installation.main_class));
            }
            Err(e) => {
                tracing::warn!("Full installer support failed: {}, using simplified mode", e);
            }
        }

        // Fallback: Vereinfachte Version
        tracing::warn!("Using simplified Forge installation - may not work for all versions");

        // Forge Main-Class (Standard für moderne Versionen)
        let main_class = if mc_version >= "1.13" {
            "cpw.mods.modlauncher.Launcher".to_string()
        } else {
            "net.minecraft.launchwrapper.Launch".to_string()
        };

        tracing::info!("Using Forge main class: {}", main_class);

        // Gebe nur den Installer-Pfad zurück - Minecraft wird versuchen, ihn selbst zu verwenden
        Ok((installer_path.display().to_string(), main_class))
    }

    /// Extrahiert Forge Libraries aus dem Installer
    async fn extract_forge_libraries(&self, installer_jar: &Path, libraries_dir: &Path, _mc_version: &str) -> Result<(Vec<String>, String)> {
        use std::io::Read;

        // Alle ZIP-Operationen synchron ausführen und Daten sammeln
        let (version_json, jars_data) = {
            let file = std::fs::File::open(installer_jar)?;
            let mut archive = zip::ZipArchive::new(file)?;

            // Versuche zuerst version.json zu lesen
            let version_json = {
                let result = archive.by_name("version.json");
                if let Ok(mut entry) = result {
                    let mut data = String::new();
                    entry.read_to_string(&mut data)?;
                    Some(data)
                } else {
                    None
                }
            };

            // Sammle alle JAR-Daten aus dem maven/ Verzeichnis
            let mut jars_data: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();

            // Erst alle Namen sammeln
            let mut jar_names: Vec<(String, std::path::PathBuf)> = Vec::new();
            for i in 0..archive.len() {
                if let Ok(entry) = archive.by_index(i) {
                    let name = entry.name().to_string();
                    if name.starts_with("maven/") && name.ends_with(".jar") {
                        let rel_path = name.strip_prefix("maven/").unwrap();
                        let dest = libraries_dir.join(rel_path);
                        if !dest.exists() {
                            jar_names.push((name, dest));
                        }
                    }
                }
            }

            // Dann die Daten extrahieren
            for (name, dest) in jar_names {
                if let Ok(mut entry) = archive.by_name(&name) {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data)?;
                    jars_data.push((dest, data));
                }
            }

            (version_json, jars_data)
        };

        let version_json = match version_json {
            Some(v) => v,
            None => {
                tracing::warn!("version.json not found in Forge installer");
                return Ok((Vec::new(), String::new()));
            }
        };

        #[derive(serde::Deserialize)]
        struct ForgeVersion {
            #[serde(rename = "mainClass")]
            main_class: String,
            libraries: Vec<ForgeLib>,
        }

        #[derive(serde::Deserialize)]
        struct ForgeLib {
            name: String,
            downloads: Option<ForgeDownloads>,
        }

        #[derive(serde::Deserialize)]
        struct ForgeDownloads {
            artifact: Option<ForgeArtifact>,
        }

        #[derive(serde::Deserialize)]
        struct ForgeArtifact {
            url: String,
            path: String,
            sha1: Option<String>,
        }

        let version: ForgeVersion = serde_json::from_str(&version_json)?;
        tracing::info!("Forge main class: {}", version.main_class);
        tracing::info!("Forge has {} libraries", version.libraries.len());

        let mut classpath_entries = Vec::new();

        // Schreibe die extrahierten JARs (jetzt asynchron sicher)
        for (dest, data) in jars_data {
            tracing::info!("Extracting: {:?}", dest);
            tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
            tokio::fs::write(&dest, data).await?;
            classpath_entries.push(dest.display().to_string());
        }

        // Downloade fehlende Libraries
        for lib in &version.libraries {
            if let Some(downloads) = &lib.downloads {
                if let Some(artifact) = &downloads.artifact {
                    let dest = libraries_dir.join(&artifact.path);

                    if !dest.exists() && !artifact.url.is_empty() {
                        tracing::info!("Downloading Forge library: {}", lib.name);
                        tokio::fs::create_dir_all(dest.parent().unwrap()).await?;

                        if let Err(e) = self.download_manager.download_with_hash(
                            &artifact.url,
                            &dest,
                            artifact.sha1.as_deref()
                        ).await {
                            tracing::warn!("Failed to download {}: {}", lib.name, e);
                            continue;
                        }
                    }

                    if dest.exists() {
                        classpath_entries.push(dest.display().to_string());
                    }
                }
            } else {
                // Versuche Standard-Maven-Pfad
                let lib_path = Self::maven_to_path(&lib.name);
                let dest = libraries_dir.join(&lib_path);

                if !dest.exists() {
                    let maven_urls = vec![
                        format!("https://maven.minecraftforge.net/{}", lib_path),
                        format!("https://repo1.maven.org/maven2/{}", lib_path),
                        format!("https://libraries.minecraft.net/{}", lib_path),
                    ];

                    for url in maven_urls {
                        if self.download_manager.download_with_hash(&url, &dest, None).await.is_ok() {
                            tracing::info!("Downloaded {} from {}", lib.name, url);
                            break;
                        }
                    }
                }

                if dest.exists() {
                    classpath_entries.push(dest.display().to_string());
                }
            }
        }

        Ok((classpath_entries, version.main_class))
    }

    /// NeoForge Loader installieren und (Classpath, MainClass) zurückgeben
    async fn install_neoforge(&self, neoforge_version: &str, libraries_dir: &Path) -> Result<(String, String)> {
        use crate::api::neoforge::NeoForgeClient;

        let neoforge_client = NeoForgeClient::new()?;

        tracing::info!("Installing NeoForge {}", neoforge_version);

        // WARNUNG: NeoForge 1.21+ hat architektonische Änderungen die nicht mit unserem Launcher kompatibel sind
        tracing::warn!("=== NeoForge Compatibility Warning ===");
        tracing::warn!("NeoForge for Minecraft 1.21+ uses BootstrapLauncher which requires Java Module System");
        tracing::warn!("This is not fully compatible with custom launchers.");
        tracing::warn!("Recommended alternatives: Fabric or Quilt");
        tracing::warn!("For Minecraft 1.20.x and earlier, NeoForge should work fine.");
        tracing::warn!("=====================================");

        // NeoForge-Installer herunterladen
        let installer_url = neoforge_client.get_installer_url(neoforge_version);
        let installer_path = libraries_dir.join(format!("neoforge-{}-installer.jar", neoforge_version));

        // Prüfe ob existierende Datei gültig ist
        if installer_path.exists() {
            if !Self::is_valid_zip(&installer_path) {
                tracing::warn!("Existing NeoForge installer is corrupted, re-downloading...");
                tokio::fs::remove_file(&installer_path).await.ok();
            }
        }

        if !installer_path.exists() {
            tracing::info!("Downloading NeoForge installer from: {}", installer_url);
            tokio::fs::create_dir_all(installer_path.parent().unwrap()).await?;

            // Versuche den Download
            if let Err(e) = self.download_manager.download_with_hash(&installer_url, &installer_path, None).await {
                tracing::error!("Failed to download NeoForge installer: {}", e);
                tracing::warn!("NeoForge version {} may not be available yet", neoforge_version);
                bail!("NeoForge installer not available. This version might not exist or the server is unreachable. Try Fabric or Quilt instead.");
            }

            // Validiere das heruntergeladene JAR
            if !Self::is_valid_zip(&installer_path) {
                tokio::fs::remove_file(&installer_path).await.ok();
                bail!("Downloaded NeoForge installer is corrupted. Please try again or use a different version.");
            }
        }

        // NEUER ANSATZ: Führe den Installer tatsächlich aus
        tracing::info!("Running NeoForge installer to create proper client profile...");
        let install_result = self.run_neoforge_installer(&installer_path, libraries_dir).await;

        if let Ok((classpath, main_class)) = install_result {
            tracing::info!("NeoForge installer completed successfully");
            return Ok((classpath, main_class));
        } else {
            tracing::warn!("Installer execution failed, falling back to manual extraction");
        }

        // Fallback: Extrahiere Libraries direkt aus dem Installer JAR
        let (classpath_entries, main_class) = self.extract_neoforge_libraries(&installer_path, libraries_dir).await?;

        tracing::info!("NeoForge library extraction complete: {} entries", classpath_entries.len());

        if classpath_entries.is_empty() {
            tracing::error!("No NeoForge libraries were extracted! This will likely cause startup failures.");
            bail!("Failed to extract NeoForge libraries from installer. The installer may be incompatible or corrupted.");
        }

        if !classpath_entries.is_empty() {
            tracing::info!("NeoForge installed successfully with {} libraries", classpath_entries.len());
            // Debug: Log erste paar Einträge
            for (i, entry) in classpath_entries.iter().take(5).enumerate() {
                tracing::debug!("  Library {}: {}", i+1, entry);
            }
            return Ok((classpath_entries.join(":"), main_class));
        }

        // Letzte Fallback-Option
        let installer = crate::core::minecraft::installer::ForgeInstaller::new()?;
        match installer.install_forge(&installer_path, libraries_dir, "neoforge").await {
            Ok(installation) => {
                tracing::info!("NeoForge installed successfully via installer");
                return Ok((installation.classpath, installation.main_class));
            }
            Err(e) => {
                tracing::warn!("Full installer support failed: {}, using simplified mode", e);
            }
        }

        // Absolute Fallback
        tracing::error!("All NeoForge installation methods failed!");
        bail!("NeoForge installation failed. This version may not be supported. Try Fabric or Quilt instead.");
    }

    /// Führt den NeoForge-Installer aus um ein ordnungsgemäßes Client-Profil zu erstellen
    async fn run_neoforge_installer(&self, _installer_jar: &Path, _libraries_dir: &Path) -> Result<(String, String)> {
        // NeoForge 1.21+ Installer hat das gleiche BootstrapLauncher-Problem
        // Wir können den Installer nicht direkt ausführen
        tracing::warn!("NeoForge 1.21+ installer cannot be executed directly due to BootstrapLauncher issues");
        tracing::warn!("This is a known limitation - NeoForge 1.21+ is not fully supported. Use Fabric or Quilt instead.");

        bail!("NeoForge installer execution not supported for 1.21+. Falling back to library extraction.");
    }

    /// Prüft ob eine Datei ein gültiges ZIP-Archiv ist
    fn is_valid_zip(path: &Path) -> bool {
        match std::fs::File::open(path) {
            Ok(file) => {
                match zip::ZipArchive::new(file) {
                    Ok(_) => true,
                    Err(e) => {
                        tracing::warn!("Invalid ZIP file {:?}: {}", path, e);
                        false
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Cannot open file {:?}: {}", path, e);
                false
            }
        }
    }

    /// Extrahiert NeoForge Libraries aus dem Installer
    async fn extract_neoforge_libraries(&self, installer_jar: &Path, libraries_dir: &Path) -> Result<(Vec<String>, String)> {
        use std::io::Read;

        // Alle ZIP-Operationen synchron ausführen und Daten sammeln
        let (version_json, jars_data) = {
            let file = std::fs::File::open(installer_jar)?;
            let mut archive = zip::ZipArchive::new(file)?;

            // Lese version.json aus dem Installer
            let version_json = {
                let mut entry = archive.by_name("version.json")
                    .map_err(|_| anyhow::anyhow!("version.json not found in installer"))?;
                let mut data = String::new();
                entry.read_to_string(&mut data)?;
                data
            };

            // Sammle alle JAR-Daten aus dem maven/ Verzeichnis
            let mut jars_data: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();

            // Erst alle Namen sammeln
            let mut jar_names: Vec<(String, std::path::PathBuf)> = Vec::new();
            for i in 0..archive.len() {
                if let Ok(entry) = archive.by_index(i) {
                    let name = entry.name().to_string();
                    if name.starts_with("maven/") && name.ends_with(".jar") {
                        let rel_path = name.strip_prefix("maven/").unwrap();
                        let dest = libraries_dir.join(rel_path);
                        if !dest.exists() {
                            jar_names.push((name, dest));
                        }
                    }
                }
            }

            // Dann die Daten extrahieren
            for (name, dest) in jar_names {
                if let Ok(mut entry) = archive.by_name(&name) {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data)?;
                    jars_data.push((dest, data));
                }
            }

            (version_json, jars_data)
        };

        #[derive(serde::Deserialize)]
        struct NeoForgeVersion {
            #[serde(rename = "mainClass")]
            main_class: String,
            libraries: Vec<NeoForgeLib>,
        }

        #[derive(serde::Deserialize)]
        struct NeoForgeLib {
            name: String,
            downloads: Option<NeoForgeDownloads>,
        }

        #[derive(serde::Deserialize)]
        struct NeoForgeDownloads {
            artifact: Option<NeoForgeArtifact>,
        }

        #[derive(serde::Deserialize)]
        struct NeoForgeArtifact {
            url: String,
            path: String,
            sha1: Option<String>,
        }

        let version: NeoForgeVersion = serde_json::from_str(&version_json)?;
        let original_main_class = version.main_class.clone();
        tracing::info!("NeoForge original main class: {}", original_main_class);
        tracing::info!("NeoForge has {} libraries", version.libraries.len());

        // Verwende die Original-Main-Class (BootstrapLauncher)
        // Die System-Properties, die wir setzen, sollten ausreichen
        let main_class = original_main_class;

        tracing::info!("Using main class: {}", main_class);

        let mut classpath_entries = Vec::new();

        // Schreibe die extrahierten JARs (jetzt asynchron sicher)
        for (dest, data) in jars_data {
            tracing::info!("Extracting: {:?}", dest);
            tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
            tokio::fs::write(&dest, data).await?;
            classpath_entries.push(dest.display().to_string());
        }

        // Downloade fehlende Libraries
        for lib in &version.libraries {
            if let Some(downloads) = &lib.downloads {
                if let Some(artifact) = &downloads.artifact {
                    let dest = libraries_dir.join(&artifact.path);

                    if !dest.exists() && !artifact.url.is_empty() {
                        tracing::info!("Downloading NeoForge library: {}", lib.name);
                        tokio::fs::create_dir_all(dest.parent().unwrap()).await?;

                        if let Err(e) = self.download_manager.download_with_hash(
                            &artifact.url,
                            &dest,
                            artifact.sha1.as_deref()
                        ).await {
                            tracing::warn!("Failed to download {}: {}", lib.name, e);
                            continue;
                        }
                    }

                    if dest.exists() {
                        classpath_entries.push(dest.display().to_string());
                    }
                }
            } else {
                // Versuche Standard-Maven-Pfad
                let lib_path = Self::maven_to_path(&lib.name);
                let dest = libraries_dir.join(&lib_path);

                if !dest.exists() {
                    let maven_urls = vec![
                        format!("https://maven.neoforged.net/releases/{}", lib_path),
                        format!("https://maven.minecraftforge.net/{}", lib_path),
                        format!("https://repo1.maven.org/maven2/{}", lib_path),
                        format!("https://libraries.minecraft.net/{}", lib_path),
                    ];

                    for url in maven_urls {
                        if self.download_manager.download_with_hash(&url, &dest, None).await.is_ok() {
                            tracing::info!("Downloaded {} from {}", lib.name, url);
                            break;
                        }
                    }
                }

                if dest.exists() {
                    classpath_entries.push(dest.display().to_string());
                }
            }
        }

        Ok((classpath_entries, main_class))
    }

    /// Hilfsfunktion: Maven-Koordinaten zu Dateipfad
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
