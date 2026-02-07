use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use serde::Deserialize;

/// NeoForge Installation und Launch-Logik
/// Basierend auf PandoraLauncher und PrismLauncher Best Practices

/// Ermittelt die neueste NeoForge-Version für eine Minecraft-Version dynamisch von der API
async fn get_latest_neoforge_version(mc_version: &str) -> Result<String> {
    tracing::info!("🔍 Searching for NeoForge versions for Minecraft {}...", mc_version);

    // Verwende die NeoForge Maven-Metadata API
    let maven_metadata_url = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";

    let response = match reqwest::get(maven_metadata_url).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("⚠️  Failed to fetch NeoForge versions: {}", e);
            return get_fallback_version(mc_version);
        }
    };

    let xml = match response.text().await {
        Ok(text) => text,
        Err(e) => {
            tracing::warn!("⚠️  Failed to read metadata: {}", e);
            return get_fallback_version(mc_version);
        }
    };

    // Parse die Maven-Metadata XML und sammle alle Versionen
    let mut all_versions: Vec<String> = Vec::new();

    for line in xml.lines() {
        let line = line.trim();
        if line.starts_with("<version>") && line.ends_with("</version>") {
            let version = line.replace("<version>", "").replace("</version>", "");
            all_versions.push(version);
        }
    }

    if all_versions.is_empty() {
        tracing::warn!("⚠️  No versions found in metadata");
        return get_fallback_version(mc_version);
    }

    // Filtere Versionen basierend auf Minecraft-Version
    let matching_versions = filter_matching_versions(&all_versions, mc_version);

    if !matching_versions.is_empty() {
        // Sortiere und nimm die neueste
        let mut sorted = matching_versions.clone();
        sorted.sort_by(|a, b| compare_versions(a, b));

        let latest = sorted.last().unwrap().clone();
        tracing::info!("✅ Found NeoForge {} for Minecraft {} (from {} candidates)",
            latest, mc_version, matching_versions.len());
        return Ok(latest);
    }

    // Fallback wenn nichts gefunden
    tracing::warn!("⚠️  No matching NeoForge version found for MC {}", mc_version);
    get_fallback_version(mc_version)
}

/// Filtert NeoForge-Versionen die zur Minecraft-Version passen
fn filter_matching_versions(all_versions: &[String], mc_version: &str) -> Vec<String> {
    let mc_parts: Vec<&str> = mc_version.split('.').collect();

    if mc_parts.len() < 2 {
        return Vec::new();
    }

    let major = mc_parts[0]; // "1"
    let minor = mc_parts[1]; // "21" oder "20" oder "19"
    let patch = mc_parts.get(2).unwrap_or(&"0"); // "2" oder "1" oder "0"

    let mut matching = Vec::new();

    // NeoForge verwendet unterschiedliche Schemas:
    // - Minecraft 1.20.2+ → NeoForge {minor}.{patch}.x (z.B. 21.1.219 für MC 1.21.1)
    // - Minecraft 1.20.1 → Forge-Schema 47.x.x

    for version in all_versions {
        let is_match = if minor == "20" && *patch == "1" {
            // Spezialfall: MC 1.20.1 verwendet alte Forge-Nummerierung (47.x.x)
            version.starts_with("47.")
        } else if minor.parse::<u32>().unwrap_or(0) >= 20 {
            // Moderne Versionen: NeoForge {minor}.{patch}.x
            let expected = if *patch == "0" {
                format!("{}.0.", minor)
            } else {
                format!("{}.{}.", minor, patch)
            };
            version.starts_with(&expected)
        } else {
            // Sehr alte Versionen (1.19.x und früher) - nicht unterstützt
            false
        };

        if is_match {
            matching.push(version.clone());
        }
    }

    matching
}

/// Vergleicht zwei Versionsnummern
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let a_parts: Vec<u32> = a.split('.')
        .filter_map(|s| s.split('-').next()?.parse().ok())
        .collect();
    let b_parts: Vec<u32> = b.split('.')
        .filter_map(|s| s.split('-').next()?.parse().ok())
        .collect();

    for i in 0..a_parts.len().max(b_parts.len()) {
        let a_val = a_parts.get(i).unwrap_or(&0);
        let b_val = b_parts.get(i).unwrap_or(&0);
        if a_val != b_val {
            return a_val.cmp(b_val);
        }
    }
    std::cmp::Ordering::Equal
}

/// Fallback-Versionen wenn API nicht erreichbar oder keine Version gefunden
fn get_fallback_version(mc_version: &str) -> Result<String> {
    let fallback = if mc_version.starts_with("1.21") {
        "21.1.219"
    } else if mc_version.starts_with("1.20.6") {
        "20.6.119"
    } else if mc_version.starts_with("1.20.5") {
        "20.5.21"
    } else if mc_version.starts_with("1.20.4") {
        "20.4.233"
    } else if mc_version.starts_with("1.20.3") {
        "20.3.8"
    } else if mc_version.starts_with("1.20.2") {
        "20.2.88"
    } else if mc_version.starts_with("1.20.1") {
        "47.1.106" // Alte Forge-Nummerierung
    } else if mc_version.starts_with("1.20") {
        "20.4.233"
    } else {
        "21.1.219"
    };

    tracing::info!("📋 Using fallback NeoForge {} for Minecraft {}", fallback, mc_version);
    Ok(fallback.to_string())
}

#[derive(Debug)]
pub struct NeoForgeInstallation {
    pub main_class: String,
    pub classpath: Vec<String>,
    pub module_path: Vec<String>,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub minecraft_jar: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NeoForgeVersion {
    id: Option<String>,
    main_class: String,
    #[serde(rename = "inheritsFrom")]
    inherits_from: Option<String>,
    libraries: Vec<NeoForgeLibrary>,
    arguments: Option<NeoForgeArguments>,
}

#[derive(Debug, Deserialize)]
struct NeoForgeLibrary {
    name: String,
    downloads: Option<NeoForgeDownloads>,
}

#[derive(Debug, Deserialize)]
struct NeoForgeDownloads {
    artifact: Option<NeoForgeArtifact>,
}

#[derive(Debug, Deserialize)]
struct NeoForgeArtifact {
    path: String,
    url: String,
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NeoForgeArguments {
    jvm: Option<Vec<serde_json::Value>>,
    game: Option<Vec<serde_json::Value>>,
}

/// Installiert NeoForge und bereitet die Launch-Konfiguration vor
pub async fn install_neoforge(
    mc_version: &str,
    neoforge_version: &str,
    libraries_dir: &Path,
    _versions_dir: &Path,
    java_path: &str,
    vanilla_classpath: &str,
) -> Result<NeoForgeInstallation> {
    // Wenn "latest" angegeben wurde, ermittle die tatsächliche Version
    let actual_version = if neoforge_version == "latest" || neoforge_version.is_empty() {
        let latest = get_latest_neoforge_version(mc_version).await?;
        tracing::info!("🔍 Resolved 'latest' to NeoForge version: {}", latest);
        latest
    } else {
        neoforge_version.to_string()
    };

    tracing::info!("🔨 Installing NeoForge {} for Minecraft {}", actual_version, mc_version);

    // 1. Lade den NeoForge-Installer
    let installer_path = download_neoforge_installer(&actual_version, libraries_dir).await?;

    // 2. Führe den Installer aus um die SRG-JAR zu erstellen
    let launcher_dir = libraries_dir.parent().unwrap();
    run_neoforge_installer(&installer_path, launcher_dir, java_path, mc_version).await?;

    // 3. Extrahiere die version.json aus dem Installer
    let version_json = extract_version_json(&installer_path)?;
    let version: NeoForgeVersion = serde_json::from_str(&version_json)?;

    tracing::info!("✅ NeoForge main class: {}", version.main_class);

    // Extrahiere die NeoForm-Version aus den Game-Args
    let neoform_version = extract_neoform_version(&version)?;
    tracing::info!("✅ Detected NeoForm version: {}", neoform_version);

    // 4. Finde die SRG-gemappte Minecraft-JAR mit der richtigen NeoForm-Version
    let minecraft_jar = find_srg_jar(mc_version, &neoform_version, libraries_dir)?;
    tracing::info!("✅ Found SRG-JAR: {:?}", minecraft_jar);

    // 5. Baue Classpath und JVM-Args
    let mut classpath = Vec::new();
    let mut module_path = Vec::new();

    // KRITISCH: Die SRG-JAR darf NICHT im Classpath sein!
    // NeoForge lädt sie über --gameJar als "minecraft" Mod
    // Wenn sie auch im Classpath ist, gibt es einen Modul-Konflikt!
    tracing::info!("⚠️  SRG-JAR wird NUR über --gameJar geladen, NICHT im Classpath!");

    // Lade NeoForge Libraries
    tracing::info!("📦 Processing {} NeoForge libraries...", version.libraries.len());
    for lib in &version.libraries {
        // Konvertiere Maven-Koordinate zu Pfad
        // Format: group.id:artifact:version -> group/id/artifact/version/artifact-version.jar
        let parts: Vec<&str> = lib.name.split(':').collect();
        if parts.len() < 3 {
            tracing::warn!("⚠️  Invalid library name: {}", lib.name);
            continue;
        }

        let group = parts[0].replace('.', "/");
        let artifact = parts[1];
        let lib_version = parts[2];

        let lib_path = libraries_dir.join(format!("{}/{}/{}/{}-{}.jar",
            group, artifact, lib_version, artifact, lib_version));

        // Lade Library herunter wenn sie fehlt
        if !lib_path.exists() {
            if let Some(downloads) = &lib.downloads {
                if let Some(artifact_info) = &downloads.artifact {
                    tracing::info!("📥 Downloading: {}", lib.name);

                    tokio::fs::create_dir_all(lib_path.parent().unwrap()).await.ok();

                    let response = reqwest::get(&artifact_info.url).await?;
                    let bytes = response.bytes().await?;
                    tokio::fs::write(&lib_path, &bytes).await?;
                }
            } else {
                tracing::warn!("⚠️  No download info for: {}", lib.name);
                continue;
            }
        }

        let path_str = lib_path.display().to_string();

        // Bestimmte Libraries gehören in den Module Path
        if lib.name.contains("bootstraplauncher") ||
           lib.name.contains("securejarhandler") ||
           lib.name.contains("JarJar") ||
           lib.name.contains("asm") {
            module_path.push(path_str);
        } else {
            classpath.push(path_str);
        }
    }

    tracing::info!("✅ Libraries loaded: {} classpath, {} module path", classpath.len(), module_path.len());

    // KRITISCH: Füge Vanilla-Libraries hinzu (LWJGL und andere)
    // ABER: Filtere Libraries die bereits in NeoForge enthalten sind!
    tracing::info!("📦 Adding Vanilla libraries (LWJGL, etc.)...");

    // Blacklist: Diese Libraries sind bereits in NeoForge enthalten und würden Konflikte verursachen
    let blacklist = [
        "asm",                    // ASM ist in NeoForge mit neuerer Version
        "bootstraplauncher",      // Bereits im Module Path
        "securejarhandler",       // Bereits im Module Path
        "JarJar",                 // Bereits im Module Path
        "eventbus",               // Teil von NeoForge
        "coremods",               // Teil von NeoForge
        "modlauncher",            // Bereits geladen
        "neoforge",               // Natürlich!
        "guava",                  // Konflikt mit NeoForge's guava
        "failureaccess",          // Teil von guava, Konflikt
        "jtracy",                 // Doppeltes Modul
    ];

    for vanilla_lib in vanilla_classpath.split(':') {
        if vanilla_lib.is_empty() {
            continue;
        }

        // Prüfe ob die Library bereits im Classpath ist
        if classpath.contains(&vanilla_lib.to_string()) {
            continue;
        }

        // Prüfe ob die Library in der Blacklist ist
        let is_blacklisted = blacklist.iter().any(|&blocked| {
            vanilla_lib.to_lowercase().contains(blocked)
        });

        if is_blacklisted {
            tracing::debug!("⚠️  Skipping blacklisted library: {}", vanilla_lib);
            continue;
        }

        classpath.push(vanilla_lib.to_string());
    }
    tracing::info!("✅ Total libraries: {} entries (after filtering)", classpath.len());

    // 6. Parse JVM-Argumente aus der version.json
    let mut jvm_args = Vec::new();
    if let Some(args) = &version.arguments {
        if let Some(jvm) = &args.jvm {
            for arg in jvm {
                if let Some(s) = arg.as_str() {
                    let processed = s
                        .replace("${library_directory}", &libraries_dir.display().to_string())
                        .replace("${classpath_separator}", ":")
                        .replace("${version_name}", &actual_version);
                    jvm_args.push(processed);
                }
            }
        }
    }

    // 7. Parse Game-Argumente - EXAKT wie in der offiziellen NeoForge version.json!
    // KRITISCH: fml.fmlVersion ist NICHT die NeoForge-Version, sondern die FML-Version (4.0.42)!
    // KRITISCH: fml.neoFormVersion wird aus der version.json extrahiert (dynamisch für jede MC-Version!)
    let fml_version = "4.0.42"; // FML Loader Version für NeoForge 21.1.x

    let game_args = vec![
        "--fml.neoForgeVersion".to_string(),
        actual_version.clone(),
        "--fml.fmlVersion".to_string(),
        fml_version.to_string(),
        "--fml.mcVersion".to_string(),
        mc_version.to_string(),
        "--fml.neoFormVersion".to_string(),
        neoform_version.clone(), // DYNAMISCH aus version.json!
        "--launchTarget".to_string(),
        "forgeclient".to_string(),
        "--gameJar".to_string(),
        minecraft_jar.display().to_string(),
    ];

    tracing::info!("✅ NeoForge installation complete");
    tracing::info!("   Game args (official format): {:?}", game_args);
    tracing::info!("   Game args: {:?}", game_args);
    tracing::info!("   Classpath: {} entries", classpath.len());
    tracing::info!("   Module path: {} entries", module_path.len());
    tracing::info!("   JVM args: {} entries", jvm_args.len());

    Ok(NeoForgeInstallation {
        main_class: version.main_class,
        classpath,
        module_path,
        jvm_args,
        game_args,
        minecraft_jar: minecraft_jar.display().to_string(),
    })
}

/// Lädt den NeoForge-Installer herunter
async fn download_neoforge_installer(
    neoforge_version: &str,
    libraries_dir: &Path,
) -> Result<PathBuf> {
    let installer_path = libraries_dir.join(format!("neoforge-{}-installer.jar", neoforge_version));

    if installer_path.exists() {
        tracing::info!("✅ NeoForge installer already exists");
        return Ok(installer_path);
    }

    let url = format!(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/{}/neoforge-{}-installer.jar",
        neoforge_version, neoforge_version
    );

    tracing::info!("📥 Downloading NeoForge installer from: {}", url);

    let response = reqwest::get(&url).await?;
    let bytes = response.bytes().await?;

    tokio::fs::create_dir_all(installer_path.parent().unwrap()).await?;
    tokio::fs::write(&installer_path, &bytes).await?;

    tracing::info!("✅ NeoForge installer downloaded");
    Ok(installer_path)
}

/// Führt den NeoForge-Installer aus
async fn run_neoforge_installer(
    installer_path: &Path,
    launcher_dir: &Path,
    java_path: &str,
    mc_version: &str,
) -> Result<()> {
    // Prüfe ob die SRG-JAR für DIESE spezifische Minecraft-Version bereits existiert
    // WICHTIG: Nicht nur prüfen ob IRGENDEINE SRG-JAR existiert, sondern die RICHTIGE Version!
    let srg_jar_dir = launcher_dir.join("libraries/net/minecraft/client");
    if srg_jar_dir.exists() {
        let has_correct_srg = std::fs::read_dir(&srg_jar_dir)?
            .filter_map(|e| e.ok())
            .any(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                // Prüfe ob es die SRG-JAR für die aktuelle MC-Version ist
                name.contains("srg.jar") && name.starts_with(&format!("client-{}-", mc_version))
            });

        if has_correct_srg {
            tracing::info!("✅ SRG-JAR for MC {} already exists, skipping installer", mc_version);
            return Ok(());
        } else {
            tracing::info!("⚠️  Found other SRG-JARs but not for MC {}, running installer...", mc_version);
        }
    }

    // Erstelle launcher_profiles.json falls nicht vorhanden
    let profiles_path = launcher_dir.join("launcher_profiles.json");
    if !profiles_path.exists() {
        tracing::info!("Creating launcher_profiles.json");
        let profiles_content = r#"{"profiles":{},"settings":{},"version":3}"#;
        tokio::fs::write(&profiles_path, profiles_content).await?;
    }

    tracing::info!("🔨 Running NeoForge installer (this may take 1-2 minutes)...");

    let mut cmd = Command::new(java_path);
    cmd.arg("-jar");
    cmd.arg(installer_path);
    cmd.arg("--installClient");
    cmd.arg(launcher_dir);
    cmd.current_dir(launcher_dir);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd.output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("❌ NeoForge installer failed: {}", stderr);
        bail!("NeoForge installer failed");
    }

    tracing::info!("✅ NeoForge installer completed successfully");
    Ok(())
}

/// Extrahiert die version.json aus dem NeoForge-Installer
fn extract_version_json(installer_path: &Path) -> Result<String> {
    use std::io::Read;

    let file = std::fs::File::open(installer_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // Suche nach version.json
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.name() == "version.json" {
            let mut contents = String::new();
            entry.read_to_string(&mut contents)?;
            return Ok(contents);
        }
    }

    bail!("version.json not found in NeoForge installer");
}

/// Extrahiert die NeoForm-Version aus der NeoForge version.json
fn extract_neoform_version(version: &NeoForgeVersion) -> Result<String> {
    if let Some(args) = &version.arguments {
        if let Some(game) = &args.game {
            // Suche nach --fml.neoFormVersion und dem folgenden Wert
            for i in 0..game.len() {
                if let Some(arg_str) = game[i].as_str() {
                    if arg_str == "--fml.neoFormVersion" {
                        // Der nächste Eintrag ist die Version
                        if i + 1 < game.len() {
                            if let Some(version_str) = game[i + 1].as_str() {
                                return Ok(version_str.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    bail!("Could not extract NeoForm version from version.json");
}

/// Findet die SRG-gemappte Minecraft-JAR mit der richtigen NeoForm-Version
fn find_srg_jar(mc_version: &str, neoform_version: &str, libraries_dir: &Path) -> Result<PathBuf> {
    // Mögliche Pfade für die SRG-JAR (mit dynamischer NeoForm-Version!)
    let possible_paths = vec![
        libraries_dir.join(format!("net/minecraft/client/{}-{}/client-{}-{}-srg.jar",
            mc_version, neoform_version, mc_version, neoform_version)),
        libraries_dir.join(format!("net/minecraft/client/{}-{}/client-{}-{}-slim.jar",
            mc_version, neoform_version, mc_version, neoform_version)),
        libraries_dir.join(format!("net/minecraft/{}/{}.jar", mc_version, mc_version)),
    ];

    tracing::info!("🔍 Searching for SRG-JAR (NeoForm {})...", neoform_version);
    for (i, path) in possible_paths.iter().enumerate() {
        tracing::info!("  [{}] Checking: {:?}", i, path);
        if path.exists() {
            // Verifiziere dass die JAR die LoadingOverlay Klasse enthält
            if verify_jar_has_class(path, "net/minecraft/client/gui/screens/LoadingOverlay.class")? {
                tracing::info!("  ✅ Found valid SRG-JAR at: {:?}", path);
                return Ok(path.clone());
            } else {
                tracing::warn!("  ⚠️  JAR exists but missing LoadingOverlay class");
            }
        }
    }

    bail!("❌ SRG-JAR not found! Run NeoForge installer first.");
}

/// Verifiziert dass eine JAR-Datei eine bestimmte Klasse enthält
fn verify_jar_has_class(jar_path: &Path, class_path: &str) -> Result<bool> {

    let file = std::fs::File::open(jar_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if entry.name() == class_path {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Baut die vollständige Command-Line für den Start
pub fn build_launch_command(
    installation: &NeoForgeInstallation,
    java_path: &str,
    memory_mb: u32,
    game_dir: &Path,
    assets_dir: &Path,
    natives_dir: &Path,
    libraries_dir: &Path,
    username: &str,
    uuid: &str,
    access_token: &str,
    version: &str,
    asset_index: &str,
) -> Command {
    let mut cmd = Command::new(java_path);

    // JVM-Optionen
    cmd.arg(format!("-Xmx{}M", memory_mb));
    cmd.arg(format!("-Xms{}M", memory_mb / 2));
    cmd.arg(format!("-Djava.library.path={}", natives_dir.display()));

    // KRITISCHE System Properties für NeoForge/BootstrapLauncher
    cmd.arg("-Djava.net.preferIPv6Addresses=system");
    cmd.arg(format!("-DignoreList={}.jar,client-extra", version));
    cmd.arg(format!("-DlibraryDirectory={}", libraries_dir.display()));
    cmd.arg(format!("-DlegacyClassPath={}", installation.classpath.join(":")));


    // NeoForge JVM-Args
    for arg in &installation.jvm_args {
        cmd.arg(arg);
    }

    // Module Path (falls vorhanden)
    if !installation.module_path.is_empty() {
        cmd.arg("-p");
        cmd.arg(installation.module_path.join(":"));
        cmd.arg("--add-modules");
        cmd.arg("ALL-MODULE-PATH");
    }

    // Classpath - KRITISCH!
    cmd.arg("-cp");
    cmd.arg(installation.classpath.join(":"));

    // Main Class
    cmd.arg(&installation.main_class);

    // NeoForge Game Args (enthält bereits --gameJar!)
    for arg in &installation.game_args {
        cmd.arg(arg);
    }

    // Vanilla Game Args
    cmd.arg("--username").arg(username);
    cmd.arg("--version").arg(version);
    cmd.arg("--gameDir").arg(game_dir);
    cmd.arg("--assetsDir").arg(assets_dir);
    cmd.arg("--assetIndex").arg(asset_index);
    cmd.arg("--uuid").arg(uuid);
    cmd.arg("--accessToken").arg(access_token);
    cmd.arg("--userType").arg("msa");

    cmd.current_dir(game_dir);
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    cmd
}
