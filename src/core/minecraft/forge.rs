use anyhow::Result;
use std::path::{Path, PathBuf};
use std::io::Read;
use crate::core::download::DownloadManager;

#[derive(serde::Deserialize, Debug)]
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
    /// Gepatchte Client-JAR — wird als --gameJar übergeben
    pub patched_client_jar: PathBuf,
}

pub struct ForgeInstaller {
    download_manager: DownloadManager,
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

        let forge_client = ForgeClient::new()?;
        tracing::info!("Installing Forge {}-{}", mc_version, forge_version);

        // Alte Forge-Versionen für dieselbe MC-Version bereinigen um Classpath-Konflikte zu vermeiden.
        // Wenn z.B. 61.0.0 installiert war und jetzt 61.1.0 kommt, müssen die alten
        // fmlloader-1.21.11-61.0.0.jar, forge-transformers-1.21.11-61.0.0.jar etc. weg.
        Self::cleanup_old_forge_versions(mc_version, forge_version, libraries_dir).await;

        let installer_path = libraries_dir.join(
            format!("forge-{}-{}-installer.jar", mc_version, forge_version)
        );

        if installer_path.exists() && !Self::is_valid_zip(&installer_path) {
            tokio::fs::remove_file(&installer_path).await.ok();
        }
        if !installer_path.exists() {
            let installer_url = forge_client.get_installer_url(mc_version, forge_version);
            tracing::info!("Downloading Forge installer: {}", installer_url);
            tokio::fs::create_dir_all(installer_path.parent().unwrap()).await?;
            self.download_manager.download_with_hash(&installer_url, &installer_path, None).await?;
        }

        // Lese version.json, install_profile.json und eingebettete JARs aus dem Installer
        let (version_json, install_profile_json, embedded_jars) = {
            let file = std::fs::File::open(&installer_path)?;
            let mut archive = zip::ZipArchive::new(file)?;

            let version_json = {
                let mut e = archive.by_name("version.json")
                    .map_err(|_| anyhow::anyhow!("version.json not found in Forge installer"))?;
                let mut d = String::new();
                e.read_to_string(&mut d)?;
                d
            };
            let install_profile_json = {
                let mut e = archive.by_name("install_profile.json")
                    .map_err(|_| anyhow::anyhow!("install_profile.json not found"))?;
                let mut d = String::new();
                e.read_to_string(&mut d)?;
                d
            };

            let mut embedded: Vec<(PathBuf, Vec<u8>)> = Vec::new();
            let mut names: Vec<(String, PathBuf)> = Vec::new();
            for i in 0..archive.len() {
                if let Ok(entry) = archive.by_index(i) {
                    let name = entry.name().to_string();
                    if name.starts_with("maven/") && (name.ends_with(".jar") || name.ends_with(".lzma")) {
                        if let Some(rel) = name.strip_prefix("maven/") {
                            let dest = libraries_dir.join(rel);
                            if !dest.exists() {
                                names.push((name, dest));
                            }
                        }
                    } else if name.starts_with("data/") && !name.ends_with('/') {
                        // data/*.lzma und andere Prozessor-Daten aus dem Installer extrahieren
                        // Diese werden von Prozessoren via {BINPATCH} etc. referenziert
                        let dest = libraries_dir.join("forge-installer-data").join(&name);
                        if !dest.exists() {
                            names.push((name, dest));
                        }
                    }
                }
            }
            for (name, dest) in names {
                if let Ok(mut entry) = archive.by_name(&name) {
                    let mut data = Vec::new();
                    entry.read_to_end(&mut data)?;
                    embedded.push((dest, data));
                }
            }
            (version_json, install_profile_json, embedded)
        };

        for (dest, data) in embedded_jars {
            tokio::fs::create_dir_all(dest.parent().unwrap()).await?;
            tokio::fs::write(&dest, &data).await?;
        }

        // Parse version.json
        #[derive(serde::Deserialize)]
        struct VersionJson {
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

        let version: VersionJson = serde_json::from_str(&version_json)?;
        let is_bootstrap = version.main_class.contains("ForgeBootstrap");
        tracing::info!("Forge main class: {}, is_bootstrap: {}", version.main_class, is_bootstrap);

        // Parse install_profile
        #[derive(serde::Deserialize)]
        struct InstallProfile {
            version: Option<String>,
            libraries: Option<Vec<ForgeLib>>,
            processors: Option<Vec<Processor>>,
            data: Option<std::collections::HashMap<String, SidedData>>,
        }
        let install_profile: InstallProfile = serde_json::from_str(&install_profile_json)
            .unwrap_or(InstallProfile { version: None, libraries: None, processors: None, data: None });

        let forge_version_from_profile = install_profile.version.as_deref()
            .and_then(|v| {
                if v.contains("-forge-") { v.split("-forge-").nth(1).map(|s| s.to_string()) }
                else { v.split('-').last().map(|s| s.to_string()) }
            });

        let mcp_version = version.libraries.iter()
            .find_map(|lib| {
                if lib.name.contains("mcp_config") {
                    let parts: Vec<&str> = lib.name.split(':').collect();
                    parts.get(2).and_then(|v| v.split('-').nth(1).map(|s| s.to_string()))
                } else { None }
            })
            .unwrap_or_else(|| "20240808.144430".to_string());

        // Installer-Libraries herunterladen
        if let Some(inst_libs) = &install_profile.libraries {
            for lib in inst_libs {
                let (url, path, sha1) = if let Some(dl) = &lib.downloads {
                    if let Some(art) = &dl.artifact {
                        (art.url.clone(), art.path.clone(), art.sha1.clone())
                    } else { continue; }
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

        // Prozessor-JARs und ihre Deps vorab herunterladen
        if let Some(procs) = &install_profile.processors {
            for proc in procs.iter() {
                let sides = proc.sides.as_deref().unwrap_or(&[]);
                if !sides.is_empty() && !sides.contains(&"client".to_string()) { continue; }
                let jar_path = libraries_dir.join(Self::maven_to_path(&proc.jar));
                if !jar_path.exists() {
                    Self::download_from_maven_repos(&self.download_manager, &Self::maven_to_path(&proc.jar), &jar_path).await;
                }
                for dep in &proc.classpath {
                    let dep_path = libraries_dir.join(Self::maven_to_path(dep));
                    if !dep_path.exists() {
                        Self::download_from_maven_repos(&self.download_manager, &Self::maven_to_path(dep), &dep_path).await;
                    }
                }
            }
        }

        // Version-Libraries klassifizieren und herunterladen
        let mut bootstrap_classpath: Vec<String> = Vec::new();
        let mut native_jars: Vec<String> = Vec::new();
        let mut classpath: Vec<String> = Vec::new();
        let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

        for lib in &version.libraries {
            let (url, path, sha1) = if let Some(downloads) = &lib.downloads {
                if let Some(artifact) = &downloads.artifact {
                    (artifact.url.clone(), artifact.path.clone(), artifact.sha1.clone())
                } else { continue; }
            } else {
                (String::new(), Self::maven_to_path(&lib.name), None)
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
                tracing::warn!("Library missing: {} at {:?}", lib.name, dest);
                continue;
            }

            if is_bootstrap {
                let is_native = path.contains("natives-linux") || path.contains("natives-windows")
                    || path.contains("natives-osx") || path.contains("natives-macos");
                if is_native {
                    native_jars.push(dest_str);
                } else if Self::is_data_only_lib(&lib.name) {
                    classpath.push(dest_str);
                } else {
                    bootstrap_classpath.push(dest_str);
                }
            } else {
                classpath.push(dest_str);
            }
        }

        tracing::info!("Library classification: {} bootstrap-cp, {} natives, {} classpath",
            bootstrap_classpath.len(), native_jars.len(), classpath.len());

        let mut jvm_args = Vec::new();
        let mut game_args = Vec::new();
        if let Some(args) = &version.arguments {
            if let Some(jvm) = &args.jvm {
                for arg in jvm { jvm_args.extend(Self::extract_arg_strings(arg)); }
            }
            if let Some(game) = &args.game {
                for arg in game { game_args.extend(Self::extract_arg_strings(arg)); }
            }
        }

        // Gepatchter Client-JAR-Pfad: net.minecraftforge:forge:{mc_version}-{forge_ver}:client
        // Dieser Pfad ist bekannt BEVOR die Prozessoren laufen.
        let forge_ver = forge_version_from_profile.as_deref().unwrap_or(forge_version);
        let patched_jar_relative = format!(
            "net/minecraftforge/forge/{mc_version}-{forge_ver}/forge-{mc_version}-{forge_ver}-client.jar"
        );
        let expected_patched = libraries_dir.join(&patched_jar_relative);
        tracing::info!("Expected patched JAR path: {:?}", expected_patched);

        // Prozessoren ausführen → erstellt expected_patched wenn nötig
        let processors = install_profile.processors.unwrap_or_default();
        let data = install_profile.data.unwrap_or_default();
        let proc_result = Self::run_processors(
            &processors, &data, libraries_dir, &installer_path, client_jar, self
        ).await?;

        // Patched JAR: zuerst expected_patched prüfen, dann Prozessor-Rückgabe, dann Fallback
        let patched_client_jar = if expected_patched.exists()
            && expected_patched.metadata().map(|m| m.len() > 0).unwrap_or(false)
        {
            tracing::info!("Using expected patched JAR: {:?}", expected_patched);
            expected_patched
        } else if proc_result != client_jar.to_path_buf() {
            tracing::info!("Using processor result: {:?}", proc_result);
            proc_result
        } else {
            // Letzter Versuch: MC_OFF direkt verwenden (remappt aber ungepacht)
            let mc_off = libraries_dir.join(format!(
                "net/minecraft/client/{mc_version}/client-{mc_version}-official.jar"
            ));
            if mc_off.exists() && mc_off.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                // Falls expected_patched noch nicht da: direkt kopieren
                if let Some(parent) = expected_patched.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::copy(&mc_off, &expected_patched).await?;
                tracing::info!("Fallback: copied MC_OFF -> {:?}", expected_patched);
                expected_patched
            } else {
                tracing::warn!("All fallbacks failed, using original client JAR");
                client_jar.to_path_buf()
            }
        };

        tracing::info!("Final patched client JAR: {:?}", patched_client_jar);

        Ok(ForgeInstallResult {
            main_class: version.main_class,
            bootstrap_classpath,
            native_jars,
            classpath,
            jvm_args,
            game_args,
            mcp_version,
            forge_version: forge_version_from_profile.unwrap_or_else(|| forge_version.to_string()),
            is_bootstrap,
            patched_client_jar,
        })
    }

    async fn run_processors(
        processors: &[Processor],
        data: &std::collections::HashMap<String, SidedData>,
        libraries_dir: &Path,
        installer_path: &Path,
        client_jar: &Path,
        forge_installer: &ForgeInstaller,
    ) -> Result<PathBuf> {
        // forge-installer-data Basisverzeichnis (enthält extrahierte data/*.lzma etc.)
        let installer_data_dir = libraries_dir.join("forge-installer-data");

        let resolve_val = |val: &str| -> String {
            let v = val.trim();
            if v.starts_with('[') && v.ends_with(']') {
                let coords = &v[1..v.len()-1];
                libraries_dir.join(Self::maven_to_path(coords)).display().to_string()
            } else if v.starts_with('\'') && v.ends_with('\'') {
                v[1..v.len()-1].to_string()
            } else if v.starts_with('/') {
                // Pfade wie /data/client.lzma → aus dem Installer extrahiert nach libraries/forge-installer-data/data/client.lzma
                installer_data_dir.join(v.trim_start_matches('/')).display().to_string()
            } else {
                v.to_string()
            }
        };

        let mut resolved_data: std::collections::HashMap<String, String> = data.iter()
            .map(|(k, v)| (k.clone(), resolve_val(&v.client)))
            .collect();

        resolved_data.insert("MINECRAFT_JAR".to_string(), client_jar.display().to_string());
        resolved_data.insert("INSTALLER".to_string(), installer_path.display().to_string());
        resolved_data.insert("ROOT".to_string(),
            libraries_dir.parent().unwrap_or(libraries_dir).display().to_string());
        resolved_data.insert("SIDE".to_string(), "client".to_string());

        let has_binpatch = data.contains_key("BINPATCH");
        tracing::info!("BINPATCH present: {}", has_binpatch);

        // Ziel-JAR (PATCHED)
        let patched_path = data.get("PATCHED")
            .map(|v| PathBuf::from(resolve_val(&v.client)))
            .unwrap_or_else(|| client_jar.to_path_buf());

        tracing::info!("Expected PATCHED path: {:?}", patched_path);

        if patched_path.exists() && patched_path.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            tracing::info!("Patched client JAR already exists, skipping processors");
            return Ok(patched_path);
        }

        tracing::info!("Running {} Forge processors...", processors.len());

        for processor in processors {
            let sides = processor.sides.as_deref().unwrap_or(&[]);
            if !sides.is_empty() && !sides.contains(&"client".to_string()) {
                tracing::debug!("Skipping server-only processor: {}", processor.jar);
                continue;
            }

            // Argumente auflösen
            let mut resolved_args: Vec<String> = processor.args.iter().map(|arg| {
                let mut resolved = arg.clone();
                for (k, v) in &resolved_data {
                    resolved = resolved.replace(&format!("{{{}}}", k), v);
                }
                if resolved.starts_with('[') && resolved.ends_with(']') {
                    let coords = &resolved[1..resolved.len()-1];
                    resolved = libraries_dir.join(Self::maven_to_path(coords)).display().to_string();
                }
                resolved
            }).collect();

            // --apply {BINPATCH} entfernen wenn kein BINPATCH (Forge 1.21.6+)
            if !has_binpatch {
                let mut i = 0;
                while i < resolved_args.len() {
                    if resolved_args[i] == "--apply" {
                        let next_is_empty_or_placeholder = resolved_args.get(i + 1)
                            .map(|a| a.is_empty() || a.contains('{'))
                            .unwrap_or(true);
                        if next_is_empty_or_placeholder {
                            resolved_args.remove(i);
                            if i < resolved_args.len() { resolved_args.remove(i); }
                            continue;
                        }
                    }
                    i += 1;
                }
            }

            // Ungelöste {PLACEHOLDER} entfernen (+ vorheriges Flag)
            let mut i = 0;
            while i < resolved_args.len() {
                if resolved_args[i].contains('{') && resolved_args[i].contains('}') {
                    if i > 0 && resolved_args[i-1].starts_with("--") {
                        resolved_args.remove(i);
                        resolved_args.remove(i - 1);
                        if i > 0 { i -= 1; }
                    } else {
                        resolved_args.remove(i);
                    }
                    continue;
                }
                i += 1;
            }

            // Spezialfall binarypatcher ohne BINPATCH: MC_OFF direkt nach PATCHED kopieren
            if processor.jar.contains("binarypatcher") && !has_binpatch {
                let mc_off = resolved_data.get("MC_OFF").cloned().unwrap_or_default();
                let patched_out = resolved_data.get("PATCHED").cloned().unwrap_or_default();
                if !mc_off.is_empty() && !patched_out.is_empty() {
                    let src = PathBuf::from(&mc_off);
                    let dst = PathBuf::from(&patched_out);
                    if src.exists() {
                        if !dst.exists() || dst.metadata().map(|m| m.len() == 0).unwrap_or(true) {
                            if let Some(parent) = dst.parent() {
                                tokio::fs::create_dir_all(parent).await?;
                            }
                            tokio::fs::copy(&src, &dst).await?;
                            tracing::info!("Copied MC_OFF -> PATCHED (no binary patches in Forge 1.21.6+)");
                        }
                    } else {
                        tracing::warn!("MC_OFF not found: {}", mc_off);
                    }
                }
                continue;
            }

            // Output-Pfad aus Args
            let output_path = resolved_args.windows(2)
                .find(|w| w[0] == "--output")
                .map(|w| PathBuf::from(&w[1]));

            if let Some(ref out) = output_path {
                if out.exists() && out.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                    tracing::info!("Output exists, skipping: {:?}", out.file_name().unwrap_or_default());
                    continue;
                }
                if let Some(parent) = out.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
            }

            let proc_jar_path = libraries_dir.join(Self::maven_to_path(&processor.jar));
            if !proc_jar_path.exists() {
                Self::download_from_maven_repos(
                    &forge_installer.download_manager, &Self::maven_to_path(&processor.jar), &proc_jar_path
                ).await;
                if !proc_jar_path.exists() {
                    tracing::warn!("Processor JAR missing: {:?}", proc_jar_path);
                    continue;
                }
            }

            let main_class = Self::read_manifest_main_class(&proc_jar_path)
                .unwrap_or_else(|| "net.minecraftforge.installertools.Main".to_string());

            let mut proc_cp = vec![proc_jar_path.display().to_string()];
            for dep in &processor.classpath {
                let dep_path = libraries_dir.join(Self::maven_to_path(dep));
                if !dep_path.exists() {
                    Self::download_from_maven_repos(
                        &forge_installer.download_manager, &Self::maven_to_path(dep), &dep_path
                    ).await;
                }
                if dep_path.exists() { proc_cp.push(dep_path.display().to_string()); }
            }

            tracing::info!("Running processor: {} ({})", processor.jar, main_class);
            tracing::info!("  args: {:?}", resolved_args);

            let java = find_java_binary();
            let output = tokio::process::Command::new(&java)
                .arg("-cp").arg(proc_cp.join(":"))
                .arg(&main_class)
                .args(&resolved_args)
                .output().await;

            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if !stdout.trim().is_empty() { tracing::info!("stdout: {}", stdout.trim()); }
                    if !stderr.trim().is_empty() { tracing::warn!("stderr: {}", stderr.trim()); }
                    if out.status.success() {
                        tracing::info!("Processor {} OK", processor.jar);
                    } else {
                        tracing::warn!("Processor {} FAILED (exit {})", processor.jar, out.status);
                    }
                },
                Err(e) => tracing::warn!("Processor failed to start: {}", e),
            }
        }

        if patched_path.exists() && patched_path.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            Ok(patched_path)
        } else {
            tracing::warn!("Patched JAR not found after processors, using original client JAR as fallback");
            Ok(client_jar.to_path_buf())
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

    fn is_data_only_lib(maven_name: &str) -> bool {
        maven_name.starts_with("de.oceanlabs.mcp:mcp_config")
    }

    fn extract_arg_strings(arg: &serde_json::Value) -> Vec<String> {
        if let Some(s) = arg.as_str() {
            vec![s.to_string()]
        } else if let Some(obj) = arg.as_object() {
            if let Some(value) = obj.get("value") {
                if let Some(s) = value.as_str() {
                    vec![s.to_string()]
                } else if let Some(arr) = value.as_array() {
                    arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
                } else { vec![] }
            } else { vec![] }
        } else { vec![] }
    }

    async fn download_from_maven_repos(dm: &DownloadManager, maven_path: &str, dest: &Path) {
        let repos = [
            "https://maven.minecraftforge.net",
            "https://maven.neoforged.net",
            "https://repo1.maven.org/maven2",
            "https://libraries.minecraft.net",
        ];
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        for repo in &repos {
            let url = format!("{}/{}", repo, maven_path);
            if dm.download_with_hash(&url, dest, None).await.is_ok() {
                if dest.exists() && std::fs::metadata(dest).map(|m| m.len() > 0).unwrap_or(false) {
                    tracing::debug!("Downloaded {} from {}", maven_path, repo);
                    return;
                }
            }
        }
        tracing::warn!("Could not download {} from any repo", maven_path);
    }

    fn maven_to_path(maven: &str) -> String {
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
            let classifier = parts[3];
            format!("{}/{}/{}/{}-{}-{}.{}", group, artifact, version, artifact, version, classifier, ext)
        } else {
            format!("{}/{}/{}/{}-{}.{}", group, artifact, version, artifact, version, ext)
        }
    }

    fn is_valid_zip(path: &Path) -> bool {
        match std::fs::File::open(path) {
            Ok(file) => zip::ZipArchive::new(file).is_ok(),
            Err(_) => false,
        }
    }

    /// Löscht alle installierten Forge-Artefakte für dieselbe MC-Version die NICHT der
    /// Ziel-forge_version entsprechen. Verhindert Classpath-Konflikte wenn z.B. 61.0.0 → 61.1.0.
    async fn cleanup_old_forge_versions(mc_version: &str, forge_version: &str, libraries_dir: &Path) {
        let target_suffix = format!("{}-{}", mc_version, forge_version);
        // Verzeichnispräfixe unter libraries/net/minecraftforge/ die versioniert sind
        let forge_dirs = [
            "forge", "fmlcore", "fmlloader", "fmlearlydisplay",
            "javafmllanguage", "lowcodelanguage", "mclanguage",
            "forge-transformers",
        ];
        let forge_base = libraries_dir.join("net").join("minecraftforge");
        for dir_name in &forge_dirs {
            let dir = forge_base.join(dir_name);
            if !dir.exists() { continue; }
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let version_dir = entry.path();
                    if !version_dir.is_dir() { continue; }
                    let ver_name = version_dir.file_name()
                        .and_then(|n| n.to_str()).unwrap_or("").to_string();
                    // Behalte die Zielversion und alles was nicht zu dieser MC-Version gehört
                    if ver_name.starts_with(mc_version) && ver_name != target_suffix {
                        tracing::info!("Removing old Forge dir: {:?}", version_dir);
                        tokio::fs::remove_dir_all(&version_dir).await.ok();
                    }
                }
            }
        }
        // Auch alte Installer-JARs entfernen (forge-1.21.11-61.0.0-installer.jar etc.)
        if let Ok(entries) = std::fs::read_dir(libraries_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() { continue; }
                let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                // Pattern: forge-{mc_version}-{old_forge_ver}-installer.jar
                if fname.starts_with(&format!("forge-{}-", mc_version))
                    && fname.ends_with("-installer.jar")
                    && !fname.contains(&target_suffix)
                {
                    tracing::info!("Removing old Forge installer: {:?}", path);
                    tokio::fs::remove_file(&path).await.ok();
                }
            }
        }
    }

    pub async fn resolve_latest_version(&self, mc_version: &str) -> Result<String> {
        use crate::api::forge::ForgeClient;
        let client = ForgeClient::new()?;
        let versions = client.get_loader_versions(mc_version).await?;
        let version = versions.first()
            .ok_or_else(|| anyhow::anyhow!("No Forge version found for MC {}", mc_version))?;
        Ok(version.forge_version.clone())
    }
}

fn find_java_binary() -> String {
    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = PathBuf::from(&home).join("bin").join("java");
        if p.exists() { return p.display().to_string(); }
    }
    for p in &[
        "/usr/lib/jvm/java-21-openjdk-amd64/bin/java",
        "/usr/lib/jvm/java-21-openjdk/bin/java",
        "/usr/bin/java",
    ] {
        if Path::new(p).exists() { return p.to_string(); }
    }
    "java".to_string()
}

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
    arg.replace("${library_directory}", &libraries_dir.display().to_string())
       .replace("${classpath_separator}", ":")
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

