use crate::api::mojang::MojangClient;
use crate::core::servers::ServerProfileManager;
use crate::types::server_profile::{ServerMode, ServerProfile};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::time::{sleep, Duration};

const MAX_CONSOLE_LINES: usize = 4000;
const MAX_EDITABLE_FILE_SIZE: u64 = 2 * 1024 * 1024;

#[derive(Clone)]
struct RunningServerState {
    pid: u32,
    started_at: std::time::Instant,
    console_lines: Arc<tokio::sync::Mutex<VecDeque<String>>>,
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
    pub pid: Option<u32>,
    pub cpu_percent: f32,
    pub memory_mb: u64,
    pub uptime_seconds: u64,
    pub network_kbps: f32,
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
    id: String,
    support: Option<PaperSupportInfo>,
    java: Option<PaperJavaInfo>,
}

#[derive(Deserialize)]
struct PaperSupportInfo {
    status: String,
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

static RUNNING_SERVERS: OnceLock<Mutex<HashMap<String, RunningServerState>>> = OnceLock::new();
static NETWORK_SAMPLE: OnceLock<Mutex<Option<(std::time::Instant, u64)>>> = OnceLock::new();

fn running_servers() -> &'static Mutex<HashMap<String, RunningServerState>> {
    RUNNING_SERVERS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn network_sample() -> &'static Mutex<Option<(std::time::Instant, u64)>> {
    NETWORK_SAMPLE.get_or_init(|| Mutex::new(None))
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

fn supported_software(mode: &ServerMode) -> &'static [&'static str] {
    match mode {
        ServerMode::Plugins => &["paper", "purpur", "vanilla"],
        ServerMode::Modded => &["fabric", "vanilla"],
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

fn sanitize_relative_path(relative_path: &str) -> Result<PathBuf, String> {
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

fn to_relative_display(root: &Path, full_path: &Path) -> String {
    full_path
        .strip_prefix(root)
        .ok()
        .unwrap_or(full_path)
        .to_string_lossy()
        .replace('\\', "/")
}

async fn append_console_line(buffer: &Arc<tokio::sync::Mutex<VecDeque<String>>>, line: String) {
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

    let mut system = System::new();
    system.refresh_processes();
    system.process(Pid::from_u32(pid)).is_some()
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

fn sample_network_kbps() -> f32 {
    use sysinfo::Networks;

    let mut networks = Networks::new_with_refreshed_list();
    networks.refresh();

    let total_now = networks
        .values()
        .map(|data| data.total_received() + data.total_transmitted())
        .sum::<u64>();
    let now = std::time::Instant::now();

    let Ok(mut guard) = network_sample().lock() else {
        return 0.0;
    };

    let kbps = if let Some((prev_ts, prev_total)) = *guard {
        let elapsed = now.duration_since(prev_ts).as_secs_f32();
        if elapsed > 0.0 {
            let delta_bytes = total_now.saturating_sub(prev_total) as f32;
            (delta_bytes / 1024.0) / elapsed
        } else {
            0.0
        }
    } else {
        0.0
    };

    *guard = Some((now, total_now));
    kbps
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

fn resolve_paper_version_alias(project: &PaperProjectResponse, requested: &str) -> Option<String> {
    if project.versions.contains_key(requested) {
        return Some(requested.to_string());
    }

    if requested.starts_with("1.") {
        let mut split = requested.split('.');
        let major = split.next().unwrap_or("1");
        let minor = split.next().unwrap_or("21");
        let short = format!("{}.{}", major, minor);
        if project.versions.contains_key(&short) {
            return Some(short);
        }
    }

    project
        .versions
        .iter()
        .find_map(|(key, aliases)| aliases.iter().any(|a| a == requested).then(|| key.clone()))
}

async fn resolve_paper_version_for_profile(
    client: &reqwest::Client,
    profile: &ServerProfile,
) -> Result<String, String> {
    let project: PaperProjectResponse = client
        .get("https://fill.papermc.io/v3/projects/paper")
        .send()
        .await
        .map_err(|e| format!("Paper Projektdaten konnten nicht geladen werden: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Paper Projektdaten konnten nicht geladen werden: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Paper Projektdaten sind ungueltig: {}", e))?;

    if let Some(alias) = resolve_paper_version_alias(&project, &profile.minecraft_version) {
        return Ok(alias);
    }

    let version_meta: Vec<PaperVersionMeta> = client
        .get("https://fill.papermc.io/v3/projects/paper/versions")
        .send()
        .await
        .map_err(|e| format!("Paper Versionen konnten nicht geladen werden: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Paper Versionen konnten nicht geladen werden: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Paper Versionen sind ungueltig: {}", e))?;

    if let Some(preferred) = version_meta.iter().find(|entry| {
        entry
            .version
            .support
            .as_ref()
            .map(|support| support.status.eq_ignore_ascii_case("SUPPORTED"))
            .unwrap_or(false)
    }) {
        return Ok(preferred.version.id.clone());
    }

    version_meta
        .first()
        .map(|v| v.version.id.clone())
        .ok_or_else(|| "Keine Paper Version verfuegbar".to_string())
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
        v if v.starts_with("1.20.5") || v.starts_with("1.21") => 21,
        v if v.starts_with("1.18") || v.starts_with("1.19") || v.starts_with("1.20") => 17,
        v if v.starts_with("1.17") => 16,
        _ => 8,
    };

    let mut required_java = resolve_mojang_java_requirement(&profile.minecraft_version)
        .await
        .unwrap_or(heuristic_java);

    // Paper liefert die exakte Mindest-Java-Version in Fill v3 Metadaten.
    if profile.software.eq_ignore_ascii_case("paper") {
        let client = crate::core::http::HTTP_CLIENT.clone();
        let paper_version = resolve_paper_version_for_profile(&client, profile).await?;
        let meta_url = format!(
            "https://fill.papermc.io/v3/projects/paper/versions/{}",
            paper_version
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

async fn download_to_path(url: &str, output_path: &Path) -> Result<(), String> {
    let response = crate::core::http::HTTP_CLIENT.clone()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Download fehlgeschlagen: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Download fehlgeschlagen: {}", e))?;

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Download konnte nicht gelesen werden: {}", e))?;

    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }

    tokio::fs::write(output_path, bytes)
        .await
        .map_err(|e| format!("Konnte Datei nicht schreiben: {}", e))
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

async fn download_paper_server_jar(profile: &ServerProfile, output_path: &Path) -> Result<(), String> {
    let client = crate::core::http::HTTP_CLIENT.clone();
    let paper_version = resolve_paper_version_for_profile(&client, profile).await?;

    let builds_url = format!(
        "https://fill.papermc.io/v3/projects/paper/versions/{}/builds",
        paper_version
    );
    let builds: Vec<PaperBuildEntry> = client
        .get(&builds_url)
        .send()
        .await
        .map_err(|e| format!("Paper Builds konnten nicht geladen werden: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Paper Builds konnten nicht geladen werden: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Paper Builds sind ungueltig: {}", e))?;

    if builds.is_empty() {
        return Err("Keine Paper Builds gefunden".to_string());
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
            .ok_or_else(|| "Kein passender Paper Build gefunden".to_string())?
    } else {
        let requested_id = profile
            .software_version
            .parse::<u32>()
            .map_err(|_| "Paper Build muss eine Zahl oder 'latest' sein".to_string())?;
        builds
            .iter()
            .find(|build| build.id == requested_id)
            .ok_or_else(|| format!("Paper Build {} wurde nicht gefunden", requested_id))?
    };

    let download = target_build
        .downloads
        .get("server:default")
        .or_else(|| target_build.downloads.values().next())
        .ok_or_else(|| "Paper Build enthaelt keinen Server Download".to_string())?;

    tracing::info!(
        "Paper Download: version={} build={} url={}",
        paper_version,
        target_build.id,
        download.url
    );

    download_to_path(&download.url, output_path).await
}

async fn download_purpur_server_jar(profile: &ServerProfile, output_path: &Path) -> Result<(), String> {
    let client = crate::core::http::HTTP_CLIENT.clone();
    let build = if profile.software_version == "latest" {
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
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        value
            .get("builds")
            .and_then(|v| v.get("latest"))
            .and_then(|v| v.as_i64())
            .ok_or_else(|| "Konnte den neuesten Purpur Build nicht ermitteln".to_string())?
            .to_string()
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
    let loader_version = if profile.software_version == "latest" {
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
            .ok_or_else(|| "Keine Fabric Loader-Version gefunden".to_string())?
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

async fn ensure_server_jar(profile: &ServerProfile) -> Result<PathBuf, String> {
    let jar_path = profile.server_dir.join("server.jar");
    if jar_path.exists() {
        return Ok(jar_path);
    }

    tracing::info!(
        "Lade Server-JAR automatisch herunter: {} ({})",
        profile.name,
        profile.software
    );

    match profile.software.as_str() {
        "paper" => download_paper_server_jar(profile, &jar_path).await?,
        "purpur" => download_purpur_server_jar(profile, &jar_path).await?,
        "fabric" => download_fabric_server_jar(profile, &jar_path).await?,
        "vanilla" => download_vanilla_server_jar(profile, &jar_path).await?,
        _ => {
            return Err(format!(
                "Software '{}' wird aktuell nicht fuer Auto-Download unterstuetzt",
                profile.software
            ));
        }
    }

    Ok(jar_path)
}

fn load_server_profile<'a>(profiles: &'a [ServerProfile], server_id: &str) -> Result<&'a ServerProfile, String> {
    profiles
        .iter()
        .find(|p| p.id == server_id)
        .ok_or_else(|| "Server-Profil nicht gefunden".to_string())
}

async fn load_server_profile_owned(server_id: &str) -> Result<ServerProfile, String> {
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

    let profile = load_server_profile_owned(&server_id).await?;
    ensure_server_layout(&profile).await?;
    sync_server_profile_properties(&profile).await?;
    ensure_eula(&profile).await?;
    ensure_server_jar(&profile).await?;

    let required_java = resolve_required_java_for_profile(&profile).await?;
    let configured_java = resolve_java_binary().await;

    let java_bin = match detect_java_major(&configured_java) {
        Ok(detected_java) if detected_java >= required_java => configured_java,
        Ok(detected_java) if configured_java != "java" => {
            return Err(format!(
                "Java {} erkannt, aber mindestens Java {} wird fuer {} benoetigt. Bitte in Settings eine neuere Java-Version waehlen.",
                detected_java, required_java, profile.software
            ));
        }
        _ => {
            tracing::info!(
                "No suitable configured Java found. Ensuring Java {} for local server...",
                required_java
            );
            crate::core::minecraft::ensure_java_runtime(required_java, None)
                .await
                .map_err(|e| format!("Java {} konnte nicht vorbereitet werden: {}", required_java, e))?
        }
    };

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

    cmd.arg("-jar")
        .arg("server.jar")
        .arg("nogui")
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

    let console_lines = Arc::new(tokio::sync::Mutex::new(VecDeque::new()));
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

    set_running_state(&server_id, state);

    let server_id_stdout = server_id.clone();
    let stdout_buffer = console_lines.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            append_console_line(&stdout_buffer, line).await;
        }
        tracing::debug!("Server stdout stream ended for {}", server_id_stdout);
    });

    let server_id_stderr = server_id.clone();
    let stderr_buffer = console_lines.clone();
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            append_console_line(&stderr_buffer, format!("[ERR] {}", line)).await;
        }
        tracing::debug!("Server stderr stream ended for {}", server_id_stderr);
    });

    let server_id_wait = server_id.clone();
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
        remove_running_state(&server_id_wait);
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

    for _ in 0..15 {
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
    use sysinfo::{Pid, System};

    let Some(state) = get_running_state(&server_id) else {
        return Ok(ServerRuntimeInfo {
            running: false,
            pid: None,
            cpu_percent: 0.0,
            memory_mb: 0,
            uptime_seconds: 0,
            network_kbps: 0.0,
        });
    };

    if !is_process_alive(state.pid) {
        remove_running_state(&server_id);
        return Ok(ServerRuntimeInfo {
            running: false,
            pid: None,
            cpu_percent: 0.0,
            memory_mb: 0,
            uptime_seconds: 0,
            network_kbps: 0.0,
        });
    }

    let mut system = System::new();
    system.refresh_processes();

    let mut cpu_percent = 0.0;
    let mut memory_mb = 0;

    if let Some(process) = system.process(Pid::from_u32(state.pid)) {
        cpu_percent = process.cpu_usage();
        memory_mb = process.memory() / 1024 / 1024;
    }

    let network_kbps = sample_network_kbps();

    Ok(ServerRuntimeInfo {
        running: true,
        pid: Some(state.pid),
        cpu_percent,
        memory_mb,
        uptime_seconds: state.started_at.elapsed().as_secs(),
        network_kbps,
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