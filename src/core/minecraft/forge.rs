/// Forge-Installer und Launch-Support
///
/// Implementiert den korrekten Forge-Installationsprozess:
/// 1. Installer-JAR herunterladen
/// 2. install_profile.json + version.json extrahieren
/// 3. Alle benötigten Libraries herunterladen
/// 4. Prozessoren ausführen (jarsplitter → installertools → binarypatcher)
/// 5. Korrekten --gameJar Pfad zurückgeben
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::io::Read;
use crate::core::download::DownloadManager;

#[derive(serde::Deserialize, Debug, Clone)]
struct Processor {
    jar: String,
    classpath: Vec<String>,
    args: Vec<String>,
    sides: Option<Vec<String>>,
}

#[derive(serde::Deserialize, Debug, Clone)]
struct SidedData {
    client: String,
    #[allow(dead_code)]
    server: String,
}

pub struct ForgeInstallResult {
    pub main_class: String,
    pub bootstrap_classpath: Vec<String>,
    pub native_jars: Vec<String>,
    pub classpath: Vec<String>,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub mcp_version: String,
    pub forge_version: String,
    pub is_bootstrap: bool,
    pub patched_client_jar: PathBuf,
    /// Legacy Forge (≤1.12.2): minecraftArguments als einzelner String mit ${Platzhaltern}
    pub minecraft_arguments: Option<String>,
}

pub struct ForgeInstaller {
    pub download_manager: DownloadManager,
}

impl ForgeInstaller {
    pub fn new(download_manager: DownloadManager) -> Self {
        Self { download_manager }
    }

    pub async fn install_forge_complete(
        &self,
        mc_version: &str,
        forge_version: &str,
        libraries_dir: &Path,
        client_jar: &Path,
    ) -> Result<ForgeInstallResult> {
        use crate::api::forge::ForgeClient;

        tracing::info!("=== Forge Install: {}-{} ===", mc_version, forge_version);

        // Alte Forge-Versionen für diese MC-Version aufräumen
        Self::cleanup_old_forge_versions(mc_version, forge_version, libraries_dir).await;

        // Installer-Pfad
        let installer_dir = libraries_dir.join("forge-installer").join(mc_version);
        let installer_path = installer_dir.join(
            format!("forge-{}-{}-installer.jar", mc_version, forge_version)
        );
        let forge_client = ForgeClient::new()?;
        let installer_url = forge_client.get_installer_url(mc_version, forge_version);

        // Corrupt installer aufräumen
        if installer_path.exists() && !Self::is_valid_zip(&installer_path) {
            tracing::warn!("Installer JAR korrupt, lade neu...");
            tokio::fs::remove_file(&installer_path).await.ok();
        }

        // Installer herunterladen
        if !installer_path.exists() {
            tracing::info!("Lade Forge Installer: {}", installer_url);
            tokio::fs::create_dir_all(&installer_dir).await?;
            self.download_manager.download_with_hash(&installer_url, &installer_path, None).await?;
            if !Self::is_valid_zip(&installer_path) {
                tokio::fs::remove_file(&installer_path).await.ok();
                bail!("Heruntergeladener Forge Installer ist kein gültiges ZIP/JAR");
            }
        }

        // Versions-spezifisches Datenverzeichnis
        let installer_data_dir = libraries_dir
            .join("forge-installer-data")
            .join(format!("{}-{}", mc_version, forge_version));
        tokio::fs::create_dir_all(&installer_data_dir).await?;

        // ── Installer-Inhalte lesen ──────────────────────────────────────────────
        let (version_json_str, install_profile_str) = Self::read_installer_contents(
            &installer_path, &installer_data_dir, libraries_dir
        ).await?;

        // ── version.json parsen ──────────────────────────────────────────────────
        #[derive(serde::Deserialize)]
        struct VersionJson {
            #[serde(rename = "mainClass")]
            main_class: String,
            libraries: Vec<ForgeLib>,
            arguments: Option<ForgeArgs>,
            /// Legacy Forge (≤1.12.2): Argumente als einzelner String
            #[serde(rename = "minecraftArguments")]
            minecraft_arguments: Option<String>,
        }
        #[derive(serde::Deserialize)]
        struct ForgeArgs {
            jvm: Option<Vec<serde_json::Value>>,
            game: Option<Vec<serde_json::Value>>,
        }
        #[derive(serde::Deserialize, Clone)]
        struct ForgeLib {
            name: String,
            downloads: Option<LibDownloads>,
            /// Legacy Forge: Base-URL für Maven-Downloads (z.B. "http://files.minecraftforge.net/maven/")
            url: Option<String>,
        }
        #[derive(serde::Deserialize, Clone)]
        struct LibDownloads {
            artifact: Option<LibArtifact>,
        }
        #[derive(serde::Deserialize, Clone)]
        struct LibArtifact {
            url: String,
            path: String,
            sha1: Option<String>,
        }

        let version_json: VersionJson = serde_json::from_str(&version_json_str)
            .map_err(|e| anyhow::anyhow!("Fehler beim Parsen von version.json: {}", e))?;

        let is_bootstrap = version_json.main_class.contains("ForgeBootstrap");
        tracing::info!("Main class: {} (bootstrap={})", version_json.main_class, is_bootstrap);

        // ── install_profile.json parsen ──────────────────────────────────────────
        #[derive(serde::Deserialize)]
        struct InstallProfile {
            version: Option<String>,
            libraries: Option<Vec<ForgeLib>>,
            processors: Option<Vec<Processor>>,
            data: Option<std::collections::HashMap<String, SidedData>>,
        }
        let install_profile: InstallProfile = serde_json::from_str(&install_profile_str)
            .unwrap_or(InstallProfile {
                version: None, libraries: None, processors: None, data: None
            });

        // Forge-Version aus install_profile.version extrahieren
        // Bekannte Formate:
        //   "1.20.1-forge-47.3.0"  → enthält "-forge-" → split liefert "47.3.0"
        //   "1.20.3-forge49.0.2"   → enthält "-forge" ohne Trennstrich → split liefert "forge49.0.2"
        //   "1.20.3-49.0.2"        → kein "forge" → split('-') liefert "49.0.2"
        let forge_version_resolved = install_profile.version.as_deref()
            .and_then(|v| {
                tracing::debug!("install_profile.version = {:?}", v);
                if v.contains("-forge-") {
                    // Format: "1.20.1-forge-47.3.0"
                    v.split("-forge-").nth(1).map(|s| s.to_string())
                } else if let Some(pos) = v.find("-forge") {
                    // Format: "1.20.3-forge49.0.2" (ohne Trennstrich nach "forge")
                    let after_forge = &v[pos + "-forge".len()..];
                    if after_forge.is_empty() {
                        None
                    } else {
                        Some(after_forge.to_string())
                    }
                } else {
                    v.split('-').next_back().map(|s| s.to_string())
                }
            })
            .unwrap_or_else(|| forge_version.to_string());

        // Cleanup: Falls doch noch ein "forge"-Präfix übrig ist (z.B. "forge49.0.2" → "49.0.2")
        let forge_version_resolved = forge_version_resolved
            .strip_prefix("forge")
            .filter(|s| s.starts_with(|c: char| c.is_ascii_digit()))
            .map(|s| s.to_string())
            .unwrap_or(forge_version_resolved);

        // MCP-Version aus version.json libraries extrahieren
        // Format: "de.oceanlabs.mcp:mcp_config:1.21.7-20250630.104312" → "20250630.104312"
        let mcp_version_from_libs = version_json.libraries.iter()
            .find_map(|lib| {
                if lib.name.contains("mcp_config") || lib.name.contains("mcpConfig") {
                    let parts: Vec<&str> = lib.name.split(':').collect();
                    if let Some(ver) = parts.get(2) {
                        if let Some(mcp) = ver.split_once('-').map(|x| x.1) {
                            return Some(mcp.to_string());
                        }
                        if ver.contains('.') && ver.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                            return Some(ver.to_string());
                        }
                    }
                }
                None
            });

        // MCP-Version auch aus install_profile.data extrahieren (zuverlässiger)
        // Format: data["MCP_VERSION"] = { client: "'20250630.104312'", ... }
        let mcp_version_from_data = install_profile.data.as_ref()
            .and_then(|d| d.get("MCP_VERSION"))
            .map(|v| v.client.trim().trim_matches('\'').to_string())
            .filter(|s| !s.is_empty());

        // MCP-Version aus version.json game args extrahieren (zusätzliche Quelle)
        // Suche nach "--fml.mcpVersion" gefolgt vom Wert in arguments.game
        let mcp_version_from_game_args: Option<String> = version_json.arguments.as_ref()
            .and_then(|args| args.game.as_ref())
            .and_then(|game| {
                let strs: Vec<String> = game.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                strs.windows(2)
                    .find(|w| w[0] == "--fml.mcpVersion")
                    .map(|w| w[1].clone())
            })
            .filter(|s| !s.is_empty() && s.starts_with(|c: char| c.is_ascii_digit()));

        let mcp_version = mcp_version_from_data
            .or(mcp_version_from_game_args)
            .or(mcp_version_from_libs)
            .unwrap_or_else(|| {
                // Versionsspezifische MCP-Fallbacks statt eines einzigen Hardcodes
                let fallback = match mc_version {
                    "1.20.1" => "20230612.114412",
                    "1.20.2" => "20231019.122523",
                    "1.20.3" => "20231205.124940",
                    "1.20.4" => "20240104.091131",
                    "1.20.5" => "20240429.145121",
                    "1.20.6" => "20240429.145121",
                    "1.21"   => "20240613.152323",
                    "1.21.1" => "20240725.100730",
                    "1.21.2" => "20241029.085956",
                    "1.21.3" => "20241029.085956",
                    "1.21.4" => "20241203.161809",
                    _ => "20240808.144430",
                };
                tracing::warn!("MCP Version nicht aus Installer extrahierbar, nutze Fallback {} für MC {}", fallback, mc_version);
                fallback.to_string()
            });

        tracing::info!("forge_version={}, mcp_version={}", forge_version_resolved, mcp_version);

        // ── Installer-Libraries herunterladen ────────────────────────────────────
        if let Some(libs) = &install_profile.libraries {
            for lib in libs {
                let (url, path, sha1) = if let Some(dl) = &lib.downloads {
                    if let Some(art) = &dl.artifact {
                        (art.url.clone(), art.path.clone(), art.sha1.clone())
                    } else {
                        continue;
                    }
                } else {
                    (String::new(), Self::maven_to_path(&lib.name), None)
                };
                let dest = libraries_dir.join(&path);
                if dest.exists() { continue; }
                tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                if !url.is_empty() {
                    if self.download_manager.download_with_hash(&url, &dest, sha1.as_deref()).await.is_err() {
                        Self::download_from_maven_repos(&self.download_manager, &path, &dest).await;
                    }
                } else {
                    Self::download_from_maven_repos(&self.download_manager, &path, &dest).await;
                }
            }
        }

        // ── Prozessor-JARs vorab herunterladen ───────────────────────────────────
        if let Some(procs) = &install_profile.processors {
            for proc in procs {
                let sides = proc.sides.as_deref().unwrap_or(&[]);
                if !sides.is_empty() && !sides.contains(&"client".to_string()) { continue; }
                let jar_path = libraries_dir.join(Self::maven_to_path(&proc.jar));
                if !jar_path.exists() {
                    Self::download_from_maven_repos(
                        &self.download_manager,
                        &Self::maven_to_path(&proc.jar),
                        &jar_path
                    ).await;
                }
                for dep in &proc.classpath {
                    let dep_path = libraries_dir.join(Self::maven_to_path(dep));
                    if !dep_path.exists() {
                        Self::download_from_maven_repos(
                            &self.download_manager,
                            &Self::maven_to_path(dep),
                            &dep_path
                        ).await;
                    }
                }
            }
        }

        // ── Version-Libraries klassifizieren und herunterladen ───────────────────
        let mut bootstrap_classpath: Vec<String> = Vec::new();
        let mut native_jars: Vec<String> = Vec::new();
        let mut classpath: Vec<String> = Vec::new();
        let mut seen_paths: std::collections::HashSet<String> = Default::default();

        for lib in &version_json.libraries {
            let (url, path, sha1) = if let Some(dl) = &lib.downloads {
                if let Some(art) = &dl.artifact {
                    (art.url.clone(), art.path.clone(), art.sha1.clone())
                } else {
                    continue;
                }
            } else {
                // Legacy oder einfaches Format: url + maven_to_path
                let maven_path = Self::maven_to_path(&lib.name);
                let download_url = if let Some(base_url) = &lib.url {
                    let base = base_url.trim_end_matches('/');
                    format!("{}/{}", base, maven_path)
                } else {
                    String::new()
                };
                (download_url, maven_path, None)
            };

            let dest = libraries_dir.join(&path);
            let dest_str = dest.display().to_string();
            if !seen_paths.insert(dest_str.clone()) { continue; }

            if !dest.exists() {
                tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
                if !url.is_empty() {
                    if self.download_manager.download_with_hash(&url, &dest, sha1.as_deref()).await.is_err() {
                        Self::download_from_maven_repos(&self.download_manager, &path, &dest).await;
                    }
                } else {
                    Self::download_from_maven_repos(&self.download_manager, &path, &dest).await;
                }
            }

            if !dest.exists() {
                tracing::warn!("Library fehlt nach allen Download-Versuchen: {}", lib.name);
                continue;
            }

            let is_native = path.contains("natives-linux") || path.contains("natives-windows")
                || path.contains("natives-osx") || path.contains("natives-macos");

            if is_native {
                native_jars.push(dest_str);
            } else if Self::is_data_only_lib(&lib.name) {
                classpath.push(dest_str);
            } else {
                bootstrap_classpath.push(dest_str);
            }
        }

        tracing::info!("Libraries: {} bootstrap-cp, {} data, {} natives",
            bootstrap_classpath.len(), classpath.len(), native_jars.len());

        // ── JVM/Game-Args extrahieren ────────────────────────────────────────────
        let mut jvm_args = Vec::new();
        let mut game_args = Vec::new();
        if let Some(args) = &version_json.arguments {
            if let Some(jvm) = &args.jvm {
                for a in jvm { jvm_args.extend(Self::extract_arg_strings(a)); }
            }
            if let Some(game) = &args.game {
                for a in game { game_args.extend(Self::extract_arg_strings(a)); }
            }
        }

        // ── Prozessoren ausführen ────────────────────────────────────────────────
        let processors = install_profile.processors.unwrap_or_default();
        let data = install_profile.data.unwrap_or_default();

        let patched_client_jar = Self::run_processors(
            &processors,
            &data,
            libraries_dir,
            &installer_data_dir,
            &installer_path,
            client_jar,
            &self.download_manager,
            mc_version,
            forge_version,
            &mcp_version,
        ).await?;

        tracing::info!("--gameJar: {:?}", patched_client_jar.file_name().unwrap_or_default());

        // Sicherstellen dass die patched_client_jar im bootstrap_classpath ist.
        // Bei Forge 1.21.7+ hat die forge-client.jar eine leere URL in version.json,
        // wird also beim Download-Schritt übersprungen und ist nicht im bootstrap_classpath.
        // ForgeProdLaunchHandler.getMinecraftPaths() sucht Minecraft.class im SecureModuleClassLoader,
        // der vom bootstrap_classpath befüllt wird — also muss forge-client.jar dort sein!
        let patched_str = patched_client_jar.display().to_string();
        if !bootstrap_classpath.contains(&patched_str) {
            tracing::info!("patched_client_jar zu bootstrap_classpath hinzugefügt: {:?}", patched_client_jar.file_name().unwrap_or_default());
            bootstrap_classpath.insert(0, patched_str);
        }

        Ok(ForgeInstallResult {
            main_class: version_json.main_class,
            bootstrap_classpath,
            native_jars,
            classpath,
            jvm_args,
            game_args,
            mcp_version,
            forge_version: forge_version_resolved,
            is_bootstrap,
            patched_client_jar,
            minecraft_arguments: version_json.minecraft_arguments,
        })
    }

    /// Liest version.json und install_profile.json aus dem Installer.
    /// Extrahiert außerdem alle eingebetteten JARs (maven/) und Datendateien (data/).
    ///
    /// Legacy-Forge (≤1.12.2) hat kein separates version.json — die Versionsdaten
    /// stehen in install_profile.json unter dem Feld "versionInfo".
    async fn read_installer_contents(
        installer_path: &Path,
        installer_data_dir: &Path,
        libraries_dir: &Path,
    ) -> Result<(String, String)> {
        let file = std::fs::File::open(installer_path)?;
        let mut archive = zip::ZipArchive::new(file)?;

        // Versuche version.json zu lesen (modern format, 1.13+)
        let version_json_opt = match archive.by_name("version.json") {
            Ok(mut entry) => {
                let mut s = String::new();
                entry.read_to_string(&mut s)?;
                Some(s)
            }
            Err(_) => None,
        };

        let install_profile = {
            let mut entry = archive.by_name("install_profile.json")
                .map_err(|_| anyhow::anyhow!("install_profile.json nicht im Forge Installer gefunden"))?;
            let mut s = String::new();
            entry.read_to_string(&mut s)?;
            s
        };

        // Falls kein version.json: Legacy-Format (≤1.12.2)
        // install_profile.json enthält "versionInfo" mit den Versionsdaten
        let version_json = match version_json_opt {
            Some(vj) => vj,
            None => {
                tracing::info!("Legacy Forge Installer erkannt (kein version.json)");
                let profile_value: serde_json::Value = serde_json::from_str(&install_profile)
                    .map_err(|e| anyhow::anyhow!("Legacy install_profile.json parse error: {}", e))?;
                if let Some(version_info) = profile_value.get("versionInfo") {
                    serde_json::to_string(version_info)
                        .map_err(|e| anyhow::anyhow!("versionInfo serialization error: {}", e))?
                } else {
                    bail!("Weder version.json noch versionInfo in install_profile.json gefunden");
                }
            }
        };

        // Embedded JARs und Datendateien extrahieren
        let mut to_extract: Vec<(PathBuf, Vec<u8>)> = Vec::new();

        for i in 0..archive.len() {
            if let Ok(mut entry) = archive.by_index(i) {
                let name = entry.name().to_string();
                if name.ends_with('/') { continue; }

                if name.starts_with("maven/") && (name.ends_with(".jar") || name.ends_with(".lzma")) {
                    if let Some(rel) = name.strip_prefix("maven/") {
                        let dest = libraries_dir.join(rel);
                        if !dest.exists() {
                            let mut data = Vec::new();
                            entry.read_to_end(&mut data)?;
                            to_extract.push((dest, data));
                        }
                    }
                } else if name.starts_with("data/") {
                    // Datendateien werden MIT dem "data/"-Präfix gespeichert:
                    // "data/client.lzma" → installer_data_dir/data/client.lzma
                    // Dies ist konsistent mit resolve_single(), das "/data/client.lzma"
                    // in installer_data_dir + "/data/client.lzma" auflöst.
                    let dest = installer_data_dir.join(&name);
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data)?;
                    to_extract.push((dest, data));
                }
            }
        }

        for (dest, data) in to_extract {
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&dest, &data).await?;
        }

        Ok((version_json, install_profile))
    }

    /// Führt die Forge-Installer-Prozessoren aus.
    ///
    /// Wichtig: `data`-Werte können verschachtelte Maven-Referenzen enthalten:
    ///   `MC_SLIM` → `[net.minecraft:client:{MC_VERSION}:slim@jar]`
    /// Diese müssen erst in den tatsächlichen Dateipfad aufgelöst werden.
    ///
    /// Rückgabe: Pfad zur Minecraft-Client-JAR die als --gameJar verwendet werden soll.
    #[allow(clippy::too_many_arguments)]
    async fn run_processors(
        processors: &[Processor],
        data: &std::collections::HashMap<String, SidedData>,
        libraries_dir: &Path,
        installer_data_dir: &Path,
        installer_path: &Path,
        client_jar: &Path,
        download_manager: &DownloadManager,
        mc_version: &str,
        forge_version: &str,
        mcp_version: &str,
    ) -> Result<PathBuf> {
        // ── Resolve-Funktion für einzelne Werte ─────────────────────────────────
        // Verarbeitet:
        //   [group:artifact:version@ext]  → libraries_dir/...
        //   [group:artifact:version:cls]  → libraries_dir/...
        //   'literal'                     → literal (ohne Anführungszeichen)
        //   /path                         → installer_data_dir/path
        //   plain text                    → unverändert
        let resolve_single = |val: &str| -> String {
            let v = val.trim();
            if v.starts_with('[') && v.ends_with(']') {
                libraries_dir.join(Self::maven_to_path(&v[1..v.len()-1])).display().to_string()
            } else if v.starts_with('\'') && v.ends_with('\'') {
                v[1..v.len()-1].to_string()
            } else if v.starts_with('/') {
                installer_data_dir.join(v.trim_start_matches('/')).display().to_string()
            } else {
                v.to_string()
            }
        };

        // ── Basisdaten aufbauen ──────────────────────────────────────────────────
        let mut resolved_data: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        // Alle data-Einträge auflösen (client-Seite)
        for (k, v) in data {
            resolved_data.insert(k.clone(), resolve_single(&v.client));
        }

        // Basis-Variablen
        resolved_data.insert("MINECRAFT_JAR".to_string(), client_jar.display().to_string());
        resolved_data.insert("INSTALLER".to_string(), installer_path.display().to_string());
        resolved_data.insert("ROOT".to_string(),
            libraries_dir.parent().unwrap_or(libraries_dir).display().to_string());
        resolved_data.insert("LIBRARIES_DIR".to_string(), libraries_dir.display().to_string());
        resolved_data.insert("SIDE".to_string(), "client".to_string());
        resolved_data.insert("MC_VERSION".to_string(), mc_version.to_string());
        resolved_data.insert("FORGE_VERSION".to_string(), forge_version.to_string());

        // MCP_VERSION: aus data (bereits aufgelöst) übernehmen, oder aus Parameter setzen.
        // Wichtig: data enthält MCP_VERSION bereits mit resolve_single() aufgelöst (Quotes entfernt).
        // Der Parameter mcp_version kommt aus install_forge_complete und ist dort korrekt extrahiert.
        if !resolved_data.contains_key("MCP_VERSION") || resolved_data.get("MCP_VERSION").map(|s| s.is_empty()).unwrap_or(true) {
            tracing::info!("MCP_VERSION aus Parameter: {}", mcp_version);
            resolved_data.insert("MCP_VERSION".to_string(), mcp_version.to_string());
        } else {
            tracing::info!("MCP_VERSION aus install_profile.data: {}", resolved_data["MCP_VERSION"]);
        }

        // Ob BINPATCH vorhanden ist (Forge < ~1.21.6)
        let has_binpatch = data.contains_key("BINPATCH");
        tracing::info!("BINPATCH vorhanden: {} (Forge 1.21.6+ hat keins mehr)", has_binpatch);

        // PATCHED-Zielpfad bestimmen
        let patched_path = data.get("PATCHED")
            .map(|v| PathBuf::from(resolve_single(&v.client)))
            .unwrap_or_else(|| {
                // Fallback: bekannter Pfad für gemappte Forge-Client-JAR
                libraries_dir.join(format!(
                    "net/minecraft/client/{}/client-{}-official.jar",
                    mc_version, mc_version
                ))
            });

        // Wenn PATCHED bereits existiert und Minecraft-Klassen enthält → sofort zurückgeben
        if patched_path.exists()
            && patched_path.metadata().map(|m| m.len() > 0).unwrap_or(false)
            && Self::jar_contains_minecraft_class(&patched_path)
        {
            tracing::info!("✅ Gepatchte JAR bereits vorhanden: {:?}", patched_path.file_name().unwrap_or_default());
            return Ok(patched_path);
        }

        // Korrupte patched_path entfernen
        if patched_path.exists() {
            tracing::warn!("patched_path existiert aber enthält keine MC-Klassen → wird neu erzeugt");
            std::fs::remove_file(&patched_path).ok();
        }

        tracing::info!("Führe {} Forge-Prozessoren aus...", processors.len());
        let java = find_java_binary();

        // ── Prozessoren ausführen ────────────────────────────────────────────────
        for proc in processors {
            // Nur Client-Seite
            let sides = proc.sides.as_deref().unwrap_or(&[]);
            if !sides.is_empty() && !sides.contains(&"client".to_string()) {
                tracing::debug!("Überspringe Server-only Prozessor: {}", proc.jar);
                continue;
            }

            // Argumente auflösen: Zuerst {PLATZHALTER} ersetzen, dann [maven] auflösen
            let mut resolved_args: Vec<String> = proc.args.iter().map(|arg| {
                // Schritt 1: {PLACEHOLDER} → Wert aus resolved_data
                let mut r = arg.clone();
                for (k, v) in &resolved_data {
                    r = r.replace(&format!("{{{}}}", k), v);
                }
                // Schritt 2: [maven:coords] → Dateipfad
                if r.starts_with('[') && r.ends_with(']') {
                    r = libraries_dir.join(Self::maven_to_path(&r[1..r.len()-1])).display().to_string();
                }
                // Schritt 3: /data/file → installer_data_dir/file
                if r.starts_with('/') && !r.starts_with("//") && !std::path::Path::new(&r).exists() {
                    let candidate = installer_data_dir.join(r.trim_start_matches('/'));
                    if candidate.exists() {
                        r = candidate.display().to_string();
                    }
                }
                r
            }).collect();

            // binarypatcher ohne BINPATCH → überspringen, MC_OFF direkt nach PATCHED kopieren
            if proc.jar.contains("binarypatcher") && !has_binpatch {
                tracing::info!("Forge 1.21.6+ ohne BINPATCH: Überspringe binarypatcher");
                // MC_OFF → PATCHED kopieren (falls MC_OFF Minecraft-Klassen enthält)
                let mc_off = resolved_data.get("MC_OFF").cloned().unwrap_or_default();
                let patched_out = resolved_data.get("PATCHED").cloned().unwrap_or_default();
                if !mc_off.is_empty() && !patched_out.is_empty() {
                    let src = PathBuf::from(&mc_off);
                    let dst = PathBuf::from(&patched_out);
                    if src.exists() && Self::jar_contains_minecraft_class(&src) && !dst.exists() {
                        if let Some(p) = dst.parent() {
                            tokio::fs::create_dir_all(p).await?;
                        }
                        tokio::fs::copy(&src, &dst).await?;
                        tracing::info!("MC_OFF → PATCHED kopiert: {:?}", dst.file_name().unwrap_or_default());
                    }
                }
                continue;
            }

            // Noch nicht aufgelöste Platzhalter entfernen
            let mut i = 0;
            while i < resolved_args.len() {
                if resolved_args[i].contains('{') && resolved_args[i].contains('}') {
                    tracing::debug!("Entferne ungelösten Platzhalter: {}", resolved_args[i]);
                    if i > 0 && resolved_args[i-1].starts_with("--") {
                        resolved_args.remove(i);
                        if i > 0 { resolved_args.remove(i - 1); }
                        i = i.saturating_sub(1);
                    } else {
                        resolved_args.remove(i);
                    }
                    continue;
                }
                i += 1;
            }

            // Output-Datei prüfen → überspringen wenn bereits vorhanden
            let output_path = resolved_args.windows(2)
                .find(|w| w[0] == "--output")
                .map(|w| PathBuf::from(&w[1]));

            if let Some(ref out) = output_path {
                if out.exists() && out.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                    tracing::info!("Output bereits vorhanden, überspringe: {:?}", out.file_name().unwrap_or_default());
                    continue;
                }
                if let Some(p) = out.parent() {
                    tokio::fs::create_dir_all(p).await?;
                }
            }

            // Prozessor-JAR herunterladen falls nötig
            let proc_jar = libraries_dir.join(Self::maven_to_path(&proc.jar));
            if !proc_jar.exists() {
                Self::download_from_maven_repos(
                    download_manager,
                    &Self::maven_to_path(&proc.jar),
                    &proc_jar
                ).await;
                if !proc_jar.exists() {
                    tracing::warn!("Prozessor-JAR fehlt, überspringe: {:?}", proc_jar);
                    continue;
                }
            }

            // Main-Class aus Manifest lesen
            let main_class = Self::read_manifest_main_class(&proc_jar)
                .unwrap_or_else(|| "net.minecraftforge.installertools.InstallerToolsMain".to_string());

            // Prozessor-Classpath aufbauen
            let mut proc_cp = vec![proc_jar.display().to_string()];
            for dep in &proc.classpath {
                let dep_path = libraries_dir.join(Self::maven_to_path(dep));
                if !dep_path.exists() {
                    Self::download_from_maven_repos(
                        download_manager,
                        &Self::maven_to_path(dep),
                        &dep_path
                    ).await;
                }
                if dep_path.exists() {
                    proc_cp.push(dep_path.display().to_string());
                }
            }

            tracing::info!("Prozessor: {} → {}", proc.jar, main_class);
            tracing::info!("Argumente: {:?}", resolved_args);

            let cp_sep = if cfg!(windows) { ";" } else { ":" };
            let out = tokio::process::Command::new(&java)
                .arg("-cp").arg(proc_cp.join(cp_sep))
                .arg(&main_class)
                .args(&resolved_args)
                .output().await;

            match out {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    if !stdout.trim().is_empty() {
                        tracing::info!("Prozessor stdout: {}", stdout.trim());
                    }
                    if !stderr.trim().is_empty() {
                        if o.status.success() {
                            tracing::debug!("Prozessor stderr: {}", stderr.trim());
                        } else {
                            tracing::error!("Prozessor stderr: {}", stderr.trim());
                        }
                    }
                    if o.status.success() {
                        tracing::info!("✅ Prozessor OK: {}", proc.jar);
                        // Wenn ein Output erzeugt wurde, prüfen ob er existiert
                        if let Some(ref out_path) = output_path {
                            if out_path.exists() {
                                tracing::info!("   Output erzeugt: {:?} ({} bytes)",
                                    out_path.file_name().unwrap_or_default(),
                                    out_path.metadata().map(|m| m.len()).unwrap_or(0));
                            } else {
                                tracing::warn!("   ⚠️ Output wurde NICHT erzeugt: {:?}", out_path);
                            }
                        }
                    } else {
                        tracing::error!("❌ Prozessor FEHLGESCHLAGEN (Exit {}): {}", o.status, proc.jar);
                    }
                }
                Err(e) => tracing::error!("Prozessor konnte nicht gestartet werden: {}", e),
            }
        }

        // ── Ergebnis-JAR bestimmen ───────────────────────────────────────────────
        // Priorität:
        // 1. PATCHED-Pfad (von binarypatcher oder kopiert von MC_OFF)
        // 2. MC_OFF (vom installertools DOWNLOAD_MOJMAPS)
        // 3. Bekannte client-{ver}-official.jar
        // 4. Vanilla client_jar (Fallback)

        // 1. PATCHED
        if patched_path.exists() && Self::jar_contains_minecraft_class(&patched_path) {
            tracing::info!("✅ Verwende PATCHED JAR: {:?}", patched_path.file_name().unwrap_or_default());
            return Ok(patched_path);
        }

        // 2. MC_OFF
        if let Some(mc_off_str) = resolved_data.get("MC_OFF") {
            let mc_off = PathBuf::from(mc_off_str);
            if mc_off.exists() && Self::jar_contains_minecraft_class(&mc_off) {
                tracing::info!("✅ Verwende MC_OFF als --gameJar: {:?}", mc_off.file_name().unwrap_or_default());
                // Für zukünftige Starts nach PATCHED kopieren
                if !patched_path.exists() {
                    if let Some(parent) = patched_path.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    tokio::fs::copy(&mc_off, &patched_path).await.ok();
                }
                return Ok(mc_off);
            }
        }

        // 3. Suche nach client-{ver}-official.jar in libraries/net/minecraft/
        let official_candidates = [
            libraries_dir.join(format!("net/minecraft/client/{}/client-{}-official.jar", mc_version, mc_version)),
            libraries_dir.join(format!("net/minecraft/client/{}-{}/client-{}-{}-srg.jar", mc_version, mcp_version, mc_version, mcp_version)),
            libraries_dir.join(format!("net/minecraft/client/{}-{}/client-{}-{}-srg.jar", mc_version, "20240808.144430", mc_version, "20240808.144430")),
        ];
        for candidate in &official_candidates {
            if candidate.exists() && Self::jar_contains_minecraft_class(candidate) {
                tracing::info!("✅ Verwende gefundene Client-JAR: {:?}", candidate.file_name().unwrap_or_default());
                return Ok(candidate.clone());
            }
        }

        // Suche in libraries/net/minecraft/client/ nach beliebiger JAR mit MC-Klassen
        let mc_client_dir = libraries_dir.join("net/minecraft/client");
        if mc_client_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&mc_client_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        if let Ok(inner) = std::fs::read_dir(&p) {
                            for file in inner.flatten() {
                                let fp = file.path();
                                if fp.extension().and_then(|e| e.to_str()) == Some("jar")
                                    && !fp.to_string_lossy().contains("sources")
                                    && !fp.to_string_lossy().contains("javadoc")
                                    && Self::jar_contains_minecraft_class(&fp)
                                {
                                    tracing::info!("✅ Gefundene MC-Client-JAR: {:?}", fp.file_name().unwrap_or_default());
                                    return Ok(fp);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 4. Forge-Client-JAR selbst (enthält MC-Klassen bei modernen Versionen)
        let forge_client_jar = libraries_dir.join(format!(
            "net/minecraftforge/forge/{}-{}/forge-{}-{}-client.jar",
            mc_version, forge_version, mc_version, forge_version
        ));
        if forge_client_jar.exists() && Self::jar_contains_minecraft_class(&forge_client_jar) {
            tracing::info!("✅ Verwende Forge-Client-JAR: {:?}", forge_client_jar.file_name().unwrap_or_default());
            return Ok(forge_client_jar);
        }

        // 5. Vanilla client_jar als letzter Fallback
        if client_jar.exists() && Self::jar_contains_minecraft_class(client_jar) {
            tracing::info!("⚠️  Verwende Vanilla client.jar als Fallback: {:?}", client_jar.file_name().unwrap_or_default());
            return Ok(client_jar.to_path_buf());
        }

        bail!("Keine gültige Minecraft-Client-JAR nach allen Prozessoren und Fallbacks gefunden für MC {} Forge {}", mc_version, forge_version)
    }

    pub fn maven_to_path(maven: &str) -> String {
        // Unterstützt:
        // group:artifact:version          → group/artifact/version/artifact-version.jar
        // group:artifact:version:classifier → group/artifact/version/artifact-version-classifier.jar
        // group:artifact:version@ext       → group/artifact/version/artifact-version.ext
        // group:artifact:version:classifier@ext → mit Classifier und Erweiterung
        let (coords, ext) = if let Some(at) = maven.find('@') {
            (&maven[..at], &maven[at + 1..])
        } else {
            (maven, "jar")
        };
        let parts: Vec<&str> = coords.split(':').collect();
        if parts.len() < 3 {
            return format!("{}.{}", maven.replace(':', "/"), ext);
        }
        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2];
        if parts.len() >= 4 {
            // Mit Classifier: artifact-version-classifier.ext
            format!("{}/{}/{}/{}-{}-{}.{}", group, artifact, version, artifact, version, parts[3], ext)
        } else {
            format!("{}/{}/{}/{}-{}.{}", group, artifact, version, artifact, version, ext)
        }
    }

    fn is_data_only_lib(maven_name: &str) -> bool {
        maven_name.contains("mcp_config") || maven_name.contains("mcpConfig")
    }

    pub fn extract_arg_strings(arg: &serde_json::Value) -> Vec<String> {
        if let Some(s) = arg.as_str() {
            vec![s.to_string()]
        } else if let Some(obj) = arg.as_object() {
            if let Some(value) = obj.get("value") {
                if let Some(s) = value.as_str() {
                    vec![s.to_string()]
                } else if let Some(arr) = value.as_array() {
                    arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    }

    fn read_manifest_main_class(jar: &Path) -> Option<String> {
        let file = std::fs::File::open(jar).ok()?;
        let mut archive = zip::ZipArchive::new(file).ok()?;
        let mut entry = archive.by_name("META-INF/MANIFEST.MF").ok()?;
        let mut content = String::new();
        entry.read_to_string(&mut content).ok()?;
        content.lines()
            .find(|l| l.starts_with("Main-Class:"))
            .map(|l| l["Main-Class:".len()..].trim().to_string())
    }

    pub fn jar_contains_minecraft_class(jar_path: &Path) -> bool {
        if !jar_path.exists() { return false; }
        match std::fs::File::open(jar_path) {
            Ok(file) => match zip::ZipArchive::new(file) {
                Ok(archive) => archive.file_names().any(|n| {
                    n.starts_with("net/minecraft/") && n.ends_with(".class")
                }),
                Err(_) => false,
            },
            Err(_) => false,
        }
    }

    fn is_valid_zip(path: &Path) -> bool {
        match std::fs::File::open(path) {
            Ok(file) => {
                let mut archive = match zip::ZipArchive::new(file) {
                    Ok(a) => a,
                    Err(_) => return false,
                };

                for i in 0..archive.len() {
                    let mut entry = match archive.by_index(i) {
                        Ok(e) => e,
                        Err(_) => return false,
                    };

                    if entry.name().ends_with('/') {
                        continue;
                    }

                    if std::io::copy(&mut entry, &mut std::io::sink()).is_err() {
                        return false;
                    }
                }

                true
            }
            Err(_) => false,
        }
    }

    pub async fn download_from_maven_repos(dm: &DownloadManager, maven_path: &str, dest: &Path) {
        let repos = [
            "https://maven.minecraftforge.net",
            "https://maven.neoforged.net/releases",
            "https://libraries.minecraft.net",
            "https://repo1.maven.org/maven2",
        ];
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        for repo in &repos {
            let url = format!("{}/{}", repo, maven_path);
            if dm.download_with_hash(&url, dest, None).await.is_ok()
                && dest.exists() && std::fs::metadata(dest).map(|m| m.len() > 0).unwrap_or(false) {
                    tracing::debug!("Heruntergeladen von {}: {}", repo, maven_path);
                    return;
                }
        }
        tracing::warn!("Konnte {} von keinem Maven-Repo herunterladen", maven_path);
    }

    pub async fn cleanup_old_forge_versions(mc_version: &str, forge_version: &str, libraries_dir: &Path) {
        let target_suffix = format!("{}-{}", mc_version, forge_version);
        let forge_dirs = [
            "forge", "fmlcore", "fmlloader", "fmlearlydisplay",
            "javafmllanguage", "lowcodelanguage", "mclanguage", "forge-transformers",
        ];
        let forge_base = libraries_dir.join("net").join("minecraftforge");
        for dir_name in &forge_dirs {
            let dir = forge_base.join(dir_name);
            if !dir.exists() { continue; }
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let vd = entry.path();
                    if !vd.is_dir() { continue; }
                    let vname = vd.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    if vname.starts_with(mc_version) && vname != target_suffix {
                        tracing::info!("Entferne alte Forge-Version: {:?}", vd);
                        tokio::fs::remove_dir_all(&vd).await.ok();
                    }
                }
            }
        }

        // Leere oder ungültige PATCHED-JAR für die aktuelle Forge-Version entfernen
        // (wird bei Prozessorfehler manchmal als leere Datei hinterlassen)
        let current_forge_dir = forge_base.join("forge").join(&target_suffix);
        if current_forge_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&current_forge_dir) {
                for entry in entries.flatten() {
                    let fp = entry.path();
                    if fp.extension().and_then(|e| e.to_str()) == Some("jar") {
                        let fname = fp.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if fname.contains("-client") {
                            let size = fp.metadata().map(|m| m.len()).unwrap_or(0);
                            if size == 0 || (size < 1_000_000 && !Self::jar_contains_minecraft_class(&fp)) {
                                tracing::warn!("Entferne ungültige/leere client JAR: {:?} ({} bytes)", fp.file_name().unwrap_or_default(), size);
                                std::fs::remove_file(&fp).ok();
                            }
                        }
                    }
                }
            }
        }

        // Alte Installer-Daten entfernen
        let data_base = libraries_dir.join("forge-installer-data");
        if data_base.exists() {
            if let Ok(entries) = std::fs::read_dir(&data_base) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    if fname.starts_with(&format!("{}-", mc_version)) && fname != target_suffix {
                        tracing::info!("Entferne alte Installer-Daten: {:?}", p);
                        tokio::fs::remove_dir_all(&p).await.ok();
                    }
                }
            }
        }

        // Prüfe ob das aktuelle installer-data-Verzeichnis das richtige Format hat
        // (data/client.lzma muss als data/data/client.lzma gespeichert sein, nicht als data/client.lzma).
        // Wenn das alte Format vorliegt (client.lzma direkt in installer_data_dir ohne data/-Unterordner),
        // lösche das Verzeichnis damit es beim nächsten Start korrekt neu extrahiert wird.
        // ABER: Nur wenn die PATCHED-JAR fehlt - wenn alles funktioniert hat, brauchen wir die Daten nicht neu.
        let current_data_dir = data_base.join(&target_suffix);
        if current_data_dir.exists() {
            let old_format_file = current_data_dir.join("client.lzma");
            let new_format_file = current_data_dir.join("data").join("client.lzma");
            // Überprüfe ob die aktuelle gepatchte JAR bereits existiert und gültig ist
            let patched_jar = libraries_dir.join(format!(
                "net/minecraftforge/forge/{tf}/forge-{tf}-client.jar",
                tf = &target_suffix
            ));
            let patched_mc_jar = libraries_dir.join(format!(
                "net/minecraft/client/{mc}/client-{mc}.jar",
                mc = mc_version
            ));
            let patched_exists = (patched_jar.exists() && Self::jar_contains_minecraft_class(&patched_jar))
                || (patched_mc_jar.exists() && Self::jar_contains_minecraft_class(&patched_mc_jar));

            if old_format_file.exists() && !new_format_file.exists() && !patched_exists {
                tracing::warn!("Installer-Daten im alten Format gefunden und PATCHED fehlt → lösche für Neuextraktion: {:?}", current_data_dir);
                tokio::fs::remove_dir_all(&current_data_dir).await.ok();
            }
        }
    }

    pub async fn resolve_latest_version(&self, mc_version: &str) -> Result<String> {
        use crate::api::forge::ForgeClient;
        let client = ForgeClient::new()?;
        let versions = client.get_loader_versions(mc_version).await?;
        versions.first()
            .map(|v| v.forge_version.clone())
            .ok_or_else(|| anyhow::anyhow!("Keine Forge-Version für MC {} gefunden", mc_version))
    }
}

pub fn find_java_binary() -> String {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = PathBuf::from(&home).join("bin").join("java");
        if p.exists() { return p.display().to_string(); }
    }
    // Launcher-managed Java
    let managed = crate::config::defaults::java_dir().join("bin").join("java");
    if managed.exists() {
        return managed.display().to_string();
    }
    for p in &[
        "/usr/lib/jvm/java-21-openjdk-amd64/bin/java",
        "/usr/lib/jvm/java-21-openjdk/bin/java",
        "/usr/lib/jvm/java-21/bin/java",
        "/usr/lib/jvm/java-17-openjdk-amd64/bin/java",
        "/usr/lib/jvm/java-17-openjdk/bin/java",
        "/usr/lib/jvm/java-17/bin/java",
        "/usr/lib/jvm/java-16-openjdk-amd64/bin/java",
        "/usr/lib/jvm/java-16-openjdk/bin/java",
        "/usr/lib/jvm/java-8-openjdk-amd64/bin/java",
        "/usr/lib/jvm/java-8-openjdk/bin/java",
        "/usr/lib/jvm/java-1.8.0-openjdk/bin/java",
        "/usr/bin/java",
    ] {
        if Path::new(p).exists() { return p.to_string(); }
    }
    "java".to_string()
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_arg_placeholders(
    arg: &str,
    libraries_dir: &Path,
    natives_dir: &Path,
    game_dir: &Path,
    assets_dir: &Path,
    assets_index: &str,
    mc_version: &str,
    uuid: &str,
    access_token: &str,
    user_type: &str,
    username: &str,
) -> String {
    arg
        .replace("${library_directory}", &libraries_dir.display().to_string())
        .replace("${classpath_separator}", if cfg!(windows) { ";" } else { ":" })
        .replace("${version_name}", mc_version)
        .replace("${launcher_name}", "lion-launcher")
        .replace("${launcher_version}", env!("CARGO_PKG_VERSION"))
        .replace("${natives_directory}", &natives_dir.display().to_string())
        .replace("${game_directory}", &game_dir.display().to_string())
        .replace("${assets_root}", &assets_dir.display().to_string())
        .replace("${assets_index_name}", assets_index)
        .replace("${auth_uuid}", uuid)
        .replace("${auth_access_token}", access_token)
        .replace("${auth_player_name}", username)
        .replace("${user_type}", user_type)
        .replace("${version_type}", "release")
        .replace("${user_properties}", "{}")
        .replace("${resolution_width}", "854")
        .replace("${resolution_height}", "480")
}
