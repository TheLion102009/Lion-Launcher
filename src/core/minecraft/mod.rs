#![allow(dead_code)]

mod installer;
mod neoforge;
mod forge;
pub mod worlds;

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use crate::types::profile::Profile;
use crate::core::download::DownloadManager;
use crate::config::defaults;

const MOJANG_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const RESOURCES_URL: &str = "https://resources.download.minecraft.net";

/// Thread-sichere globale Variable für extra Launch-Argumente (Quick Play).
/// thread_local! funktioniert NICHT mit async/Tokio – nach einem .await kann der Task
/// auf einem anderen Thread fortgesetzt werden, wo die thread_local leer ist.
static EXTRA_LAUNCH_ARGS: std::sync::OnceLock<std::sync::Mutex<Vec<String>>> =
    std::sync::OnceLock::new();

fn extra_launch_args() -> &'static std::sync::Mutex<Vec<String>> {
    EXTRA_LAUNCH_ARGS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Setzt die Extra-Launch-Argumente (z.B. --quickPlaySingleplayer <world>).
fn set_extra_launch_args(args: Vec<String>) {
    if let Ok(mut guard) = extra_launch_args().lock() {
        *guard = args;
    }
}

/// Nimmt die Extra-Launch-Argumente heraus und leert den Puffer.
fn take_extra_launch_args() -> Vec<String> {
    extra_launch_args().lock()
        .map(|mut guard| std::mem::take(&mut *guard))
        .unwrap_or_default()
}

/// Liest die Extra-Launch-Argumente (ohne sie zu leeren).
fn get_extra_launch_args() -> Vec<String> {
    extra_launch_args().lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}

/// Globaler Speicher für Launch-Warnungen (thread-sicher via Mutex)
static LAUNCH_WARNINGS: std::sync::OnceLock<std::sync::Mutex<Vec<String>>> =
    std::sync::OnceLock::new();

/// Globale Map: Profile-ID → PID der laufenden Minecraft-Instanz
static RUNNING_PROCESSES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, u32>>> =
    std::sync::OnceLock::new();

fn running_processes() -> &'static std::sync::Mutex<std::collections::HashMap<String, u32>> {
    RUNNING_PROCESSES.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Registriert eine laufende Minecraft-Instanz.
pub fn register_running_process(profile_id: &str, pid: u32) {
    if let Ok(mut map) = running_processes().lock() {
        map.insert(profile_id.to_string(), pid);
    }
}

/// Entfernt eine beendete Minecraft-Instanz aus der globalen Map.
pub fn unregister_running_process(profile_id: &str) {
    if let Ok(mut map) = running_processes().lock() {
        map.remove(profile_id);
    }
}

/// Gibt alle aktuell laufenden Profil-IDs zurück.
pub fn get_running_profile_ids() -> Vec<String> {
    running_processes().lock()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Beendet die laufende Minecraft-Instanz eines Profils.
pub fn kill_running_process(profile_id: &str) -> bool {
    let pid = {
        running_processes().lock().ok()
            .and_then(|m| m.get(profile_id).copied())
    };
    if let Some(pid) = pid {
        tracing::info!("Killing Minecraft process PID {} for profile {}", pid, profile_id);
        #[cfg(unix)]
        {
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM); }
        }
        #[cfg(windows)]
        {
            std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .spawn().ok();
        }
        unregister_running_process(profile_id);
        true
    } else {
        false
    }
}

fn launch_warnings() -> &'static std::sync::Mutex<Vec<String>> {
    LAUNCH_WARNINGS.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Fügt eine Launch-Warnung hinzu, die nach dem Start dem User angezeigt wird.
pub fn add_launch_warning(msg: impl Into<String>) {
    if let Ok(mut w) = launch_warnings().lock() {
        w.push(msg.into());
    }
}

/// Nimmt alle akkumulierten Warnungen heraus und leert den Puffer.
pub fn take_launch_warnings() -> Vec<String> {
    launch_warnings().lock().map(|mut w| std::mem::take(&mut *w)).unwrap_or_default()
}

pub struct MinecraftLauncher {
    download_manager: DownloadManager,
}

#[derive(Debug, serde::Deserialize)]
struct VersionManifest {
    versions: Vec<VersionEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct VersionEntry {
    id: String,
    url: String,
}

#[derive(Debug, serde::Deserialize)]
#[allow(non_snake_case)]
struct VersionInfo {
    id: String,
    mainClass: String,
    libraries: Vec<Library>,
    downloads: GameDownloads,
    assetIndex: AssetIndexInfo,
    javaVersion: Option<JavaVersionInfo>,
}

#[derive(Debug, serde::Deserialize)]
#[allow(non_snake_case)]
struct JavaVersionInfo {
    majorVersion: u32,
}

#[derive(Debug, serde::Deserialize)]
struct Library {
    name: String,
    downloads: Option<LibraryDownloads>,
    rules: Option<Vec<Rule>>,
    natives: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, serde::Deserialize)]
struct LibraryDownloads {
    artifact: Option<Artifact>,
    classifiers: Option<std::collections::HashMap<String, Artifact>>,
}

#[derive(Debug, serde::Deserialize)]
struct Artifact {
    path: String,
    sha1: String,
    url: String,
}

#[derive(Debug, serde::Deserialize)]
struct Rule {
    action: String,
    os: Option<OsRule>,
}

#[derive(Debug, serde::Deserialize)]
struct OsRule {
    name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GameDownloads {
    client: DownloadInfo,
}

#[derive(Debug, serde::Deserialize)]
struct DownloadInfo {
    sha1: String,
    url: String,
}

#[derive(Debug, serde::Deserialize)]
struct AssetIndexInfo {
    id: String,
    sha1: String,
    url: String,
}

#[derive(Debug, serde::Deserialize)]
struct AssetIndex {
    objects: std::collections::HashMap<String, AssetObject>,
}

#[derive(Debug, serde::Deserialize)]
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
    /// SRG-JAR Pfad (für NeoForge --gameJar)
    srg_jar_path: Option<String>,
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

fn classpath_separator() -> &'static str {
    if cfg!(windows) { ";" } else { ":" }
}

fn split_classpath_entries(classpath: &str) -> Vec<String> {
    std::env::split_paths(std::ffi::OsStr::new(classpath))
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

fn join_classpath_entries<T: AsRef<str>>(entries: impl IntoIterator<Item = T>) -> String {
    entries
        .into_iter()
        .map(|entry| entry.as_ref().to_string())
        .collect::<Vec<_>>()
        .join(classpath_separator())
}

impl MinecraftLauncher {
    pub fn new() -> Result<Self> {
        Ok(Self {
            download_manager: DownloadManager::new()?,
        })
    }

    /// Startet Minecraft mit zusätzlichen Argumenten (z.B. für Quick Play)
    pub async fn launch_with_extra_args(
        &self,
        profile: &Profile,
        username: &str,
        uuid: &str,
        access_token: Option<&str>,
        extra_args: Vec<String>
    ) -> Result<()> {
        tracing::info!("Launching with extra args: {:?}", extra_args);

        // Setze extra args in thread-sicherer statischer Variable (Mutex)
        set_extra_launch_args(extra_args);

        let result = self.launch(profile, username, uuid, access_token).await;

        // Clear extra args nach dem Launch
        take_extra_launch_args();

        result.map(|_| ())
    }

    /// Startet Minecraft und gibt Warnungen zurück (z.B. Quilt-Fallback-Info).
    pub async fn launch(&self, profile: &Profile, username: &str, uuid: &str, access_token: Option<&str>) -> Result<Vec<String>> {
        // Warnungs-Puffer leeren (Überrest aus vorherigem Start)
        take_launch_warnings();

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
        // IMMER leeren: verhindert LWJGL-Versionskonflikte wenn MC-Version gewechselt wird.
        if natives_dir.exists() {
            tokio::fs::remove_dir_all(&natives_dir).await.ok();
        }
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
        self.download_assets(&version_info.assetIndex, &assets_dir).await?;

        // NeoForge/Forge verwendet einen speziellen Launch-Mechanismus
        if matches!(loader, crate::types::version::ModLoader::NeoForge) {
            self.launch_neoforge_new(
                profile, &version_info, &classpath, &libraries_dir,
                &versions_dir, &assets_dir, &natives_dir, game_dir,
                username, uuid, access_token
            ).await?;
            return Ok(take_launch_warnings());
        }

        if matches!(loader, crate::types::version::ModLoader::Forge) {
            self.launch_neoforge_or_forge(
                profile, &version_info, &client_jar, &classpath,
                &libraries_dir, &assets_dir, &natives_dir, game_dir,
                username, uuid, access_token
            ).await?;
            return Ok(take_launch_warnings());
        }

        // Mod-Loader-spezifische Konfiguration für Fabric/Quilt/Vanilla
        let (main_class, final_classpath) = match loader {
            crate::types::version::ModLoader::Fabric => {
                tracing::info!("Installing Fabric loader...");
                let (fabric_classpath, fabric_main_class) = self.install_fabric(version, &libraries_dir).await?;

                let mut cp_entries = split_classpath_entries(&fabric_classpath);
                cp_entries.extend(
                    split_classpath_entries(&classpath)
                        .into_iter()
                        .filter(|path| !path.contains("/org/ow2/asm/") && !path.contains("\\org\\ow2\\asm\\"))
                );
                cp_entries.push(client_jar.display().to_string());
                let cp = join_classpath_entries(cp_entries);
                (fabric_main_class, cp)
            }
            crate::types::version::ModLoader::Quilt => {
                tracing::info!("Installing Quilt loader...");
                let (quilt_classpath, quilt_main_class) = self.install_quilt(version, &libraries_dir).await?;

                let mut cp_entries = split_classpath_entries(&quilt_classpath);
                cp_entries.extend(
                    split_classpath_entries(&classpath)
                        .into_iter()
                        .filter(|path| !path.contains("/org/ow2/asm/") && !path.contains("\\org\\ow2\\asm\\"))
                );
                cp_entries.push(client_jar.display().to_string());
                let cp = join_classpath_entries(cp_entries);
                (quilt_main_class, cp)
            }
            crate::types::version::ModLoader::Vanilla => {
                let mut cp_entries = split_classpath_entries(&classpath);
                cp_entries.push(client_jar.display().to_string());
                let cp = join_classpath_entries(cp_entries);
                (version_info.mainClass.clone(), cp)
            }
            _ => unreachable!()
        };

        // Standard-Launch für Fabric/Quilt/Vanilla
        self.launch_standard(
            profile, &main_class, &final_classpath, &client_jar,
            &assets_dir, &natives_dir, game_dir, &version_info,
            username, uuid, access_token
        ).await?;

        Ok(take_launch_warnings())
    }

    /// Launch für NeoForge mit der neuen neoforge.rs Implementation
    #[allow(clippy::too_many_arguments)]
    async fn launch_neoforge_new(
        &self,
        profile: &Profile,
        version_info: &VersionInfo,
        vanilla_classpath: &str,
        libraries_dir: &Path,
        versions_dir: &Path,
        assets_dir: &Path,
        natives_dir: &Path,
        game_dir: &Path,
        username: &str,
        uuid: &str,
        access_token: Option<&str>,
    ) -> Result<()> {
        let version = &profile.minecraft_version;
        let loader_version = if profile.loader.version.is_empty() {
            "latest"
        } else {
            &profile.loader.version
        };

        tracing::info!("🚀 Launching NeoForge {} for Minecraft {}", loader_version, version);

        // Wichtige Verzeichnisse sicherstellen
        tokio::fs::create_dir_all(game_dir.join("mods")).await.ok();
        tokio::fs::create_dir_all(game_dir.join("config")).await.ok();
        tokio::fs::create_dir_all(game_dir.join("logs")).await.ok();
        tokio::fs::create_dir_all(game_dir.join("saves")).await.ok();
        tokio::fs::create_dir_all(game_dir.join("resourcepacks")).await.ok();
        tracing::info!("mods/ dir: {:?} ({} files)",
            game_dir.join("mods"),
            std::fs::read_dir(game_dir.join("mods")).map(|d| d.count()).unwrap_or(0)
        );

        // Finde Java – verwende die von Mojang angegebene Mindestversion (mindestens 21 für NeoForge)
        let required_java = version_info.javaVersion.as_ref().map(|j| j.majorVersion).unwrap_or(21).max(21);
        tracing::info!("Required Java version: {}", required_java);
        let java_path = self.ensure_java_installed(required_java, None).await?;

        // Installiere NeoForge (mit Vanilla-Libraries)
        let installation = neoforge::install_neoforge(
            version,
            loader_version,
            libraries_dir,
            versions_dir,
            &java_path,
            vanilla_classpath,
        ).await?;

        // Baue das Launch-Command
        let memory_mb = profile.memory_mb.unwrap_or(4096);
        let token = access_token.unwrap_or("0");

        let mut cmd = neoforge::build_launch_command(
            &installation,
            &java_path,
            memory_mb,
            game_dir,
            assets_dir,
            natives_dir,
            libraries_dir,
            username,
            uuid,
            token,
            version,
            &version_info.assetIndex.id,
        );

        // Display-Umgebungsvariablen weitergeben (verhindert GBM/EGL-Fallback → SIGABRT)
        if let Ok(display) = std::env::var("DISPLAY") {
            cmd.env("DISPLAY", display);
        }
        if let Ok(wd) = std::env::var("WAYLAND_DISPLAY") {
            cmd.env("WAYLAND_DISPLAY", wd);
        }
        if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
            cmd.env("XDG_RUNTIME_DIR", xdg);
        }
        cmd.env("_JAVA_AWT_WM_NONREPARENTING", "1");

        // Extra-Args (Quick Play) — fehlte hier komplett!
        let extra_args = get_extra_launch_args();
        if !extra_args.is_empty() {
            tracing::info!("Appending Quick Play args to NeoForge: {:?}", extra_args);
            for arg in &extra_args {
                cmd.arg(arg);
            }
        }

        tracing::info!("✅ Starting NeoForge...");

        // Starte das Spiel
        let mut child = cmd.spawn()?;
        let pid = child.id();
        tracing::info!("🎮 Minecraft started with PID: {}", pid);

        // PID in globalem Zustand registrieren
        let profile_id_owned = profile.id.clone();
        register_running_process(&profile.id, pid);

        // Warte auf das Spiel im Hintergrund
        tokio::spawn(async move {
            match child.wait() {
                Ok(status) => {
                    if status.success() {
                        tracing::info!("✅ Minecraft (PID {}) exited successfully", pid);
                    } else {
                        tracing::warn!("⚠️  Minecraft (PID {}) exited with status: {}", pid, status);
                    }
                }
                Err(e) => tracing::error!("❌ Error waiting for Minecraft: {}", e),
            }
            unregister_running_process(&profile_id_owned);
        });

        Ok(())
    }

    /// Launch für Forge mit korrektem Modulpfad.
    /// Verwendet die version.json aus dem Forge-Installer als einzige Quelle der Wahrheit.
    /// Alle JVM/Game-Args werden 1:1 übernommen und Platzhalter korrekt ersetzt.
    /// Launch für Forge (modern + legacy).
    ///
    /// Für Forge 1.13+ mit ForgeBootstrap:
    /// ┌──────────────────────────────────────────────────────────────────────┐
    /// │  java                                                                │
    /// │    -Xmx... -Xms... -Djava.library.path=...                          │
    /// │    [JVM-args aus version.json]                                       │
    /// │    -cp bootstrap.jar:bootstrap-api.jar                               │
    /// │    net.minecraftforge.bootstrap.ForgeBootstrap                       │
    /// │    --module-path [alle anderen Forge-JARs]                           │
    /// │    --add-modules ALL-MODULE-PATH                                     │
    /// │    [game-args]                                                       │
    /// └──────────────────────────────────────────────────────────────────────┘
    ///
    /// ForgeBootstrap.main() baut dann INTERN den richtigen Java-Modul-Kontext auf.
    /// NICHT via `-m net.minecraftforge.bootstrap/...` starten!
    #[allow(clippy::too_many_arguments)]
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

        tracing::info!("=== Forge Launch ===");

        // Loader-Version auflösen
        let loader_version = if profile.loader.version == "latest" || profile.loader.version.is_empty() {
            self.resolve_latest_forge_version(version).await?
        } else {
            profile.loader.version.clone()
        };

        tracing::info!("Using Forge version: {}", loader_version);

        // KRITISCH: mods/-Verzeichnis sicherstellen.
        // Forge sucht Mods ausschließlich in ${gameDir}/mods/.
        // Ohne dieses Verzeichnis lädt Forge KEINE Mods – auch wenn die JARs im Cache sind.
        let mods_dir = game_dir.join("mods");
        tokio::fs::create_dir_all(&mods_dir).await?;
        tracing::info!("Mods directory: {:?} ({} files)",
            mods_dir,
            std::fs::read_dir(&mods_dir).map(|d| d.count()).unwrap_or(0)
        );

        // Weitere wichtige Forge-Verzeichnisse sicherstellen
        tokio::fs::create_dir_all(game_dir.join("config")).await.ok();
        tokio::fs::create_dir_all(game_dir.join("logs")).await.ok();
        tokio::fs::create_dir_all(game_dir.join("saves")).await.ok();
        tokio::fs::create_dir_all(game_dir.join("resourcepacks")).await.ok();
        tokio::fs::create_dir_all(game_dir.join("shaderpacks")).await.ok();

        tracing::info!("Final Forge version: {}", loader_version);

        // Java-Version bestimmen: ZUERST, bevor fml.toml und Installer laufen
        // WICHTIG: .max(17) NICHT verwenden! Alte MC-Versionen (≤1.16.x) brauchen Java 8,
        // MC 1.17 braucht Java 16, MC 1.18+ braucht Java 17+.
        let required_java = version_info.javaVersion.as_ref().map(|j| j.majorVersion).unwrap_or(8);

        // Alte Forge-Versionen (MC ≤ 1.16.5) nutzen Nashorn (jdk.nashorn), das in Java 15 entfernt wurde.
        // → Java maximal 8 (sicherster Wert, alle alten Forge-Versionen getestet damit).
        // Für MC 1.17 ist Java 16 vorgeschrieben, ab 1.18 Java 17+, ab 1.20.5 Java 21+.
        let max_java: Option<u32> = if required_java <= 8 {
            Some(8)
        } else {
            None
        };

        tracing::info!("Required Java version for Forge: {} (max: {:?})", required_java, max_java);
        let java_path = self.ensure_java_installed(required_java, max_java).await?;

        // fml.toml schreiben: EarlyDisplay deaktivieren.
        // earlyWindowControl=true + NVIDIA/GLX → "BadValue" bei allen GL-Profilen (3.2–4.6).
        // earlyWindowProvider="none" deaktiviert das GLFW-Fenster komplett.
        // NUR für moderne Forge-Versionen (1.17+) die fml.toml verwenden.
        if required_java >= 17 {
            let config_dir = game_dir.join("config");
            tokio::fs::create_dir_all(&config_dir).await.ok();
            let fml_toml = config_dir.join("fml.toml");
            let content = "\
#FML config - managed by Lion-Launcher
earlyWindowControl = false
earlyWindowProvider = \"none\"
earlyWindowHeight = 480
earlyWindowWidth = 854
earlyWindowMaximized = false
earlyWindowFBScale = 1
earlyWindowSkipGLVersions = []
earlyWindowLogHelpMessage = false
earlyWindowSquir = false
earlyWindowShowCPU = false
versionCheck = true
defaultConfigPath = \"defaultconfigs\"
disableOptimizedDFU = true
maxThreads = -1
";
            tokio::fs::write(&fml_toml, content).await.ok();
            tracing::info!("Wrote fml.toml (earlyWindowControl=false) to {:?}", fml_toml);
        }

        // Forge installieren

        let forge_installer = forge::ForgeInstaller::new(self.download_manager.clone());
        let install_result = forge_installer.install_forge_complete(
            version, &loader_version, libraries_dir, client_jar
        ).await?;

        // Natives-Verzeichnis leeren und neu befüllen
        if natives_dir.exists() {
            tokio::fs::remove_dir_all(natives_dir).await.ok();
        }
        tokio::fs::create_dir_all(natives_dir).await?;
        let os = Self::get_os();

        // Nur natives-JARs extrahieren die explizit von Forge oder Vanilla referenziert werden.
        // KEINE blinde Suche im libraries-Verzeichnis — das würde alle LWJGL-Versionen mischen!
        //
        // Quellen:
        // 1. install_result.native_jars: natives-JARs aus der Forge version.json
        // 2. vanilla_classpath: natives-JARs die download_libraries heruntergeladen hat
        //    (da download_libraries natives aus CP filtert, sind sie NICHT im vanilla_classpath —
        //     aber die JARs liegen auf Disk und können über den bekannten Pfad gefunden werden)

        // ── Natives extrahieren ──────────────────────────────────────────────────
        // Quelle 1: Forge-natives aus version.json (install_result.native_jars)
        for jar_path in &install_result.native_jars {
            let path = std::path::Path::new(jar_path);
            let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let is_my_os = (os == "linux" && (fname.contains("natives-linux") || fname.contains("linux")))
                || (os == "windows" && fname.contains("natives-windows"))
                || (os == "osx" && (fname.contains("natives-osx") || fname.contains("natives-macos")));
            if is_my_os && path.exists() {
                tracing::info!("Extracting Forge native: {}", fname);
                self.extract_native(path, natives_dir)?;
            }
        }

        // Quelle 2: Natives direkt aus version_info.libraries ableiten.
        // Das deckt LWJGL, jtracy (MC 1.21.11+) und alle anderen Vanilla-Natives ab.
        // download_libraries hat diese JARs bereits heruntergeladen, aber aus dem CP gefiltert.
        let native_os_suffix = match os.as_str() {
            "linux"   => "natives-linux",
            "windows" => "natives-windows",
            "osx"     => "natives-macos",
            _         => "natives-linux",
        };
        for lib in &version_info.libraries {
            if let Some(rules) = &lib.rules {
                if !self.check_rules(rules) { continue; }
            }
            if let Some(dl) = &lib.downloads {
                if let Some(art) = &dl.artifact {
                    if art.path.contains(native_os_suffix) {
                        let native_path = libraries_dir.join(&art.path);
                        if native_path.exists() {
                            let fname = native_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            tracing::info!("Extracting Vanilla native: {}", fname);
                            self.extract_native(&native_path, natives_dir)?;
                        }
                    }
                }
                // Altes Format (classifiers)
                if let Some(classifiers) = &dl.classifiers {
                    let key = format!("natives-{}", os);
                    if let Some(nat) = classifiers.get(&key) {
                        let native_path = libraries_dir.join(&nat.path);
                        if native_path.exists() {
                            let fname = native_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                            tracing::info!("Extracting Vanilla native (legacy): {}", fname);
                            self.extract_native(&native_path, natives_dir)?;
                        }
                    }
                }
            }
        }

        // Quelle 3: Direkte Suche im libraries-Verzeichnis nach natives-linux JARs.
        // Fallback wenn Quellen 1+2 nichts fanden (z.B. frischer Download).
        let extracted = std::fs::read_dir(natives_dir)
            .map(|d| d.count()).unwrap_or(0);
        if extracted == 0 {
            tracing::warn!("No natives extracted from classpath — scanning libraries dir");
            if let Ok(entries) = walkdir_lwjgl_natives(libraries_dir, &os) {
                for native_path in entries {
                    let fname = native_path.file_name()
                        .and_then(|n| n.to_str()).unwrap_or("");
                    tracing::info!("Fallback: Extracting native {}", fname);
                    self.extract_native(&native_path, natives_dir)?;
                }
            }
        }

        tracing::info!("Natives directory populated: {} files",
            std::fs::read_dir(natives_dir).map(|d| d.count()).unwrap_or(0));

        // options.txt: fullscreen deaktivieren um Absturz-Loop nach unfertigem Start zu verhindern.
        // Minecraft speichert "fullscreen:true" und versucht beim nächsten Start sofort
        // Fullscreen zu aktivieren — ohne GL-Context → Absturz → Loop.
        {
            let options_file = game_dir.join("options.txt");
            if options_file.exists() {
                if let Ok(content) = tokio::fs::read_to_string(&options_file).await {
                    if content.contains("fullscreen:true") {
                        let patched = content.replace("fullscreen:true", "fullscreen:false");
                        tokio::fs::write(&options_file, patched).await.ok();
                        tracing::info!("Patched options.txt: fullscreen→false");
                    }
                }
            }
        }

        let memory_mb = profile.memory_mb.unwrap_or(4096);
        let token = access_token.unwrap_or("0");
        let user_type = if access_token.is_some() && token != "0" { "msa" } else { "legacy" };

        // Platzhalter in JVM-Args ersetzen
        let resolved_jvm_args: Vec<String> = install_result.jvm_args.iter()
            .map(|arg| forge::resolve_arg_placeholders(
                arg, libraries_dir, natives_dir, game_dir, assets_dir,
                &version_info.assetIndex.id, version, uuid, token, user_type, username,
            ))
            .collect();

        // Platzhalter in Game-Args ersetzen
        // --gameJar aus game_args filtern: Wir setzen ihn immer explizit auf die gepatchte JAR.
        // Der Platzhalter-Wert aus version.json zeigt auf die falsche (ungepatchte) JAR.
        // NUR für Bootstrap-Forge (1.18+) filtern, da wir --gameJar nur dort re-adden.
        let is_bootstrap_era = install_result.main_class.contains("ForgeBootstrap")
            || install_result.main_class.contains("BootstrapLauncher");
        let mut game_args_filtered: Vec<String> = Vec::new();
        let mut skip_next = false;
        for arg in &install_result.game_args {
            if skip_next {
                skip_next = false;
                continue;
            }
            if is_bootstrap_era && arg == "--gameJar" {
                skip_next = true;
                continue;
            }
            game_args_filtered.push(arg.clone());
        }

        let resolved_game_args: Vec<String> = game_args_filtered.iter()
            .map(|arg| forge::resolve_arg_placeholders(
                arg, libraries_dir, natives_dir, game_dir, assets_dir,
                &version_info.assetIndex.id, version, uuid, token, user_type, username,
            ))
            .collect();

        tracing::info!("JVM args resolved: {}", resolved_jvm_args.len());
        tracing::info!("Game args resolved: {}", resolved_game_args.len());

        let mut cmd = Command::new(&java_path);

        // ── Linux/NVIDIA Umgebungs-Fixes ─────────────────────────────────────────
        // Kontext: NVIDIA Kernel-Modul und Userspace-Treiber können unterschiedliche
        // Versionen haben (z.B. nach Kernel-Update ohne Reboot). Das bricht GLX mit
        // "BadValue X_GLXCreateNewContext". LWJGL/GLFW kann aber über EGL-X11 rendern.
        //
        // DISPLAY muss explizit gesetzt sein – Tauri-Kindprozesse erben env nicht immer.
        cmd.env("DISPLAY", std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string()));
        if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
            cmd.env("XDG_RUNTIME_DIR", xdg);
        }
        // NVIDIA-Treiber für GLX explizit wählen (nicht Mesa-Fallback)
        cmd.env("__GLX_VENDOR_LIBRARY_NAME", "nvidia");
        // NVIDIA EGL-Treiber explizit (für LWJGL EGL-Pfad)
        cmd.env("__EGL_VENDOR_LIBRARY_FILENAMES", "/usr/lib/x86_64-linux-gnu/libEGL_nvidia.so.0");
        // Threaded Optimizations AUS – verursacht BadValue bei Context-Create
        cmd.env("__GL_THREADED_OPTIMIZATIONS", "0");
        // Kein indirektes Rendering (würde Software-Fallback erzwingen)
        cmd.env("LIBGL_ALWAYS_INDIRECT", "0");
        // Reparenting-WM Fix: verhindert GLXBadDrawable beim Fensterwechsel
        cmd.env("_JAVA_AWT_WM_NONREPARENTING", "1");
        // Mesa-Overrides entfernen – interferieren mit NVIDIA-Treiber
        cmd.env_remove("MESA_GL_VERSION_OVERRIDE");
        cmd.env_remove("MESA_GLSL_VERSION_OVERRIDE");
        cmd.env_remove("LIBGL_DRIVERS_PATH");
        // LWJGL: EGL-X11 Plattform statt Standard-GLX wählen
        // Das umgeht den GLX BadValue Bug bei NVIDIA Version-Mismatch
        cmd.env("LIBGL_KOPPER_DISABLE", "1");
        // ────────────────────────────────────────────────────────────────────────

        // === BASIS JVM-ARGUMENTE ===
        cmd.arg(format!("-Xmx{}M", memory_mb));
        cmd.arg(format!("-Xms{}M", memory_mb / 2));
        // Beide Properties setzen: LWJGL im Forge SECURE-BOOTSTRAP ModuleLayer
        // ignoriert java.library.path und liest stattdessen org.lwjgl.librarypath
        cmd.arg(format!("-Djava.library.path={}", natives_dir.display()));
        cmd.arg(format!("-Dorg.lwjgl.librarypath={}", natives_dir.display()));
        // Forge EarlyDisplay auf Linux deaktivieren: vermeidet GLFW BadValue-Fehler
        // durch das frühe OpenGL-Fenster das Forge vor dem eigentlichen MC öffnet.
        // GLFW versucht dort alle GL-Profile 4.6..3.2 und scheitert auf NVIDIA/GLX.
        cmd.arg("-Dfml.earlyprogresswindow=false");
        cmd.arg("-Dforge.earlyprogresswindow=false");

        // Forge Patch-Toleranz: Ignoriere Diskrepanzen beim Patching und ungültige Zertifikate.
        // Notwendig für Forge 56.x/57.x (MC 1.21.6/1.21.7) wo die field_to_method.js-Coremods
        // fehlschlagen weil Mojang das "fluid"-Feld in FlowingFluidBlock nicht mehr private gemacht hat.
        // Ohne diese Flags lädt FML die Mods möglicherweise im "reduced mode".
        cmd.arg("-Dfml.ignoreInvalidMinecraftCertificates=true");
        cmd.arg("-Dfml.ignorePatchDiscrepancies=true");
        // Weitere Toleranz-Flags für Forge 56.x/57.x Coremod-Fehler:
        // Verhindert dass FML bei Transformationsfehlern in einen eingeschränkten Modus wechselt
        cmd.arg("-Dfml.noGPGCheck=true");
        cmd.arg("-Dforge.logging.console.level=error");
        // Verhindert dass Transformationsfehler die Mod-Loading-Pipeline unterbrechen
        cmd.arg("-Dfml.earlywindow.headless=true");

        // === MODUL-ARGS: NUR für Java 9+ und nur was zur Forge-Ära passt ===
        // Erkennung: ForgeBootstrap (1.20.2+), BootstrapLauncher (1.18-1.20.1),
        //            ModLauncher (1.13-1.17), LaunchWrapper (≤1.12.2)
        let is_bootstrap_era = install_result.main_class.contains("ForgeBootstrap")
            || install_result.main_class.contains("BootstrapLauncher");
        let is_launchwrapper = install_result.main_class.contains("launchwrapper");

        if required_java >= 9 && !is_launchwrapper {
            // DNS-Fix: Forge's SecureBootstrap ModuleLayer exportiert jdk.naming.dns nicht.
            // Das führt zu "NoInitialContextException: Cannot instantiate DnsContextFactory"
            // beim Server-Pingen auf dem Multiplayer-Screen. Mit --add-modules wird das
            // Modul explizit in den Boot-Layer hinzugefügt, bevor Forge seinen ModuleLayer baut.
            cmd.arg("--add-modules=jdk.naming.dns");
            // --add-opens damit javax.naming.spi auf com.sun.jndi.dns zugreifen darf
            cmd.arg("--add-opens=jdk.naming.dns/com.sun.jndi.dns=java.naming");
        }

        if is_bootstrap_era {
            // --add-opens für Forge-Coremods (SecureJarHandler, ASMAPI etc.)
            // Notwendig für Forge 56.x/57.x wo die Bytecode-Transformation
            // auf interne Java-Module zugreifen muss.
            // NUR für Bootstrap-basierte Forge-Versionen (1.18+) die SecureJarHandler nutzen.
            cmd.arg("--add-opens=java.base/java.util.jar=cpw.mods.securejarhandler");
            cmd.arg("--add-opens=java.base/sun.security.util=ALL-UNNAMED");
            cmd.arg("--add-opens=java.base/sun.security.pkcs=ALL-UNNAMED");
        }

        // JVM-Args aus version.json (ohne -p / --module-path - das bauen wir selbst)
        // WICHTIG: -p ist ein Positional-Arg, der nächste Wert ist der Module-Path → beide entfernen!
        // NUR für moderne Forge-Versionen die Module-Args haben.
        if !is_launchwrapper {
            let mut skip_next_jvm = false;
            for arg in &resolved_jvm_args {
                if skip_next_jvm {
                    skip_next_jvm = false;
                    continue;
                }
                if arg == "-p" || arg == "--module-path" {
                    skip_next_jvm = true;
                    continue;
                }
                if arg.starts_with("--module-path=") {
                    continue;
                }
                cmd.arg(arg);
            }
        }

        // KRITISCH: -DlibraryDirectory MUSS ein absoluter Pfad sein!
        // Forge version.json enthält oft "-DlibraryDirectory=libraries" (relativer Pfad),
        // aber das Arbeitsverzeichnis ist game_dir (Profil-Verzeichnis), nicht der Launcher-Root.
        // FMLLoader's LibraryFinder.findLibsPath() liest diese Property und sucht dort Forge-JARs.
        // Da Java bei mehrfacher -D die LETZTE nimmt, überschreibt dies den falschen Wert.
        cmd.arg(format!("-DlibraryDirectory={}", libraries_dir.display()));

        if install_result.is_bootstrap {
            // === FORGE BOOTSTRAP START (Forge 1.20.2+) ===
            //
            // ForgeBootstrap.main() → Bootstrap.selectBootModules() liest ALLE JARs
            // aus dem AppClassLoader (Classpath) und baut intern selbst den ModuleLayer.
            // Deshalb müssen ALLE Forge-JARs im -cp sein — nicht in --module-path.
            //
            // Vanilla-Classpath: JARs entfernen die Forge bereits mitbringt (Versions-Konflikte)
            fn artifact_base(fname: &str) -> &str {
                let name = fname.strip_suffix(".jar").unwrap_or(fname);
                let bytes = name.as_bytes();
                let mut pos = 0;
                for i in 0..name.len() {
                    if bytes[i] == b'-' && i + 1 < name.len() && bytes[i+1].is_ascii_digit() {
                        pos = i;
                        break;
                    }
                }
                if pos > 0 { &name[..pos] } else { name }
            }

            // Basis-Namen aller Forge-JARs sammeln
            let forge_bases: std::collections::HashSet<&str> = install_result.bootstrap_classpath
                .iter()
                .chain(install_result.classpath.iter())
                .map(|p| artifact_base(
                    std::path::Path::new(p).file_name()
                        .and_then(|n| n.to_str()).unwrap_or("")
                ))
                .collect();

            // Vanilla-CP filtern: Konflikte mit Forge-JARs und Natives entfernen
            let filtered_vanilla: Vec<String> = split_classpath_entries(vanilla_classpath)
                .into_iter()
                .filter(|e| {
                    if e.is_empty() { return false; }
                    let fname = std::path::Path::new(e)
                        .file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if fname.contains("-natives-") { return false; }
                    let base = artifact_base(fname);
                    if forge_bases.contains(base) {
                        tracing::debug!("Filtering vanilla JAR (conflicts with forge): {}", fname);
                        return false;
                    }
                    true
                })
                .collect();

            // Classpath aufbauen:
            // bootstrap_classpath (alle Forge-JARs) + classpath (mcp_config etc.)
            // + client.jar + gefilterter Vanilla-CP
            let mut cp_parts: Vec<String> = Vec::new();
            cp_parts.extend(install_result.bootstrap_classpath.iter().cloned());
            cp_parts.extend(install_result.classpath.iter().cloned());
            cp_parts.push(client_jar.display().to_string());
            cp_parts.extend(filtered_vanilla.iter().cloned());

            // Deduplizieren
            let mut seen_cp = std::collections::HashSet::new();
            cp_parts.retain(|p| seen_cp.insert(p.clone()));

            let bootstrap_cp = join_classpath_entries(cp_parts.iter());
            tracing::info!("Bootstrap classpath: {} JARs ({} forge + {} vanilla)",
                cp_parts.len(),
                install_result.bootstrap_classpath.len() + install_result.classpath.len(),
                filtered_vanilla.len()
            );

            cmd.arg("-cp").arg(&bootstrap_cp);

            // Main-Class: ForgeBootstrap direkt via Classpath (NICHT -m !)
            tracing::info!("Starting: {}", install_result.main_class);
            cmd.arg(&install_result.main_class);


        } else {
            // === ALLE ANDEREN FORGE-VERSIONEN (ModLauncher 1.13-1.20.1, LaunchWrapper ≤1.12.2) ===
            //
            // Für ModLauncher (1.13+): cpw.mods.modlauncher.Launcher liest ALLE JARs
            // vom Classpath und baut daraus den TransformingClassLoader.
            //
            // Für BootstrapLauncher (1.18-1.20.1): Ähnlich wie ForgeBootstrap.
            //
            // Für LaunchWrapper (≤1.12.2): net.minecraft.launchwrapper.Launch
            // liest vom Classpath und nutzt TweakClasses für Forge-Injection.
            //
            // ALLE Forge-JARs müssen auf dem Classpath sein!
            let mut full_cp_entries: Vec<String> = Vec::new();
            full_cp_entries.extend(install_result.bootstrap_classpath.iter().cloned());
            full_cp_entries.extend(install_result.classpath.iter().cloned());
            full_cp_entries.push(client_jar.display().to_string());
            full_cp_entries.extend(split_classpath_entries(vanilla_classpath));

            // Deduplizieren
            let mut seen_cp = std::collections::HashSet::new();
            full_cp_entries.retain(|p| !p.is_empty() && seen_cp.insert(p.clone()));

            let full_cp = join_classpath_entries(full_cp_entries.iter());
            tracing::info!("Forge classpath: {} JARs ({} forge + vanilla)",
                full_cp_entries.len(),
                install_result.bootstrap_classpath.len() + install_result.classpath.len()
            );
            cmd.arg("-cp").arg(&full_cp);
            cmd.arg(&install_result.main_class);
        }

        // === GAME-ARGUMENTE (abhängig von Forge-Ära) ===
        if let Some(ref mc_args_str) = install_result.minecraft_arguments {
            // ── LEGACY FORGE (≤1.12.2): minecraftArguments ──────────────────────
            // Einzelner String mit ${Platzhaltern}, durch Leerzeichen getrennt.
            // Enthält bereits --tweakClass und alle nötigen Minecraft-Args.
            tracing::info!("Legacy Forge: Verwende minecraftArguments");
            let legacy_args: Vec<String> = mc_args_str.split_whitespace()
                .map(|arg| forge::resolve_arg_placeholders(
                    arg, libraries_dir, natives_dir, game_dir, assets_dir,
                    &version_info.assetIndex.id, version, uuid, token, user_type, username,
                ))
                .collect();
            for arg in &legacy_args {
                cmd.arg(arg);
            }
        } else {
            // ── MODERNE FORGE (1.13+): Strukturierte arguments ──────────────────
            for arg in &resolved_game_args {
                cmd.arg(arg);
            }

            let has_arg = |name: &str| resolved_game_args.iter().any(|a| a == name);

            // --gameJar: NUR für ForgeBootstrap (1.20.2+) und BootstrapLauncher (1.18+).
            // Ältere ModLauncher-Versionen (1.13-1.17) kennen --gameJar NICHT —
            // der FMLClientLaunchProvider sucht den Client über den Classpath/ModuleLayer.
            if is_bootstrap_era {
                cmd.arg("--gameJar").arg(&install_result.patched_client_jar);
                tracing::info!("--gameJar: {:?}", install_result.patched_client_jar.display());
            }

            if !has_arg("--fml.forgeVersion") {
                cmd.arg("--fml.forgeVersion").arg(&install_result.forge_version);
            }
            if !has_arg("--fml.mcVersion") {
                cmd.arg("--fml.mcVersion").arg(version);
            }
            if !has_arg("--fml.forgeGroup") {
                cmd.arg("--fml.forgeGroup").arg("net.minecraftforge");
            }
            if !has_arg("--fml.mcpVersion") {
                cmd.arg("--fml.mcpVersion").arg(&install_result.mcp_version);
            }
            if !has_arg("--username") {
                cmd.arg("--username").arg(username);
            }
            if !has_arg("--version") {
                cmd.arg("--version").arg(version);
            }
            if !has_arg("--gameDir") {
                cmd.arg("--gameDir").arg(game_dir);
            }
            if !has_arg("--assetsDir") {
                cmd.arg("--assetsDir").arg(assets_dir);
            }
            if !has_arg("--assetIndex") {
                cmd.arg("--assetIndex").arg(&version_info.assetIndex.id);
            }
            if !has_arg("--uuid") {
                cmd.arg("--uuid").arg(uuid);
            }
            if !has_arg("--accessToken") {
                cmd.arg("--accessToken").arg(token);
            }
            if !has_arg("--userType") {
                cmd.arg("--userType").arg(user_type);
            }
        }

        // Extra-Args (Quick Play)
        let extra_args = get_extra_launch_args();
        for arg in &extra_args {
            cmd.arg(arg);
        }

        // Debug-Kommando speichern
        let debug_cmd_path = game_dir.join("java_command_debug.txt");
        let full_cmd_str = format!("{} {}",
            cmd.get_program().to_string_lossy(),
            cmd.get_args()
                .map(|a| {
                    let s = a.to_string_lossy().to_string();
                    if s.contains(' ') { format!("\"{}\"", s) } else { s }
                })
                .collect::<Vec<_>>()
                .join(" ")
        );
        std::fs::write(&debug_cmd_path, &full_cmd_str).ok();
        tracing::info!("Java command saved to: {:?}", debug_cmd_path);

        // Starte den Prozess
        cmd.current_dir(game_dir);
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());

        tracing::info!("Launching Forge {} for MC {}...", loader_version, version);

        let mut child = cmd.spawn()?;
        let pid = child.id();
        tracing::info!("Forge started with PID: {}", pid);

        let profile_id_owned = profile.id.clone();
        register_running_process(&profile.id, pid);

        tokio::spawn(async move {
            match child.wait() {
                Ok(status) => {
                    if status.success() {
                        tracing::info!("Forge (PID {}) exited successfully", pid);
                    } else {
                        tracing::warn!("Forge (PID {}) exited with status: {}", pid, status);
                    }
                }
                Err(e) => tracing::error!("Error waiting for Forge: {}", e),
            }
            unregister_running_process(&profile_id_owned);
        });

        Ok(())
    }

    /// Standard-Launch für Fabric/Quilt/Vanilla
    #[allow(clippy::too_many_arguments)]
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
        // Verwende die von Mojang angegebene Java-Version (aus version.json javaVersion.majorVersion)
        let required_java = version_info.javaVersion.as_ref().map(|j| j.majorVersion).unwrap_or(21);
        tracing::info!("Required Java version: {}", required_java);
        let java_path = self.ensure_java_installed(required_java, None).await?;

        // Auf Windows javaw.exe nutzen (kein Konsolenfenster)
        let java_bin = if cfg!(windows) {
            java_path.replace("java.exe", "javaw.exe")
        } else {
            java_path.clone()
        };

        let memory_mb = profile.memory_mb.unwrap_or(4096);
        let loader = &profile.loader.loader;

        // logs/ Verzeichnis sicherstellen für eigenen Log-Datei
        tokio::fs::create_dir_all(game_dir.join("logs")).await.ok();

        let mut cmd = Command::new(&java_bin);

        // ── Linux/NVIDIA Display-Umgebungsvariablen ──────────────────────────────
        // Ohne DISPLAY startet kein Fenster auf X11. Muss explizit gesetzt werden,
        // da Tauri-Kindprozesse DISPLAY nicht immer erben (z.B. AppImage-Launch).
        cmd.env("DISPLAY", std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string()));
        if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
            cmd.env("XDG_RUNTIME_DIR", xdg);
        }
        if let Ok(wd) = std::env::var("WAYLAND_DISPLAY") {
            cmd.env("WAYLAND_DISPLAY", wd);
        }
        cmd.env("_JAVA_AWT_WM_NONREPARENTING", "1");
        // ────────────────────────────────────────────────────────────────────────

        cmd.arg(format!("-Xmx{}M", memory_mb));
        cmd.arg(format!("-Xms{}M", memory_mb / 2));
        cmd.arg(format!("-Djava.library.path={}", natives_dir.display()));
        cmd.arg("-XX:+UseG1GC");
        cmd.arg("-Dminecraft.launcher.brand=lion-launcher");
        cmd.arg("-Dminecraft.launcher.version=1.0");

        // Notwendige --add-opens für Java 17+ (Minecraft 1.17+)
        if required_java >= 17 {
            cmd.arg("--add-opens=java.base/java.net=ALL-UNNAMED");
            cmd.arg("--add-opens=java.base/java.lang=ALL-UNNAMED");
            cmd.arg("--add-opens=java.base/java.io=ALL-UNNAMED");
            cmd.arg("--add-opens=java.base/java.util=ALL-UNNAMED");
            cmd.arg("--add-opens=java.base/java.util.concurrent=ALL-UNNAMED");
        }

        // Loader-spezifische JVM-Args
        match loader {
            crate::types::version::ModLoader::Fabric => {
                cmd.arg(format!("-Dfabric.gameJarPath={}", client_jar.display()));
            }
            crate::types::version::ModLoader::Quilt => {
                // Korrekte Quilt-Loader-Properties (nicht -Dquilt.*, sondern -Dloader.*)
                // Ohne loader.gameJarPath kann Quilt ClassTweakers nicht von official→intermediary remappen
                cmd.arg(format!("-Dloader.gameJarPath={}", client_jar.display()));
                // Fabric-API-Kompatibilität: emuliert Fabric-Launcher-Verhalten für Fabric-Mods unter Quilt
                cmd.arg("-DFabricMcEmu=net.minecraft.client.main.Main");
                // Ziel-Namespace explizit setzen – verhindert ClassTweakerFormatException
                // "Namespace (official) does not match current runtime namespace (intermediary)"
                cmd.arg("-Dloader.targetNamespace=intermediary");
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
        cmd.arg("--assetIndex").arg(&version_info.assetIndex.id);
        cmd.arg("--uuid").arg(uuid);
        cmd.arg("--accessToken").arg(token);
        cmd.arg("--userType").arg(user_type);

        // Extra args (z.B. für Quick Play)
        let extra_args = get_extra_launch_args();
        for arg in &extra_args {
            cmd.arg(arg);
        }

        cmd.current_dir(game_dir);
        // stdout/stderr pipen und via tracing loggen (funktioniert auch ohne Terminal)
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        tracing::info!("Launching Minecraft ({})...", loader.as_str());
        tracing::info!("Java: {}", java_bin);
        let mut child = cmd.spawn()
            .map_err(|e| anyhow::anyhow!("Konnte Minecraft nicht starten ({}): {}", java_bin, e))?;
        let pid = child.id();
        tracing::info!("🎮 Minecraft gestartet mit PID: {}", pid);

        let profile_id_owned = profile.id.clone();
        register_running_process(&profile.id, pid);

        // stdout/stderr im Hintergrund lesen und loggen
        if let Some(stdout) = child.stdout.take() {
            use std::io::{BufRead, BufReader};
            tokio::task::spawn_blocking(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().flatten() {
                    tracing::info!("[MC stdout] {}", line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            use std::io::{BufRead, BufReader};
            tokio::task::spawn_blocking(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    tracing::warn!("[MC stderr] {}", line);
                }
            });
        }

        tokio::spawn(async move {
            match child.wait() {
                Ok(status) => {
                    if status.success() {
                        tracing::info!("✅ Minecraft (PID {}) erfolgreich beendet", pid);
                    } else {
                        tracing::warn!("⚠️ Minecraft (PID {}) beendet mit Status: {}", pid, status);
                    }
                }
                Err(e) => tracing::error!("❌ Fehler beim Warten auf Minecraft: {}", e),
            }
            unregister_running_process(&profile_id_owned);
        });

        Ok(())
    }

    /// Entfernt doppelte Einträge aus dem Classpath
    fn deduplicate_classpath(classpath: &str) -> String {
        use std::collections::{HashSet, HashMap};

        let mut seen_paths = HashSet::new();
        let mut seen_libraries: HashMap<String, String> = HashMap::new(); // library_name -> full_path
        let mut unique_entries = Vec::new();

        // Extrahiere den Library-Namen aus einem JAR-Pfad
        // z.B. "guava-33.3.1-jre.jar" -> "guava"
        // z.B. "asm-9.6.jar" -> "asm"
        let extract_library_name = |path: &str| -> Option<String> {
            let filename = path.split('/').next_back()?;
            // Entferne .jar Endung
            let name = filename.strip_suffix(".jar")?;
            // Entferne Version und Klassifier (z.B. -33.3.1-jre, -9.6, -natives-linux)
            // Regex-ähnliches Pattern: alles vor der ersten Ziffer nach einem Bindestrich
            let parts: Vec<&str> = name.split('-').collect();
            if parts.is_empty() {
                return Some(name.to_string());
            }

            // Finde den ersten Teil der eine Versionsnummer ist (beginnt mit Ziffer)
            let mut lib_name_parts = Vec::new();
            for part in parts {
                if part.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                    break;
                }
                lib_name_parts.push(part);
            }

            if lib_name_parts.is_empty() {
                Some(name.to_string())
            } else {
                Some(lib_name_parts.join("-"))
            }
        };

        for entry in split_classpath_entries(classpath) {
            let entry = entry.trim();
            if entry.is_empty() {
                continue;
            }

            // 1. Prüfe ob exakt gleicher Pfad bereits vorhanden
            if !seen_paths.insert(entry.to_string()) {
                tracing::debug!("Removing duplicate classpath entry: {}", entry);
                continue;
            }

            // 2. Prüfe ob eine andere Version der gleichen Library bereits vorhanden
            if let Some(lib_name) = extract_library_name(entry) {
                // Bestimmte Libraries die bekannt für Konflikte sind
                let conflict_libraries = [
                    "guava", "failureaccess", "asm", "asm-commons", "asm-tree",
                    "asm-util", "asm-analysis", "jtracy", "gson", "netty",
                    "commons-io", "commons-lang3", "log4j"
                ];

                let is_conflict_library = conflict_libraries.iter()
                    .any(|&lib| lib_name.eq_ignore_ascii_case(lib) || lib_name.starts_with(&format!("{}-", lib)));

                if is_conflict_library {
                    if let Some(existing) = seen_libraries.get(&lib_name.to_lowercase()) {
                        tracing::debug!("Skipping duplicate library {} (already have {})", entry, existing);
                        continue;
                    }
                    seen_libraries.insert(lib_name.to_lowercase(), entry.to_string());
                }
            }

            unique_entries.push(entry.to_string());
        }

        let result = join_classpath_entries(unique_entries.iter());
        tracing::info!("Classpath deduplicated: {} -> {} entries",
            split_classpath_entries(classpath).len(),
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

        // Bekannte crashende Forge-Versionen – werden bei automatischer Auflösung übersprungen
        // Format: (mc_version, forge_version)
        let known_broken: &[(&str, &str)] = &[
            // 1.21.11-61.0.0: FieldToMethodTransformer crash beim Bootstrap
            ("1.21.11", "61.0.0"),
            // 1.21.7-57.0.3: field_to_method.js Coremod-Fehler → Mods werden nicht geladen
            ("1.21.7", "57.0.3"),
            // 1.21.6-56.0.9: field_to_method.js Coremod-Fehler + MC_OFF-Classpath-Problem
            ("1.21.6", "56.0.9"),
        ];

        let client = ForgeClient::new()?;
        let versions = client.get_loader_versions(mc_version).await?;

        // Filtere bekannte kaputte Versionen heraus
        let usable_versions: Vec<_> = versions.iter()
            .filter(|v| {
                !known_broken.iter().any(|(mc, fv)| *mc == mc_version && *fv == v.forge_version)
            })
            .collect();

        let search_in = if usable_versions.is_empty() {
            tracing::warn!("All Forge versions for {} are on the broken list, falling back to latest", mc_version);
            versions.iter().collect::<Vec<_>>()
        } else {
            usable_versions
        };

        // Bevorzuge empfohlene Version, sonst die neueste stabile (versions sind bereits neueste-zuerst sortiert)
        let version = search_in.iter()
            .find(|v| v.recommended)
            .or_else(|| search_in.first())
            .ok_or_else(|| anyhow::anyhow!("No Forge version found for MC {}", mc_version))?;

        tracing::info!("Resolved Forge version for {}: {} (recommended: {})", mc_version, version.forge_version, version.recommended);
        Ok(version.forge_version.clone())
    }

    /// Installiert NeoForge vollständig und gibt das Ergebnis zurück
    async fn install_neoforge_complete(&self, neoforge_version: &str, libraries_dir: &Path, _client_jar: &Path, mc_version: &str) -> Result<ForgeInstallResult> {
        use crate::api::neoforge::NeoForgeClient;
        use std::io::Read;

        let neoforge_client = NeoForgeClient::new()?;
        tracing::info!("Installing NeoForge {} for MC {} (complete)", neoforge_version, mc_version);

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

        // Prüfe ob die gepatchte Client-JAR bereits existiert (verschiedene mögliche Pfade)
        let patched_client_path = libraries_dir.join(format!(
            "net/neoforged/neoforge/{}/neoforge-{}-client.jar",
            neoforge_version, neoforge_version
        ));

        // Alternative Pfade wo die gepatchte JAR sein könnte
        let alt_patched_paths = [
            libraries_dir.join(format!("net/minecraft/client/{}-{}/client-{}-{}-srg.jar", mc_version, "20240808.144430", mc_version, "20240808.144430")),
            libraries_dir.join(format!("net/neoforged/neoforge/{}/neoforge-{}-patched.jar", neoforge_version, neoforge_version)),
        ];

        let patched_exists = patched_client_path.exists() || alt_patched_paths.iter().any(|p| p.exists());

        if !patched_exists {
            // Führe den NeoForge-Installer aus, um die Client-JAR zu patchen
            tracing::info!("🔨 Running NeoForge installer to create SRG-mapped client JAR...");
            tracing::info!("This may take 1-2 minutes on first launch...");

            let java_path = self.ensure_java_installed(21, None).await?;
            // Der Installer erwartet das .minecraft-ähnliche Verzeichnis (parent von libraries)
            let launcher_dir = libraries_dir.parent().unwrap();

            // Erstelle eine minimale launcher_profiles.json wenn sie nicht existiert
            let profiles_path = launcher_dir.join("launcher_profiles.json");
            if !profiles_path.exists() {
                tracing::info!("Creating launcher_profiles.json for NeoForge installer");
                let profiles_content = r#"{"profiles":{},"settings":{},"version":3}"#;
                std::fs::write(&profiles_path, profiles_content).ok();
            }

            let mut installer_cmd = std::process::Command::new(&java_path);
            installer_cmd.arg("-jar");
            installer_cmd.arg(&installer_path);
            installer_cmd.arg("--installClient");
            installer_cmd.arg(launcher_dir);
            installer_cmd.current_dir(launcher_dir);
            installer_cmd.stdout(std::process::Stdio::inherit()); // Show installer output
            installer_cmd.stderr(std::process::Stdio::inherit());

            tracing::info!("Running: java -jar {} --installClient {}", installer_path.display(), launcher_dir.display());

            match installer_cmd.status() {
                Ok(status) => {
                    if status.success() {
                        tracing::info!("✅ NeoForge installer completed successfully");

                        // Prüfe ob die SRG-JAR jetzt existiert
                        let srg_path = libraries_dir.join(format!(
                            "net/minecraft/client/{}-20240808.144430/client-{}-20240808.144430-srg.jar",
                            mc_version, mc_version
                        ));

                        if srg_path.exists() {
                            tracing::info!("✅ SRG-JAR created: {:?}", srg_path);
                        } else {
                            tracing::warn!("⚠️ SRG-JAR not found after installer run!");
                            tracing::warn!("Expected: {:?}", srg_path);
                        }
                    } else {
                        tracing::error!("❌ NeoForge installer failed with status: {}", status);
                        bail!("NeoForge installer failed");
                    }
                }
                Err(e) => {
                    tracing::error!("❌ Failed to run NeoForge installer: {}", e);
                    bail!("Failed to run NeoForge installer: {}", e);
                }
            }
        } else {
            tracing::info!("✅ Patched client JAR already exists");
        }

        // Extrahiere version.json aus dem Installer
        let (version_json, _jvm_args_json, jars_data) = {
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
        #[allow(non_snake_case)]
        struct NeoForgeVersion {
            id: Option<String>,
            mainClass: String,
            inheritsFrom: Option<String>,
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
        tracing::info!("NeoForge main class: {}", version.mainClass);
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
                            .replace("${classpath_separator}", classpath_separator())
                            .replace("${version_name}", neoforge_version);
                        jvm_args.push(processed);
                    }
                }
            }
        }

        // WENN keine JVM-Args in version.json, verwende Standard-Args
        // Diese sind notwendig für das Java Module System
        if jvm_args.is_empty() || !jvm_args.iter().any(|a| a.starts_with("--add-opens") || a.starts_with("-p")) {
            tracing::info!("No JVM args in version.json, using defaults");
            jvm_args.extend(vec![
                "--add-opens=java.base/java.util.jar=ALL-UNNAMED".to_string(),
                "--add-opens=java.base/java.lang.invoke=ALL-UNNAMED".to_string(),
            ]);
        }

        // Finde FML und NeoForm Versionen aus den Libraries
        let mut fml_version = String::new();
        let mut neoform_version = String::new();

        for lib in &version.libraries {
            if lib.name.contains("fmlcore") {
                // Format: net.neoforged.fancymodloader:fmlcore:VERSION
                if let Some(v) = lib.name.split(':').next_back() {
                    fml_version = v.to_string();
                }
            }
            if lib.name.contains("neoform") {
                if let Some(v) = lib.name.split(':').next_back() {
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

        // KRITISCH: Suche die SRG-JAR in mehreren möglichen Pfaden
        tracing::info!("🔍 Suche SRG-JAR für NeoForge...");

        let _srg_jar_path = libraries_dir.join(format!(
            "net/minecraft/client/{}-20240808.144430/client-{}-20240808.144430-srg.jar",
            mc_version, mc_version
        ));

        let alternative_srg_paths = vec![
            libraries_dir.join(format!("net/minecraft/client/{}-20240808.144430/client-{}-20240808.144430-srg.jar",
                mc_version, mc_version)),
            libraries_dir.join(format!("net/neoforged/neoforge/{}/client-{}-srg.jar",
                neoforge_version, mc_version)),
            libraries_dir.join(format!("net/neoforged/neoforge/{}/neoforge-{}-client.jar",
                neoforge_version, neoforge_version)),
        ];

        // Debug: Liste alle Pfade
        for (i, path) in alternative_srg_paths.iter().enumerate() {
            if path.exists() {
                tracing::info!("✅ Gefunden [{}]: {:?}", i, path);
            } else {
                tracing::info!("❌ Nicht gefunden [{}]: {:?}", i, path);
            }
        }

        let srg_exists = alternative_srg_paths.iter().any(|p| p.exists());

        if !srg_exists {
            tracing::info!("🔨 SRG-JAR nicht gefunden! Führe NeoForge-Installer aus...");

            let java_path = self.ensure_java_installed(21, None).await?;
            let launcher_dir = libraries_dir.parent().unwrap();

            // Erstelle launcher_profiles.json falls nicht vorhanden
            let profiles_path = launcher_dir.join("launcher_profiles.json");
            if !profiles_path.exists() {
                tracing::info!("Erstelle launcher_profiles.json für Installer");
                let profiles_content = r#"{"profiles":{},"settings":{},"version":3}"#;
                tokio::fs::write(&profiles_path, profiles_content).await?;
            }

            let mut installer_cmd = std::process::Command::new(&java_path);
            installer_cmd.arg("-jar");
            installer_cmd.arg(&installer_path);
            installer_cmd.arg("--installClient");
            installer_cmd.arg(launcher_dir);
            installer_cmd.current_dir(launcher_dir);
            installer_cmd.stdout(std::process::Stdio::inherit());
            installer_cmd.stderr(std::process::Stdio::inherit());

            tracing::info!("Führe NeoForge-Installer aus: java -jar {} --installClient {}",
                installer_path.display(), launcher_dir.display());

            match installer_cmd.status() {
                Ok(status) if status.success() => {
                    tracing::info!("✅ NeoForge-Installer erfolgreich");

                    // Prüfe nochmal ob SRG-JAR jetzt existiert
                    let mut found_srg = None;
                    for path in &alternative_srg_paths {
                        if path.exists() {
                            tracing::info!("✅ SRG-JAR erstellt: {:?}", path);
                            found_srg = Some(path.clone());
                            break;
                        }
                    }

                    if found_srg.is_none() {
                        tracing::error!("❌ SRG-JAR nicht gefunden nach Installer!");
                        bail!("SRG-JAR wurde vom Installer nicht erstellt");
                    }
                }
                Ok(status) => {
                    tracing::error!("❌ NeoForge-Installer fehlgeschlagen: {}", status);
                    bail!("NeoForge-Installer fehlgeschlagen");
                }
                Err(e) => {
                    tracing::error!("❌ NeoForge-Installer konnte nicht gestartet werden: {}", e);
                    bail!("NeoForge-Installer konnte nicht gestartet werden: {}", e);
                }
            }
        } else {
            tracing::info!("✅ SRG-JAR existiert bereits");
        }

        // Finde die tatsächlich existierende SRG-JAR
        let srg_jar = alternative_srg_paths.iter()
            .find(|p| p.exists())
            .ok_or_else(|| anyhow::anyhow!("SRG-JAR nicht gefunden!"))?;

        tracing::info!("✅ Verwende SRG-JAR: {:?}", srg_jar);

        // KRITISCH: NeoForge-spezifische Argumente
        // --gameJar wird im ForgeInstallResult gespeichert
        let game_args = vec![
            "--launchTarget".to_string(), "forgeclient".to_string(),
            "--fml.fmlVersion".to_string(), fml_version.clone(),
            "--fml.mcVersion".to_string(), mc_version.to_string(),
            "--fml.neoForgeVersion".to_string(), neoforge_version.to_string(),
            "--fml.neoFormVersion".to_string(), neoform_version.clone(),
        ];

        tracing::info!("✅ NeoForge game args: {:?}", game_args);

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

        // KRITISCH: NeoForge Client JAR - erstellt vom Installer, enthält gepatchte Minecraft-Klassen
        let neoforge_client_path = libraries_dir.join(format!("net/neoforged/neoforge/{}/neoforge-{}-client.jar", neoforge_version, neoforge_version));
        if neoforge_client_path.exists() {
            if !classpath.iter().any(|p| p.contains("neoforge") && p.contains("client.jar")) {
                tracing::info!("✅ Adding NeoForge client JAR to classpath: {:?}", neoforge_client_path);
                classpath.push(neoforge_client_path.display().to_string());
            }
        } else {
            tracing::warn!("⚠️ NeoForge client JAR not found: {:?}", neoforge_client_path);
            tracing::warn!("This may cause runtime errors!");
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

        tracing::info!("✅ NeoForge complete: {} classpath, {} module path, {} jvm args, {} game args",
            classpath.len(), module_path.len(), jvm_args.len(), game_args.len());

        // Debug: Print first 5 classpath entries
        for (i, entry) in classpath.iter().take(5).enumerate() {
            tracing::info!("Classpath[{}]: {}", i, entry);
        }

        Ok(ForgeInstallResult {
            main_class: version.mainClass,
            classpath,
            module_path,
            jvm_args,
            game_args,
            srg_jar_path: Some(srg_jar.display().to_string()),
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
        Ok((join_classpath_entries(classpath_entries), main_class))
    }

    /// Quilt Loader installieren und (Classpath, MainClass) zurückgeben.
    ///
    /// Verwendet den `/profile/json`-Endpunkt der Quilt-Meta-API, der für ALLE
    /// Loader-Versionen funktioniert – auch für neuere Versionen (0.21+), die im
    /// veralteten `/versions/loader/{mc_version}`-Listen-Endpunkt nicht erscheinen.
    ///
    /// Hintergrund: Der Listen-Endpunkt gibt maximal `0.20.0-beta.9` zurück, welcher
    /// nur `fabricloader 0.14.21` bereitstellt. Fabric-API >= 0.140.x benötigt aber
    /// `fabricloader >= 0.17.3`, weshalb neuere Loader-Versionen zwingend notwendig sind.
    async fn install_quilt(&self, mc_version: &str, libraries_dir: &Path) -> Result<(String, String)> {
        use crate::api::quilt::QuiltClient;

        let quilt = QuiltClient::new()?;


        let loader_version = quilt.get_latest_loader_version().await
            .unwrap_or_else(|e| {
                tracing::warn!("Konnte neueste Quilt Loader Version nicht ermitteln: {} – nutze Fallback 0.30.0-beta.7", e);
                "0.30.0-beta.7".to_string()
            });

        tracing::info!("Verwende Quilt Loader Version: {}", loader_version);

        let profile = quilt.get_loader_profile(mc_version, &loader_version).await
            .map_err(|e| anyhow::anyhow!(
                "Konnte Quilt-Profil für MC {} / Loader {} nicht laden: {}",
                mc_version, loader_version, e
            ))?;

        tracing::info!("Quilt main class: {}", profile.main_class);
        tracing::info!("Quilt Profil hat {} Libraries", profile.libraries.len());

        let mut classpath_entries = Vec::new();

        // Alle Libraries aus dem Profil herunterladen und zum Classpath hinzufügen.
        // Das Profil liefert bereits die korrekte Reihenfolge (Mappings vor dem Loader).
        for lib in &profile.libraries {
            let lib_path = maven_to_path(&lib.name);
            // Die URL im Profil ist der Maven-Repository-Basis-URL (mit trailing slash)
            let lib_url = format!("{}{}", lib.url, lib_path);
            let lib_dest = libraries_dir.join(&lib_path);

            if !lib_dest.exists() {
                tracing::info!("Lade Quilt Library: {}", lib.name);
                tokio::fs::create_dir_all(lib_dest.parent().unwrap()).await?;
                if let Err(e) = self.download_manager.download_with_hash(&lib_url, &lib_dest, None).await {
                    tracing::warn!("Fehler beim Laden von {}: {} – versuche Maven Central...", lib.name, e);
                    let fallback_url = format!("https://repo1.maven.org/maven2/{}", lib_path);
                    if let Err(e2) = self.download_manager.download_with_hash(&fallback_url, &lib_dest, None).await {
                        tracing::warn!("Auch Maven Central fehlgeschlagen: {} – überspringe Library", e2);
                        continue;
                    }
                }
            }
            if lib_dest.exists() {
                classpath_entries.push(lib_dest.display().to_string());
            }
        }

        tracing::info!("Quilt installiert mit {} Libraries (Loader {})", classpath_entries.len(), loader_version);
        Ok((join_classpath_entries(classpath_entries), profile.main_class))
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
        if installer_path.exists()
            && !Self::is_valid_zip(&installer_path) {
                tracing::warn!("Existing Forge installer is corrupted, re-downloading...");
                tokio::fs::remove_file(&installer_path).await.ok();
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
            return Ok((join_classpath_entries(classpath_entries), main_class));
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
        #[allow(non_snake_case)]
        struct ForgeVersion {
            mainClass: String,
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
        tracing::info!("Forge main class: {}", version.mainClass);
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

        Ok((classpath_entries, version.mainClass))
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
        if installer_path.exists()
            && !Self::is_valid_zip(&installer_path) {
                tracing::warn!("Existing NeoForge installer is corrupted, re-downloading...");
                tokio::fs::remove_file(&installer_path).await.ok();
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
            return Ok((join_classpath_entries(classpath_entries), main_class));
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
                    Ok(mut archive) => {
                        // Vollstaendige Lesbarkeit pruefen, um "corrupt deflate stream" frueh zu erkennen.
                        for i in 0..archive.len() {
                            let mut entry = match archive.by_index(i) {
                                Ok(e) => e,
                                Err(e) => {
                                    tracing::warn!("Invalid ZIP entry {:?} at {}: {}", path, i, e);
                                    return false;
                                }
                            };

                            if entry.name().ends_with('/') {
                                continue;
                            }

                            if std::io::copy(&mut entry, &mut std::io::sink()).is_err() {
                                tracing::warn!("Unreadable ZIP payload in {:?} at index {}", path, i);
                                return false;
                            }
                        }
                        true
                    }
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
        #[allow(non_snake_case)]
        struct NeoForgeVersion {
            mainClass: String,
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
        let original_main_class = version.mainClass.clone();
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
                    let is_archive = art.path.ends_with(".jar") || art.path.ends_with(".zip");

                    if dest.exists() && is_archive && !Self::is_valid_zip(&dest) {
                        tracing::warn!("Corrupt cached archive detected, re-downloading: {:?}", dest);
                        tokio::fs::remove_file(&dest).await.ok();
                    }

                    if !dest.exists() {
                        tracing::info!("Downloading: {}", lib.name);
                        tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                        self.download_manager.download_with_hash(&art.url, &dest, Some(&art.sha1)).await?;
                    }

                    // Modernes Format (1.19+): natives-JARs haben "natives-<os>" im Pfad
                    // z.B. lwjgl-3.3.3-natives-linux.jar → extrahieren, NICHT in Classpath
                    let is_native_jar = art.path.contains("natives-linux")
                        || art.path.contains("natives-windows")
                        || art.path.contains("natives-osx")
                        || art.path.contains("natives-macos");

                    if is_native_jar {
                        // Nur Linux-Natives auf Linux extrahieren
                        if (os == "linux" && art.path.contains("natives-linux"))
                            || (os == "windows" && art.path.contains("natives-windows"))
                            || (os == "osx" && (art.path.contains("natives-osx") || art.path.contains("natives-macos")))
                        {
                            if !Self::is_valid_zip(&dest) {
                                tracing::warn!("Corrupt native archive detected, re-downloading: {:?}", dest);
                                tokio::fs::remove_file(&dest).await.ok();
                                self.download_manager.download_with_hash(&art.url, &dest, Some(&art.sha1)).await?;
                                if !Self::is_valid_zip(&dest) {
                                    bail!("Native archive remains corrupt after redownload: {}", dest.display());
                                }
                            }
                            tracing::debug!("Extracting native: {}", lib.name);
                            self.extract_native(&dest, natives_dir)?;
                        }
                        // Natives kommen NICHT in den Classpath
                    } else {
                        cp.push(dest.display().to_string());
                    }
                } else {
                    tracing::debug!("Library {} has no artifact", lib.name);
                }

                // Altes Format (pre-1.19): classifiers mit "natives-linux" key
                if let Some(natives_map) = &lib.natives {
                    if let Some(key) = natives_map.get(&os) {
                        if let Some(cls) = &dl.classifiers {
                            if let Some(nat) = cls.get(key) {
                                let dest = lib_dir.join(&nat.path);
                                if dest.exists() && !Self::is_valid_zip(&dest) {
                                    tracing::warn!("Corrupt legacy native archive detected, re-downloading: {:?}", dest);
                                    tokio::fs::remove_file(&dest).await.ok();
                                }
                                if !dest.exists() {
                                    tracing::info!("Downloading native (legacy): {}", lib.name);
                                    tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                                    self.download_manager.download_with_hash(&nat.url, &dest, Some(&nat.sha1)).await?;
                                }
                                if !Self::is_valid_zip(&dest) {
                                    bail!("Legacy native archive is corrupt: {}", dest.display());
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
        Ok(join_classpath_entries(cp))
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

        for asset in idx.objects.values() {
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
        let file = std::fs::File::open(jar)
            .map_err(|e| anyhow::anyhow!("Cannot open native JAR {:?}: {}", jar, e))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| anyhow::anyhow!("Cannot read native JAR {:?}: {}", jar, e))?;

        for i in 0..archive.len() {
            let mut f = archive.by_index(i)?;
            let name = f.name().to_string();

            // Überspringe Verzeichnisse und META-INF
            if name.ends_with('/') || name.starts_with("META-INF") { continue; }

            // Extrahiere nur .so / .dll / .dylib
            let native_ext = name.ends_with(".so")
                || name.ends_with(".dll")
                || name.ends_with(".dylib");
            if !native_ext { continue; }

            // LWJGL 3.3.2+ packt Natives nach Architektur:
            // linux/x64/org/lwjgl/liblwjgl.so
            // linux/arm64/org/lwjgl/liblwjgl.so  ← auf x86_64 NICHT extrahieren!
            // Prüfe ob der Pfad eine falsche Architektur enthält und überspringe sie.
            let arch = std::env::consts::ARCH; // "x86_64", "aarch64", "arm", ...
            let path_lower = name.to_lowercase();

            let wrong_arch = match arch {
                "x86_64" => path_lower.contains("/arm64/") || path_lower.contains("/arm32/") || path_lower.contains("/arm/"),
                "aarch64" => path_lower.contains("/x64/") || path_lower.contains("/x86/") || path_lower.contains("/arm32/"),
                "arm"     => path_lower.contains("/x64/") || path_lower.contains("/x86/") || path_lower.contains("/arm64/"),
                _         => false,
            };
            if wrong_arch {
                tracing::debug!("Skipping wrong-arch native: {}", name);
                continue;
            }

            // Nur den Dateinamen (letztes Pfad-Segment) verwenden
            let file_name = std::path::Path::new(&name)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&name);

            let dest = dir.join(file_name);
            // Immer überschreiben – stellt sicher dass die Natives zur aktuellen LWJGL-Version passen
            tracing::debug!("Extracting native: {} -> {:?}", name, dest);
            if let Ok(mut out) = std::fs::File::create(&dest) {
                std::io::copy(&mut f, &mut out)?;
            }
        }
        Ok(())
    }

    fn find_java(&self) -> Result<String> {
        // 1. JAVA_HOME
        if let Ok(home) = std::env::var("JAVA_HOME") {
            let p = PathBuf::from(&home).join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
            if p.exists() { return Ok(p.display().to_string()); }
        }

        // 2. Launcher's own managed Java
        let java_bin = defaults::java_dir().join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
        if java_bin.exists() {
            return Ok(java_bin.display().to_string());
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

    /// Findet oder installiert Java mit der passenden Version.
    /// `max_major`: Wenn gesetzt, wird NUR Java im Bereich [required_major, max_major] akzeptiert.
    ///              Wichtig für alte Forge-Versionen die Nashorn brauchen (Java ≤ 14).
    async fn ensure_java_installed(&self, required_major: u32, max_major: Option<u32>) -> Result<String> {
        let java_bin_name = if cfg!(windows) { "java.exe" } else { "java" };

        let version_ok = |v: u32| -> bool {
            v >= required_major && max_major.map_or(true, |max| v <= max)
        };

        let label = if let Some(max) = max_major {
            format!("Java {}-{}", required_major, max)
        } else {
            format!("Java {}+", required_major)
        };

        tracing::info!("Looking for {}", label);

        // 1. JAVA_HOME (nur wenn Version passt)
        if let Ok(home) = std::env::var("JAVA_HOME") {
            let p = PathBuf::from(&home).join("bin").join(java_bin_name);
            if p.exists() {
                let v = Self::java_major_version(&p.display().to_string()).await;
                if version_ok(v) {
                    tracing::info!("Using JAVA_HOME Java {}: {}", v, p.display());
                    return Ok(p.display().to_string());
                }
            }
        }

        // 2. Launcher-managed Java (per-version Verzeichnisse: java/java-8/, java/java-21/ etc.)
        let java_base_dir = defaults::java_dir();
        // Bestimme die Zielversion für Download/Managed: bei max_major nutze required_major,
        // sonst required_major (wie bisher)
        let target_version = required_major;

        let java_dir = java_base_dir.join(format!("java-{}", target_version));
        let java_bin = java_dir.join("bin").join(java_bin_name);
        if java_bin.exists() {
            let installed = Self::java_major_version(&java_bin.display().to_string()).await;
            if version_ok(installed) {
                tracing::info!("Using managed Java {}: {}", installed, java_bin.display());
                return Ok(java_bin.display().to_string());
            }
        }
        // Auch andere managed-Versionen prüfen
        if java_base_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&java_base_dir) {
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let candidate = entry.path().join("bin").join(java_bin_name);
                        if candidate.exists() {
                            let v = Self::java_major_version(&candidate.display().to_string()).await;
                            if version_ok(v) {
                                tracing::info!("Using managed Java {}: {}", v, candidate.display());
                                return Ok(candidate.display().to_string());
                            }
                        }
                    }
                }
            }
        }
        // Legacy: altes einzelnes java/ Verzeichnis (Migration)
        let legacy_java_bin = java_base_dir.join("bin").join(java_bin_name);
        if legacy_java_bin.exists() {
            let v = Self::java_major_version(&legacy_java_bin.display().to_string()).await;
            if version_ok(v) {
                tracing::info!("Using legacy managed Java {}: {}", v, legacy_java_bin.display());
                return Ok(legacy_java_bin.display().to_string());
            }
        }

        // 3. System paths — Reihenfolge: bei max_major von niedrig nach hoch suchen
        let system_paths_low_first: &[&str] = if cfg!(target_os = "linux") {
            &[
                "/usr/lib/jvm/java-8-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-8-openjdk/bin/java",
                "/usr/lib/jvm/java-1.8.0-openjdk/bin/java",
                "/usr/lib/jvm/java-1.8.0-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-11-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-11-openjdk/bin/java",
                "/usr/lib/jvm/java-16-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-16-openjdk/bin/java",
                "/usr/lib/jvm/java-17-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-17-openjdk/bin/java",
                "/usr/lib/jvm/java-21-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-21-openjdk/bin/java",
                "/usr/bin/java",
            ]
        } else {
            &["java"]
        };
        let system_paths_high_first: &[&str] = if cfg!(target_os = "linux") {
            &[
                "/usr/lib/jvm/java-21-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-17-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-21-openjdk/bin/java",
                "/usr/lib/jvm/java-17-openjdk/bin/java",
                "/usr/lib/jvm/java-16-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-16-openjdk/bin/java",
                "/usr/lib/jvm/java-11-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-11-openjdk/bin/java",
                "/usr/lib/jvm/java-8-openjdk-amd64/bin/java",
                "/usr/lib/jvm/java-8-openjdk/bin/java",
                "/usr/lib/jvm/java-1.8.0-openjdk/bin/java",
                "/usr/bin/java",
            ]
        } else {
            &["java"]
        };

        // Bei max_major: niedrige Versionen zuerst (finde Java 8 vor Java 21)
        // Sonst: hohe Versionen zuerst (wie bisher)
        let system_paths: &[&str] = if max_major.is_some() {
            system_paths_low_first
        } else {
            system_paths_high_first
        };

        for p in system_paths {
            if Path::new(p).exists() {
                let v = Self::java_major_version(p).await;
                if version_ok(v) {
                    tracing::info!("Using system Java {}: {}", v, p);
                    return Ok(p.to_string());
                }
            }
        }

        if tokio::process::Command::new("java").arg("-version").output().await.is_ok() {
            let v = Self::java_major_version("java").await;
            if version_ok(v) {
                return Ok("java".to_string());
            }
        }

        // 4. Auto-download from Adoptium
        tracing::info!("Downloading Java {} from Adoptium...", target_version);
        tokio::fs::create_dir_all(&java_dir).await?;
        self.download_java(&java_dir, target_version).await?;
        if java_bin.exists() {
            let v = Self::java_major_version(&java_bin.display().to_string()).await;
            if version_ok(v) {
                tracing::info!("Java {} installed: {}", v, java_bin.display());
                return Ok(java_bin.display().to_string());
            }
        }
        bail!("{} installation failed. Please install {} manually.", label, label)
    }
    /// Returns the major version number of the given java binary (e.g. 21, 25).
    /// Returns 0 if the version cannot be determined.
    async fn java_major_version(java_bin: &str) -> u32 {
        // java -version writes to stderr, e.g.: openjdk version "21.0.2" 2024-01-16
        let Ok(out) = tokio::process::Command::new(java_bin)
            .arg("-version")
            .output().await
        else { return 0; };
        let text = String::from_utf8_lossy(&out.stderr);
        for line in text.lines() {
            if line.contains("version \"") {
                if let Some(start) = line.find('"') {
                    let rest = &line[start + 1..];
                    if let Some(end) = rest.find('"') {
                        let ver = &rest[..end];
                        // "21.0.2" -> 21,  "1.8.0_392" -> 8
                        let raw = ver.split('.').next().unwrap_or("0");
                        let major: u32 = if raw == "1" {
                            ver.split('.').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0)
                        } else {
                            raw.parse().unwrap_or(0)
                        };
                        if major > 0 { return major; }
                    }
                }
            }
        }
        0
    }
    async fn download_java(&self, java_dir: &Path, major: u32) -> Result<()> {
        let os = if cfg!(target_os = "windows") { "windows" }
                 else if cfg!(target_os = "macos") { "mac" }
                 else { "linux" };
        let arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x64" };
        let url = format!(
            "https://api.adoptium.net/v3/binary/latest/{}/ga/{}/{}/jre/hotspot/normal/eclipse",
            major, os, arch
        );
        tracing::info!("Downloading Java {} JRE from Adoptium...", major);
        let archive_name = if cfg!(windows) { "java_download.zip" } else { "java_download.tar.gz" };
        let archive_path = java_dir.parent().unwrap_or(java_dir).join(archive_name);
        self.download_manager
            .download_file(&url, &archive_path, None::<fn(u64, u64)>)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to download Java {}: {}", major, e))?;
        tracing::info!("Extracting Java {}...", major);
        if cfg!(windows) {
            let status = tokio::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &format!(
                    "Expand-Archive -Path '{p}' -DestinationPath '{d}_tmp' -Force;                      $s=Get-ChildItem '{d}_tmp'|?{{$_.PSIsContainer}}|select -First 1;                      if($s){{Copy-Item \"$($s.FullName)\\*\" '{d}' -Recurse -Force}};                      Remove-Item '{d}_tmp' -Recurse -Force",
                    p = archive_path.display(), d = java_dir.display()
                )])
                .status().await?;
            if !status.success() { bail!("Failed to extract Java on Windows"); }
        } else {
            let status = tokio::process::Command::new("tar")
                .args(["-xzf", &archive_path.display().to_string(),
                       "--strip-components=1",
                       "-C", &java_dir.display().to_string()])
                .status().await?;
            if !status.success() { bail!("Failed to extract Java archive with tar"); }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let bin = java_dir.join("bin").join("java");
            if bin.exists() {
                let mut perms = std::fs::metadata(&bin)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&bin, perms)?;
            }
        }
        tokio::fs::remove_file(&archive_path).await.ok();
        tracing::info!("Java {} installation complete.", major);
        Ok(())
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

/// Sucht alle natives-JARs für das gegebene OS im libraries-Verzeichnis.
/// Inkludiert LWJGL, jtracy (Mojang 1.21.11+) und andere Minecraft-natives.
fn walkdir_lwjgl_natives(dir: &Path, os: &str) -> std::io::Result<Vec<PathBuf>> {
    let suffix = match os {
        "linux"   => "-natives-linux.jar",
        "windows" => "-natives-windows.jar",
        "osx"     => "-natives-macos.jar",
        _         => "-natives-linux.jar",
    };
    // Alle relevanten natives inkl. jtracy (ab MC 1.21.11) und LWJGL
    let all = walkdir_find_natives(dir, suffix)?;
    Ok(all.into_iter().filter(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| {
                n.starts_with("lwjgl")
                    || n.contains("glfw")
                    || n.contains("openal")
                    || n.contains("jemalloc")
                    || n.contains("stb")
                    || n.contains("freetype")
                    || n.contains("jtracy")   // Mojang Tracy profiler (MC 1.21.11+)
                    || n.contains("tinyfd")
            })
            .unwrap_or(false)
    }).collect())
}

/// Durchsucht `dir` rekursiv nach JARs die `suffix` im Dateinamen haben.
fn walkdir_find_natives(dir: &Path, suffix: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut results = Vec::new();
    if !dir.is_dir() { return Ok(results); }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Ok(mut sub) = walkdir_find_natives(&path, suffix) {
                results.append(&mut sub);
            }
        } else if path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.ends_with(suffix))
            .unwrap_or(false)
        {
            results.push(path);
        }
    }
    Ok(results)
}

