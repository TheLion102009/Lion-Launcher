use crate::api::mojang::MojangClient;
use crate::core::servers::ServerProfileManager;
use crate::types::server_profile::{ServerMode, ServerProfile};
use base64::{engine::general_purpose, Engine as _};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::time::{sleep, Duration};

const MAX_CONSOLE_LINES: usize = 4000;
const MAX_EDITABLE_FILE_SIZE: u64 = 2 * 1024 * 1024;

pub(crate) type ConsoleBuffer = Arc<tokio::sync::Mutex<VecDeque<String>>>;

#[derive(Clone)]
struct RunningServerState {
    pid: u32,
    started_at: std::time::Instant,
    console_lines: ConsoleBuffer,
    stdin: Arc<tokio::sync::Mutex<Option<ChildStdin>>>,
}

#[derive(Serialize)]
pub struct ServerPropertiesResponse {
    pub exists: bool,
    pub values: HashMap<String, String>,
}

#[derive(Serialize)]
pub struct ServerRuntimeInfo {
    pub running: bool,
    pub installing: bool,
    pub pid: Option<u32>,
    /// CPU-Last des Server-Prozesses in Prozent der GESAMTEN CPU (0-100)
    pub cpu_percent: f32,
    pub cpu_cores: usize,
    pub memory_mb: u64,
    pub uptime_seconds: u64,
    /// Systemweiter Netzwerkdurchsatz (Download + Upload)
    pub network_kbps: f32,
    pub network_rx_kbps: f32,
    pub network_tx_kbps: f32,
}

#[derive(Serialize)]
pub struct ServerFileEntry {
    pub name: String,
    pub relative_path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_unix: u64,
}

#[derive(Serialize)]
pub struct ServerFileListResponse {
    pub current_path: String,
    pub entries: Vec<ServerFileEntry>,
}

#[derive(Deserialize)]
struct PaperProjectResponse {
    versions: HashMap<String, Vec<String>>,
}

#[derive(Deserialize)]
struct PaperVersionMeta {
    version: PaperVersionInfo,
}

#[derive(Deserialize)]
struct PaperVersionInfo {
    java: Option<PaperJavaInfo>,
}

#[derive(Deserialize)]
struct PaperJavaInfo {
    version: PaperJavaVersionInfo,
}

#[derive(Deserialize)]
struct PaperJavaVersionInfo {
    minimum: u32,
}

#[derive(Deserialize)]
struct PaperBuildEntry {
    id: u32,
    channel: Option<String>,
    downloads: HashMap<String, PaperDownloadEntry>,
}

#[derive(Deserialize)]
struct PaperDownloadEntry {
    url: String,
}

/// Wie der Server-Prozess gestartet wird.
enum LaunchTarget {
    /// `java ... -jar <datei>`
    Jar(String),
    /// `java ... @libraries/.../win_args.txt` (moderne Forge/NeoForge-Server)
    ArgsFile(String),
}

struct NetworkSample {
    at: std::time::Instant,
    received: u64,
    transmitted: u64,
    rx_kbps: f32,
    tx_kbps: f32,
}

struct NetworkSampler {
    networks: sysinfo::Networks,
    last: Option<NetworkSample>,
}

static RUNNING_SERVERS: OnceLock<Mutex<HashMap<String, RunningServerState>>> = OnceLock::new();
static NETWORK_SAMPLER: OnceLock<Mutex<NetworkSampler>> = OnceLock::new();
static PROCESS_SAMPLER: OnceLock<Mutex<sysinfo::System>> = OnceLock::new();
static CONSOLE_BUFFERS: OnceLock<Mutex<HashMap<String, ConsoleBuffer>>> = OnceLock::new();
static INSTALLING_SERVERS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// Eigener Client fuer grosse Downloads (Server-JARs, Installer, Plugins):
/// der geteilte API-Client bricht nach 30s ab, das reicht fuer 50 MB+ nicht.
pub(crate) static DOWNLOAD_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(concat!("LionLauncher/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(900))
        .build()
        .unwrap_or_default()
});

fn running_servers() -> &'static Mutex<HashMap<String, RunningServerState>> {
    RUNNING_SERVERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn console_buffers() -> &'static Mutex<HashMap<String, ConsoleBuffer>> {
    CONSOLE_BUFFERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn installing_servers() -> &'static Mutex<HashSet<String>> {
    INSTALLING_SERVERS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Liefert den Konsolen-Puffer eines Servers (wird bei Bedarf angelegt).
pub(crate) fn console_buffer(server_id: &str) -> ConsoleBuffer {
    let mut map = console_buffers().lock().unwrap_or_else(|e| e.into_inner());
    map.entry(server_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(VecDeque::new())))
        .clone()
}

fn reset_console_buffer(server_id: &str) -> ConsoleBuffer {
    let buffer: ConsoleBuffer = Arc::new(tokio::sync::Mutex::new(VecDeque::new()));
    let mut map = console_buffers().lock().unwrap_or_else(|e| e.into_inner());
    map.insert(server_id.to_string(), buffer.clone());
    buffer
}

pub(crate) fn is_server_installing(server_id: &str) -> bool {
    installing_servers()
        .lock()
        .map(|set| set.contains(server_id))
        .unwrap_or(false)
}

/// Markiert einen Server als "wird installiert/gestartet" solange der Guard lebt.
struct InstallGuard(String);

impl InstallGuard {
    fn acquire(server_id: &str) -> Option<Self> {
        let mut set = installing_servers().lock().unwrap_or_else(|e| e.into_inner());
        if !set.insert(server_id.to_string()) {
            return None;
        }
        Some(Self(server_id.to_string()))
    }
}

impl Drop for InstallGuard {
    fn drop(&mut self) {
        if let Ok(mut set) = installing_servers().lock() {
            set.remove(&self.0);
        }
    }
}

fn get_running_state(server_id: &str) -> Option<RunningServerState> {
    running_servers()
        .lock()
        .ok()
        .and_then(|map| map.get(server_id).cloned())
}

fn set_running_state(server_id: &str, state: RunningServerState) {
    if let Ok(mut map) = running_servers().lock() {
        map.insert(server_id.to_string(), state);
    }
}

fn remove_running_state(server_id: &str) {
    if let Ok(mut map) = running_servers().lock() {
        map.remove(server_id);
    }
}

fn running_server_ids() -> Vec<String> {
    running_servers()
        .lock()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Prueft ob der Server-Prozess aktuell laeuft.
pub(crate) fn is_server_running(server_id: &str) -> bool {
    match get_running_state(server_id) {
        Some(state) => is_process_alive(state.pid),
        None => false,
    }
}

/// Schreibt einen Befehl in die Server-Konsole (stdin). Gibt false zurueck wenn der Server nicht laeuft.
pub(crate) async fn write_server_stdin(server_id: &str, command: &str) -> bool {
    let Some(state) = get_running_state(server_id) else {
        return false;
    };
    let mut guard = state.stdin.lock().await;
    let Some(stdin) = guard.as_mut() else {
        return false;
    };
    if stdin.write_all(format!("{}\n", command).as_bytes()).await.is_err() {
        return false;
    }
    stdin.flush().await.is_ok()
}

fn supported_software(mode: &ServerMode) -> &'static [&'static str] {
    match mode {
        ServerMode::Plugins => &["paper", "purpur", "folia", "spigot", "bukkit", "vanilla"],
        ServerMode::Modded => &["fabric", "quilt", "forge", "neoforge", "vanilla"],
    }
}

fn normalize_software(mode: &ServerMode, software: &str) -> Result<String, String> {
    let normalized = software.trim().to_lowercase();
    if supported_software(mode)
        .iter()
        .any(|candidate| candidate == &normalized)
    {
        return Ok(normalized);
    }

    Err(format!(
        "Software '{}' wird fuer {:?} nicht unterstuetzt. Unterstuetzt: {}",
        software,
        mode,
        supported_software(mode).join(", ")
    ))
}

fn parse_properties(content: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

fn format_properties(values: &HashMap<String, String>) -> String {
    let mut keys: Vec<&String> = values.keys().collect();
    keys.sort();

    let mut lines = vec![
        "# Lion Launcher server.properties".to_string(),
        "# Datei wird vom Server-Manager verwaltet".to_string(),
    ];

    for key in keys {
        if let Some(value) = values.get(key) {
            lines.push(format!("{}={}", key, value));
        }
    }

    format!("{}\n", lines.join("\n"))
}

fn default_server_properties(profile: &ServerProfile) -> HashMap<String, String> {
    let mut props = HashMap::new();
    props.insert("motd".to_string(), format!("A Lion Server - {}", profile.name));
    props.insert("server-port".to_string(), profile.port.to_string());
    props.insert("max-players".to_string(), "20".to_string());
    props.insert("online-mode".to_string(), "true".to_string());
    props.insert("allow-flight".to_string(), "false".to_string());
    props.insert("view-distance".to_string(), "10".to_string());
    props.insert("simulation-distance".to_string(), "10".to_string());
    props.insert("pvp".to_string(), "true".to_string());
    props.insert("difficulty".to_string(), "easy".to_string());
    props.insert("gamemode".to_string(), "survival".to_string());
    props.insert("enable-command-block".to_string(), "false".to_string());
    props
}

async fn ensure_server_layout(profile: &ServerProfile) -> Result<(), String> {
    tokio::fs::create_dir_all(&profile.server_dir)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(profile.server_dir.join("logs"))
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(profile.server_dir.join("world"))
        .await
        .map_err(|e| e.to_string())?;

    match profile.mode {
        ServerMode::Plugins => {
            tokio::fs::create_dir_all(profile.server_dir.join("plugins"))
                .await
                .map_err(|e| e.to_string())?;
        }
        ServerMode::Modded => {
            tokio::fs::create_dir_all(profile.server_dir.join("mods"))
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    let properties_path = profile.server_dir.join("server.properties");
    if !properties_path.exists() {
        let defaults = format_properties(&default_server_properties(profile));
        tokio::fs::write(properties_path, defaults)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

async fn sync_server_profile_properties(profile: &ServerProfile) -> Result<(), String> {
    let properties_path = profile.server_dir.join("server.properties");
    let mut values = if properties_path.exists() {
        let content = tokio::fs::read_to_string(&properties_path)
            .await
            .map_err(|e| e.to_string())?;
        parse_properties(&content)
    } else {
        default_server_properties(profile)
    };

    values.insert("server-port".to_string(), profile.port.to_string());
    tokio::fs::write(properties_path, format_properties(&values))
        .await
        .map_err(|e| e.to_string())
}

async fn ensure_eula(profile: &ServerProfile) -> Result<(), String> {
    let path = profile.server_dir.join("eula.txt");
    let content = "# Automatically managed by Lion Launcher\neula=true\n";
    tokio::fs::write(path, content)
        .await
        .map_err(|e| format!("Konnte eula.txt nicht schreiben: {}", e))
}

pub(crate) fn sanitize_relative_path(relative_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative_path);
    if path.is_absolute() {
        return Err("Absolute Pfade sind nicht erlaubt".to_string());
    }

    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(seg) => clean.push(seg),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("Pfad darf nicht aus dem Server-Ordner ausbrechen".to_string());
            }
        }
    }

    Ok(clean)
}

fn resolve_server_path(profile: &ServerProfile, relative_path: Option<&str>) -> Result<PathBuf, String> {
    match relative_path {
        Some(value) if !value.trim().is_empty() => {
            let clean = sanitize_relative_path(value)?;
            Ok(profile.server_dir.join(clean))
        }
        _ => Ok(profile.server_dir.clone()),
    }
}

pub(crate) fn to_relative_display(root: &Path, full_path: &Path) -> String {
    full_path
        .strip_prefix(root)
        .ok()
        .unwrap_or(full_path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) async fn append_console_line(buffer: &Arc<tokio::sync::Mutex<VecDeque<String>>>, line: String) {
    let mut guard = buffer.lock().await;
    guard.push_back(line);
    while guard.len() > MAX_CONSOLE_LINES {
        guard.pop_front();
    }
}

async fn read_tail_lines(path: &Path, max_lines: usize) -> String {
    let content = tokio::fs::read_to_string(path).await.unwrap_or_default();
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

fn is_process_alive(pid: u32) -> bool {
    use sysinfo::{Pid, System};

    // Eigene, kurzlebige Instanz: darf den CPU-Sampler (PROCESS_SAMPLER) nicht
    // zwischendurch refreshen, sonst wird das Messintervall der CPU-Last verfaelscht.
    let mut system = System::new();
    system.refresh_process(Pid::from_u32(pid))
}

fn cpu_core_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .max(1)
}

/// Liefert (CPU-Last in % der Gesamt-CPU, RAM in Bytes) fuer einen Prozess.
///
/// sysinfo berechnet die CPU-Last als Differenz zwischen zwei Refreshes. Deshalb
/// wird eine dauerhafte `System`-Instanz verwendet: der erste Aufruf liefert 0,
/// alle weiteren den echten Wert ueber das Intervall seit dem letzten Poll.
fn sample_process(pid: u32) -> Option<(f32, u64)> {
    use sysinfo::{Pid, System};

    let sampler = PROCESS_SAMPLER.get_or_init(|| Mutex::new(System::new()));
    let mut system = sampler.lock().unwrap_or_else(|e| e.into_inner());
    let pid = Pid::from_u32(pid);
    if !system.refresh_process(pid) {
        return None;
    }

    let process = system.process(pid)?;
    // sysinfo liefert 100% pro Kern -> auf Gesamt-CPU normalisieren
    let cpu = (process.cpu_usage() / cpu_core_count() as f32).clamp(0.0, 100.0);
    Some((cpu, process.memory()))
}

fn kill_process(pid: u32) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }

    #[cfg(windows)]
    {
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .spawn()
            .ok();
    }
}

/// Systemweiter Netzwerkdurchsatz in KB/s als (Download, Upload).
///
/// Mehrere Server-Karten pollen quasi gleichzeitig; ohne Mindestintervall wuerde
/// der zweite Aufruf ein Delta von ~0 Bytes sehen und die Kurve springt auf 0.
fn sample_network_kbps() -> (f32, f32) {
    use sysinfo::Networks;

    let sampler = NETWORK_SAMPLER.get_or_init(|| {
        Mutex::new(NetworkSampler {
            networks: Networks::new_with_refreshed_list(),
            last: None,
        })
    });
    let Ok(mut guard) = sampler.lock() else {
        return (0.0, 0.0);
    };

    let now = std::time::Instant::now();
    if let Some(last) = &guard.last {
        if now.duration_since(last.at) < Duration::from_millis(900) {
            return (last.rx_kbps, last.tx_kbps);
        }
    }

    guard.networks.refresh_list();
    let (received, transmitted) = guard.networks.values().fold((0u64, 0u64), |acc, data| {
        (acc.0 + data.total_received(), acc.1 + data.total_transmitted())
    });

    let (rx_kbps, tx_kbps) = match &guard.last {
        Some(last) => {
            let elapsed = now.duration_since(last.at).as_secs_f32().max(0.001);
            (
                received.saturating_sub(last.received) as f32 / 1024.0 / elapsed,
                transmitted.saturating_sub(last.transmitted) as f32 / 1024.0 / elapsed,
            )
        }
        None => (0.0, 0.0),
    };

    guard.last = Some(NetworkSample {
        at: now,
        received,
        transmitted,
        rx_kbps,
        tx_kbps,
    });
    (rx_kbps, tx_kbps)
}

async fn resolve_java_binary() -> String {
    match crate::gui::settings::get_config().await {
        Ok(cfg) => cfg
            .game_settings
            .java_path
            .map(|p| p.to_string_lossy().to_string())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "java".to_string()),
        Err(_) => "java".to_string(),
    }
}

fn parse_java_major(version: &str) -> Option<u32> {
    let v = version.trim();
    if let Some(stripped) = v.strip_prefix("1.") {
        let digits: String = stripped.chars().take_while(|c| c.is_ascii_digit()).collect();
        return digits.parse::<u32>().ok();
    }

    let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<u32>().ok()
}

fn detect_java_major(java_bin: &str) -> Result<u32, String> {
    let output = std::process::Command::new(java_bin)
        .arg("-version")
        .output()
        .map_err(|e| format!("Java konnte nicht gestartet werden ({}): {}", java_bin, e))?;

    let merged = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    for line in merged.lines() {
        if !line.to_lowercase().contains("version") {
            continue;
        }
        if let Some(start) = line.find('"') {
            let rest = &line[start + 1..];
            if let Some(end) = rest.find('"') {
                let raw = &rest[..end];
                if let Some(major) = parse_java_major(raw) {
                    return Ok(major);
                }
            }
        }
    }

    for token in merged.split_whitespace() {
        if let Some(major) = parse_java_major(token.trim_matches('"')) {
            if major >= 6 {
                return Ok(major);
            }
        }
    }

    Err(format!(
        "Java-Version konnte nicht erkannt werden. Ausgabe: {}",
        merged.lines().next().unwrap_or("<leer>")
    ))
}

/// Anzeigename der Server-Software fuer Fehlermeldungen
fn software_display_name(software: &str) -> &str {
    match software {
        "paper" => "Paper",
        "purpur" => "Purpur",
        "folia" => "Folia",
        "spigot" => "Spigot",
        "bukkit" => "Bukkit",
        "vanilla" => "Vanilla",
        "fabric" => "Fabric",
        "quilt" => "Quilt",
        "forge" => "Forge",
        "neoforge" => "NeoForge",
        other => other,
    }
}

/// Nur echte Releases (1.21.1, 26.1) – keine Snapshots, Pre-Releases oder RCs.
fn is_release_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    (2..=3).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn compare_minecraft_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect() };
    parse(a).cmp(&parse(b))
}

/// Ordnet eine NeoForge-Version ihrer Minecraft-Version zu.
/// Alt: 21.1.77 -> 1.21.1, 21.0.5 -> 1.21 | Neu (ab 2026): 26.1.2.5 -> 26.1.2, 26.2.0.81 -> 26.2
fn neoforge_minecraft_version(neoforge_version: &str) -> Option<String> {
    let parts: Vec<&str> = neoforge_version.split('.').collect();
    let major = parts.first()?.parse::<u32>().ok()?;
    if major >= 26 {
        let minor = parts.get(1)?;
        let patch = parts.get(2)?;
        return Some(if *patch == "0" {
            format!("{}.{}", major, minor)
        } else {
            format!("{}.{}.{}", major, minor, patch)
        });
    }
    if major >= 20 {
        let minor = parts.get(1)?;
        // 20.1 gibt es nicht als NeoForge (MC 1.20.1 lief noch ueber net.neoforged:forge)
        if major == 20 && *minor == "1" {
            return None;
        }
        return Some(if *minor == "0" {
            format!("1.{}", major)
        } else {
            format!("1.{}.{}", major, minor)
        });
    }
    None
}

async fn fetch_neoforge_versions() -> Result<Vec<String>, String> {
    #[derive(Deserialize)]
    struct NeoForgeVersions {
        versions: Vec<String>,
    }

    let response: NeoForgeVersions = crate::core::http::HTTP_CLIENT
        .get("https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge")
        .send()
        .await
        .map_err(|e| format!("NeoForge-Versionen konnten nicht geladen werden: {}", e))?
        .error_for_status()
        .map_err(|e| format!("NeoForge-Versionen konnten nicht geladen werden: {}", e))?
        .json()
        .await
        .map_err(|e| format!("NeoForge-Versionen sind ungueltig: {}", e))?;
    Ok(response.versions)
}

/// Neueste NeoForge-Version fuer eine Minecraft-Version (stabile Versionen bevorzugt).
async fn resolve_latest_neoforge_version(minecraft_version: &str) -> Result<String, String> {
    let versions = fetch_neoforge_versions().await?;
    let mut matching: Vec<&String> = versions
        .iter()
        .filter(|v| neoforge_minecraft_version(v).as_deref() == Some(minecraft_version))
        .collect();

    let build_key = |v: &str| -> Vec<u64> {
        v.split(|c: char| c == '.' || c == '-')
            .map(|p| p.parse::<u64>().unwrap_or(0))
            .collect()
    };
    matching.sort_by(|a, b| build_key(b).cmp(&build_key(a)));

    matching
        .iter()
        .find(|v| !v.contains("beta") && !v.contains("alpha"))
        .or_else(|| matching.first())
        .map(|v| v.to_string())
        .ok_or_else(|| format!("Keine NeoForge-Version fuer Minecraft {} gefunden", minecraft_version))
}

async fn fetch_json<T: serde::de::DeserializeOwned>(url: &str, label: &str) -> Result<T, String> {
    crate::core::http::HTTP_CLIENT
        .get(url)
        .send()
        .await
        .map_err(|e| format!("{} konnten nicht geladen werden: {}", label, e))?
        .error_for_status()
        .map_err(|e| format!("{} konnten nicht geladen werden: {}", label, e))?
        .json()
        .await
        .map_err(|e| format!("{} sind ungueltig: {}", label, e))
}

/// Liefert alle Minecraft-Versionen, fuer die es die Server-Software wirklich gibt (neueste zuerst).
#[tauri::command]
pub async fn get_server_software_versions(software: String) -> Result<Vec<String>, String> {
    #[derive(Deserialize)]
    struct GameVersionEntry {
        version: String,
        #[serde(default)]
        stable: bool,
    }

    let software = software.trim().to_lowercase();
    let label = format!("{}-Versionen", software_display_name(&software));

    let mut versions: Vec<String> = match software.as_str() {
        "paper" | "folia" => {
            let project: PaperProjectResponse = fetch_json(
                &format!("https://fill.papermc.io/v3/projects/{}", software),
                &label,
            )
            .await?;
            project.versions.into_values().flatten().collect()
        }
        "purpur" => {
            let value: serde_json::Value = fetch_json("https://api.purpurmc.org/v2/purpur", &label).await?;
            value
                .get("versions")
                .and_then(|v| v.as_array())
                .map(|list| list.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default()
        }
        "spigot" | "bukkit" => {
            // BuildTools kennt genau die Versionen, fuer die es eine <version>.json gibt
            let listing = crate::core::http::HTTP_CLIENT
                .get("https://hub.spigotmc.org/versions/")
                .send()
                .await
                .map_err(|e| format!("{} konnten nicht geladen werden: {}", label, e))?
                .error_for_status()
                .map_err(|e| format!("{} konnten nicht geladen werden: {}", label, e))?
                .text()
                .await
                .map_err(|e| e.to_string())?;
            listing
                .split("href=\"")
                .skip(1)
                .filter_map(|chunk| chunk.split('"').next())
                .filter_map(|name| name.strip_suffix(".json"))
                .map(String::from)
                .collect()
        }
        "vanilla" => {
            let client = MojangClient::new().map_err(|e| e.to_string())?;
            client
                .get_version_manifest()
                .await
                .map_err(|e| format!("{} konnten nicht geladen werden: {}", label, e))?
                .into_iter()
                .filter(|v| v.version_type == crate::types::version::VersionType::Release)
                .map(|v| v.id)
                .collect()
        }
        "fabric" | "quilt" => {
            let url = if software == "fabric" {
                "https://meta.fabricmc.net/v2/versions/game"
            } else {
                "https://meta.quiltmc.org/v3/versions/game"
            };
            let entries: Vec<GameVersionEntry> = fetch_json(url, &label).await?;
            entries
                .into_iter()
                .filter(|e| e.stable)
                .map(|e| e.version)
                .collect()
        }
        "forge" => crate::api::forge::ForgeClient::new()
            .map_err(|e| e.to_string())?
            .get_supported_game_versions()
            .await
            .map_err(|e| format!("{} konnten nicht geladen werden: {}", label, e))?,
        "neoforge" => fetch_neoforge_versions()
            .await?
            .iter()
            .filter_map(|v| neoforge_minecraft_version(v))
            .collect(),
        other => return Err(format!("Unbekannte Server-Software '{}'", other)),
    };

    versions.retain(|v| is_release_version(v));
    versions.sort_by(|a, b| compare_minecraft_versions(b, a));
    versions.dedup();
    Ok(versions)
}

/// Prueft vor dem Download, ob es die Software fuer die Minecraft-Version gibt.
/// Ist die Versionsliste nicht erreichbar (offline), wird der Download trotzdem versucht.
async fn ensure_software_supports_version(profile: &ServerProfile) -> Result<(), String> {
    match get_server_software_versions(profile.software.clone()).await {
        Ok(versions) if !versions.is_empty() && !versions.contains(&profile.minecraft_version) => {
            Err(format!(
                "{} gibt es nicht fuer Minecraft {}. Verfuegbar sind z.B.: {}",
                software_display_name(&profile.software),
                profile.minecraft_version,
                versions.iter().take(8).cloned().collect::<Vec<_>>().join(", ")
            ))
        }
        _ => Ok(()),
    }
}

/// `project_name` ist das PaperMC-Projekt ("paper" oder "folia").
/// Die Fill-API gruppiert Versionen (z.B. "26.1" -> ["26.1.2", "26.1.1"]); der Gruppenname
/// selbst ist KEINE gueltige Version, deshalb wird nur gegen die echten Versions-IDs geprueft.
async fn resolve_paper_version_for_profile(
    client: &reqwest::Client,
    project_name: &str,
    profile: &ServerProfile,
) -> Result<String, String> {
    let project: PaperProjectResponse = client
        .get(format!("https://fill.papermc.io/v3/projects/{}", project_name))
        .send()
        .await
        .map_err(|e| format!("{} Projektdaten konnten nicht geladen werden: {}", project_name, e))?
        .error_for_status()
        .map_err(|e| format!("{} Projektdaten konnten nicht geladen werden: {}", project_name, e))?
        .json()
        .await
        .map_err(|e| format!("{} Projektdaten sind ungueltig: {}", project_name, e))?;

    let requested = &profile.minecraft_version;
    if project.versions.values().flatten().any(|v| v == requested) {
        return Ok(requested.clone());
    }

    let mut available: Vec<String> = project
        .versions
        .into_values()
        .flatten()
        .filter(|v| is_release_version(v))
        .collect();
    available.sort_by(|a, b| compare_minecraft_versions(b, a));
    Err(format!(
        "{} gibt es nicht fuer Minecraft {}. Verfuegbar sind z.B.: {}",
        software_display_name(project_name),
        requested,
        available.into_iter().take(8).collect::<Vec<_>>().join(", ")
    ))
}

async fn resolve_mojang_java_requirement(minecraft_version: &str) -> Result<u32, String> {
    let client = MojangClient::new().map_err(|e| e.to_string())?;
    let manifest = client
        .get_version_manifest()
        .await
        .map_err(|e| e.to_string())?;

    let entry = manifest
        .into_iter()
        .find(|v| v.id == minecraft_version)
        .ok_or_else(|| format!("Minecraft-Version {} nicht gefunden", minecraft_version))?;

    let url = entry
        .url
        .ok_or_else(|| "Versionseintrag ohne Detail-URL".to_string())?;

    let value: serde_json::Value = crate::core::http::HTTP_CLIENT.clone()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Mojang Version-JSON konnte nicht geladen werden: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Mojang Version-JSON konnte nicht geladen werden: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Mojang Version-JSON ist ungueltig: {}", e))?;

    let java_major = value
        .get("javaVersion")
        .and_then(|v| v.get("majorVersion"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(8);

    Ok(java_major)
}

async fn resolve_required_java_for_profile(profile: &ServerProfile) -> Result<u32, String> {
    let heuristic_java = match profile.minecraft_version.as_str() {
        v if !v.starts_with("1.") && v.split('.').next().and_then(|m| m.parse::<u32>().ok()).unwrap_or(0) >= 26 => 25,
        v if v.starts_with("1.20.5") || v.starts_with("1.20.6") || v.starts_with("1.21") => 21,
        v if v.starts_with("1.18") || v.starts_with("1.19") || v.starts_with("1.20") => 17,
        v if v.starts_with("1.17") => 16,
        _ => 8,
    };

    let mut required_java = resolve_mojang_java_requirement(&profile.minecraft_version)
        .await
        .unwrap_or(heuristic_java);

    // Paper/Folia liefern die exakte Mindest-Java-Version in den Fill v3 Metadaten.
    let software = profile.software.to_lowercase();
    if software == "paper" || software == "folia" {
        let client = crate::core::http::HTTP_CLIENT.clone();
        let paper_version = resolve_paper_version_for_profile(&client, &software, profile).await?;
        let meta_url = format!(
            "https://fill.papermc.io/v3/projects/{}/versions/{}",
            software, paper_version
        );
        let meta: PaperVersionMeta = client
            .get(&meta_url)
            .send()
            .await
            .map_err(|e| format!("Paper Java-Metadaten konnten nicht geladen werden: {}", e))?
            .error_for_status()
            .map_err(|e| format!("Paper Java-Metadaten konnten nicht geladen werden: {}", e))?
            .json()
            .await
            .map_err(|e| format!("Paper Java-Metadaten sind ungueltig: {}", e))?;

        if let Some(java) = meta.version.java {
            required_java = required_java.max(java.version.minimum);
        }
    }

    Ok(required_java.max(8))
}

pub(crate) async fn download_to_path(url: &str, output_path: &Path) -> Result<(), String> {
    use futures_util::StreamExt;

    let response = DOWNLOAD_CLIENT
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Download fehlgeschlagen: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Download fehlgeschlagen: {}", e))?;

    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Erst in eine Temp-Datei schreiben: ein abgebrochener Download darf keine
    // halbe server.jar hinterlassen, die beim naechsten Start als "installiert" gilt.
    let tmp_path = output_path.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Konnte Datei nicht schreiben: {}", e))?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download abgebrochen: {}", e))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Konnte Datei nicht schreiben: {}", e))?;
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);

    tokio::fs::rename(&tmp_path, output_path)
        .await
        .map_err(|e| format!("Konnte Datei nicht verschieben: {}", e))
}

async fn download_vanilla_server_jar(profile: &ServerProfile, output_path: &Path) -> Result<(), String> {
    let client = MojangClient::new().map_err(|e| e.to_string())?;
    let manifest = client
        .get_version_manifest()
        .await
        .map_err(|e| e.to_string())?;

    let version = manifest
        .into_iter()
        .find(|v| v.id == profile.minecraft_version)
        .ok_or_else(|| {
            format!(
                "Minecraft-Version {} wurde im Mojang Manifest nicht gefunden",
                profile.minecraft_version
            )
        })?;

    let version_url = version
        .url
        .ok_or_else(|| "Versionseintrag hat keine Detail-URL".to_string())?;

    let info = client
        .get_version_info(&version_url)
        .await
        .map_err(|e| e.to_string())?;

    let server_download = info
        .downloads
        .server
        .ok_or_else(|| "Diese Minecraft-Version hat kein Server-Downloadobjekt".to_string())?;

    download_to_path(&server_download.url, output_path).await
}

async fn download_papermc_server_jar(
    project_name: &str,
    profile: &ServerProfile,
    output_path: &Path,
) -> Result<(), String> {
    let client = crate::core::http::HTTP_CLIENT.clone();
    let paper_version = resolve_paper_version_for_profile(&client, project_name, profile).await?;

    let builds_url = format!(
        "https://fill.papermc.io/v3/projects/{}/versions/{}/builds",
        project_name, paper_version
    );
    let builds: Vec<PaperBuildEntry> = client
        .get(&builds_url)
        .send()
        .await
        .map_err(|e| format!("{} Builds konnten nicht geladen werden: {}", project_name, e))?
        .error_for_status()
        .map_err(|e| format!("{} Builds konnten nicht geladen werden: {}", project_name, e))?
        .json()
        .await
        .map_err(|e| format!("{} Builds sind ungueltig: {}", project_name, e))?;

    if builds.is_empty() {
        return Err(format!("Keine {} Builds gefunden", project_name));
    }

    let target_build = if profile.software_version.eq_ignore_ascii_case("latest") {
        builds
            .iter()
            .find(|build| {
                build
                    .channel
                    .as_deref()
                    .map(|c| c.eq_ignore_ascii_case("stable"))
                    .unwrap_or(false)
            })
            .or_else(|| builds.first())
            .ok_or_else(|| format!("Kein passender {} Build gefunden", project_name))?
    } else {
        let requested_id = profile
            .software_version
            .parse::<u32>()
            .map_err(|_| format!("{} Build muss eine Zahl oder 'latest' sein", project_name))?;
        builds
            .iter()
            .find(|build| build.id == requested_id)
            .ok_or_else(|| format!("{} Build {} wurde nicht gefunden", project_name, requested_id))?
    };

    let download = target_build
        .downloads
        .get("server:default")
        .or_else(|| target_build.downloads.values().next())
        .ok_or_else(|| format!("{} Build enthaelt keinen Server Download", project_name))?;

    tracing::info!(
        "{} Download: version={} build={} url={}",
        project_name,
        paper_version,
        target_build.id,
        download.url
    );

    download_to_path(&download.url, output_path).await
}

async fn download_purpur_server_jar(profile: &ServerProfile, output_path: &Path) -> Result<(), String> {
    let client = crate::core::http::HTTP_CLIENT.clone();
    let build = if profile.software_version.eq_ignore_ascii_case("latest") {
        let meta_url = format!(
            "https://api.purpurmc.org/v2/purpur/{}",
            profile.minecraft_version
        );
        let value: serde_json::Value = client
            .get(&meta_url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| format!("Purpur ist fuer Minecraft {} nicht verfuegbar ({})", profile.minecraft_version, e))?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        let latest = value.get("builds").and_then(|v| v.get("latest"));
        latest
            .and_then(|v| v.as_str().map(|s| s.to_string()).or_else(|| v.as_i64().map(|n| n.to_string())))
            .ok_or_else(|| "Konnte den neuesten Purpur Build nicht ermitteln".to_string())?
    } else {
        profile.software_version.clone()
    };

    let url = format!(
        "https://api.purpurmc.org/v2/purpur/{}/{}/download",
        profile.minecraft_version, build
    );

    download_to_path(&url, output_path).await
}

async fn download_fabric_server_jar(profile: &ServerProfile, output_path: &Path) -> Result<(), String> {
    let client = crate::core::http::HTTP_CLIENT.clone();
    let loader_version = if profile.software_version.eq_ignore_ascii_case("latest") {
        let url = format!(
            "https://meta.fabricmc.net/v2/versions/loader/{}",
            profile.minecraft_version
        );
        let versions: Vec<serde_json::Value> = client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        versions
            .first()
            .and_then(|v| v.get("loader"))
            .and_then(|v| v.get("version"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("Keine Fabric Loader-Version fuer Minecraft {} gefunden", profile.minecraft_version))?
            .to_string()
    } else {
        profile.software_version.clone()
    };

    let installers: Vec<serde_json::Value> = client
        .get("https://meta.fabricmc.net/v2/versions/installer")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let installer_version = installers
        .iter()
        .find(|entry| entry.get("stable").and_then(|v| v.as_bool()).unwrap_or(false))
        .or_else(|| installers.first())
        .and_then(|entry| entry.get("version"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Konnte keine Fabric Installer-Version finden".to_string())?;

    let download_url = format!(
        "https://meta.fabricmc.net/v2/versions/loader/{}/{}/{}/server/jar",
        profile.minecraft_version, loader_version, installer_version
    );

    download_to_path(&download_url, output_path).await
}

/// Startet einen Hilfsprozess (Installer, BuildTools) und leitet dessen Ausgabe in die Server-Konsole.
async fn run_logged_process(
    program: &str,
    args: &[String],
    cwd: &Path,
    console: &ConsoleBuffer,
    label: &str,
) -> Result<(), String> {
    append_console_line(console, format!("[Lion] {} gestartet...", label)).await;

    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("{} konnte nicht gestartet werden: {}", label, e))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let out_console = console.clone();
    let out_task = tokio::spawn(async move {
        if let Some(stdout) = stdout {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                append_console_line(&out_console, line).await;
            }
        }
    });
    let err_console = console.clone();
    let err_task = tokio::spawn(async move {
        if let Some(stderr) = stderr {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                append_console_line(&err_console, format!("[ERR] {}", line)).await;
            }
        }
    });

    let status = child
        .wait()
        .await
        .map_err(|e| format!("{} ist fehlgeschlagen: {}", label, e))?;
    let _ = out_task.await;
    let _ = err_task.await;

    if !status.success() {
        return Err(format!(
            "{} ist fehlgeschlagen ({}). Details stehen in der Konsole.",
            label, status
        ));
    }

    append_console_line(console, format!("[Lion] {} abgeschlossen", label)).await;
    Ok(())
}

/// Spigot / CraftBukkit duerfen nicht direkt verteilt werden und muessen mit
/// den offiziellen BuildTools lokal gebaut werden (dauert beim ersten Start einige Minuten).
async fn build_spigot_server_jar(
    profile: &ServerProfile,
    java_bin: &str,
    console: &ConsoleBuffer,
    output_path: &Path,
) -> Result<(), String> {
    let compile_target = if profile.software == "bukkit" { "craftbukkit" } else { "spigot" };
    let work_dir = profile.server_dir.join(".buildtools");
    let out_dir = work_dir.join("out");
    tokio::fs::create_dir_all(&out_dir)
        .await
        .map_err(|e| e.to_string())?;

    append_console_line(console, "[Lion] Lade Spigot BuildTools herunter...".to_string()).await;
    let build_tools = work_dir.join("BuildTools.jar");
    download_to_path(
        "https://hub.spigotmc.org/jenkins/job/BuildTools/lastSuccessfulBuild/artifact/target/BuildTools.jar",
        &build_tools,
    )
    .await?;

    append_console_line(
        console,
        format!(
            "[Lion] Baue {} {} - das kann beim ersten Mal 5-10 Minuten dauern.",
            compile_target, profile.minecraft_version
        ),
    )
    .await;

    let args = vec![
        "-jar".to_string(),
        "BuildTools.jar".to_string(),
        "--rev".to_string(),
        profile.minecraft_version.clone(),
        "--compile".to_string(),
        compile_target.to_string(),
        "--output-dir".to_string(),
        out_dir.to_string_lossy().to_string(),
    ];
    run_logged_process(java_bin, &args, &work_dir, console, "BuildTools").await?;

    let mut built_jar = None;
    let mut entries = tokio::fs::read_dir(&out_dir)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with(compile_target) && name.ends_with(".jar") {
            built_jar = Some(entry.path());
            break;
        }
    }

    let built_jar = built_jar
        .ok_or_else(|| format!("BuildTools hat keine {}-JAR erzeugt", compile_target))?;
    tokio::fs::copy(&built_jar, output_path)
        .await
        .map_err(|e| format!("Server-JAR konnte nicht kopiert werden: {}", e))?;

    // Build-Ordner ist mehrere hundert MB gross und wird nicht mehr gebraucht
    let _ = tokio::fs::remove_dir_all(&work_dir).await;
    Ok(())
}

async fn install_quilt_server(
    profile: &ServerProfile,
    java_bin: &str,
    console: &ConsoleBuffer,
) -> Result<(), String> {
    let client = crate::core::http::HTTP_CLIENT.clone();
    let metadata = client
        .get("https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-installer/maven-metadata.xml")
        .send()
        .await
        .map_err(|e| format!("Quilt Installer-Metadaten konnten nicht geladen werden: {}", e))?
        .text()
        .await
        .map_err(|e| e.to_string())?;

    let installer_version = metadata
        .split("<release>")
        .nth(1)
        .and_then(|rest| rest.split("</release>").next())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| "Quilt Installer-Version konnte nicht ermittelt werden".to_string())?;

    let installer_path = profile.server_dir.join("quilt-installer.jar");
    append_console_line(
        console,
        format!("[Lion] Lade Quilt Installer {} herunter...", installer_version),
    )
    .await;
    download_to_path(
        &format!(
            "https://maven.quiltmc.org/repository/release/org/quiltmc/quilt-installer/{0}/quilt-installer-{0}.jar",
            installer_version
        ),
        &installer_path,
    )
    .await?;

    let mut args = vec![
        "-jar".to_string(),
        "quilt-installer.jar".to_string(),
        "install".to_string(),
        "server".to_string(),
        profile.minecraft_version.clone(),
    ];
    if !profile.software_version.eq_ignore_ascii_case("latest") {
        args.push(profile.software_version.clone());
    }
    args.push("--download-server".to_string());
    // Der Quilt-Installer verlangt den Pfad in Anfuehrungszeichen
    args.push(format!("--install-dir=\"{}\"", profile.server_dir.to_string_lossy()));

    let result = run_logged_process(java_bin, &args, &profile.server_dir, console, "Quilt Installer").await;
    let _ = tokio::fs::remove_file(&installer_path).await;
    result
}

async fn install_forge_like_server(
    profile: &ServerProfile,
    java_bin: &str,
    console: &ConsoleBuffer,
) -> Result<(), String> {
    let latest = profile.software_version.eq_ignore_ascii_case("latest");

    let (installer_url, label) = if profile.software == "neoforge" {
        let version = if latest {
            resolve_latest_neoforge_version(&profile.minecraft_version).await?
        } else {
            profile.software_version.clone()
        };
        (
            format!(
                "https://maven.neoforged.net/releases/net/neoforged/neoforge/{0}/neoforge-{0}-installer.jar",
                version
            ),
            format!("NeoForge {} Installer", version),
        )
    } else {
        let client = crate::api::forge::ForgeClient::new().map_err(|e| e.to_string())?;
        let version = if latest {
            let versions = client
                .get_loader_versions(&profile.minecraft_version)
                .await
                .map_err(|e| e.to_string())?;
            versions
                .iter()
                .find(|v| v.recommended)
                .or_else(|| versions.first())
                .map(|v| v.forge_version.clone())
                .ok_or_else(|| format!("Keine Forge-Version fuer Minecraft {} gefunden", profile.minecraft_version))?
        } else {
            profile.software_version.clone()
        };
        (
            client.get_installer_url(&profile.minecraft_version, &version),
            format!("Forge {} Installer", version),
        )
    };

    let installer_path = profile.server_dir.join("lion-server-installer.jar");
    append_console_line(console, format!("[Lion] Lade {} herunter...", label)).await;
    download_to_path(&installer_url, &installer_path).await?;

    let args = vec![
        "-jar".to_string(),
        "lion-server-installer.jar".to_string(),
        "--installServer".to_string(),
    ];
    let result = run_logged_process(java_bin, &args, &profile.server_dir, console, &label).await;
    let _ = tokio::fs::remove_file(&installer_path).await;
    result
}

/// Sucht nach einer fertigen Forge/NeoForge-Installation im Server-Ordner.
fn detect_forge_launch_target(server_dir: &Path) -> Option<LaunchTarget> {
    let args_name = if cfg!(windows) { "win_args.txt" } else { "unix_args.txt" };
    let libraries = server_dir.join("libraries");
    let roots = [
        libraries.join("net").join("neoforged").join("neoforge"),
        libraries.join("net").join("neoforged").join("forge"),
        libraries.join("net").join("minecraftforge").join("forge"),
    ];

    for root in roots.iter() {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let mut candidates: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path().join(args_name))
            .filter(|p| p.is_file())
            .collect();
        candidates.sort();
        if let Some(path) = candidates.pop() {
            return Some(LaunchTarget::ArgsFile(format!(
                "@{}",
                to_relative_display(server_dir, &path)
            )));
        }
    }

    // Alte Forge-Versionen (< 1.17) erzeugen eine startbare forge-*.jar im Hauptordner
    let entries = std::fs::read_dir(server_dir).ok()?;
    let mut legacy: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| {
            let lower = name.to_lowercase();
            lower.starts_with("forge-") && lower.ends_with(".jar") && !lower.contains("installer")
        })
        .collect();
    legacy.sort();
    legacy.pop().map(LaunchTarget::Jar)
}

fn is_software_installed(profile: &ServerProfile) -> bool {
    match profile.software.as_str() {
        "forge" | "neoforge" => detect_forge_launch_target(&profile.server_dir).is_some(),
        "quilt" => profile.server_dir.join("quilt-server-launch.jar").exists(),
        _ => profile.server_dir.join("server.jar").exists(),
    }
}

/// Stellt sicher, dass die Server-Software installiert ist und liefert das Startziel.
async fn ensure_server_installed(
    profile: &ServerProfile,
    java_bin: &str,
    console: &ConsoleBuffer,
) -> Result<LaunchTarget, String> {
    let software = profile.software.as_str();
    let jar_path = profile.server_dir.join("server.jar");

    match software {
        "forge" | "neoforge" => {
            if let Some(target) = detect_forge_launch_target(&profile.server_dir) {
                return Ok(target);
            }
            install_forge_like_server(profile, java_bin, console).await?;
            detect_forge_launch_target(&profile.server_dir).ok_or_else(|| {
                format!(
                    "{} wurde installiert, aber es wurde keine Startdatei gefunden",
                    software
                )
            })
        }
        "quilt" => {
            let launch_jar = profile.server_dir.join("quilt-server-launch.jar");
            if !launch_jar.exists() {
                install_quilt_server(profile, java_bin, console).await?;
            }
            if !launch_jar.exists() {
                return Err("Quilt Installer hat keine quilt-server-launch.jar erzeugt".to_string());
            }
            Ok(LaunchTarget::Jar("quilt-server-launch.jar".to_string()))
        }
        _ => {
            if !jar_path.exists() {
                append_console_line(
                    console,
                    format!(
                        "[Lion] Lade {} Server fuer Minecraft {} herunter...",
                        software, profile.minecraft_version
                    ),
                )
                .await;

                match software {
                    "paper" | "folia" => download_papermc_server_jar(software, profile, &jar_path).await?,
                    "purpur" => download_purpur_server_jar(profile, &jar_path).await?,
                    "fabric" => download_fabric_server_jar(profile, &jar_path).await?,
                    "vanilla" => download_vanilla_server_jar(profile, &jar_path).await?,
                    "spigot" | "bukkit" => build_spigot_server_jar(profile, java_bin, console, &jar_path).await?,
                    _ => {
                        return Err(format!(
                            "Software '{}' wird aktuell nicht fuer Auto-Download unterstuetzt",
                            profile.software
                        ));
                    }
                }
                append_console_line(console, "[Lion] Server-Software ist bereit".to_string()).await;
            }
            Ok(LaunchTarget::Jar("server.jar".to_string()))
        }
    }
}

fn load_server_profile<'a>(profiles: &'a [ServerProfile], server_id: &str) -> Result<&'a ServerProfile, String> {
    profiles
        .iter()
        .find(|p| p.id == server_id)
        .ok_or_else(|| "Server-Profil nicht gefunden".to_string())
}

pub(crate) async fn load_server_profile_owned(server_id: &str) -> Result<ServerProfile, String> {
    let manager = ServerProfileManager::new().map_err(|e| e.to_string())?;
    let list = manager.load_profiles().await.map_err(|e| e.to_string())?;
    load_server_profile(&list.profiles, server_id).cloned()
}

#[tauri::command]
pub async fn get_server_profiles() -> Result<Vec<ServerProfile>, String> {
    let manager = ServerProfileManager::new().map_err(|e| e.to_string())?;
    let list = manager.load_profiles().await.map_err(|e| e.to_string())?;
    Ok(list.profiles)
}

#[tauri::command]
pub async fn create_server_profile(
    name: String,
    mode: String,
    software: String,
    minecraft_version: String,
    software_version: String,
    port: Option<u16>,
) -> Result<ServerProfile, String> {
    let mode_enum = ServerMode::from_str(&mode);
    let normalized_software = normalize_software(&mode_enum, &software)?;
    let minecraft_version = minecraft_version.trim().to_string();

    let profile = ServerProfile::new(
        name,
        mode_enum,
        normalized_software,
        minecraft_version,
        if software_version.trim().is_empty() {
            "latest".to_string()
        } else {
            software_version.trim().to_string()
        },
        port.unwrap_or(25565),
    );

    // Nur Versionen erlauben, die es fuer die Software wirklich gibt
    ensure_software_supports_version(&profile).await?;
    ensure_server_layout(&profile).await?;

    let manager = ServerProfileManager::new().map_err(|e| e.to_string())?;
    manager
        .create_profile(profile.clone())
        .await
        .map_err(|e| e.to_string())?;

    Ok(profile)
}

#[tauri::command]
pub async fn delete_server_profile(server_id: String) -> Result<(), String> {
    let _ = stop_server_profile(server_id.clone()).await;
    let manager = ServerProfileManager::new().map_err(|e| e.to_string())?;
    manager
        .delete_profile(&server_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn update_server_profile_settings(
    server_id: String,
    memory_mb: Option<u32>,
    java_args: Option<String>,
    auto_restart: Option<bool>,
    port: Option<u16>,
) -> Result<ServerProfile, String> {
    let manager = ServerProfileManager::new().map_err(|e| e.to_string())?;
    let mut list = manager.load_profiles().await.map_err(|e| e.to_string())?;

    let profile = list
        .get_profile_mut(&server_id)
        .ok_or_else(|| "Server-Profil nicht gefunden".to_string())?;

    if let Some(memory) = memory_mb {
        profile.memory_mb = memory.clamp(512, 65536);
    }

    if let Some(raw_java_args) = java_args {
        profile.java_args = raw_java_args
            .split_whitespace()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    if let Some(restart) = auto_restart {
        profile.auto_restart = restart;
    }

    if let Some(next_port) = port {
        profile.port = if next_port == 0 { 25565 } else { next_port };
    }

    profile.touch();
    let updated = profile.clone();

    manager
        .save_profiles(&list)
        .await
        .map_err(|e| e.to_string())?;

    sync_server_profile_properties(&updated).await?;

    Ok(updated)
}

#[tauri::command]
pub async fn start_server_profile(server_id: String) -> Result<(), String> {
    if let Some(state) = get_running_state(&server_id) {
        if is_process_alive(state.pid) {
            return Ok(());
        }
        remove_running_state(&server_id);
    }

    let Some(_guard) = InstallGuard::acquire(&server_id) else {
        return Err("Server wird bereits vorbereitet".to_string());
    };

    let console = reset_console_buffer(&server_id);
    let result = prepare_and_spawn_server(&server_id, &console).await;
    if let Err(err) = &result {
        append_console_line(&console, format!("[Lion] Start fehlgeschlagen: {}", err)).await;
    }
    result
}

async fn resolve_server_java(profile: &ServerProfile, console: &ConsoleBuffer) -> Result<String, String> {
    let required_java = resolve_required_java_for_profile(profile).await?;
    let configured_java = resolve_java_binary().await;

    match detect_java_major(&configured_java) {
        Ok(detected_java) if detected_java >= required_java => Ok(configured_java),
        Ok(detected_java) if configured_java != "java" => Err(format!(
            "Java {} erkannt, aber mindestens Java {} wird fuer {} benoetigt. Bitte in Settings eine neuere Java-Version waehlen.",
            detected_java, required_java, profile.software
        )),
        _ => {
            append_console_line(
                console,
                format!("[Lion] Bereite Java {} vor...", required_java),
            )
            .await;
            crate::core::minecraft::ensure_java_runtime(required_java, None)
                .await
                .map_err(|e| format!("Java {} konnte nicht vorbereitet werden: {}", required_java, e))
        }
    }
}

async fn prepare_and_spawn_server(server_id: &str, console: &ConsoleBuffer) -> Result<(), String> {
    let profile = load_server_profile_owned(server_id).await?;
    append_console_line(
        console,
        format!(
            "[Lion] Starte {} ({} {})",
            profile.name, profile.software, profile.minecraft_version
        ),
    )
    .await;

    ensure_server_layout(&profile).await?;
    sync_server_profile_properties(&profile).await?;
    ensure_eula(&profile).await?;

    if !is_software_installed(&profile) {
        ensure_software_supports_version(&profile).await?;
    }

    let java_bin = resolve_server_java(&profile, console).await?;
    let launch_target = ensure_server_installed(&profile, &java_bin, console).await?;

    let xmx = profile.memory_mb.max(512);
    let xms = (profile.memory_mb / 2).max(512);

    let mut cmd = Command::new(&java_bin);
    cmd.current_dir(&profile.server_dir)
        .arg(format!("-Xms{}M", xms))
        .arg(format!("-Xmx{}M", xmx));

    let java_args = if profile.java_args.is_empty() {
        crate::config::defaults::default_java_args()
    } else {
        profile.java_args.clone()
    };

    for java_arg in &java_args {
        cmd.arg(java_arg);
    }

    match &launch_target {
        LaunchTarget::Jar(jar) => {
            cmd.arg("-jar").arg(jar);
        }
        LaunchTarget::ArgsFile(args_file) => {
            cmd.arg(args_file);
        }
    }

    cmd.arg("nogui")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "Konnte Server nicht starten (Java: {}): {}",
            java_bin, e
        )
    })?;

    let pid = child
        .id()
        .ok_or_else(|| "Server-PID konnte nicht ermittelt werden".to_string())?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Server stdout konnte nicht angebunden werden".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Server stderr konnte nicht angebunden werden".to_string())?;
    let stdin = child.stdin.take();

    let console_lines = console.clone();
    append_console_line(
        &console_lines,
        format!("[Lion] Server gestartet (PID {})", pid),
    )
        .await;

    let state = RunningServerState {
        pid,
        started_at: std::time::Instant::now(),
        console_lines: console_lines.clone(),
        stdin: Arc::new(tokio::sync::Mutex::new(stdin)),
    };

    set_running_state(server_id, state);

    let server_id_stdout = server_id.to_string();
    let stdout_buffer = console_lines.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            append_console_line(&stdout_buffer, line).await;
        }
        tracing::debug!("Server stdout stream ended for {}", server_id_stdout);
    });

    let server_id_stderr = server_id.to_string();
    let stderr_buffer = console_lines.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            append_console_line(&stderr_buffer, format!("[ERR] {}", line)).await;
        }
        tracing::debug!("Server stderr stream ended for {}", server_id_stderr);
    });

    let server_id_wait = server_id.to_string();
    let wait_buffer = console_lines.clone();
    tokio::spawn(async move {
        let status = child.wait().await;
        match status {
            Ok(exit_status) => {
                append_console_line(
                    &wait_buffer,
                    format!("[Lion] Server beendet: {}", exit_status),
                )
                    .await;
            }
            Err(err) => {
                append_console_line(
                    &wait_buffer,
                    format!("[Lion] Fehler beim Beenden des Server-Prozesses: {}", err),
                )
                    .await;
            }
        }
        // Nur entfernen, wenn inzwischen nicht schon ein neuer Prozess registriert wurde
        if get_running_state(&server_id_wait).map(|s| s.pid) == Some(pid) {
            remove_running_state(&server_id_wait);
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn stop_server_profile(server_id: String) -> Result<bool, String> {
    let Some(state) = get_running_state(&server_id) else {
        return Ok(false);
    };

    {
        let mut stdin_guard = state.stdin.lock().await;
        if let Some(stdin) = stdin_guard.as_mut() {
            let _ = stdin.write_all(b"stop\n").await;
            let _ = stdin.flush().await;
        }
    }

    // Modded-Server brauchen zum Speichern der Welten teils deutlich laenger als ein paar Sekunden.
    // Zu frueh zu killen kann Chunks/Weltdaten beschaedigen.
    for _ in 0..150 {
        if !is_process_alive(state.pid) {
            remove_running_state(&server_id);
            return Ok(true);
        }
        sleep(Duration::from_millis(200)).await;
    }

    kill_process(state.pid);
    remove_running_state(&server_id);
    Ok(true)
}

#[tauri::command]
pub async fn kill_server_profile(server_id: String) -> Result<bool, String> {
    let Some(state) = get_running_state(&server_id) else {
        return Ok(false);
    };

    kill_process(state.pid);
    remove_running_state(&server_id);
    append_console_line(&state.console_lines, "[Lion] Server-Prozess wurde gekillt".to_string()).await;
    Ok(true)
}

#[tauri::command]
pub async fn restart_server_profile(server_id: String) -> Result<(), String> {
    let _ = stop_server_profile(server_id.clone()).await;
    sleep(Duration::from_millis(500)).await;
    start_server_profile(server_id).await
}

#[tauri::command]
pub async fn get_running_server_profiles() -> Result<Vec<String>, String> {
    let mut running = Vec::new();
    for server_id in running_server_ids() {
        if let Some(state) = get_running_state(&server_id) {
            if is_process_alive(state.pid) {
                running.push(server_id.clone());
                continue;
            }
        }
        remove_running_state(&server_id);
    }

    Ok(running)
}

#[tauri::command]
pub async fn get_server_runtime(server_id: String) -> Result<ServerRuntimeInfo, String> {
    let offline = |installing: bool| ServerRuntimeInfo {
        running: false,
        installing,
        pid: None,
        cpu_percent: 0.0,
        cpu_cores: cpu_core_count(),
        memory_mb: 0,
        uptime_seconds: 0,
        network_kbps: 0.0,
        network_rx_kbps: 0.0,
        network_tx_kbps: 0.0,
    };

    let installing = is_server_installing(&server_id);
    let Some(state) = get_running_state(&server_id) else {
        return Ok(offline(installing));
    };

    let pid = state.pid;
    let sample = tokio::task::spawn_blocking(move || (sample_process(pid), sample_network_kbps()))
        .await
        .map_err(|e| e.to_string())?;

    let (Some((cpu_percent, memory_bytes)), (rx_kbps, tx_kbps)) = sample else {
        remove_running_state(&server_id);
        return Ok(offline(installing));
    };

    Ok(ServerRuntimeInfo {
        running: true,
        installing: false,
        pid: Some(pid),
        cpu_percent,
        cpu_cores: cpu_core_count(),
        memory_mb: memory_bytes / 1024 / 1024,
        uptime_seconds: state.started_at.elapsed().as_secs(),
        network_kbps: rx_kbps + tx_kbps,
        network_rx_kbps: rx_kbps,
        network_tx_kbps: tx_kbps,
    })
}

#[tauri::command]
pub async fn send_server_console_command(server_id: String, command: String) -> Result<(), String> {
    let state = get_running_state(&server_id)
        .ok_or_else(|| "Server laeuft nicht".to_string())?;

    let mut stdin_guard = state.stdin.lock().await;
    let stdin = stdin_guard
        .as_mut()
        .ok_or_else(|| "Server-Eingabe ist nicht verfuegbar".to_string())?;

    let cmd = command.trim();
    if cmd.is_empty() {
        return Ok(());
    }

    stdin
        .write_all(format!("{}\n", cmd).as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())?;

    append_console_line(&state.console_lines, format!("[CMD] {}", cmd)).await;
    Ok(())
}

#[tauri::command]
pub async fn get_server_properties(server_id: String) -> Result<ServerPropertiesResponse, String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let properties_path = profile.server_dir.join("server.properties");

    if !properties_path.exists() {
        return Ok(ServerPropertiesResponse {
            exists: false,
            values: HashMap::new(),
        });
    }

    let content = tokio::fs::read_to_string(&properties_path)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ServerPropertiesResponse {
        exists: true,
        values: parse_properties(&content),
    })
}

#[tauri::command]
pub async fn save_server_properties(
    server_id: String,
    values: HashMap<String, String>,
) -> Result<(), String> {
    let profile = load_server_profile_owned(&server_id).await?;
    ensure_server_layout(&profile).await?;
    let properties_path = profile.server_dir.join("server.properties");

    tokio::fs::write(properties_path, format_properties(&values))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_server_content(server_id: String, content_type: String) -> Result<Vec<String>, String> {
    let profile = load_server_profile_owned(&server_id).await?;

    let folder_name = match content_type.trim().to_lowercase().as_str() {
        "plugins" => "plugins",
        "mods" => "mods",
        other => {
            return Err(format!(
                "Unbekannter Content-Typ '{}'. Erlaubt: plugins, mods",
                other
            ));
        }
    };

    let content_dir = profile.server_dir.join(folder_name);
    tokio::fs::create_dir_all(&content_dir)
        .await
        .map_err(|e| e.to_string())?;

    let mut entries = tokio::fs::read_dir(&content_dir)
        .await
        .map_err(|e| e.to_string())?;

    let mut files = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let file_type = entry.file_type().await.map_err(|e| e.to_string())?;
        if !file_type.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_lowercase().ends_with(".jar") {
            files.push(name);
        }
    }

    files.sort_by_key(|name| name.to_lowercase());
    Ok(files)
}

#[tauri::command]
pub async fn import_server_content_file(
    server_id: String,
    content_type: String,
    file_name: String,
    file_base64: String,
) -> Result<(), String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("Ungueltiger Dateiname".to_string());
    }

    let profile = load_server_profile_owned(&server_id).await?;
    let content_dir = match content_type.trim().to_lowercase().as_str() {
        "plugins" => profile.server_dir.join("plugins"),
        "mods" => profile.server_dir.join("mods"),
        _ => return Err("Unbekannter Content-Typ".to_string()),
    };

    tokio::fs::create_dir_all(&content_dir)
        .await
        .map_err(|e| e.to_string())?;

    let bytes = general_purpose::STANDARD
        .decode(file_base64.trim())
        .map_err(|e| format!("Base64 konnte nicht dekodiert werden: {}", e))?;

    tokio::fs::write(content_dir.join(file_name), bytes)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_server_content_file(
    server_id: String,
    content_type: String,
    file_name: String,
) -> Result<(), String> {
    if file_name.contains('/') || file_name.contains('\\') || file_name.contains("..") {
        return Err("Ungueltiger Dateiname".to_string());
    }

    let profile = load_server_profile_owned(&server_id).await?;
    let content_dir = match content_type.trim().to_lowercase().as_str() {
        "plugins" => profile.server_dir.join("plugins"),
        "mods" => profile.server_dir.join("mods"),
        _ => return Err("Unbekannter Content-Typ".to_string()),
    };

    let target = content_dir.join(file_name);
    if !target.exists() {
        return Ok(());
    }

    tokio::fs::remove_file(target)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_server_console_output(server_id: String) -> Result<String, String> {
    if let Some(state) = get_running_state(&server_id) {
        let lines = state.console_lines.lock().await;
        return Ok(lines.iter().cloned().collect::<Vec<_>>().join("\n"));
    }

    // Installations-/Startausgabe bzw. Ausgabe des zuletzt beendeten Prozesses dieser Sitzung
    {
        let buffer = console_buffer(&server_id);
        let lines = buffer.lock().await;
        if !lines.is_empty() {
            return Ok(lines.iter().cloned().collect::<Vec<_>>().join("\n"));
        }
    }

    let profile = load_server_profile_owned(&server_id).await?;
    let latest_log = profile.server_dir.join("logs").join("latest.log");
    if latest_log.exists() {
        return Ok(read_tail_lines(&latest_log, 2000).await);
    }

    Ok("Server ist derzeit nicht gestartet.".to_string())
}

#[tauri::command]
pub async fn open_server_folder(server_id: String, subfolder: Option<String>) -> Result<(), String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let path = resolve_server_path(&profile, subfolder.as_deref())?;

    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| e.to_string())?;

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn list_server_files(
    server_id: String,
    relative_path: Option<String>,
) -> Result<ServerFileListResponse, String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let current_full_path = resolve_server_path(&profile, relative_path.as_deref())?;

    if !current_full_path.exists() {
        tokio::fs::create_dir_all(&current_full_path)
            .await
            .map_err(|e| e.to_string())?;
    }

    if !current_full_path.is_dir() {
        return Err("Pfad ist kein Ordner".to_string());
    }

    let mut read_dir = tokio::fs::read_dir(&current_full_path)
        .await
        .map_err(|e| e.to_string())?;

    let mut entries = Vec::new();
    while let Some(entry) = read_dir.next_entry().await.map_err(|e| e.to_string())? {
        let metadata = entry.metadata().await.map_err(|e| e.to_string())?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        let full_path = entry.path();
        let modified_unix = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        entries.push(ServerFileEntry {
            name: file_name,
            relative_path: to_relative_display(&profile.server_dir, &full_path),
            is_dir: metadata.is_dir(),
            size: if metadata.is_file() { metadata.len() } else { 0 },
            modified_unix,
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(ServerFileListResponse {
        current_path: to_relative_display(&profile.server_dir, &current_full_path),
        entries,
    })
}

#[tauri::command]
pub async fn read_server_text_file(server_id: String, relative_path: String) -> Result<String, String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let path = resolve_server_path(&profile, Some(&relative_path))?;

    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|e| e.to_string())?;
    if metadata.is_dir() {
        return Err("Datei erwartet, aber Ordner erhalten".to_string());
    }
    if metadata.len() > MAX_EDITABLE_FILE_SIZE {
        return Err(format!(
            "Datei ist zu gross fuer den integrierten Editor (max {} MB)",
            MAX_EDITABLE_FILE_SIZE / 1024 / 1024
        ));
    }

    tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("Datei konnte nicht als Text gelesen werden: {}", e))
}

#[tauri::command]
pub async fn write_server_text_file(
    server_id: String,
    relative_path: String,
    content: String,
) -> Result<(), String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let path = resolve_server_path(&profile, Some(&relative_path))?;

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    tokio::fs::write(path, content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_server_path(server_id: String, relative_path: String) -> Result<(), String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let path = resolve_server_path(&profile, Some(&relative_path))?;

    if !path.exists() {
        return Ok(());
    }

    if path.is_dir() {
        tokio::fs::remove_dir_all(path)
            .await
            .map_err(|e| e.to_string())
    } else {
        tokio::fs::remove_file(path)
            .await
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn move_server_path(
    server_id: String,
    from_path: String,
    to_path: String,
) -> Result<(), String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let source = resolve_server_path(&profile, Some(&from_path))?;
    let target = resolve_server_path(&profile, Some(&to_path))?;

    if !source.exists() {
        return Err("Quelle existiert nicht".to_string());
    }

    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    tokio::fs::rename(source, target)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_server_folder(server_id: String, relative_path: String) -> Result<(), String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let path = resolve_server_path(&profile, Some(&relative_path))?;
    tokio::fs::create_dir_all(path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upload_server_file(
    server_id: String,
    relative_path: String,
    file_base64: String,
) -> Result<(), String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let path = resolve_server_path(&profile, Some(&relative_path))?;

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    let bytes = general_purpose::STANDARD
        .decode(file_base64.trim())
        .map_err(|e| format!("Base64 konnte nicht dekodiert werden: {}", e))?;

    tokio::fs::write(path, bytes)
        .await
        .map_err(|e| e.to_string())
}