use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use serde::Deserialize;

// NeoForge Installation und Launch-Logik
// Basierend auf PandoraLauncher und PrismLauncher Best Practices

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

    let _major = mc_parts[0]; // "1"
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

    // 2. Führe den Installer aus um die PATCHED-Client-JAR zu erstellen
    let launcher_dir = libraries_dir.parent().unwrap();
    run_neoforge_installer(&installer_path, launcher_dir, java_path, mc_version, &actual_version).await?;

    // 3. Extrahiere die version.json aus dem Installer
    let version_json = extract_version_json(&installer_path)?;
    let version: NeoForgeVersion = serde_json::from_str(&version_json)?;

    tracing::info!("✅ NeoForge main class: {}", version.main_class);

    // Extrahiere die NeoForm-Version aus den Game-Args
    let neoform_version = extract_neoform_version(&version)?;
    tracing::info!("✅ Detected NeoForm version: {}", neoform_version);

    // 4. Finde die Game-JAR (SRG-JAR oder patched-JAR je nach NeoForge-Version)
    let minecraft_jar = find_game_jar(mc_version, &neoform_version, &actual_version, libraries_dir)?;
    tracing::info!("✅ Found game JAR: {:?}", minecraft_jar);

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

    tracing::info!("📦 Adding Vanilla libraries (LWJGL, etc.)...");

    let blacklist = [
        "asm",                    // ASM ist in NeoForge mit neuerer Version
        "bootstraplauncher",      // Bereits im Module Path
        "securejarhandler",       // Bereits im Module Path
        "JarJar",                 // Bereits im Module Path
        "eventbus",               // Teil von NeoForge
        "coremods",               // Teil von NeoForge
        "modlauncher",            // Bereits geladen
        "neoforge",               // Natürlich!
    ];

    for vanilla_lib in std::env::split_paths(std::ffi::OsStr::new(vanilla_classpath))
        .map(|p| p.to_string_lossy().to_string())
    {
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
                        .replace("${classpath_separator}", if cfg!(windows) { ";" } else { ":" })
                        .replace("${version_name}", &actual_version);
                    jvm_args.push(processed);
                }
            }
        }
    }

    // 7. Parse Game-Argumente - EXAKT wie in der offiziellen NeoForge version.json!
    // KRITISCH: fml.fmlVersion und fml.neoFormVersion DYNAMISCH aus der version.json lesen,
    // da sie sich zwischen NeoForge-Versionen unterscheiden.
    let fml_version = extract_fml_version(&version);
    tracing::info!("✅ Detected FML version: {}", fml_version);

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
    _mc_version: &str,
    neoforge_version: &str,
) -> Result<()> {
    // CRITISCH: Der PATCHED Client JAR ist die einzig korrekte Datei die den Installer-Abschluss beweist.
    // Format: libraries/net/neoforged/neoforge/{version}/neoforge-{version}-client.jar
    // Dieser JAR wird durch die Installer-Prozessoren erstellt und enthält die gepatchte Minecraft-Version.
    // NUR dieser JAR registriert sich bei FML als "minecraft" und "neoforge" Mod korrekt.
    let patched_client_jar = launcher_dir.join(format!(
        "libraries/net/neoforged/neoforge/{}/neoforge-{}-client.jar",
        neoforge_version, neoforge_version
    ));
    tracing::info!("🔍 Prüfe patched client JAR: {:?}", patched_client_jar);
    if patched_client_jar.exists() && is_valid_zip_file(&patched_client_jar) {
        tracing::info!("✅ Patched client JAR für NeoForge {} existiert bereits, Installer wird übersprungen", neoforge_version);
        return Ok(());
    }

    tracing::info!("⚠️  Patched client JAR für NeoForge {} fehlt, Installer wird ausgeführt...", neoforge_version);

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
    extract_game_arg_value(version, "--fml.neoFormVersion")
        .ok_or_else(|| anyhow::anyhow!("Could not extract NeoForm version from version.json"))
}

/// Extrahiert die FML-Version aus der NeoForge version.json
fn extract_fml_version(version: &NeoForgeVersion) -> String {
    extract_game_arg_value(version, "--fml.fmlVersion")
        .unwrap_or_else(|| {
            tracing::warn!("--fml.fmlVersion not found in version.json, using fallback 4.0.42");
            "4.0.42".to_string()
        })
}

/// Allgemeine Hilfsfunktion: Sucht den Wert nach einem bestimmten Schlüssel in arguments.game
fn extract_game_arg_value(version: &NeoForgeVersion, key: &str) -> Option<String> {
    if let Some(args) = &version.arguments {
        if let Some(game) = &args.game {
            for i in 0..game.len() {
                if let Some(arg_str) = game[i].as_str() {
                    if arg_str == key {
                        if i + 1 < game.len() {
                            if let Some(version_str) = game[i + 1].as_str() {
                                return Some(version_str.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Findet die Game-JAR: der durch den Installer erstellte PATCHED Client JAR
fn find_game_jar(mc_version: &str, neoform_version: &str, neoforge_version: &str, libraries_dir: &Path) -> Result<PathBuf> {
    tracing::info!("🔍 Suche NeoForge Game-JAR (NeoForge {}, MC {}, NeoForm {})...", neoforge_version, mc_version, neoform_version);

    // PRIMÄR (NeoForge 21.x / 1.21.x): Der PATCHED Client JAR der durch Installer-Prozessoren erstellt wird.
    // Koordinate: [net.neoforged:neoforge:{version}:client]
    // Pfad: libraries/net/neoforged/neoforge/{version}/neoforge-{version}-client.jar
    // Dieser JAR wird von NeoForge/FML als "minecraft" und "neoforge" Mod erkannt.
    let patched_client = libraries_dir.join(format!(
        "net/neoforged/neoforge/{}/neoforge-{}-client.jar",
        neoforge_version, neoforge_version
    ));
    tracing::info!("  [1/3] Prüfe PATCHED client JAR: {:?}", patched_client);
    if patched_client.exists() && is_valid_zip_file(&patched_client) {
        tracing::info!("  ✅ PATCHED client JAR gefunden: {:?}", patched_client);
        return Ok(patched_client);
    }

    // FALLBACK (ältere NeoForge-Versionen mit minecraft-client-patched Koordinate):
    // Scanne net/neoforged/minecraft-client-patched/ nach beliebiger passender Version
    let old_patched_dir = libraries_dir.join("net/neoforged/minecraft-client-patched");
    if old_patched_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&old_patched_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let ver = entry.file_name().to_string_lossy().to_string();
                    let jar = path.join(format!("minecraft-client-patched-{}.jar", ver));
                    if jar.exists() && is_valid_zip_file(&jar) {
                        tracing::info!("  ✅ Alter minecraft-client-patched JAR gefunden: {:?}", jar);
                        return Ok(jar);
                    }
                }
            }
        }
    }

    // LETZTER AUSWEG (sollte nicht passieren wenn Installer korrekt lief):
    // Falls das Installer-Ergebnis an einem anderen Ort liegt
    let srg_jar = libraries_dir.join(format!(
        "net/minecraft/client/{}-{}/client-{}-{}-srg.jar",
        mc_version, neoform_version, mc_version, neoform_version
    ));
    tracing::info!("  [3/3] Prüfe SRG-JAR als letzten Ausweg: {:?}", srg_jar);
    if srg_jar.exists() && is_valid_zip_file(&srg_jar) {
        tracing::warn!("  ⚠️  Nur SRG-JAR gefunden! NeoForge PATCHED client JAR fehlt.");
        tracing::warn!("      Der Installer wurde möglicherweise nicht vollständig ausgeführt.");
        tracing::warn!("      Bitte NeoForge neu installieren oder Profil zurücksetzen.");
        // SRG-JAR NICHT zurückgeben – NeoForge würde minecraft/neoforge als [MISSING] melden!
    }

    bail!("❌ NeoForge Game-JAR nicht gefunden!\n\
           Erwartet: {:?}\n\
           Der NeoForge-Installer muss erst vollständig ausgeführt werden.\n\
           Die Installer-Prozessoren erstellen den PATCHED client JAR (net.neoforged:neoforge:{}:client).",
        patched_client, neoforge_version)
}

/// Prüft ob eine Datei eine gültige ZIP/JAR-Datei ist
fn is_valid_zip_file(path: &Path) -> bool {
    if let Ok(file) = std::fs::File::open(path) {
        zip::ZipArchive::new(file).is_ok()
    } else {
        false
    }
}

/// Baut die vollständige Command-Line für den Start
#[allow(clippy::too_many_arguments)]
pub fn build_launch_command(
    installation: &NeoForgeInstallation,
    java_path: &str,
    memory_mb: u32,
    java_version: u32,
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
    // Auf Windows javaw.exe nutzen um kein CMD-Fenster zu öffnen.
    // Tauri-Apps sind windowless (windows_subsystem = "windows"), daher würde java.exe
    // ein CMD-Fenster öffnen. javaw.exe ist eine Windows-GUI-Anwendung ohne Konsole.
    let java_bin = if cfg!(windows) {
        // Robuste Konvertierung: nur das letzte Segment ersetzen
        let p = std::path::Path::new(java_path);
        if p.file_name().and_then(|n| n.to_str()) == Some("java.exe") {
            p.with_file_name("javaw.exe").display().to_string()
        } else {
            java_path.to_string()
        }
    } else {
        java_path.to_string()
    };

    let mut cmd = Command::new(&java_bin);

    // Plattform-optimierte JVM-Flags (Xmx/Xms + G1GC-Tuning + OS-spezifische Flags)
    let os_name = std::env::consts::OS; // "linux", "windows", "macos"
    for flag in super::get_jvm_flags(os_name, java_version, memory_mb) {
        cmd.arg(flag);
    }
    // java.library.path: Standard-JVM-Pfad für native Bibliotheken (alle Versionen)
    cmd.arg(format!("-Djava.library.path={}", natives_dir.display()));
    // org.lwjgl.librarypath: LWJGL 3.3.2+ bevorzugt diese Property auf Windows.
    // Ohne sie findet LWJGL lwjgl.dll nicht auch wenn java.library.path korrekt gesetzt ist.
    cmd.arg(format!("-Dorg.lwjgl.librarypath={}", natives_dir.display()));
    // JNA-Bibliothekspfad auf Linux: damit text2speech/libflite.so im natives-Dir gefunden wird.
    #[cfg(target_os = "linux")]
    {
        // Symlink für libflite.so erstellen falls nur versionierte lib vorhanden
        let search_dirs = [
            "/usr/lib",
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib64",
            "/usr/local/lib",
            "/lib",
            "/lib/x86_64-linux-gnu",
        ];
        let unversioned_name = "libflite.so";
        let mut flite_found = false;
        'search: for dir in &search_dirs {
            let dir_path = std::path::Path::new(dir);
            if dir_path.join(unversioned_name).exists() {
                flite_found = true;
                break 'search;
            }
            if let Ok(entries) = std::fs::read_dir(dir_path) {
                for entry in entries.flatten() {
                    let fname = entry.file_name().to_string_lossy().to_string();
                    if fname.starts_with("libflite.so.") {
                        let symlink_target = natives_dir.join(unversioned_name);
                        if !symlink_target.exists() {
                            std::os::unix::fs::symlink(entry.path(), &symlink_target).ok();
                            tracing::info!("Narrator-Fix: Symlink {} → {}", symlink_target.display(), entry.path().display());
                        }
                        flite_found = true;
                        break 'search;
                    }
                }
            }
        }
        if !flite_found {
            tracing::warn!("libflite.so nicht gefunden – Narrator deaktiviert. Installiere 'flite' (pacman -S flite oder apt install libflite1).");
        }
        cmd.arg(format!("-Djna.library.path={}", natives_dir.display()));
    }

    // KRITISCHE System Properties für NeoForge/BootstrapLauncher
    cmd.arg("-Djava.net.preferIPv6Addresses=system");
    cmd.arg(format!("-DignoreList={}.jar,client-extra", version));
    cmd.arg(format!("-DlibraryDirectory={}", libraries_dir.display()));
    let cp_sep = if cfg!(windows) { ";" } else { ":" };
    cmd.arg(format!("-DlegacyClassPath={}", installation.classpath.join(cp_sep)));

    // NeoForge JVM-Args
    for arg in &installation.jvm_args {
        cmd.arg(arg);
    }

    // Module Path (falls vorhanden)
    if !installation.module_path.is_empty() {
        cmd.arg("-p");
        cmd.arg(installation.module_path.join(cp_sep));
        cmd.arg("--add-modules");
        cmd.arg("ALL-MODULE-PATH");
    }

    // Classpath - KRITISCH!
    cmd.arg("-cp");
    cmd.arg(installation.classpath.join(cp_sep));

    // Main Class
    cmd.arg(&installation.main_class);

    // NeoForge Game Args (enthält bereits --gameJar!)
    for arg in &installation.game_args {
        cmd.arg(arg);
    }

    // userType: "msa" für Microsoft-Accounts, "legacy" für Offline-Accounts.
    // NICHT hardcoden – bei Offline-Accounts führt "msa" zu Auth-Fehlern.
    let user_type = if !access_token.is_empty() && access_token != "0" {
        "msa"
    } else {
        "legacy"
    };

    // Vanilla Game Args
    cmd.arg("--username").arg(username);
    cmd.arg("--version").arg(version);
    cmd.arg("--gameDir").arg(game_dir);
    cmd.arg("--assetsDir").arg(assets_dir);
    cmd.arg("--assetIndex").arg(asset_index);
    cmd.arg("--uuid").arg(uuid);
    cmd.arg("--accessToken").arg(access_token);
    cmd.arg("--userType").arg(user_type);

    cmd.current_dir(game_dir);

    // Auf Windows: Stdio::null() statt inherit(), da der Tauri-Prozess kein Konsolenfenster hat
    // (windows_subsystem = "windows"). Inherit würde ungültige Handles vererben.
    // NeoForge schreibt Logs ohnehin in debug.log / latest.log im GameDir.
    #[cfg(windows)]
    {
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());
    }
    #[cfg(not(windows))]
    {
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
    }

    cmd
}
