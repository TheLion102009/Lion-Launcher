//! Horizon-Builds: GitHub-Releases von `horizon-rust/horizon` finden, prüfen und entpacken.
//!
//! Der neueste Build ist das Release `nightly` (ältere `nightly-<datum>` werden nach sieben Tagen
//! gelöscht – deshalb gibt es keine wählbaren Versionen). Jedes Archiv wird vor dem Entpacken
//! geprüft: `SHA256SUMS` ist mit Horizons Ed25519-Release-Schlüssel signiert (über
//! `horizon release <tag>\n` + `SHA256SUMS`), das Archiv muss die dort gelistete SHA-256 haben.
//!
//! Builds liegen im Layout des offiziellen Horizon-Launchers (`<daten>/versions/<id>` mit
//! `.installed`, Ressourcen in `.local/horizon-client/<mc>`). So kann dessen Kommandozeile
//! (für native Mods, siehe [`super::native`]) dieselben Builds und Ressourcen nutzen, ohne
//! etwas doppelt zu laden oder vorzubereiten.

use crate::utils::process::HideConsoleWindow;
use base64::Engine as _;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const RELEASE_API: &str = "https://api.github.com/repos/horizon-rust/horizon/releases/tags/nightly";
const RELEASES_API: &str = "https://api.github.com/repos/horizon-rust/horizon/releases?per_page=40";
/// Tag, über den die Signatur eines Nightlys läuft (unabhängig von seinem Datum).
const RELEASE_TAG: &str = "nightly";
/// Öffentliche Release-Schlüssel (crates/horizon-release-sign/release-keys.txt im Horizon-Repo).
const RELEASE_KEYS: &[&str] = &["MU4oiIQcCxkZvbwSjjXs7SySCuM6ywRQ3nGaJYZG0gw="];
/// Markiert einen von Lion Launcher vollständig entpackten Build (Inhalt: Anzeigename).
const COMPLETE_MARKER: &str = ".lion-installed";
/// Marker des Horizon-Launchers für einen installierten Build.
const HORIZON_MARKER: &str = ".installed";

pub fn horizon_dir() -> PathBuf {
    crate::config::defaults::launcher_dir().join("horizon")
}

/// Datenordner für die Kommandozeile des Horizon-Launchers (`HORIZON_LAUNCHER_HOME`).
pub fn launcher_home() -> PathBuf {
    horizon_dir().join("launcher")
}

fn builds_dir() -> PathBuf {
    launcher_home().join("versions")
}

pub fn build_dir(id: &str) -> PathBuf {
    builds_dir().join(id)
}

/// Minecraft-Version, die ein Build nachbaut (aus seiner `research-lock.json`).
fn game_version(id: &str) -> String {
    std::fs::read_to_string(build_dir(id).join("research-lock.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|lock| lock["minecraft"]["version"].as_str().map(String::from))
        .unwrap_or_else(|| super::MINECRAFT_VERSION.to_string())
}

/// Vorbereitete Ressourcen eines Builds – dort, wo auch der Horizon-Launcher sie erwartet.
pub fn resources_dir(id: &str) -> PathBuf {
    build_dir(id)
        .join(".local")
        .join("horizon-client")
        .join(game_version(id))
}

fn architecture() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// Marker, an denen der Horizon-Launcher einen installierten Build erkennt.
fn write_horizon_markers(dir: &Path) -> Result<(), String> {
    std::fs::write(dir.join(HORIZON_MARKER), "").map_err(|e| e.to_string())?;
    let arch = architecture();
    std::fs::write(
        dir.join(".horizon-game-architecture.json"),
        format!(r#"{{"requested":"{arch}","binary":"{arch}"}}"#),
    )
    .map_err(|e| e.to_string())
}

/// Übernimmt Builds aus dem alten Layout (`horizon/builds`, `horizon/resources`).
pub fn migrate_legacy_layout() {
    let old_builds = horizon_dir().join("builds");
    let Ok(entries) = std::fs::read_dir(&old_builds) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let id = entry.file_name().to_string_lossy().to_string();
        if !entry.path().join(COMPLETE_MARKER).is_file() || build_dir(&id).exists() {
            std::fs::remove_dir_all(entry.path()).ok();
            continue;
        }
        if std::fs::create_dir_all(builds_dir()).is_err()
            || std::fs::rename(entry.path(), build_dir(&id)).is_err()
        {
            continue;
        }
        write_horizon_markers(&build_dir(&id)).ok();
        let old_resources = horizon_dir().join("resources").join(&id);
        let resources = resources_dir(&id);
        if old_resources.exists() {
            if let Some(parent) = resources.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::rename(&old_resources, &resources).ok();
        }
    }
    std::fs::remove_dir_all(&old_builds).ok();
    std::fs::remove_dir_all(horizon_dir().join("resources")).ok();
}

pub fn client_binary(id: &str) -> PathBuf {
    build_dir(id).join(if cfg!(windows) {
        "horizon-client.exe"
    } else {
        "horizon-client"
    })
}

pub fn prepare_binary(id: &str) -> PathBuf {
    build_dir(id).join(if cfg!(windows) {
        "horizon-prepare.exe"
    } else {
        "horizon-prepare"
    })
}

/// Archivname des Builds für dieses System.
pub fn platform_asset() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("horizon-windows-x64.zip"),
        ("windows", "aarch64") => Some("horizon-windows-arm64.zip"),
        ("linux", "x86_64") => Some("horizon-linux-x86_64.tar.gz"),
        ("macos", "aarch64") => Some("horizon-macos-arm64.zip"),
        ("macos", "x86_64") => Some("horizon-macos-x64.zip"),
        _ => None,
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Release {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    target_commitish: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Der aktuell veröffentlichte Nightly.
#[derive(Debug, Clone)]
pub struct LatestBuild {
    /// Ordnername, z.B. `nightly-2026-11-13-9f42064`
    pub id: String,
    /// Anzeigename, z.B. `Nightly 2026-11-13 · 9f42064`
    pub name: String,
    release: Release,
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("Lion-Launcher/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .unwrap_or_default()
}

/// Ordnername aus Datum (im Release-Namen) und Commit: sortiert chronologisch.
fn build_id(name: &str, commit: &str) -> String {
    let date = name
        .split(|c: char| !(c.is_ascii_digit() || c == '-'))
        .find(|part| part.len() == 10 && part.as_bytes()[4] == b'-' && part.as_bytes()[7] == b'-')
        .unwrap_or("unknown");
    let short: String = commit
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(7)
        .collect::<String>()
        .to_lowercase();
    format!("nightly-{date}-{short}")
}

pub async fn latest() -> Result<LatestBuild, String> {
    let release: Release = http()
        .get(RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Horizon-Release nicht erreichbar: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Ungültige Release-Antwort: {e}"))?;
    Ok(LatestBuild {
        id: build_id(&release.name, &release.target_commitish),
        name: release.name.clone(),
        release,
    })
}

/// Installierte Builds, neuester zuerst.
pub fn installed_builds() -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(builds_dir())
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().join(COMPLETE_MARKER).is_file())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    ids
}

pub fn is_installed(id: &str) -> bool {
    build_dir(id).join(COMPLETE_MARKER).is_file()
}

/// Anzeigename eines installierten Builds (beim Installieren gespeichert).
pub fn display_name(id: &str) -> String {
    std::fs::read_to_string(build_dir(id).join(COMPLETE_MARKER))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| id.to_string())
}

/// Prüft die Signatur von `SHA256SUMS` (eine Base64-Signatur pro Zeile).
pub fn verify_checksums(sums: &[u8], signatures: &str, tag: &str) -> bool {
    let mut message = format!("horizon release {tag}\n").into_bytes();
    message.extend_from_slice(sums);
    let engine = base64::engine::general_purpose::STANDARD;
    RELEASE_KEYS.iter().any(|key| {
        let Some(key) = engine
            .decode(key)
            .ok()
            .and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok())
            .and_then(|b| iroh::PublicKey::from_bytes(&b).ok())
        else {
            return false;
        };
        signatures.lines().any(|line| {
            engine
                .decode(line.trim())
                .ok()
                .and_then(|b| <[u8; 64]>::try_from(b.as_slice()).ok())
                .is_some_and(|sig| {
                    key.verify(&message, &iroh::Signature::from_bytes(&sig))
                        .is_ok()
                })
        })
    })
}

/// SHA-256 eines Archivs aus `SHA256SUMS` (`sha256sum`-Format).
fn expected_sha256(sums: &str, asset: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == asset).then(|| hash.trim().to_lowercase())
    })
}

async fn download_text(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| e.to_string())
}

/// Lädt ein Archiv eines Releases, prüft es gegen den signierten `SHA256SUMS` und entpackt es
/// nach `dest` (Inhalt des einzigen Unterordners, falls das Archiv einen hat).
async fn download_signed_asset(
    release: &Release,
    asset_name: &str,
    signed_tag: &str,
    dest: &Path,
    label: &str,
    progress: &(dyn Fn(String, u8) + Sync),
) -> Result<(), String> {
    let find = |name: &str| release.assets.iter().find(|a| a.name == name);
    let asset = find(asset_name).ok_or_else(|| {
        format!("Das Release enthält kein Archiv für dieses System ({asset_name})")
    })?;
    let sums_asset = find("SHA256SUMS").ok_or("Release ohne SHA256SUMS")?;
    let sig_asset = find("SHA256SUMS.sig").ok_or("Release ohne Signatur")?;

    let client = http();
    let sums = download_text(&client, &sums_asset.browser_download_url).await?;
    let signature = download_text(&client, &sig_asset.browser_download_url).await?;
    if !verify_checksums(&sums, &String::from_utf8_lossy(&signature), signed_tag) {
        return Err(format!(
            "Die Signatur von {label} ist ungültig – Download abgebrochen"
        ));
    }
    let expected = expected_sha256(&String::from_utf8_lossy(&sums), asset_name)
        .ok_or("Das Archiv fehlt in SHA256SUMS")?;

    // Herunterladen und dabei hashen
    let downloads = horizon_dir().join("downloads");
    tokio::fs::create_dir_all(&downloads)
        .await
        .map_err(|e| e.to_string())?;
    let archive = downloads.join(asset_name);
    let part = downloads.join(format!("{asset_name}.part"));
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Download fehlgeschlagen: {e}"))?;
    let total = response.content_length().unwrap_or(asset.size).max(1);
    let mut file = tokio::fs::File::create(&part)
        .await
        .map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    let mut last_percent = u8::MAX;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download abgebrochen: {e}"))?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        done += chunk.len() as u64;
        let percent = ((done * 100) / total).min(100) as u8;
        if percent != last_percent {
            last_percent = percent;
            progress(
                format!(
                    "Lade {label} ({} / {} MB)...",
                    done / 1_048_576,
                    total / 1_048_576
                ),
                // Download belegt 0–10 % des Gesamtfortschritts
                percent / 10,
            );
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    if hex::encode(hasher.finalize()) != expected {
        tokio::fs::remove_file(&part).await.ok();
        return Err(format!(
            "Prüfsumme von {label} stimmt nicht – Download abgebrochen"
        ));
    }
    tokio::fs::remove_file(&archive).await.ok();
    tokio::fs::rename(&part, &archive)
        .await
        .map_err(|e| e.to_string())?;

    progress(format!("Entpacke {label}..."), 10);
    let staging = dest.with_extension("tmp");
    let archive_for_task = archive.clone();
    let staging_for_task = staging.clone();
    tokio::task::spawn_blocking(move || extract(&archive_for_task, &staging_for_task))
        .await
        .map_err(|e| e.to_string())??;
    let root = single_subdir(&staging).unwrap_or_else(|| staging.clone());
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| e.to_string())?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&root, dest).map_err(|e| e.to_string())?;
    std::fs::remove_dir_all(&staging).ok();
    tokio::fs::remove_file(&archive).await.ok();
    Ok(())
}

/// Lädt, prüft und entpackt den Build. Liefert seine ID.
pub async fn install(
    latest: &LatestBuild,
    progress: &(dyn Fn(String, u8) + Sync),
) -> Result<String, String> {
    if is_installed(&latest.id) {
        return Ok(latest.id.clone());
    }
    let asset_name = platform_asset().ok_or("Horizon gibt es für dieses System (noch) nicht")?;
    let target = build_dir(&latest.id);
    download_signed_asset(
        &latest.release,
        asset_name,
        RELEASE_TAG,
        &target,
        "Horizon",
        progress,
    )
    .await?;
    write_horizon_markers(&target)?;
    std::fs::write(target.join(COMPLETE_MARKER), &latest.name).map_err(|e| e.to_string())?;
    remove_old_builds(&latest.id);
    Ok(latest.id.clone())
}

// ── Kommandozeile des Horizon-Launchers (für native Mods) ─────────────────────

fn cli_asset() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("horizon-launcher-windows-x64.zip"),
        ("windows", "aarch64") => Some("horizon-launcher-windows-arm64.zip"),
        ("linux", "x86_64") => Some("horizon-launcher-linux-x86_64.tar.gz"),
        ("macos", "aarch64") => Some("horizon-launcher-macos-arm64.zip"),
        ("macos", "x86_64") => Some("horizon-launcher-macos-x64.zip"),
        _ => None,
    }
}

fn tools_dir() -> PathBuf {
    horizon_dir().join("tools")
}

/// Programm `horizon-launcher` in einem entpackten Release (auf macOS in der App).
fn find_cli(dir: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "horizon-launcher.exe"
    } else {
        "horizon-launcher"
    };
    walkdir::WalkDir::new(dir)
        .max_depth(5)
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.file_type().is_file() && e.file_name() == name)
        .map(|e| e.into_path())
}

/// Zuletzt installierte Kommandozeile (neueste zuerst).
fn installed_cli() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(tools_dir())
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.join(COMPLETE_MARKER).is_file())
        .collect();
    dirs.sort();
    dirs.iter().rev().find_map(|d| find_cli(d))
}

static CLI_CHECK: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
const CLI_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Stellt die Kommandozeile des Horizon-Launchers bereit (signiert wie die Spiel-Builds,
/// höchstens einmal am Tag auf eine neue Version geprüft).
pub async fn ensure_cli(progress: &(dyn Fn(String, u8) + Sync)) -> Result<PathBuf, String> {
    let existing = installed_cli();
    let due = CLI_CHECK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_none_or(|t| t.elapsed() > CLI_CHECK_INTERVAL);
    if let (Some(cli), false) = (&existing, due) {
        return Ok(cli.clone());
    }
    let asset = cli_asset().ok_or("Native Horizon-Mods gibt es für dieses System nicht")?;
    let releases: Result<Vec<Release>, String> = async {
        http()
            .get(RELEASES_API)
            .header("Accept", "application/vnd.github+json")
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    let release = match releases {
        Ok(list) => list.into_iter().find(|r| {
            r.tag_name.starts_with("launcher@") && r.assets.iter().any(|a| a.name == asset)
        }),
        Err(e) => {
            return existing.ok_or_else(|| {
                format!(
                    "Native Mods können gerade nicht gebaut werden: Die Werkzeuge dafür kommen von Horizons \
                     GitHub-Projekt, das nicht erreichbar ist ({e}). Schalte die nativen Mods aus, um ohne \
                     sie zu spielen, oder versuche es später erneut."
                )
            })
        }
    };
    *CLI_CHECK.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
    let Some(release) = release else {
        return existing.ok_or_else(|| "Kein Horizon-Launcher-Release gefunden".to_string());
    };
    let folder: String = release
        .tag_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let dest = tools_dir().join(&folder);
    if dest.join(COMPLETE_MARKER).is_file() {
        if let Some(cli) = find_cli(&dest) {
            return Ok(cli);
        }
    }
    download_signed_asset(
        &release,
        asset,
        &release.tag_name,
        &dest,
        "Horizon-Werkzeuge",
        progress,
    )
    .await?;
    std::fs::write(dest.join(COMPLETE_MARKER), &release.tag_name).map_err(|e| e.to_string())?;
    // Ältere Versionen aufräumen
    if let Ok(entries) = std::fs::read_dir(tools_dir()) {
        for entry in entries.filter_map(|e| e.ok()) {
            if entry.path() != dest {
                std::fs::remove_dir_all(entry.path()).ok();
            }
        }
    }
    find_cli(&dest).ok_or_else(|| "horizon-launcher fehlt im Release".to_string())
}

fn single_subdir(dir: &Path) -> Option<PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .collect();
    match entries.as_slice() {
        [only] if only.path().is_dir() => Some(only.path()),
        _ => None,
    }
}

fn extract(archive: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let name = archive.to_string_lossy();
    if name.ends_with(".tar.gz") {
        let status = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(dest)
            .status()
            .map_err(|e| format!("tar nicht gefunden: {e}"))?;
        return if status.success() {
            Ok(())
        } else {
            Err("Das Horizon-Archiv konnte nicht entpackt werden".into())
        };
    }

    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        // Nur Pfade innerhalb des Zielordners (kein `..`, keine absoluten Pfade)
        let Some(relative) = entry.enclosed_name().map(Path::to_path_buf) else {
            continue;
        };
        let out = dest.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut target = std::fs::File::create(&out).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut target).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode)).ok();
        }
    }
    Ok(())
}

/// Behält den neuen und den davor genutzten Build (zum Zurückgehen), räumt die übrigen von Lion
/// installierten auf. Native Builds verwaltet der Horizon-Launcher selbst.
fn remove_old_builds(current: &str) {
    let keep: Vec<String> = installed_builds()
        .into_iter()
        .filter(|id| id != current)
        .take(1)
        .chain(std::iter::once(current.to_string()))
        .collect();
    let removed: Vec<String> = installed_builds()
        .into_iter()
        .filter(|id| !keep.contains(id))
        .collect();
    for id in &removed {
        std::fs::remove_dir_all(build_dir(id)).ok();
    }
    // Native Builds auf einem entfernten Build starten ohne ihn ohnehin nicht mehr
    let Ok(entries) = std::fs::read_dir(builds_dir()) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let base = std::fs::read_to_string(entry.path().join("native-mods.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|m| m["base"].as_str().map(String::from));
        if base.is_some_and(|b| removed.contains(&b)) {
            std::fs::remove_dir_all(entry.path()).ok();
        }
    }
}

// ── Launch-API ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiOption {
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
}

/// Antwort von `horizon-client --launch-api`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientApi {
    #[serde(default)]
    pub launch_api: String,
    #[serde(default)]
    pub horizon: String,
    #[serde(default)]
    pub minecraft: String,
    #[serde(default)]
    pub options: Vec<ApiOption>,
    /// Was der Build lädt (Mod-API, Shader-API, Pack-Formate)
    #[serde(default)]
    pub content: serde_json::Value,
}

/// Antwort von `horizon-prepare --launch-api`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrepareApi {
    #[serde(default)]
    pub minecraft: String,
    #[serde(default)]
    pub java_major: u32,
    #[serde(default)]
    pub options: Vec<ApiOption>,
}

pub trait LaunchOptions {
    fn options(&self) -> &[ApiOption];

    /// Nur Optionen übergeben, die der Build kennt – alles andere lehnt er mit Exit-Code 2 ab.
    fn supports(&self, option: &str) -> bool {
        self.options().iter().any(|o| o.name == option)
    }
}

impl LaunchOptions for ClientApi {
    fn options(&self) -> &[ApiOption] {
        &self.options
    }
}

impl LaunchOptions for PrepareApi {
    fn options(&self) -> &[ApiOption] {
        &self.options
    }
}

impl ClientApi {
    /// Unterstützte API-Linien einer Inhaltsart, z.B. `("mod", "api")` → `["0.2", "0.3"]`.
    pub fn content_list(&self, kind: &str, key: &str) -> Vec<String> {
        self.content[kind][key]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    }
}

async fn query_api<T: serde::de::DeserializeOwned + Serialize>(
    binary: &Path,
    cache: &Path,
) -> Result<T, String> {
    if let Some(cached) = std::fs::read_to_string(cache)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    {
        return Ok(cached);
    }
    let mut command = std::process::Command::new(binary);
    command
        .arg("--launch-api")
        .current_dir(binary.parent().unwrap_or(Path::new(".")))
        .hide_console_window();
    let output = tokio::task::spawn_blocking(move || command.output())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Horizon konnte nicht gestartet werden: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Horizon antwortet nicht auf --launch-api: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let value: T = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Ungültige Launch-API-Antwort: {e}"))?;
    if let Ok(json) = serde_json::to_string(&value) {
        std::fs::write(cache, json).ok();
    }
    Ok(value)
}

pub async fn client_api(id: &str) -> Result<ClientApi, String> {
    query_api(
        &client_binary(id),
        &build_dir(id).join(".lion-client-api.json"),
    )
    .await
}

pub async fn prepare_api(id: &str) -> Result<PrepareApi, String> {
    query_api(
        &prepare_binary(id),
        &build_dir(id).join(".lion-prepare-api.json"),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Echte Daten des Nightlys vom 2026-11-13.
    const SUMS: &str = "83d36c5a2f8b6690d4d77411dfc3f85753319184fffe738ee5f0a4c552a0eabe  horizon-linux-x86_64.tar.gz
f47f9f988b7c30b727dfb35aed2b74f01428925b2eccc9bafff2352f4847f7d6  horizon-windows-x64.zip
9cc4b7a72afba2df2a2c729f76a983d1e6cb82de44d1474fca04bc4d2a40d553  horizon-macos-arm64.zip
f7d2d17d488ab2111fa0e7ce36a033fe6eb6db3ee6b74bc86aaa26dbab7e0144  horizon-macos-x64.zip
f621e334c254d56bfeed16ed989500a36ef82a6a2e84ee6b861adcb774ced9b5  horizon-server-linux-x86_64.tar.gz
aa9bf9383ae4e926d5f9c2ca8be997e843a6cf590fc3c8c04162a26718adeabd  horizon-source.tar.gz
";
    const SIG: &str = "aKtoYPO/dznYhh78vFORQvWnW/vzUvi6mM7w2PSNZcVwmMkuFpJzRbu+YvqMOJ0krL8Qkec/wNPYLZ6wzPmoCg==\n";

    #[test]
    fn verifies_real_release_signature() {
        assert!(verify_checksums(SUMS.as_bytes(), SIG, "nightly"));
        // Unter einem anderen Tag oder mit veränderter Liste ungültig
        assert!(!verify_checksums(SUMS.as_bytes(), SIG, "v0.2.0"));
        let tampered = SUMS.replace("f47f9f98", "00000000");
        assert!(!verify_checksums(tampered.as_bytes(), SIG, "nightly"));
    }

    #[test]
    fn finds_archive_checksum() {
        assert_eq!(
            expected_sha256(SUMS, "horizon-windows-x64.zip").as_deref(),
            Some("f47f9f988b7c30b727dfb35aed2b74f01428925b2eccc9bafff2352f4847f7d6")
        );
        assert_eq!(expected_sha256(SUMS, "missing.zip"), None);
    }

    #[test]
    fn build_ids_sort_by_date() {
        let id = build_id(
            "Nightly 2026-11-13 · 9f42064",
            "9f42064c11efe7ef3e5ed2baad7ddc2a3b36eea0",
        );
        assert_eq!(id, "nightly-2026-11-13-9f42064");
        assert!(build_id("Nightly 2026-11-14 · a", "abcdef0") > id);
    }
}
