//! Auto-Updater über die GitHub-Releases des Projekts.
//!
//! * Stabile Builds (aus einem `v*`-Tag) vergleichen ihre Version mit dem neuesten Release.
//! * Nightly-Builds vergleichen ihren Commit mit dem des `nightly`-Releases – über die
//!   GitHub-Compare-API, damit nur wirklich *neuere* Builds als Update gelten.
//! * Lokale Entwicklungs-Builds aktualisieren sich nicht.
//!
//! Installation: Windows startet den NSIS-Installer, Linux-AppImages werden ersetzt. Für andere
//! Pakete (deb, rpm, dmg, ...) wird auf die Release-Seite verwiesen. Jede Datei wird vor der
//! Installation gegen die von GitHub gemeldete SHA-256-Prüfsumme geprüft.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

const REPO: &str = "TheLion102009/Lion-Launcher";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CURRENT_SHA: &str = env!("LION_GIT_SHA");
const BUILD_CHANNEL: &str = env!("LION_BUILD_CHANNEL");
const PROGRESS_EVENT: &str = "updater-progress";

#[derive(Debug, Clone, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    /// z.B. "sha256:0361bc…"
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    target_commitish: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhCompare {
    status: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub channel: String,
    pub commit: String,
    pub available: bool,
    /// Update kann direkt im Launcher installiert werden
    pub can_install: bool,
    pub latest_label: Option<String>,
    pub published_at: Option<String>,
    pub notes: Option<String>,
    pub release_url: Option<String>,
    pub asset_name: Option<String>,
    pub asset_size: Option<u64>,
    /// Hinweis, wenn nicht automatisch installiert werden kann
    pub hint: Option<String>,
}

#[derive(Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: u64,
}

/// Ergebnis einer Prüfung: Infos für die Oberfläche + passende Installationsdatei.
type CheckResult = (UpdateInfo, Option<GhAsset>);

/// Ergebnis der letzten Prüfung (für die Installation).
static LAST_CHECK: once_cell::sync::Lazy<std::sync::Mutex<Option<CheckResult>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));

fn short_sha(sha: &str) -> String {
    sha.chars().take(7).collect()
}

fn base_info() -> UpdateInfo {
    UpdateInfo {
        current_version: CURRENT_VERSION.to_string(),
        channel: BUILD_CHANNEL.to_string(),
        commit: short_sha(CURRENT_SHA),
        ..Default::default()
    }
}

/// Vergleicht `1.2.3` mit `1.2.10` usw. Vorabversionen (`1.2.3-beta`) sind älter als `1.2.3`.
fn is_newer_version(candidate: &str, current: &str) -> bool {
    fn parse(v: &str) -> (Vec<u64>, bool) {
        let v = v.trim().trim_start_matches('v');
        let (numbers, pre) = v.split_once('-').map_or((v, false), |(n, _)| (n, true));
        let parts = numbers
            .split('.')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect();
        (parts, pre)
    }
    let (mut a, a_pre) = parse(candidate);
    let (mut b, b_pre) = parse(current);
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => b_pre && !a_pre,
    }
}

async fn github_json<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let url = format!("https://api.github.com/repos/{REPO}/{path}");
    let resp = crate::core::http::HTTP_CLIENT
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub nicht erreichbar: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub-Anfrage fehlgeschlagen: {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// Ist der Nightly-Build `remote_sha` neuer als der laufende Build?
async fn nightly_is_newer(remote_sha: &str) -> bool {
    if remote_sha.is_empty() || remote_sha.eq_ignore_ascii_case(CURRENT_SHA) {
        return false;
    }
    if CURRENT_SHA.is_empty() {
        return true;
    }
    match github_json::<GhCompare>(&format!("compare/{CURRENT_SHA}...{remote_sha}")).await {
        // "ahead": Nightly enthält neuere Commits; "diverged": anderer Stand → anbieten
        Ok(compare) => matches!(compare.status.as_str(), "ahead" | "diverged"),
        // Vergleich nicht möglich (z.B. Commit unbekannt) → im Zweifel anbieten
        Err(_) => true,
    }
}

/// Passende Installationsdatei für dieses System.
fn pick_asset(assets: &[GhAsset]) -> Option<GhAsset> {
    let find = |suffix: &str| {
        assets
            .iter()
            .find(|a| a.name.to_lowercase().ends_with(suffix))
            .cloned()
    };
    if cfg!(target_os = "windows") {
        return find("-setup.exe");
    }
    if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some() {
        return find(".appimage");
    }
    None
}

async fn check_for_update() -> Result<CheckResult, String> {
    let mut info = base_info();
    if BUILD_CHANNEL == "dev" {
        info.hint = Some("Entwicklungs-Build – automatische Updates sind deaktiviert".to_string());
        return Ok((info, None));
    }

    let stable = BUILD_CHANNEL == "stable";
    let release: GhRelease = if stable {
        github_json("releases/latest").await?
    } else {
        github_json("releases/tags/nightly").await?
    };

    info.available = if stable {
        is_newer_version(&release.tag_name, CURRENT_VERSION)
    } else {
        nightly_is_newer(&release.target_commitish).await
    };
    info.latest_label = Some(if stable {
        release.tag_name.clone()
    } else {
        format!("Nightly {}", short_sha(&release.target_commitish))
    });
    info.published_at = release.published_at.clone();
    info.notes = release.body.clone();
    info.release_url = Some(release.html_url.clone());

    let asset = pick_asset(&release.assets);
    if let Some(asset) = &asset {
        info.asset_name = Some(asset.name.clone());
        info.asset_size = Some(asset.size);
    }
    info.can_install = info.available && asset.is_some();
    if info.available && asset.is_none() {
        info.hint = Some(
            "Für dieses Paketformat bitte das Update von der Release-Seite installieren"
                .to_string(),
        );
    }
    Ok((info, asset))
}

#[tauri::command]
pub async fn updater_check() -> Result<UpdateInfo, String> {
    let (info, asset) = check_for_update().await?;
    *LAST_CHECK.lock().unwrap_or_else(|e| e.into_inner()) = Some((info.clone(), asset));
    Ok(info)
}

/// Lädt die Datei mit Fortschrittsanzeige (ohne Gesamt-Timeout – Installer sind groß).
async fn download(
    app: &AppHandle,
    url: &str,
    dest: &Path,
    expected_size: u64,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("Lion-Launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Download fehlgeschlagen: {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(expected_size);
    let part = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&part)
        .await
        .map_err(|e| e.to_string())?;
    let mut stream = resp.bytes_stream();
    let mut downloaded = 0u64;
    let mut last_percent = u64::MAX;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download abgebrochen: {e}"))?;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;
        let percent = (downloaded * 100).checked_div(total).unwrap_or(0);
        if percent != last_percent {
            last_percent = percent;
            let _ = app.emit(PROGRESS_EVENT, Progress { downloaded, total });
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    tokio::fs::remove_file(dest).await.ok();
    tokio::fs::rename(&part, dest)
        .await
        .map_err(|e| e.to_string())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::Digest as _;
    use std::io::Read as _;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Startet die neue Version bzw. den Installer und beendet den Launcher.
fn launch_update(app: &AppHandle, file: &Path) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        // NSIS-Installer im Passiv-Modus (Fortschritt ohne Rückfragen), danach Launcher neu starten
        std::process::Command::new(file)
            .args(["/P", "/R"])
            .spawn()
            .map_err(|e| format!("Installer konnte nicht gestartet werden: {e}"))?;
    } else if let Some(appimage) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
        replace_appimage(file, &appimage)?;
        std::process::Command::new(&appimage)
            .spawn()
            .map_err(|e| format!("Neue Version konnte nicht gestartet werden: {e}"))?;
    } else {
        return Err("Automatische Installation wird hier nicht unterstützt".to_string());
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Kurz warten, damit die Oberfläche den Status noch anzeigen kann
        tokio::time::sleep(Duration::from_millis(800)).await;
        app.exit(0);
    });
    Ok(())
}

#[cfg(unix)]
fn replace_appimage(new_file: &Path, appimage: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let staged = appimage.with_extension("new");
    std::fs::copy(new_file, &staged).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&staged, appimage).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn replace_appimage(_new_file: &Path, _appimage: &Path) -> Result<(), String> {
    Err("AppImages gibt es nur unter Linux".to_string())
}

/// Lädt das zuletzt gefundene Update, prüft die Prüfsumme und installiert es.
#[tauri::command]
pub async fn updater_install(app: AppHandle) -> Result<(), String> {
    let (info, asset) = LAST_CHECK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or_else(|| "Bitte zuerst nach Updates suchen".to_string())?;
    let asset = asset
        .filter(|_| info.can_install)
        .ok_or_else(|| "Kein installierbares Update gefunden".to_string())?;

    let filename = crate::gui::content_share::safe_file_name(&asset.name)
        .ok_or_else(|| "Ungültiger Dateiname im Release".to_string())?;
    let dir = std::env::temp_dir().join("lion-launcher-update");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    let file = dir.join(filename);

    download(&app, &asset.browser_download_url, &file, asset.size).await?;

    if let Some(expected) = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
    {
        let path = file.clone();
        let actual = tokio::task::spawn_blocking(move || sha256_file(&path))
            .await
            .map_err(|e| e.to_string())??;
        if !actual.eq_ignore_ascii_case(expected) {
            tokio::fs::remove_file(&file).await.ok();
            return Err(
                "Prüfsumme des Updates stimmt nicht – Installation abgebrochen".to_string(),
            );
        }
    }

    launch_update(&app, &file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer_version("v0.2.0", "0.1.0"));
        assert!(is_newer_version("0.1.10", "0.1.9"));
        assert!(is_newer_version("1.0.0", "1.0.0-beta"));
        assert!(!is_newer_version("v0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.1.0-rc1", "0.1.0"));
        assert!(!is_newer_version("0.0.9", "0.1.0"));
        assert!(is_newer_version("1.0", "0.9.9"));
    }

    #[test]
    fn picks_platform_asset() {
        let asset = |name: &str| GhAsset {
            name: name.to_string(),
            browser_download_url: String::new(),
            size: 0,
            digest: None,
        };
        let assets = vec![
            asset("Lion.Launcher_0.1.0_amd64.deb"),
            asset("Lion.Launcher_0.1.0_x64-setup.exe"),
            asset("Lion.Launcher_0.1.0_universal.dmg"),
        ];
        let picked = pick_asset(&assets);
        if cfg!(target_os = "windows") {
            assert_eq!(picked.unwrap().name, "Lion.Launcher_0.1.0_x64-setup.exe");
        } else if std::env::var_os("APPIMAGE").is_none() {
            assert!(picked.is_none());
        }
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;

    /// `cargo test --bin Lion-Launcher updater_against_github -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "benötigt Internet"]
    async fn updater_against_github() {
        let release: GhRelease = github_json("releases/tags/nightly").await.unwrap();
        println!("nightly commit {}", short_sha(&release.target_commitish));
        let asset = pick_asset(&release.assets);
        println!("asset: {:?}", asset.as_ref().map(|a| (&a.name, &a.digest)));
        // Älterer Commit → die Nightly ist neuer
        let compare: GhCompare =
            github_json(&format!("compare/c91f6ef...{}", release.target_commitish))
                .await
                .unwrap();
        println!("c91f6ef...nightly: {}", compare.status);
        assert_eq!(compare.status, "ahead");
        // Gleicher Commit → identisch
        let same: GhCompare = github_json(&format!("compare/{0}...{0}", release.target_commitish))
            .await
            .unwrap();
        assert_eq!(same.status, "identical");
    }
}
