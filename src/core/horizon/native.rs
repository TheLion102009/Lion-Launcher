//! Native Horizon-Mods: Rust-Quellcode (`.hnmod`), der auf dem PC des Spielers gegen den Build
//! kompiliert wird.
//!
//! Das übernimmt die Kommandozeile des offiziellen Horizon-Launchers
//! (`horizon-launcher native-build`, Horizons Referenz-Implementierung): Sie lädt den festgelegten
//! Rust-Compiler und Horizons Quellcode, wendet die Mixins der Mods an und baut daraus ein eigenes
//! Spielprogramm. Ihr Datenordner ist [`build::launcher_home`]; Builds und vorbereitete Ressourcen
//! teilt sie mit Lion Launcher. Der erste Bau dauert laut Horizon 20–30 Minuten, danach wird ein
//! passender Bau wiederverwendet.
//!
//! Native Mods laufen ohne Sandbox mit den Rechten des Spielers – deshalb muss er einmal zustimmen
//! ([`set_trusted`]).

use super::build;
use super::registry;
use crate::core::minecraft::send_launch_progress;
use crate::utils::process::HideConsoleWindow;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Fehlertext, an dem die Oberfläche erkennt, dass sie erst um Zustimmung bitten muss.
pub const TRUST_REQUIRED: &str = "NATIVE_TRUST_REQUIRED";

/// Nur ein Bau gleichzeitig: alle Profile teilen den Datenordner des Horizon-Launchers.
static BUILD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn trust_file() -> PathBuf {
    build::horizon_dir().join("native-trusted")
}

pub fn is_trusted() -> bool {
    trust_file().is_file()
}

pub fn set_trusted(trusted: bool) -> Result<(), String> {
    if trusted {
        std::fs::create_dir_all(build::horizon_dir()).map_err(|e| e.to_string())?;
        std::fs::write(trust_file(), "").map_err(|e| e.to_string())
    } else {
        std::fs::remove_file(trust_file()).ok();
        Ok(())
    }
}

/// Zuletzt gebautes Spielprogramm eines Profils.
#[derive(Serialize, Deserialize)]
struct BuildState {
    key: String,
    version: String,
}

fn state_file(game_dir: &Path) -> PathBuf {
    game_dir.join(".lion-native-build.json")
}

/// Schlüssel aus Build und Inhalt der eingeschalteten Mods: ändert sich einer, wird neu gebaut.
fn build_key(base: &str, mods: &[PathBuf]) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(base.as_bytes());
    for path in mods {
        hasher.update(path.file_name().unwrap_or_default().as_encoded_bytes());
        hasher.update(Sha256::digest(
            std::fs::read(path).map_err(|e| e.to_string())?,
        ));
    }
    Ok(hex::encode(hasher.finalize()))
}

fn client_of(version: &str) -> PathBuf {
    build::build_dir(version).join(if cfg!(windows) {
        "horizon-client.exe"
    } else {
        "horizon-client"
    })
}

fn cli_command(cli: &Path, game_dir: &Path) -> Command {
    let mut command = Command::new(cli);
    command
        .env("HORIZON_LAUNCHER_HOME", build::launcher_home())
        .env("HORIZON_HOME", game_dir)
        .env("HORIZON_LAUNCHER_NO_SELF_UPDATE", "1")
        .stdin(Stdio::null())
        .hide_console_window();
    command
}

/// Version, die der Bau für das Standard-Profil des Horizon-Launchers ausgewählt hat.
fn selected_version(stdout: &str) -> Option<String> {
    let from_instances = std::fs::read_to_string(build::launcher_home().join("instances.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            v["instances"]["horizon"]["version"]
                .as_str()
                .map(String::from)
        })
        .filter(|v| v.starts_with("native-"));
    from_instances.or_else(|| {
        stdout
            .split_whitespace()
            .find(|word| word.starts_with("native-"))
            .map(String::from)
    })
}

/// Fortschrittszeile des Horizon-Launchers → (Schritt, Prozent), z.B.
/// `[ 0:01] Downloading Rust 1.99.1 (once): 45%`,
/// `[ 4:10] Compiling the game (performance): 40 of 468 crates, now flate2` oder
/// `[ 1:44] Applying the mixins`.
fn parse_progress(line: &str) -> Option<(String, Option<u8>)> {
    let rest = line.trim().strip_prefix('[')?;
    let (_, label) = rest.split_once(']')?;
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    let Some((step, detail)) = label.split_once(": ") else {
        return Some((label.to_string(), None));
    };
    if let Some(percent) = detail.strip_suffix('%') {
        return Some((step.to_string(), percent.trim().parse().ok()));
    }
    // „N of M …“
    let mut words = detail.split_whitespace();
    let percent = match (words.next(), words.next(), words.next()) {
        (Some(done), Some("of"), Some(total)) => {
            match (done.parse::<u64>(), total.parse::<u64>()) {
                (Ok(done), Ok(total)) if total > 0 => Some((done * 100 / total).min(100) as u8),
                _ => None,
            }
        }
        _ => None,
    };
    Some((step.to_string(), percent))
}

/// Spielprogramm mit den eingeschalteten nativen Mods eines Profils (None = keine nativen Mods).
pub async fn ensure_build(game_dir: &Path, base: &str) -> Result<Option<PathBuf>, String> {
    let mods = registry::enabled_native_mods(game_dir);
    if mods.is_empty() {
        return Ok(None);
    }
    if !is_trusted() {
        return Err(format!(
            "{TRUST_REQUIRED}: Native Mods laufen ohne Sandbox mit vollen Rechten und müssen erst erlaubt werden"
        ));
    }
    let key = build_key(base, &mods)?;
    let reusable = |state: &BuildState| state.key == key && client_of(&state.version).is_file();
    let read_state = || {
        std::fs::read_to_string(state_file(game_dir))
            .ok()
            .and_then(|s| serde_json::from_str::<BuildState>(&s).ok())
    };
    if let Some(state) = read_state().filter(reusable) {
        return Ok(Some(client_of(&state.version)));
    }

    let _guard = BUILD_LOCK.lock().await;
    let progress = |status: String, percent: u8| send_launch_progress(status, percent);
    let cli = build::ensure_cli(&progress).await?;
    // Englische Ausgabe, damit Fortschritt und Ergebnis zuverlässig gelesen werden
    let mut language = cli_command(&cli, game_dir);
    language
        .args(["settings", "language", "en"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    tokio::task::spawn_blocking(move || language.status())
        .await
        .ok();

    send_launch_progress(
        "Baue native Mods (der erste Bau lädt Rust und dauert 20–30 Minuten)...",
        40,
    );
    let mut outcome = run_native_build(&cli, game_dir, base).await?;
    // Unvollständig entpackte Rust-Pakete (z.B. nach einem abgebrochenen Bau): Cargo entpackt
    // sie aus seinem Download-Cache neu, ohne erneuten Download – einmal neu versuchen
    if !outcome.status.success() && outcome.mentions_broken_crate_source() {
        tracing::warn!("Broken cargo sources, extracting them again");
        std::fs::remove_dir_all(
            build::launcher_home()
                .join("cargo")
                .join("registry")
                .join("src"),
        )
        .ok();
        outcome = run_native_build(&cli, game_dir, base).await?;
    }
    if !outcome.status.success() {
        let reason = outcome.last_lines.join("\n");
        return Err(format!(
            "Die nativen Mods konnten nicht gebaut werden: {}",
            if reason.is_empty() {
                format!("Exit-Code {:?}", outcome.status.code())
            } else {
                reason
            }
        ));
    }
    let version = selected_version(&outcome.stdout)
        .ok_or("Der Bau ist fertig, aber das Spielprogramm wurde nicht gefunden")?;
    let exe = client_of(&version);
    if !exe.is_file() {
        return Err(format!("Spielprogramm fehlt: {}", exe.display()));
    }
    warn_left_out(&version);

    let previous = read_state().map(|s| s.version);
    let state = BuildState {
        key,
        version: version.clone(),
    };
    std::fs::write(
        state_file(game_dir),
        serde_json::to_string(&state).map_err(|e| e.to_string())?,
    )
    .ok();
    if let Some(previous) = previous.filter(|p| *p != version) {
        remove_if_unused(&previous).await;
    }
    Ok(Some(exe))
}

struct BuildOutcome {
    status: std::process::ExitStatus,
    stdout: String,
    last_lines: Vec<String>,
}

impl BuildOutcome {
    fn mentions_broken_crate_source(&self) -> bool {
        self.last_lines.iter().any(|l| {
            l.contains("failed to parse manifest")
                || l.contains("no targets specified in the manifest")
        })
    }
}

/// `horizon-launcher native-build` mit Fortschritt in der Oberfläche.
async fn run_native_build(cli: &Path, game_dir: &Path, base: &str) -> Result<BuildOutcome, String> {
    let mut command = cli_command(cli, game_dir);
    command
        .args(["native-build", "--trust-native-code", "--version", base])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    tokio::task::spawn_blocking(move || {
        let mut child = command
            .spawn()
            .map_err(|e| format!("Horizon-Werkzeuge konnten nicht starten: {e}"))?;
        let stdout = child.stdout.take();
        let stdout_reader = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(out) = stdout {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    tracing::info!("[horizon native-build] {}", line);
                    text.push_str(&line);
                    text.push('\n');
                }
            }
            text
        });
        let mut last_lines: Vec<String> = Vec::new();
        let mut last_label = String::new();
        if let Some(err) = child.stderr.take() {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                match parse_progress(&line) {
                    Some((label, percent)) => {
                        if label != last_label {
                            tracing::info!("[horizon native-build] {}", label);
                            last_label = label.clone();
                        }
                        // Bau belegt 40–95 % des Gesamtfortschritts
                        let overall = 40 + percent.map_or(0, |p| p.min(100) * 55 / 100);
                        send_launch_progress(format!("Native Mods: {label}"), overall);
                    }
                    None if !line.trim().is_empty() => {
                        tracing::warn!("[horizon native-build] {}", line);
                        last_lines.push(line);
                        if last_lines.len() > 8 {
                            last_lines.remove(0);
                        }
                    }
                    None => {}
                }
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        let stdout = stdout_reader.join().unwrap_or_default();
        Ok(BuildOutcome {
            status,
            stdout,
            last_lines,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Mods, die nicht zu diesem Build passten und deshalb nicht mitgebaut wurden, als Hinweis melden.
fn warn_left_out(version: &str) {
    let Some(manifest) =
        std::fs::read_to_string(build::build_dir(version).join("native-mods.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    else {
        return;
    };
    let left_out: Vec<String> = manifest["left_out"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            entry
                .as_str()
                .or_else(|| entry["id"].as_str())
                .map(String::from)
        })
        .collect();
    if !left_out.is_empty() {
        crate::core::minecraft::add_launch_warning(format!(
            "Diese nativen Mods passen nicht zum aktuellen Horizon-Build und wurden weggelassen: {}",
            left_out.join(", ")
        ));
    }
}

/// Löscht einen alten nativen Bau, wenn kein Profil ihn mehr nutzt (jeder ist ~1 GB groß).
async fn remove_if_unused(version: &str) {
    let Ok(manager) = crate::core::profiles::ProfileManager::new() else {
        return;
    };
    let Ok(profiles) = manager.load_profiles().await else {
        return;
    };
    let used = profiles.profiles.iter().any(|p| {
        std::fs::read_to_string(state_file(&p.game_dir))
            .ok()
            .and_then(|s| serde_json::from_str::<BuildState>(&s).ok())
            .is_some_and(|s| s.version == version)
    });
    if !used && version.starts_with("native-") {
        std::fs::remove_dir_all(build::build_dir(version)).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_progress_lines() {
        assert_eq!(
            parse_progress("[ 0:01] Downloading Rust 1.99.1 (once): 45%"),
            Some(("Downloading Rust 1.99.1 (once)".into(), Some(45)))
        );
        assert_eq!(
            parse_progress("[ 1:44] Applying the mixins"),
            Some(("Applying the mixins".into(), None))
        );
        assert_eq!(
            parse_progress(
                "[ 4:10] Compiling the game (performance): 117 of 468 crates, now flate2"
            ),
            Some(("Compiling the game (performance)".into(), Some(25)))
        );
        assert_eq!(parse_progress("error: something"), None);
    }

    #[test]
    fn finds_built_version_in_output() {
        assert_eq!(
            selected_version_from_text("native-9f42064-abc built and selected for its instance."),
            Some("native-9f42064-abc".into())
        );
    }

    fn selected_version_from_text(text: &str) -> Option<String> {
        text.split_whitespace()
            .find(|word| word.starts_with("native-"))
            .map(String::from)
    }
}
