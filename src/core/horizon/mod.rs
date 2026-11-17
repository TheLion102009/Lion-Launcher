//! Horizon – Minecraft: Java Edition 26.3 in Rust nachgebaut (github.com/horizon-rust/horizon).
//!
//! Lion Launcher behandelt Horizon wie einen weiteren Loader und folgt dabei Horizons
//! offizieller Launch-API (`docs/api/launch.md`, `launch-guide.md`, `content.md`):
//!
//! 1. **Build**: der neueste Nightly wird geladen, gegen den signierten `SHA256SUMS` geprüft und
//!    entpackt ([`build`]).
//! 2. **Vorbereiten**: Horizon enthält keine Mojang-Dateien. `horizon-prepare` baut seine
//!    Ressourcen aus den Original-Dateien von Minecraft 26.3 (Version-JSON, Client-JAR,
//!    Libraries, Assets, Server-JAR) und braucht dafür Java 25. Das geschieht nur für einen
//!    Microsoft-Account, der Minecraft besitzt – dieselbe Bedingung wie im Horizon-Launcher.
//! 3. **Starten**: `horizon-client --resources … --game-dir … --session-stdin`; die Sitzung geht
//!    über die Standardeingabe, nie über die Kommandozeile.
//!
//! Inhalte (Mods, Resource Packs, Shader) kommen aus Horizons eigenem Verzeichnis ([`registry`]).

pub mod build;
pub mod native;
pub mod registry;

use crate::core::minecraft::{
    register_running_process, send_launch_progress, unregister_running_process,
};
use crate::types::profile::Profile;
use crate::utils::process::HideConsoleWindow;
use build::LaunchOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Minecraft-Version, die Horizon nachbaut (falls der Build noch nicht gefragt werden konnte).
pub const MINECRAFT_VERSION: &str = "26.3";
/// So oft wird höchstens bei GitHub nach einem neuen Nightly gefragt (API-Limit: 60/h).
const LATEST_CHECK_INTERVAL: Duration = Duration::from_secs(10 * 60);

static LATEST_CHECK: Mutex<Option<(Instant, build::LatestBuild)>> = Mutex::new(None);

const NEEDS_MICROSOFT: &str = "Horizon braucht einen Microsoft-Account, der Minecraft: Java Edition besitzt. \
     Horizon baut seine Spieldateien aus deinem gekauften Minecraft – mit einem Offline-Account geht das nicht.";

/// Ein startbereiter Build.
pub struct Ready {
    pub build_id: String,
    pub resources: PathBuf,
    pub client: build::ClientApi,
}

async fn latest_cached() -> Result<build::LatestBuild, String> {
    if let Some((checked, latest)) = LATEST_CHECK
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        if checked.elapsed() < LATEST_CHECK_INTERVAL {
            return Ok(latest.clone());
        }
    }
    let latest = build::latest().await?;
    *LATEST_CHECK.lock().unwrap_or_else(|e| e.into_inner()) =
        Some((Instant::now(), latest.clone()));
    Ok(latest)
}

/// Neuesten Build sicherstellen; ohne Internet (oder bei einem Fehler) den zuletzt installierten.
async fn current_build() -> Result<String, String> {
    build::migrate_legacy_layout();
    let progress = |status: String, percent: u8| send_launch_progress(status, percent);
    let fallback = build::installed_builds().into_iter().next();
    match latest_cached().await {
        Ok(latest) => match build::install(&latest, &progress).await {
            Ok(id) => Ok(id),
            Err(e) => match fallback {
                Some(id) => {
                    tracing::warn!("Horizon update failed, using {}: {}", id, e);
                    crate::core::minecraft::add_launch_warning(format!(
                        "Neuer Horizon-Build konnte nicht installiert werden ({e}) – es startet der vorherige."
                    ));
                    Ok(id)
                }
                None => Err(e),
            },
        },
        Err(e) => fallback.ok_or_else(|| {
            format!("Horizon ist noch nicht installiert und GitHub ist nicht erreichbar: {e}")
        }),
    }
}

async fn require_microsoft() -> Result<(), String> {
    if crate::gui::auth::active_account_is_microsoft().await {
        Ok(())
    } else {
        Err(NEEDS_MICROSOFT.to_string())
    }
}

/// Ordner, die Horizon im Spielordner erwartet (`content.md`).
fn create_game_folders(game_dir: &Path) {
    for folder in [
        "horizon-mods",
        "resourcepacks",
        "shaderpacks",
        "saves",
        "logs",
    ] {
        std::fs::create_dir_all(game_dir.join(folder)).ok();
    }
}

/// `horizon-prepare --check`: schnell, liest keine Spieldateien.
async fn is_prepared(build_id: &str, api: &build::PrepareApi, resources: &Path) -> bool {
    if !resources.exists() {
        return false;
    }
    if !api.supports("--check") || !api.supports("--output") {
        return true;
    }
    let mut command = Command::new(build::prepare_binary(build_id));
    command
        .current_dir(build::build_dir(build_id))
        .arg("--check")
        .arg("--output")
        .arg(resources)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .hide_console_window();
    tokio::task::spawn_blocking(move || command.status())
        .await
        .ok()
        .and_then(|r| r.ok())
        .is_some_and(|s| s.success())
}

/// Lädt den Build und bereitet seine Ressourcen vor (nur wenn nötig).
pub async fn prepare(profile: &Profile) -> Result<Ready, String> {
    create_game_folders(&profile.game_dir);
    prepare_build(true).await
}

/// `check_owner`: nur für einen Microsoft-Account (in Tests abschaltbar).
async fn prepare_build(check_owner: bool) -> Result<Ready, String> {
    send_launch_progress("Suche nach dem neuesten Horizon-Build...", 2);
    let build_id = current_build().await?;
    let client = build::client_api(&build_id).await?;
    let prepare_api = build::prepare_api(&build_id).await?;
    let resources = build::resources_dir(&build_id);

    if is_prepared(&build_id, &prepare_api, &resources).await {
        return Ok(Ready {
            build_id,
            resources,
            client,
        });
    }

    if check_owner {
        require_microsoft().await?;
    }
    let minecraft = if prepare_api.minecraft.is_empty() {
        MINECRAFT_VERSION.to_string()
    } else {
        prepare_api.minecraft.clone()
    };

    send_launch_progress(
        format!("Lade Minecraft {minecraft} (Grundlage für Horizon)..."),
        11,
    );
    let launcher = crate::core::minecraft::MinecraftLauncher::new().map_err(|e| e.to_string())?;
    let files = launcher
        .ensure_vanilla_files(&minecraft)
        .await
        .map_err(|e| format!("Minecraft {minecraft} konnte nicht geladen werden: {e}"))?;

    // Das Server-JAR braucht nur die erste Vorbereitung; kein Launcher hat es sonst
    let server_jar = build::horizon_dir()
        .join("server-jars")
        .join(format!("{minecraft}.jar"));
    if !server_jar.exists() {
        let (url, sha1) = files
            .server_jar
            .clone()
            .ok_or("Die Version-JSON enthält keinen Server-Download")?;
        send_launch_progress(format!("Lade Minecraft {minecraft} Server-JAR..."), 26);
        launcher
            .download_verified(&url, &server_jar, &sha1)
            .await
            .map_err(|e| e.to_string())?;
    }

    let java_major = if prepare_api.java_major > 0 {
        prepare_api.java_major
    } else {
        25
    };
    send_launch_progress(format!("Prüfe Java {java_major}..."), 28);
    let java = crate::core::minecraft::ensure_java_runtime(java_major, None).await?;
    let java_home = Path::new(&java)
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or("Java-Ordner nicht gefunden")?;

    let mut args: Vec<std::ffi::OsString> = vec!["--prepare".into()];
    let mut push = |option: &str, value: Option<&Path>| {
        if prepare_api.supports(option) {
            args.push(option.into());
            if let Some(value) = value {
                args.push(value.as_os_str().to_owned());
            }
        }
    };
    push("--server-jar", Some(&server_jar));
    push("--version-json", Some(&files.version_json));
    push("--client-jar", Some(&files.client_jar));
    push("--libraries-dir", Some(&files.libraries_dir));
    push("--assets-dir", Some(&files.assets_dir));
    push("--java-home", Some(&java_home));
    push("--output", Some(&resources));
    push("--complete", None);
    if prepare_api.supports("--progress") {
        args.push("--progress".into());
        args.push("json".into());
    }

    send_launch_progress(
        "Bereite Horizon vor (beim ersten Mal einige Minuten)...",
        30,
    );
    run_preparer(&build_id, args, &java_home).await?;

    Ok(Ready {
        build_id,
        resources,
        client,
    })
}

/// Führt `horizon-prepare` aus und meldet `--progress json` an die Oberfläche.
async fn run_preparer(
    build_id: &str,
    args: Vec<std::ffi::OsString>,
    java_home: &Path,
) -> Result<(), String> {
    let mut command = Command::new(build::prepare_binary(build_id));
    command
        .current_dir(build::build_dir(build_id))
        .args(&args)
        .env("JAVA_HOME", java_home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .hide_console_window();

    tokio::task::spawn_blocking(move || {
        let mut child = command
            .spawn()
            .map_err(|e| format!("horizon-prepare konnte nicht starten: {e}"))?;
        let stderr = child.stderr.take();
        let stderr_lines = std::thread::spawn(move || {
            stderr
                .map(|s| {
                    BufReader::new(s)
                        .lines()
                        .map_while(Result::ok)
                        .inspect(|line| tracing::info!("[horizon-prepare] {}", line))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        });

        let mut percent: u8 = 30;
        let mut message = String::from("Bereite Horizon vor...");
        let mut error: Option<String> = None;
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Ok(event) = serde_json::from_str::<serde_json::Value>(&line) else {
                    tracing::info!("[horizon-prepare] {}", line);
                    continue;
                };
                if let Some(text) = event["message"].as_str() {
                    tracing::info!("[horizon-prepare] {}", text);
                    message = text.to_string();
                }
                if let Some(fraction) = event["fraction"].as_f64() {
                    // Vorbereitung belegt 30–95 % des Gesamtfortschritts
                    percent = (30.0 + fraction.clamp(0.0, 1.0) * 65.0) as u8;
                }
                if let Some(text) = event["error"].as_str() {
                    error = Some(text.to_string());
                }
                send_launch_progress(format!("Horizon: {message}"), percent);
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        let stderr_lines = stderr_lines.join().unwrap_or_default();
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "Horizon konnte nicht vorbereitet werden: {}",
                error
                    .or_else(|| stderr_lines.last().cloned())
                    .unwrap_or_else(|| format!("Exit-Code {:?}", status.code()))
            ))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Startet Horizon für ein Profil (eigener Spielordner pro Profil).
pub async fn launch(
    profile: &Profile,
    username: &str,
    uuid: &str,
    access_token: Option<&str>,
    extra_args: Vec<String>,
) -> Result<(), String> {
    let token = access_token
        .filter(|t| !t.is_empty() && *t != "0")
        .ok_or(NEEDS_MICROSOFT)?;
    require_microsoft().await?;
    let ready = prepare(profile).await?;
    let game_dir = &profile.game_dir;

    // Native Mods: eigenes Spielprogramm, auf diesem Build kompiliert (nutzt dessen Ressourcen)
    let program = match native::ensure_build(game_dir, &ready.build_id).await? {
        Some(native) => native,
        None => build::client_binary(&ready.build_id),
    };

    send_launch_progress("Starte Horizon...", 97);
    let mut command = Command::new(&program);
    let mut push = |option: &str, value: Option<&std::ffi::OsStr>| {
        if ready.client.supports(option) {
            command.arg(option);
            if let Some(value) = value {
                command.arg(value);
            }
        }
    };
    push("--resources", Some(ready.resources.as_os_str()));
    push("--game-dir", Some(game_dir.as_os_str()));
    push("--home", Some(game_dir.as_os_str()));
    push("--session-stdin", None);
    // Schnellstart in eine Welt / auf einen Server (gleiche Optionen wie Vanilla)
    for pair in extra_args.chunks(2) {
        if let [option, value] = pair {
            push(option, Some(std::ffi::OsStr::new(value)));
        }
    }
    command
        .current_dir(game_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .hide_console_window();

    let mut child = command
        .spawn()
        .map_err(|e| format!("Horizon konnte nicht gestartet werden: {e}"))?;
    // Sitzung über die Standardeingabe – nie auf der Kommandozeile, wo andere sie lesen könnten
    let session = serde_json::json!({
        "name": username,
        "uuid": uuid,
        "access_token": token,
        "user_type": "msa",
    });
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(session.to_string().as_bytes())
            .map_err(|e| format!("Sitzung konnte nicht übergeben werden: {e}"))?;
    }

    let pid = child.id();
    tracing::info!("Horizon {} gestartet mit PID {}", ready.build_id, pid);
    register_running_process(&profile.id, pid);

    if let Some(stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                tracing::info!("[Horizon] {}", line);
            }
        });
    }
    let stderr = child.stderr.take();
    let profile_id = profile.id.clone();
    std::thread::spawn(move || {
        let mut last_line = String::new();
        if let Some(stderr) = stderr {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                tracing::warn!("[Horizon] {}", line);
                last_line = line;
            }
        }
        match child.wait() {
            Ok(status) if status.success() => tracing::info!("Horizon (PID {}) beendet", pid),
            // 1 = Fehler, 101 = Absturz: die letzte Zeile nennt den Crash-Report
            Ok(status) => tracing::warn!(
                "Horizon (PID {}) beendet mit {:?}: {}",
                pid,
                status.code(),
                last_line
            ),
            Err(e) => tracing::error!("Warten auf Horizon fehlgeschlagen: {}", e),
        }
        unregister_running_process(&profile_id);
    });
    Ok(())
}

/// Verwirft alle vorbereiteten Ressourcen (Profil reparieren).
pub fn forget_prepared_resources() {
    for id in build::installed_builds() {
        std::fs::remove_dir_all(build::resources_dir(&id)).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ganzer Weg bis vor den Start: Build laden und prüfen, Minecraft-Dateien, Server-JAR und
    /// Java besorgen, `horizon-prepare` laufen lassen und mit `--check` bestätigen.
    /// Schreibt in den echten Launcher-Ordner (wie ein Start).
    /// `cargo test --bin Lion-Launcher horizon_end_to_end_prepare -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "benötigt Internet, lädt mehrere hundert MB"]
    async fn horizon_end_to_end_prepare() {
        let ready = prepare_build(false).await.expect("prepare");
        let api = build::prepare_api(&ready.build_id).await.unwrap();
        assert!(is_prepared(&ready.build_id, &api, &ready.resources).await);
        assert!(ready.client.supports("--session-stdin"));
        println!("ready: {} -> {}", ready.build_id, ready.resources.display());
    }
}
