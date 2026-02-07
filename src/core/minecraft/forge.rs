use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::io::Read;
use crate::core::download::DownloadManager;

pub struct ForgeInstallResult {
    pub main_class: String,
    pub classpath: Vec<String>,
    pub module_path: Vec<String>,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
}

pub struct ForgeInstaller {
    download_manager: DownloadManager,
}

impl ForgeInstaller {
    pub fn new(download_manager: DownloadManager) -> Self {
        Self { download_manager }
    }

    /// Installiert Forge vollständig und gibt das Ergebnis zurück
    pub async fn install_forge_complete(
        &self,
        mc_version: &str,
        forge_version: &str,
        libraries_dir: &Path,
        client_jar: &Path,
    ) -> Result<ForgeInstallResult> {
        use crate::api::forge::ForgeClient;

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

        // Extrahiere version.json und JAR-Dateien
        let (version_json, jars_data) = {
            let file = std::fs::File::open(&installer_path)?;
            let mut archive = zip::ZipArchive::new(file)?;

            let version_json = {
                let mut entry = archive.by_name("version.json")
                    .map_err(|_| anyhow::anyhow!("version.json not found in Forge installer"))?;
                let mut data = String::new();
                entry.read_to_string(&mut data)?;
                data
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

        // Parse version.json
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
            game: Option<Vec<serde_json::Value>>,
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
        tracing::info!("✅ Forge main class: {}", version.main_class);

        // WICHTIG: Extrahiere ModLauncher Version aus den Libraries
        let modlauncher_version = version.libraries.iter()
            .find(|lib| lib.name.contains("modlauncher"))
            .and_then(|lib| {
                let parts: Vec<&str> = lib.name.split(':').collect();
                if parts.len() >= 3 {
                    Some(parts[2].to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "10.2.4".to_string()); // Fallback

        tracing::info!("📦 Detected ModLauncher version: {}", modlauncher_version);

        // JVM args aus version.json extrahieren
        let mut jvm_args = Vec::new();

        // WICHTIG: Immer diese grundlegenden JVM-Args hinzufügen für Forge 1.21.6+
        jvm_args.extend(vec![
            // Module System - KRITISCH
            "--add-modules=ALL-MODULE-PATH".to_string(),
            "--add-opens=java.base/java.lang=ALL-UNNAMED".to_string(),
            "--add-opens=java.base/java.util=ALL-UNNAMED".to_string(),
            "--add-opens=java.base/java.util.concurrent=ALL-UNNAMED".to_string(),
            "--add-opens=java.base/java.util.jar=ALL-UNNAMED".to_string(),
            "--add-opens=java.base/java.lang.invoke=ALL-UNNAMED".to_string(),
            "--add-opens=java.base/java.nio.file=ALL-UNNAMED".to_string(),
            "--add-opens=java.base/java.io=ALL-UNNAMED".to_string(),
            "--add-opens=java.base/sun.nio.ch=ALL-UNNAMED".to_string(),
            "--add-exports=java.base/sun.security.util=ALL-UNNAMED".to_string(),
            "--add-exports=jdk.naming.dns/com.sun.jndi.dns=java.naming".to_string(),

            // Forge-spezifische Properties - KRITISCH für ModLauncher Environment!
            format!("-DforgeVersion={}", forge_version),
            format!("-DmcVersion={}", mc_version),
            format!("-DmcpVersion={}", "20240808.144430"),
            format!("-DforgeGroup={}", "net.minecraftforge"),

            // ModLauncher Properties - VERHINDERT NullPointerException!
            format!("-Dmodlauncher.name=forge-{}-{}", mc_version, forge_version),
            format!("-Dmodlauncher.version={}", modlauncher_version),

            // Minecraft Properties
            format!("-Dminecraft.client.jar={}", client_jar.display()),
            format!("-DlibraryDirectory={}", libraries_dir.display()),

            // Java Security (für Mixin/ASM)
            "-Djava.security.manager=allow".to_string(),

            // Logging
            "-Dlog4j2.formatMsgNoLookups=true".to_string(),

            // FML/Forge Properties
            "-DignoreList=bootstraplauncher,securejarhandler,asm-commons,asm-util,asm-analysis,asm-tree,asm,client-extra,fmlcore,javafmllanguage,lowcodelanguage,mclanguage,forge-".to_string(),
            "-Dfml.earlyprogresswindow=false".to_string(),

            // FML Version Properties - DUPLIZIERT aber anders benannt für Kompatibilität
            format!("-Dfml.forgeVersion={}", forge_version),
            format!("-Dfml.mcVersion={}", mc_version),
            "-Dfml.forgeGroup=net.minecraftforge".to_string(),

            // Mixin
            "-Dmixin.env.remapRefMap=true".to_string(),
            "-Dmixin.env.refMapRemappingFile=".to_string(),
        ]);

        // Zusätzliche JVM args aus version.json hinzufügen
        if let Some(args) = &version.arguments {
            if let Some(jvm) = &args.jvm {
                for arg in jvm {
                    if let Some(s) = arg.as_str() {
                        let processed = s
                            .replace("${library_directory}", &libraries_dir.display().to_string())
                            .replace("${classpath_separator}", ":")
                            .replace("${version_name}", forge_version);

                        // Vermeide Duplikate
                        if !jvm_args.contains(&processed) {
                            jvm_args.push(processed);
                        }
                    }
                }
            }
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

                // KRITISCH: EXPLIZITE AUSSCHLUSSLISTE für Module die NIEMALS als Module geladen werden dürfen!
                // Diese verursachen Konflikte mit Guava und anderen Modulen!
                let is_explicitly_excluded = lib.name.contains("failureaccess") ||
                    lib.name.contains("listenablefuture") ||
                    lib.name.contains("commons-codec") ||  // Kann auch Konflikte verursachen
                    lib.name.contains("error_prone_annotations"); // Ebenfalls oft problematisch

                if is_explicitly_excluded {
                    tracing::debug!("❌ EXCLUDED from modules (forced to classpath): {}", lib.name);
                    classpath.push(path_str);
                    continue; // WICHTIG: Überspringe den Rest der Schleife!
                }

                // WICHTIG: lib.name enthält die Maven-Koordinaten, z.B. "net.sf.jopt-simple:jopt-simple:5.0.4"
                // Wir müssen den ORIGINALEN Namen prüfen (nicht lowercase), da die Modulnamen Case-Sensitive sind!

                // Debug: Log jopt-simple Libraries
                if lib.name.contains("jopt") {
                    tracing::info!("🔍 Checking library: {} (path: {})", lib.name, path_str);
                }

                // WICHTIG: Für Forge 1.21.6+ müssen ALLE diese Libraries im MODULE-PATH sein
                let is_module = lib.name.contains("bootstraplauncher") ||
                   lib.name.contains("bootstrap-launcher") ||
                   lib.name.contains("bootstrap-api") ||
                   lib.name.contains("securejarhandler") ||
                   lib.name.contains("securemodules") ||
                   lib.name.contains("fmlloader") ||
                   lib.name.contains("modlauncher") ||
                   lib.name.contains("cpw.mods") ||
                   lib.name.contains("net.minecraftforge") ||
                   lib.name.contains("minecraftforge") ||
                   lib.name.contains("unsafe") ||
                   lib.name.contains("forgespi") ||
                   lib.name.contains("coremods") ||
                   lib.name.contains("eventbus") ||
                   lib.name.contains("mergetool") ||
                   lib.name.contains("srgutils") ||
                   lib.name.contains("accesstransformer") ||
                   // Logging
                   lib.name.contains("log4j") ||
                   lib.name.contains("slf4j") ||
                   lib.name.contains("terminalconsoleappender") ||
                   // JavaScript Engine
                   lib.name.contains("nashorn") ||
                   // Bytecode - ASM Module
                   lib.name.contains("org.ow2.asm") ||
                   lib.name.contains(":asm:") ||  // Maven-Koordinaten-Format
                   lib.name.contains(":asm-") ||  // asm-tree, asm-commons, etc.
                   lib.name.contains("mixin") ||
                   // Google Libraries - NUR Guava als Modul!
                   lib.name.contains("guava") ||
                   lib.name.contains("checker-qual") ||
                   lib.name.contains("j2objc") ||
                   lib.name.contains("jspecify") ||
                   lib.name.contains("jopt-simple") ||  // Maven artifact name
                   lib.name.contains("jopt.simple") ||  // Java module name
                   lib.name.contains("net.sf.jopt") ||  // Maven group + artifact
                   lib.name.contains("jline") ||
                   // Weitere Dependencies
                   lib.name.contains("gson") ||
                   lib.name.contains("commons-") ||  // commons-lang3, commons-io, etc.
                   lib.name.contains("httpclient") ||
                   lib.name.contains("httpcore") ||
                   lib.name.contains("netty") ||
                   lib.name.contains("fastutil") ||
                   lib.name.contains("brigadier") ||
                   lib.name.contains("datafixerupper") ||
                   lib.name.contains("authlib") ||
                   lib.name.contains("text2speech") ||
                   lib.name.contains("jna") ||
                   lib.name.contains("oshi") ||
                   lib.name.contains("caffeine") ||
                   lib.name.contains("lz4") ||
                   lib.name.contains("antlr") ||
                   lib.name.contains("maven") ||
                   lib.name.contains("plexus") ||
                   lib.name.contains("night-config") ||
                   lib.name.contains("javaparser");

                if is_module {
                    tracing::debug!("📦 Module path: {}", lib.name);
                    module_path.push(path_str);
                } else {
                    tracing::debug!("📦 Classpath: {}", lib.name);
                    classpath.push(path_str);
                }
            } else {
                tracing::warn!("⚠️ Library not found: {} at {:?}", lib.name, lib_path);
            }
        }

        // KRITISCH: Prüfe und lade ALLE kritischen Module für Forge 1.21.6+
        tracing::info!("🔍 Checking critical modules for Forge...");

        // Diese Module MÜSSEN vorhanden sein - mit alternativen Versionen und URLs
        // WICHTIG: Forge 1.21.6+ verwendet net.minecraftforge.* statt cpw.mods.*!
        let critical_modules: Vec<(&str, &str, Vec<&str>)> = vec![
            // Command Line Parser
            ("net/sf/jopt-simple/jopt-simple/5.0.4/jopt-simple-5.0.4.jar", "jopt-simple", vec![
                "https://repo1.maven.org/maven2/net/sf/jopt-simple/jopt-simple/5.0.4/jopt-simple-5.0.4.jar",
                "https://libraries.minecraft.net/net/sf/jopt-simple/jopt-simple/5.0.4/jopt-simple-5.0.4.jar",
            ]),
            // KRITISCH: Bootstrap JARs für Forge 1.21.6+ (net.minecraftforge.*)!
            ("net/minecraftforge/bootstrap/2.1.8/bootstrap-2.1.8.jar", "bootstrap", vec![
                "https://maven.minecraftforge.net/net/minecraftforge/bootstrap/2.1.8/bootstrap-2.1.8.jar",
            ]),
            ("net/minecraftforge/bootstrap-api/2.1.8/bootstrap-api-2.1.8.jar", "bootstrap-api", vec![
                "https://maven.minecraftforge.net/net/minecraftforge/bootstrap-api/2.1.8/bootstrap-api-2.1.8.jar",
            ]),
            // KRITISCH: modlauncher (net.minecraftforge, NICHT cpw.mods!)
            ("net/minecraftforge/modlauncher/10.2.4/modlauncher-10.2.4.jar", "modlauncher", vec![
                "https://maven.minecraftforge.net/net/minecraftforge/modlauncher/10.2.4/modlauncher-10.2.4.jar",
            ]),
            // ASM Libraries
            ("org/ow2/asm/asm/9.7/asm-9.7.jar", "asm", vec![
                "https://repo1.maven.org/maven2/org/ow2/asm/asm/9.7/asm-9.7.jar",
            ]),
            ("org/ow2/asm/asm-commons/9.7/asm-commons-9.7.jar", "asm-commons", vec![
                "https://repo1.maven.org/maven2/org/ow2/asm/asm-commons/9.7/asm-commons-9.7.jar",
            ]),
            ("org/ow2/asm/asm-tree/9.7/asm-tree-9.7.jar", "asm-tree", vec![
                "https://repo1.maven.org/maven2/org/ow2/asm/asm-tree/9.7/asm-tree-9.7.jar",
            ]),
            ("org/ow2/asm/asm-analysis/9.7/asm-analysis-9.7.jar", "asm-analysis", vec![
                "https://repo1.maven.org/maven2/org/ow2/asm/asm-analysis/9.7/asm-analysis-9.7.jar",
            ]),
            ("org/ow2/asm/asm-util/9.7/asm-util-9.7.jar", "asm-util", vec![
                "https://repo1.maven.org/maven2/org/ow2/asm/asm-util/9.7/asm-util-9.7.jar",
            ]),
        ];

        for (rel_path, name, urls) in &critical_modules {
            let full_path = libraries_dir.join(rel_path);

            // Prüfe ob Datei existiert UND nicht leer ist
            let needs_download = if full_path.exists() {
                match std::fs::metadata(&full_path) {
                    Ok(meta) => meta.len() == 0, // Leer = neu downloaden
                    Err(_) => true,
                }
            } else {
                true
            };

            if needs_download {
                tracing::warn!("⚠️ Missing or empty critical module: {} at {:?}", name, full_path);
                tokio::fs::create_dir_all(full_path.parent().unwrap()).await.ok();

                // Versuche alle URLs
                let mut downloaded = false;
                for url in urls {
                    tracing::info!("📥 Trying to download {} from {}", name, url);
                    match self.download_manager.download_with_hash(url, &full_path, None).await {
                        Ok(_) => {
                            // Verifiziere dass die Datei nicht leer ist
                            if let Ok(meta) = std::fs::metadata(&full_path) {
                                if meta.len() > 0 {
                                    tracing::info!("✅ Downloaded {} ({} bytes)", name, meta.len());
                                    downloaded = true;
                                    break;
                                } else {
                                    tracing::warn!("⚠️ Downloaded file is empty, trying next URL...");
                                    tokio::fs::remove_file(&full_path).await.ok();
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("⚠️ Failed to download from {}: {}", url, e);
                        }
                    }
                }

                if !downloaded {
                    tracing::error!("❌ CRITICAL: Could not download {} - Forge will likely fail!", name);
                }
            } else {
                tracing::info!("✅ {} already exists", name);
            }

            if full_path.exists() {
                if let Ok(meta) = std::fs::metadata(&full_path) {
                    if meta.len() > 0 {
                        let path_str = full_path.display().to_string();
                        if !module_path.contains(&path_str) {
                            tracing::debug!("📦 Adding {} to module path", name);
                            module_path.push(path_str);
                        }
                    }
                }
            }
        }

        // WICHTIG: fmlloader MUSS vorhanden sein für Forge 1.21.6+
        let fmlloader_path = libraries_dir.join(format!(
            "net/minecraftforge/fmlloader/{0}-{1}/fmlloader-{0}-{1}.jar",
            mc_version, forge_version
        ));
        if fmlloader_path.exists() {
            let fmlloader_str = fmlloader_path.display().to_string();
            if !module_path.contains(&fmlloader_str) {
                tracing::info!("✅ Adding fmlloader to module path: {:?}", fmlloader_path);
                module_path.push(fmlloader_str);
            }
        } else {
            tracing::warn!("⚠️ fmlloader not found at {:?}", fmlloader_path);
        }

        // WICHTIG: slf4j-api MUSS im Module-Path sein für log4j-slf4j2-impl!
        let slf4j_path = libraries_dir.join("org/slf4j/slf4j-api/2.0.9/slf4j-api-2.0.9.jar");
        if !slf4j_path.exists() {
            // Suche nach anderen slf4j Versionen
            let slf4j_base = libraries_dir.join("org/slf4j/slf4j-api");
            if slf4j_base.exists() {
                for version in &["2.0.16", "2.0.13", "2.0.9", "2.0.7", "1.8.0-beta4"] {
                    let alt_path = slf4j_base.join(format!("{0}/slf4j-api-{0}.jar", version));
                    if alt_path.exists() {
                        tracing::info!("✅ Found slf4j-api version {}", version);
                        module_path.push(alt_path.display().to_string());
                        break;
                    }
                }
            }
        } else {
            tracing::info!("✅ Adding slf4j-api to module path: {:?}", slf4j_path);
            module_path.push(slf4j_path.display().to_string());
        }

        // WICHTIG: log4j MUSS im Module-Path sein für modlauncher!
        // Diese sind oft nicht in der Forge version.json, also manuell hinzufügen
        let log4j_libs = [
            "org/apache/logging/log4j/log4j-api/2.22.1/log4j-api-2.22.1.jar",
            "org/apache/logging/log4j/log4j-core/2.22.1/log4j-core-2.22.1.jar",
            "org/apache/logging/log4j/log4j-slf4j2-impl/2.22.1/log4j-slf4j2-impl-2.22.1.jar",
        ];

        for log4j_lib in &log4j_libs {
            let log4j_path = libraries_dir.join(log4j_lib);
            if log4j_path.exists() {
                let log4j_str = log4j_path.display().to_string();
                if !module_path.iter().any(|p| p.contains("log4j")) || !module_path.contains(&log4j_str) {
                    tracing::info!("✅ Adding log4j to module path: {:?}", log4j_path);
                    module_path.push(log4j_str);
                }
            }
        }

        // Suche nach alternativen log4j Versionen wenn 2.22.1 nicht existiert
        if !module_path.iter().any(|p| p.contains("log4j-api")) {
            let log4j_base = libraries_dir.join("org/apache/logging/log4j");
            if log4j_base.exists() {
                // Suche nach vorhandener Version
                for version in &["2.25.2", "2.24.1", "2.22.1", "2.19.0"] {
                    let api_path = log4j_base.join(format!("log4j-api/{0}/log4j-api-{0}.jar", version));
                    let core_path = log4j_base.join(format!("log4j-core/{0}/log4j-core-{0}.jar", version));

                    if api_path.exists() && core_path.exists() {
                        tracing::info!("✅ Found log4j version {}", version);
                        module_path.push(api_path.display().to_string());
                        module_path.push(core_path.display().to_string());

                        // Auch slf4j-impl wenn vorhanden
                        let slf4j_path = log4j_base.join(format!("log4j-slf4j2-impl/{0}/log4j-slf4j2-impl-{0}.jar", version));
                        if slf4j_path.exists() {
                            module_path.push(slf4j_path.display().to_string());
                        }
                        break;
                    }
                }
            }
        }

        // WICHTIG: nashorn MUSS im Module-Path sein für coremods!
        if !module_path.iter().any(|p| p.contains("nashorn")) {
            let nashorn_base = libraries_dir.join("org/openjdk/nashorn/nashorn-core");
            if nashorn_base.exists() {
                // Suche nach vorhandener Version
                for version in &["15.4", "15.3", "15.2", "15.1"] {
                    let nashorn_path = nashorn_base.join(format!("{0}/nashorn-core-{0}.jar", version));
                    if nashorn_path.exists() {
                        tracing::info!("✅ Found nashorn version {}", version);
                        module_path.push(nashorn_path.display().to_string());
                        break;
                    }
                }
            }

            // Alternative: Download nashorn wenn nicht vorhanden
            if !module_path.iter().any(|p| p.contains("nashorn")) {
                let nashorn_path = libraries_dir.join("org/openjdk/nashorn/nashorn-core/15.4/nashorn-core-15.4.jar");
                if !nashorn_path.exists() {
                    tracing::info!("📥 Downloading nashorn...");
                    tokio::fs::create_dir_all(nashorn_path.parent().unwrap()).await.ok();
                    let url = "https://repo1.maven.org/maven2/org/openjdk/nashorn/nashorn-core/15.4/nashorn-core-15.4.jar";
                    if self.download_manager.download_with_hash(url, &nashorn_path, None).await.is_ok() {
                        module_path.push(nashorn_path.display().to_string());
                    }
                } else {
                    module_path.push(nashorn_path.display().to_string());
                }
            }
        }

        // WICHTIG: Für Forge 1.21.6+ verwenden wir den LaunchTarget aus version.json
        // Wenn fmlloader korrekt im Module-Path ist, funktioniert "forge_client"
        // Wenn nicht, fallback zu "minecraft"
        let has_fmlloader = module_path.iter().any(|p| p.contains("fmlloader"));

        let mut launch_target = if has_fmlloader {
            "forge_client".to_string() // Korrekter Target wenn fmlloader vorhanden
        } else {
            tracing::warn!("⚠️ fmlloader nicht im module-path, fallback zu 'minecraft'");
            "minecraft".to_string()
        };

        let mut extra_game_args = Vec::new();

        // Parse game args aus version.json (falls vorhanden)
        if let Some(args) = &version.arguments {
            if let Some(game) = &args.game {
                for arg in game {
                    if let Some(s) = arg.as_str() {
                        extra_game_args.push(s.to_string());
                    }
                }

                // Suche nach --launchTarget in den game args
                for i in 0..extra_game_args.len() {
                    if extra_game_args[i] == "--launchTarget" && i + 1 < extra_game_args.len() {
                        let target_from_json = extra_game_args[i + 1].clone();
                        tracing::info!("Found launch target in version.json: {}", target_from_json);

                        // WICHTIG: Für Forge 1.21.6+ mit fmlloader im Module-Path
                        // sind diese Targets ALLE gültig:
                        // - forge_client (korrekt für Client)
                        // - forgeclient (ältere Schreibweise)
                        // - fmlclient (alternative)
                        // - minecraft (nur wenn fmlloader fehlt)
                        if target_from_json == "forge_client" ||
                           target_from_json == "forgeclient" ||
                           target_from_json == "fmlclient" {
                            // Nur auf minecraft mappen wenn fmlloader NICHT vorhanden
                            if !has_fmlloader {
                                tracing::warn!("⚠️ Remapping launch target '{}' -> 'minecraft' (fmlloader missing)", target_from_json);
                                launch_target = "minecraft".to_string();
                            } else {
                                tracing::info!("✅ Using launch target '{}' (fmlloader found)", target_from_json);
                                launch_target = target_from_json;
                            }
                        } else {
                            launch_target = target_from_json;
                        }
                        break;
                    }
                }
            }
        }

        // Baue finale game args
        // WICHTIG: Für Forge 1.21.6+ mit minecraft LaunchTarget brauchen wir FML-spezifische Args
        let mut game_args = vec![
            "--launchTarget".to_string(),
            launch_target.clone(),
            "--gameJar".to_string(),
            client_jar.display().to_string(),
            // FML-spezifische Argumente für Forge 1.21.6+ (DYNAMISCH aus Parametern!)
            "--fml.forgeVersion".to_string(),
            forge_version.to_string(),  // ← DYNAMISCH!
            "--fml.mcVersion".to_string(),
            mc_version.to_string(),     // ← DYNAMISCH!
            "--fml.forgeGroup".to_string(),
            "net.minecraftforge".to_string(),
            "--fml.mcpVersion".to_string(),
            "20240808.144430".to_string(), // Standard MCP Version für 1.21.x
        ];

        // Füge weitere args aus version.json hinzu (außer --launchTarget und --gameJar)
        let mut skip_next = false;
        for i in 0..extra_game_args.len() {
            if skip_next {
                skip_next = false;
                continue;
            }

            if extra_game_args[i] == "--launchTarget" || extra_game_args[i] == "--gameJar" {
                skip_next = true;
                continue;
            }

            game_args.push(extra_game_args[i].clone());
        }

        tracing::info!("✅ Forge libraries loaded: {} classpath, {} module path", classpath.len(), module_path.len());
        tracing::info!("   Game JAR: {:?}", client_jar);
        tracing::info!("   Launch Target: {}", launch_target);
        tracing::info!("   Main Class: {}", version.main_class);
        tracing::info!("   ModLauncher Version: {}", modlauncher_version);

        // DEBUG: Zeige alle Module im Module-Path
        tracing::info!("=== Module Path ({} entries) ===", module_path.len());
        for (i, module) in module_path.iter().enumerate() {
            let file_name = std::path::Path::new(module).file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let exists = std::path::Path::new(module).exists();
            tracing::debug!("[{:03}] {} (exists: {})", i, file_name, exists);
        }

        // DEBUG: Zeige kritische System-Properties die gesetzt werden
        tracing::info!("=== System Properties ===");
        tracing::info!("  forgeVersion={}", forge_version);
        tracing::info!("  mcVersion={}", mc_version);
        tracing::info!("  modlauncher.version={}", modlauncher_version);
        tracing::info!("  modlauncher.name=forge-{}-{}", mc_version, forge_version);

        Ok(ForgeInstallResult {
            main_class: version.main_class,
            classpath,
            module_path,
            jvm_args,
            game_args,
        })
    }

    /// Konvertiert Maven-Koordinaten zu Pfad
    fn maven_to_path(maven: &str) -> String {
        let parts: Vec<&str> = maven.split(':').collect();
        if parts.len() < 3 {
            return maven.to_string();
        }

        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        let version = parts[2];

        format!("{}/{}/{}/{}-{}.jar", group, artifact, version, artifact, version)
    }

    /// Prüft ob eine Datei ein gültiges ZIP/JAR ist
    fn is_valid_zip(path: &Path) -> bool {
        if !path.exists() {
            return false;
        }

        match std::fs::File::open(path) {
            Ok(file) => {
                match zip::ZipArchive::new(file) {
                    Ok(_) => true,
                    Err(e) => {
                        tracing::warn!("Invalid ZIP at {:?}: {}", path, e);
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

    /// Löst die neueste Forge-Version für eine Minecraft-Version auf
    pub async fn resolve_latest_version(&self, mc_version: &str) -> Result<String> {
        use crate::api::forge::ForgeClient;

        let client = ForgeClient::new()?;
        let versions = client.get_loader_versions(mc_version).await?;

        let version = versions.first()
            .ok_or_else(|| anyhow::anyhow!("No Forge version found for MC {}", mc_version))?;

        Ok(version.forge_version.clone())
    }
}
