//! Export/Import von Profil-Inhalten (Mods, Resource Packs, Shader Packs) für `.lion`-Dateien.
//!
//! Ziel: Der Empfänger bekommt **exakt dieselben Dateien** wie der Absender.
//!
//! Export: Für jede Datei wird der SHA-1/SHA-512-Hash berechnet und per Modrinth
//! (`POST /v2/version_files`) die exakte Version ermittelt. Dateien, die es auf Modrinth
//! nicht gibt (z.B. CurseForge-only oder selbst hinzugefügte JARs), werden direkt in die
//! `.lion`-Datei eingebettet – so geht beim Teilen nichts verloren.
//!
//! Import: Die exakten Versionen werden gesammelt (wenige Bulk-Requests statt hunderter
//! Einzelanfragen → kein Rate-Limit) und mit Hash-Prüfung heruntergeladen. Nur wenn eine
//! Version auf Modrinth gelöscht wurde, wird auf die neueste kompatible Version
//! zurückgegriffen (mit Warnung).

use crate::core::download::DownloadManager;
use crate::core::profiles::ProfileManager;
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::Emitter;

const MODRINTH_API: &str = "https://api.modrinth.com/v2";

// ==================== Datenformat ====================

/// Ein Eintrag in `content.mods` / `content.resourcepacks` / `content.shaderpacks`.
///
/// Abwärtskompatibel: Alte `.lion`-Dateien enthalten nur `mod_id` (Mods) bzw.
/// `project_id` (Packs) – diese werden mit der neuesten kompatiblen Version installiert.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SharedContentEntry {
    /// Dateiname ohne `.disabled`
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub sha512: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    /// "modrinth" oder "embedded"
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// Alias für `project_id` (Mods, altes Format)
    #[serde(default)]
    pub mod_id: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub version_number: Option<String>,
    /// Eingebettete Datei (bzw. ZIP eines Pack-Ordners) als Base64
    #[serde(default)]
    pub data_base64: Option<String>,
    /// `true` = Resource-/Shader-Pack ist ein Ordner (eingebettet als ZIP)
    #[serde(default)]
    pub is_folder: bool,
}

impl SharedContentEntry {
    fn project(&self) -> Option<&str> {
        self.project_id
            .as_deref()
            .or(self.mod_id.as_deref())
            .filter(|id| !id.trim().is_empty())
    }

    fn display_name(&self) -> String {
        self.name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .or_else(|| self.filename.clone())
            .or_else(|| self.project().map(str::to_string))
            .unwrap_or_else(|| "?".to_string())
    }
}

#[derive(Debug, Serialize)]
pub struct ContentImportFailure {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Default, Serialize)]
pub struct ContentImportReport {
    pub total: usize,
    pub installed: usize,
    pub failed: Vec<ContentImportFailure>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ContentImportProgress {
    kind: String,
    done: usize,
    total: usize,
    name: String,
}

// ==================== Inhaltsarten ====================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentKind {
    Mods,
    ResourcePacks,
    ShaderPacks,
}

impl ContentKind {
    fn parse(kind: &str) -> Result<Self, String> {
        match kind {
            "mods" => Ok(Self::Mods),
            "resourcepacks" => Ok(Self::ResourcePacks),
            "shaderpacks" => Ok(Self::ShaderPacks),
            other => Err(format!("Unbekannte Inhaltsart: {}", other)),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Mods => "mods",
            Self::ResourcePacks => "resourcepacks",
            Self::ShaderPacks => "shaderpacks",
        }
    }

    fn metadata_dir(self) -> &'static str {
        match self {
            Self::Mods => "modinfos",
            Self::ResourcePacks => "resourcepackinfos",
            Self::ShaderPacks => "shaderpackinfos",
        }
    }

    fn file_extension(self) -> &'static str {
        match self {
            Self::Mods => ".jar",
            Self::ResourcePacks | Self::ShaderPacks => ".zip",
        }
    }
}

// ==================== Modrinth-API ====================

#[derive(Debug, Clone, Deserialize)]
struct MrVersion {
    id: String,
    project_id: String,
    version_number: String,
    #[serde(default)]
    files: Vec<MrFile>,
}

#[derive(Debug, Clone, Deserialize)]
struct MrFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    hashes: MrHashes,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MrHashes {
    sha1: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MrProject {
    id: String,
    slug: String,
    title: String,
    #[serde(default)]
    icon_url: Option<String>,
}

/// Sendet eine Modrinth-Anfrage mit Wiederholung bei Rate-Limit (429) und Serverfehlern.
async fn modrinth_send(
    build: impl Fn() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, String> {
    let mut last_error = String::from("Modrinth nicht erreichbar");
    for attempt in 0..4u64 {
        match build().send().await {
            Ok(resp) if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                let wait = resp
                    .headers()
                    .get("x-ratelimit-reset")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(2)
                    .clamp(1, 30);
                tracing::warn!("Modrinth rate limit reached, waiting {}s", wait);
                last_error = "Modrinth: zu viele Anfragen (Rate-Limit)".to_string();
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
            Ok(resp) if resp.status().is_server_error() => {
                last_error = format!("Modrinth-Serverfehler: {}", resp.status());
                tokio::time::sleep(Duration::from_secs(1 + attempt)).await;
            }
            Ok(resp) => return Ok(resp),
            Err(e) => {
                last_error = e.to_string();
                tokio::time::sleep(Duration::from_secs(1 + attempt)).await;
            }
        }
    }
    Err(last_error)
}

async fn modrinth_json<T: serde::de::DeserializeOwned>(
    build: impl Fn() -> reqwest::RequestBuilder,
) -> Result<T, String> {
    let resp = modrinth_send(build).await?;
    if !resp.status().is_success() {
        return Err(format!("Modrinth-Anfrage fehlgeschlagen: {}", resp.status()));
    }
    resp.json::<T>().await.map_err(|e| e.to_string())
}

/// SHA-1 → Version (exakte Version zur Datei).
async fn lookup_versions_by_sha1(hashes: &[String]) -> Result<HashMap<String, MrVersion>, String> {
    let client = crate::core::http::HTTP_CLIENT.clone();
    let mut result = HashMap::new();
    for chunk in hashes.chunks(200) {
        let body = serde_json::json!({ "hashes": chunk, "algorithm": "sha1" });
        let found: HashMap<String, MrVersion> = modrinth_json(|| {
            client
                .post(format!("{}/version_files", MODRINTH_API))
                .json(&body)
        })
        .await?;
        result.extend(found.into_iter().map(|(k, v)| (k.to_lowercase(), v)));
    }
    Ok(result)
}

/// Version-ID → Version.
async fn fetch_versions(ids: &[String]) -> Result<HashMap<String, MrVersion>, String> {
    let client = crate::core::http::HTTP_CLIENT.clone();
    let mut result = HashMap::new();
    for chunk in ids.chunks(100) {
        let ids_json = serde_json::to_string(chunk).map_err(|e| e.to_string())?;
        let versions: Vec<MrVersion> = modrinth_json(|| {
            client
                .get(format!("{}/versions", MODRINTH_API))
                .query(&[("ids", ids_json.as_str())])
        })
        .await?;
        result.extend(versions.into_iter().map(|v| (v.id.clone(), v)));
    }
    Ok(result)
}

/// Projekt-ID/Slug → Projekt (Titel, Icon für die Metadaten).
async fn fetch_projects(ids: &[String]) -> HashMap<String, MrProject> {
    let client = crate::core::http::HTTP_CLIENT.clone();
    let mut result = HashMap::new();
    for chunk in ids.chunks(100) {
        let Ok(ids_json) = serde_json::to_string(chunk) else {
            continue;
        };
        let projects: Result<Vec<MrProject>, String> = modrinth_json(|| {
            client
                .get(format!("{}/projects", MODRINTH_API))
                .query(&[("ids", ids_json.as_str())])
        })
        .await;
        match projects {
            Ok(projects) => {
                for project in projects {
                    result.insert(project.slug.clone(), project.clone());
                    result.insert(project.id.clone(), project);
                }
            }
            Err(e) => tracing::warn!("Failed to fetch project metadata: {}", e),
        }
    }
    result
}

/// Neueste zur Profil-Version passende Version (Fallback für altes Format / gelöschte Versionen).
async fn find_compatible_version(
    project_id: &str,
    kind: ContentKind,
    mc_version: &str,
    loader: &str,
) -> Result<Option<MrVersion>, String> {
    let client = crate::core::http::HTTP_CLIENT.clone();
    let game_versions = serde_json::to_string(&[mc_version]).map_err(|e| e.to_string())?;
    let loaders = match kind {
        ContentKind::Mods if loader == "quilt" => Some(r#"["quilt","fabric"]"#.to_string()),
        ContentKind::Mods if loader != "vanilla" => Some(format!(r#"["{}"]"#, loader)),
        _ => None,
    };
    let url = format!(
        "{}/project/{}/version",
        MODRINTH_API,
        urlencoding::encode(project_id)
    );
    let resp = modrinth_send(|| {
        let mut req = client
            .get(&url)
            .query(&[("game_versions", game_versions.as_str())]);
        if let Some(loaders) = &loaders {
            req = req.query(&[("loaders", loaders.as_str())]);
        }
        req
    })
    .await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("Modrinth-Anfrage fehlgeschlagen: {}", resp.status()));
    }
    let versions: Vec<MrVersion> = resp.json().await.map_err(|e| e.to_string())?;
    Ok(versions.into_iter().next())
}

// ==================== Datei-Hilfen ====================

fn hash_file(path: &Path) -> std::io::Result<(String, String, u64)> {
    use sha1::Digest as _;
    let mut file = std::fs::File::open(path)?;
    let mut sha1 = sha1::Sha1::new();
    let mut sha512 = sha2::Sha512::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        sha1.update(&buffer[..read]);
        sha512.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((
        hex::encode(sha1.finalize()),
        hex::encode(sha512.finalize()),
        size,
    ))
}

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest as _;
    hex::encode(sha1::Sha1::digest(bytes))
}

/// Erlaubt nur einfache Dateinamen (kein Pfad, kein `..`) – die Namen stammen aus einer
/// fremden `.lion`-Datei bzw. der Modrinth-API.
fn safe_file_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '\0'])
        || name.chars().any(|c| c.is_control())
    {
        return None;
    }
    Some(name.to_string())
}

fn zip_folder(dir: &Path) -> anyhow::Result<Vec<u8>> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for entry in walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            let Ok(rel) = path.strip_prefix(dir) else {
                continue;
            };
            if rel.as_os_str().is_empty() {
                continue;
            }
            let rel_name = rel.to_string_lossy().replace('\\', "/");
            if path.is_dir() {
                zip.add_directory(rel_name, options)?;
            } else if path.is_file() {
                zip.start_file(rel_name, options)?;
                let mut f = std::fs::File::open(path)?;
                std::io::copy(&mut f, &mut zip)?;
            }
        }
        zip.finish()?;
    }
    Ok(cursor.into_inner())
}

fn unzip_into(bytes: &[u8], target_dir: &Path) -> anyhow::Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    std::fs::create_dir_all(target_dir)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        // enclosed_name verhindert Pfad-Traversal (../)
        let Some(rel) = file.enclosed_name().map(|p| p.to_owned()) else {
            continue;
        };
        let out_path = target_dir.join(rel);
        if file.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut buffer = Vec::new();
            file.read_to_end(&mut buffer)?;
            std::fs::File::create(&out_path)?.write_all(&buffer)?;
        }
    }
    Ok(())
}

/// Liest den lokal gespeicherten Anzeigenamen einer Datei aus den Metadaten.
fn local_display_name(game_dir: &Path, kind: ContentKind, filename: &str) -> Option<String> {
    let (meta_file, key) = match kind {
        ContentKind::Mods => (
            format!("{}.json", filename.trim_end_matches(".jar")),
            "mod_name",
        ),
        _ => (format!("{}.json", filename), "title"),
    };
    let content = std::fs::read_to_string(game_dir.join(kind.metadata_dir()).join(meta_file)).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&content).ok()?;
    meta.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}

async fn load_profile(profile_id: &str) -> Result<crate::types::profile::Profile, String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let profiles = manager.load_profiles().await.map_err(|e| e.to_string())?;
    profiles
        .get_profile(profile_id)
        .cloned()
        .ok_or_else(|| "Profile not found".to_string())
}

// ==================== Export ====================

struct LocalContentFile {
    filename: String,
    disabled: bool,
    path: PathBuf,
    is_folder: bool,
    sha1: Option<String>,
    sha512: Option<String>,
    size: u64,
    name: Option<String>,
}

fn scan_local_content(game_dir: &Path, kind: ContentKind) -> Result<Vec<LocalContentFile>, String> {
    let dir = game_dir.join(kind.as_str());
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let raw_name = entry.file_name().to_string_lossy().to_string();
        let lower = raw_name.to_lowercase();

        let (filename, disabled, is_folder) = match kind {
            ContentKind::Mods if path.is_file() && lower.ends_with(".jar") => (raw_name, false, false),
            ContentKind::Mods if path.is_file() && lower.ends_with(".jar.disabled") => (
                raw_name[..raw_name.len() - ".disabled".len()].to_string(),
                true,
                false,
            ),
            ContentKind::ResourcePacks | ContentKind::ShaderPacks
                if path.is_file() && lower.ends_with(".zip") =>
            {
                (raw_name, false, false)
            }
            ContentKind::ResourcePacks | ContentKind::ShaderPacks if path.is_dir() => {
                (raw_name, false, true)
            }
            _ => continue,
        };

        let (sha1, sha512, size) = if is_folder {
            (None, None, 0)
        } else {
            let (sha1, sha512, size) =
                hash_file(&path).map_err(|e| format!("{}: {}", filename, e))?;
            (Some(sha1), Some(sha512), size)
        };
        let name = local_display_name(game_dir, kind, &filename);

        files.push(LocalContentFile {
            filename,
            disabled,
            path,
            is_folder,
            sha1,
            sha512,
            size,
            name,
        });
    }
    files.sort_by_key(|f| f.filename.to_lowercase());
    Ok(files)
}

/// Erstellt die Inhaltsliste für den `.lion`-Export (exakte Versionen bzw. eingebettete Dateien).
///
/// `embed_missing = false`: Dateien, die es nicht auf Modrinth gibt, werden nur namentlich
/// aufgeführt (kleinere Datei; der Empfänger bekommt sie als "fehlt" gemeldet).
#[tauri::command]
pub async fn export_profile_content(
    profile_id: String,
    kind: String,
    embed_missing: Option<bool>,
) -> Result<Vec<SharedContentEntry>, String> {
    let kind = ContentKind::parse(&kind)?;
    let profile = load_profile(&profile_id).await?;
    export_content_from(&profile.game_dir, kind, embed_missing.unwrap_or(true)).await
}

async fn export_content_from(
    game_dir: &Path,
    kind: ContentKind,
    embed_missing: bool,
) -> Result<Vec<SharedContentEntry>, String> {
    let game_dir = game_dir.to_path_buf();
    let files = tokio::task::spawn_blocking(move || scan_local_content(&game_dir, kind))
        .await
        .map_err(|e| e.to_string())??;
    if files.is_empty() {
        return Ok(Vec::new());
    }

    let hashes: Vec<String> = files.iter().filter_map(|f| f.sha1.clone()).collect();
    let by_hash = lookup_versions_by_sha1(&hashes).await.map_err(|e| {
        format!(
            "Modrinth konnte nicht erreicht werden – Export abgebrochen, damit keine Versionen fehlen: {}",
            e
        )
    })?;

    let mut entries = Vec::with_capacity(files.len());
    let mut embedded = 0usize;
    for file in files {
        let version = file
            .sha1
            .as_ref()
            .and_then(|h| by_hash.get(&h.to_lowercase()));

        let mut entry = SharedContentEntry {
            filename: Some(file.filename.clone()),
            disabled: file.disabled,
            name: file.name.clone(),
            sha1: file.sha1.clone(),
            sha512: file.sha512.clone(),
            size: Some(file.size),
            is_folder: file.is_folder,
            ..Default::default()
        };

        if let Some(version) = version {
            entry.source = Some("modrinth".to_string());
            entry.project_id = Some(version.project_id.clone());
            entry.version_id = Some(version.id.clone());
            entry.version_number = Some(version.version_number.clone());
            if kind == ContentKind::Mods {
                // Ältere Launcher lesen `mod_id`
                entry.mod_id = Some(version.project_id.clone());
            }
        } else if !embed_missing {
            entry.source = Some("local".to_string());
        } else {
            // Nicht auf Modrinth → Datei direkt mitschicken, damit sie beim Empfänger nicht fehlt
            let path = file.path.clone();
            let is_folder = file.is_folder;
            let bytes = tokio::task::spawn_blocking(move || {
                if is_folder {
                    zip_folder(&path).map_err(|e| e.to_string())
                } else {
                    std::fs::read(&path).map_err(|e| e.to_string())
                }
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("{}: {}", file.filename, e))?;
            entry.source = Some("embedded".to_string());
            entry.data_base64 = Some(general_purpose::STANDARD.encode(bytes));
            embedded += 1;
        }
        entries.push(entry);
    }

    tracing::info!(
        "Exported {} {} ({} from Modrinth, {} embedded)",
        entries.len(),
        kind.as_str(),
        entries.len() - embedded,
        embedded
    );
    Ok(entries)
}

// ==================== Import ====================

enum ImportJob {
    /// Exakte (oder Fallback-)Version von Modrinth laden
    Modrinth {
        entry: SharedContentEntry,
        version: MrVersion,
        warning: Option<String>,
    },
    /// Neueste kompatible Version suchen (altes Format / Version nicht mehr verfügbar)
    Fallback {
        entry: SharedContentEntry,
        project_id: String,
        warn_exact_missing: bool,
    },
    /// In der `.lion`-Datei eingebettete Datei
    Embedded { entry: SharedContentEntry },
    Fail { name: String, reason: String },
}

struct ImportContext {
    kind: ContentKind,
    game_dir: PathBuf,
    target_dir: PathBuf,
    mc_version: String,
    loader: String,
    projects: HashMap<String, MrProject>,
    downloader: DownloadManager,
}

struct InstalledContent {
    project_id: Option<String>,
    warning: Option<String>,
}

fn final_file_name(kind: ContentKind, name: &str, disabled: bool) -> Option<String> {
    let name = safe_file_name(name)?;
    if !name.to_lowercase().ends_with(kind.file_extension()) {
        return None;
    }
    Some(if disabled && kind == ContentKind::Mods {
        format!("{}.disabled", name)
    } else {
        name
    })
}

async fn write_metadata(
    ctx: &ImportContext,
    filename: &str,
    project_id: Option<&str>,
    version: Option<(&str, &str)>,
    entry: &SharedContentEntry,
    sha1: Option<&str>,
) {
    let project = project_id.and_then(|id| ctx.projects.get(id));
    let title = project
        .map(|p| p.title.clone())
        .or_else(|| entry.name.clone().filter(|n| !n.trim().is_empty()));
    let icon_url = project.and_then(|p| p.icon_url.clone());
    let source = if project_id.is_some() { "modrinth" } else { "embedded" };
    let version_number = version
        .map(|(_, number)| number)
        .or(entry.version_number.as_deref());
    let version_id = version.map(|(id, _)| id);

    let (meta_name, payload) = match ctx.kind {
        ContentKind::Mods => (
            format!("{}.json", filename.trim_end_matches(".jar")),
            serde_json::json!({
                "mod_id": project_id,
                "mod_name": title,
                "icon_url": icon_url,
                "version": version_number,
                "version_id": version_id,
                "source": source,
                "filename": filename,
                "sha1": sha1,
            }),
        ),
        _ => (
            format!("{}.json", filename),
            serde_json::json!({
                "project_id": project_id,
                "slug": project.map(|p| p.slug.clone()).or_else(|| entry.slug.clone()),
                "title": title,
                "icon_url": icon_url,
                "filename": filename,
                "version_id": version_id,
            }),
        ),
    };

    // Ohne Projekt und ohne Namen bringen Metadaten nichts
    if project_id.is_none() && title.is_none() {
        return;
    }

    let meta_dir = ctx.game_dir.join(ctx.kind.metadata_dir());
    if let Err(e) = tokio::fs::create_dir_all(&meta_dir).await {
        tracing::warn!("Failed to create metadata dir: {}", e);
        return;
    }
    if let Ok(content) = serde_json::to_string_pretty(&payload) {
        if let Err(e) = tokio::fs::write(meta_dir.join(meta_name), content).await {
            tracing::warn!("Failed to write metadata for {}: {}", filename, e);
        }
    }
}

async fn install_modrinth_version(
    ctx: &ImportContext,
    entry: &SharedContentEntry,
    version: &MrVersion,
) -> Result<(), String> {
    // Exakt die Datei mit demselben Hash wählen, sonst die primäre Datei
    let wanted_sha1 = entry.sha1.as_deref().map(str::to_lowercase);
    let file = version
        .files
        .iter()
        .find(|f| {
            wanted_sha1.is_some()
                && f.hashes.sha1.as_deref().map(str::to_lowercase) == wanted_sha1
        })
        .or_else(|| version.files.iter().find(|f| f.primary))
        .or_else(|| version.files.first())
        .ok_or_else(|| "Version enthält keine Dateien".to_string())?;

    let filename = final_file_name(ctx.kind, &file.filename, entry.disabled)
        .ok_or_else(|| format!("Ungültiger Dateiname: {}", file.filename))?;
    let dest = ctx.target_dir.join(&filename);

    ctx.downloader
        .download_with_hash(&file.url, &dest, file.hashes.sha1.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    write_metadata(
        ctx,
        filename.trim_end_matches(".disabled"),
        Some(version.project_id.as_str()),
        Some((version.id.as_str(), version.version_number.as_str())),
        entry,
        file.hashes.sha1.as_deref(),
    )
    .await;
    Ok(())
}

async fn install_embedded(ctx: &ImportContext, entry: &SharedContentEntry) -> Result<(), String> {
    let data = entry
        .data_base64
        .as_deref()
        .ok_or_else(|| "Keine eingebetteten Daten".to_string())?;
    let bytes = general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|e| format!("Eingebettete Datei ist beschädigt: {}", e))?;

    let raw_name = entry
        .filename
        .as_deref()
        .ok_or_else(|| "Dateiname fehlt".to_string())?;

    if entry.is_folder {
        if ctx.kind == ContentKind::Mods {
            return Err("Ordner sind als Mod nicht erlaubt".to_string());
        }
        let folder = safe_file_name(raw_name)
            .ok_or_else(|| format!("Ungültiger Ordnername: {}", raw_name))?;
        let target = ctx.target_dir.join(&folder);
        tokio::task::spawn_blocking(move || unzip_into(&bytes, &target))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        write_metadata(ctx, &folder, entry.project(), None, entry, None).await;
        return Ok(());
    }

    let actual_sha1 = sha1_hex(&bytes);
    if let Some(expected) = entry.sha1.as_deref() {
        if !expected.eq_ignore_ascii_case(&actual_sha1) {
            return Err("Prüfsumme der eingebetteten Datei stimmt nicht".to_string());
        }
    }

    let filename = final_file_name(ctx.kind, raw_name, entry.disabled)
        .ok_or_else(|| format!("Ungültiger Dateiname: {}", raw_name))?;
    tokio::fs::write(ctx.target_dir.join(&filename), &bytes)
        .await
        .map_err(|e| e.to_string())?;

    write_metadata(
        ctx,
        filename.trim_end_matches(".disabled"),
        entry.project(),
        None,
        entry,
        Some(&actual_sha1),
    )
    .await;
    Ok(())
}

async fn run_import_job(ctx: &ImportContext, job: ImportJob) -> Result<InstalledContent, (String, String)> {
    match job {
        ImportJob::Fail { name, reason } => Err((name, reason)),
        ImportJob::Embedded { entry } => install_embedded(ctx, &entry)
            .await
            .map(|_| InstalledContent {
                project_id: entry.project().map(str::to_string),
                warning: None,
            })
            .map_err(|e| (entry.display_name(), e)),
        ImportJob::Modrinth {
            entry,
            version,
            warning,
        } => install_modrinth_version(ctx, &entry, &version)
            .await
            .map(|_| InstalledContent {
                project_id: Some(version.project_id.clone()),
                warning,
            })
            .map_err(|e| (entry.display_name(), e)),
        ImportJob::Fallback {
            entry,
            project_id,
            warn_exact_missing,
        } => {
            let name = entry.display_name();
            let version = find_compatible_version(&project_id, ctx.kind, &ctx.mc_version, &ctx.loader)
                .await
                .map_err(|e| (name.clone(), e))?
                .ok_or_else(|| {
                    (
                        name.clone(),
                        format!(
                            "Keine passende Version für Minecraft {} ({}) gefunden",
                            ctx.mc_version, ctx.loader
                        ),
                    )
                })?;
            install_modrinth_version(ctx, &entry, &version)
                .await
                .map_err(|e| (name.clone(), e))?;
            let warning = warn_exact_missing.then(|| {
                format!(
                    "{}: Version {} ist auf Modrinth nicht mehr verfügbar – {} installiert",
                    name,
                    entry.version_number.as_deref().unwrap_or("?"),
                    version.version_number
                )
            });
            Ok(InstalledContent {
                project_id: Some(version.project_id),
                warning,
            })
        }
    }
}

/// Installiert die Inhalte aus einer `.lion`-Datei in ein Profil – in exakt den
/// exportierten Versionen.
#[tauri::command]
pub async fn import_profile_content(
    app_handle: tauri::AppHandle,
    profile_id: String,
    kind: String,
    entries: Vec<SharedContentEntry>,
    parallel: Option<bool>,
) -> Result<ContentImportReport, String> {
    let kind = ContentKind::parse(&kind)?;
    let profile = load_profile(&profile_id).await?;
    let loader = profile.loader.loader.to_string().to_lowercase();

    let (report, installed_projects) = import_content_into(
        &profile.game_dir,
        &profile.minecraft_version,
        &loader,
        kind,
        entries,
        parallel.unwrap_or(true),
        |progress| {
            let _ = app_handle.emit("content-import-progress", progress);
        },
    )
    .await?;

    // Mod-Liste des Profils einmalig aktualisieren (keine konkurrierenden Schreibzugriffe)
    if kind == ContentKind::Mods && !installed_projects.is_empty() {
        let manager = ProfileManager::new().map_err(|e| e.to_string())?;
        let mut profiles = manager.load_profiles().await.map_err(|e| e.to_string())?;
        if let Some(profile) = profiles.get_profile_mut(&profile_id) {
            for project_id in installed_projects {
                profile.add_mod(project_id);
            }
            manager
                .save_profiles(&profiles)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(report)
}

/// Kern des Imports (ohne Tauri-Abhängigkeit, damit testbar).
/// Gibt den Bericht und die Projekt-IDs der installierten Inhalte zurück.
async fn import_content_into(
    game_dir: &Path,
    mc_version: &str,
    loader: &str,
    kind: ContentKind,
    entries: Vec<SharedContentEntry>,
    parallel: bool,
    mut on_progress: impl FnMut(ContentImportProgress),
) -> Result<(ContentImportReport, Vec<String>), String> {
    let target_dir = game_dir.join(kind.as_str());
    tokio::fs::create_dir_all(&target_dir)
        .await
        .map_err(|e| e.to_string())?;

    let total = entries.len();
    let mut report = ContentImportReport {
        total,
        ..Default::default()
    };
    if total == 0 {
        return Ok((report, Vec::new()));
    }

    // 1. Exakte Versionen gesammelt auflösen (wenige Bulk-Requests)
    let version_ids: Vec<String> = entries
        .iter()
        .filter(|e| e.data_base64.is_none())
        .filter_map(|e| e.version_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut versions = if version_ids.is_empty() {
        HashMap::new()
    } else {
        fetch_versions(&version_ids).await.unwrap_or_else(|e| {
            tracing::warn!("Bulk version lookup failed: {}", e);
            HashMap::new()
        })
    };

    // Einträge ohne auflösbare Version-ID, aber mit Hash → über den Hash suchen
    let missing_hashes: Vec<String> = entries
        .iter()
        .filter(|e| e.data_base64.is_none())
        .filter(|e| {
            e.version_id
                .as_ref()
                .map_or(true, |id| !versions.contains_key(id))
        })
        .filter_map(|e| e.sha1.as_ref().map(|h| h.to_lowercase()))
        .collect();
    let by_hash = if missing_hashes.is_empty() {
        HashMap::new()
    } else {
        lookup_versions_by_sha1(&missing_hashes)
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Hash lookup failed: {}", e);
                HashMap::new()
            })
    };
    for version in by_hash.values() {
        versions.insert(version.id.clone(), version.clone());
    }

    // 2. Jobs bestimmen
    let mut jobs = Vec::with_capacity(total);
    for entry in entries {
        if entry.data_base64.is_some() {
            jobs.push(ImportJob::Embedded { entry });
            continue;
        }
        let exact = entry
            .version_id
            .as_ref()
            .and_then(|id| versions.get(id))
            .or_else(|| {
                entry
                    .sha1
                    .as_ref()
                    .and_then(|h| by_hash.get(&h.to_lowercase()))
            })
            .cloned();
        if let Some(version) = exact {
            jobs.push(ImportJob::Modrinth {
                entry,
                version,
                warning: None,
            });
        } else if let Some(project_id) = entry.project().map(str::to_string) {
            let warn_exact_missing = entry.version_id.is_some() || entry.sha1.is_some();
            jobs.push(ImportJob::Fallback {
                entry,
                project_id,
                warn_exact_missing,
            });
        } else {
            jobs.push(ImportJob::Fail {
                name: entry.display_name(),
                reason: "Nicht auf Modrinth verfügbar und nicht in der .lion-Datei enthalten \
                         (beim Teilen \"Dateien einbetten\" aktivieren)"
                    .to_string(),
            });
        }
    }

    // 3. Projekt-Metadaten (Titel, Icon) gesammelt laden
    let project_ids: Vec<String> = jobs
        .iter()
        .filter_map(|job| match job {
            ImportJob::Modrinth { version, .. } => Some(version.project_id.clone()),
            ImportJob::Fallback { project_id, .. } => Some(project_id.clone()),
            ImportJob::Embedded { entry } => entry.project().map(str::to_string),
            ImportJob::Fail { .. } => None,
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let projects = fetch_projects(&project_ids).await;

    let ctx = ImportContext {
        kind,
        game_dir: game_dir.to_path_buf(),
        target_dir,
        mc_version: mc_version.to_string(),
        loader: loader.to_string(),
        projects,
        downloader: DownloadManager::new().map_err(|e| e.to_string())?,
    };

    // 4. Herunterladen / Schreiben (begrenzt parallel)
    use futures_util::stream::{self, StreamExt};
    let concurrency = if parallel { 8 } else { 3 };
    let ctx_ref = &ctx;
    let mut results = stream::iter(jobs)
        .map(|job| async move { run_import_job(ctx_ref, job).await })
        .buffer_unordered(concurrency);

    let mut installed_projects = Vec::new();
    let mut done = 0usize;
    while let Some(result) = results.next().await {
        done += 1;
        let name = match result {
            Ok(installed) => {
                report.installed += 1;
                if let Some(warning) = installed.warning {
                    report.warnings.push(warning);
                }
                if let Some(project_id) = installed.project_id {
                    installed_projects.push(project_id);
                }
                String::new()
            }
            Err((name, reason)) => {
                tracing::warn!("Import of {} failed: {}", name, reason);
                report.failed.push(ContentImportFailure {
                    name: name.clone(),
                    reason,
                });
                name
            }
        };
        on_progress(ContentImportProgress {
            kind: kind.as_str().to_string(),
            done,
            total,
            name,
        });
    }
    drop(results);

    tracing::info!(
        "Imported {}/{} {} ({} failed, {} warnings)",
        report.installed,
        report.total,
        kind.as_str(),
        report.failed.len(),
        report.warnings.len()
    );
    Ok((report, installed_projects))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsafe_file_names() {
        assert_eq!(safe_file_name("sodium.jar").as_deref(), Some("sodium.jar"));
        assert!(safe_file_name("../evil.jar").is_none());
        assert!(safe_file_name("..\\evil.jar").is_none());
        assert!(safe_file_name("C:evil.jar").is_none());
        assert!(safe_file_name("..").is_none());
        assert!(safe_file_name("").is_none());
    }

    #[test]
    fn final_file_name_enforces_extension_and_disabled_suffix() {
        assert_eq!(
            final_file_name(ContentKind::Mods, "jei.jar", true).as_deref(),
            Some("jei.jar.disabled")
        );
        assert_eq!(
            final_file_name(ContentKind::Mods, "jei.jar", false).as_deref(),
            Some("jei.jar")
        );
        assert!(final_file_name(ContentKind::Mods, "evil.exe", false).is_none());
        assert_eq!(
            final_file_name(ContentKind::ResourcePacks, "Faithful.zip", true).as_deref(),
            Some("Faithful.zip")
        );
    }

    #[test]
    fn old_format_entries_still_parse() {
        let old_mod: SharedContentEntry = serde_json::from_str(
            r#"{"mod_id":"AANobbMI","source":"modrinth","name":"Sodium","disabled":true}"#,
        )
        .unwrap();
        assert_eq!(old_mod.project(), Some("AANobbMI"));
        assert!(old_mod.disabled);

        let old_pack: SharedContentEntry =
            serde_json::from_str(r#"{"project_id":"abc","name":"Pack","slug":"pack"}"#).unwrap();
        assert_eq!(old_pack.project(), Some("abc"));
    }

    #[test]
    fn hashes_files() {
        let dir = std::env::temp_dir().join(format!("lion-hash-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.jar");
        std::fs::write(&path, b"hello").unwrap();
        let (sha1, sha512, size) = hash_file(&path).unwrap();
        assert_eq!(sha1, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
        assert_eq!(sha512.len(), 128);
        assert_eq!(size, 5);
        assert_eq!(sha1_hex(b"hello"), sha1);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Ende-zu-Ende: Export aus einem echten Profil → JSON (wie in der .lion-Datei) → Import in
    /// einen Temp-Ordner → jede Datei muss mit identischem Hash ankommen.
    ///
    /// `LION_E2E_GAME_DIR=<profil-ordner> LION_E2E_MC=1.21.1 LION_E2E_LOADER=neoforge \
    ///  cargo test e2e_export_import_roundtrip -- --ignored --nocapture`
    #[test]
    #[ignore = "benötigt Netzwerk und ein echtes Profil (LION_E2E_GAME_DIR)"]
    fn e2e_export_import_roundtrip() {
        let source = PathBuf::from(std::env::var("LION_E2E_GAME_DIR").expect("LION_E2E_GAME_DIR"));
        let mc = std::env::var("LION_E2E_MC").unwrap_or_else(|_| "1.21.1".into());
        let loader = std::env::var("LION_E2E_LOADER").unwrap_or_else(|_| "neoforge".into());
        let target = std::env::temp_dir().join(format!("lion-e2e-import-{}", std::process::id()));
        std::fs::remove_dir_all(&target).ok();

        let rt = tokio::runtime::Runtime::new().unwrap();
        for kind in [ContentKind::Mods, ContentKind::ResourcePacks, ContentKind::ShaderPacks] {
            let exported = rt.block_on(export_content_from(&source, kind, true)).unwrap();
            let embedded = exported.iter().filter(|e| e.data_base64.is_some()).count();
            let json = serde_json::to_string(&exported).unwrap();
            let entries: Vec<SharedContentEntry> = serde_json::from_str(&json).unwrap();

            let (report, _) = rt
                .block_on(import_content_into(&target, &mc, &loader, kind, entries, true, |_| {}))
                .unwrap();
            println!(
                "{}: exported {} ({} embedded, {} KB json) → installed {}/{}, failed {:?}, warnings {:?}",
                kind.as_str(),
                exported.len(),
                embedded,
                json.len() / 1024,
                report.installed,
                report.total,
                report.failed,
                report.warnings
            );

            let src_files = scan_local_content(&source, kind).unwrap();
            let dst_files = scan_local_content(&target, kind).unwrap();
            let fingerprint = |files: &[LocalContentFile]| {
                let mut v: Vec<(String, bool, Option<String>)> = files
                    .iter()
                    .map(|f| (f.filename.clone(), f.disabled, f.sha1.clone()))
                    .collect();
                v.sort();
                v
            };
            assert_eq!(src_files.len(), dst_files.len(), "{}: Anzahl unterschiedlich", kind.as_str());
            assert_eq!(fingerprint(&src_files), fingerprint(&dst_files), "{}: Dateien/Versionen unterschiedlich", kind.as_str());
            assert!(report.failed.is_empty());
        }
        std::fs::remove_dir_all(&target).ok();
    }

    #[test]
    fn folder_zip_roundtrip() {
        let base = std::env::temp_dir().join(format!("lion-zip-test-{}", std::process::id()));
        let src = base.join("src");
        std::fs::create_dir_all(src.join("assets")).unwrap();
        std::fs::write(src.join("pack.mcmeta"), b"{}").unwrap();
        std::fs::write(src.join("assets").join("x.txt"), b"x").unwrap();
        let bytes = zip_folder(&src).unwrap();
        let dst = base.join("dst");
        unzip_into(&bytes, &dst).unwrap();
        assert_eq!(std::fs::read(dst.join("pack.mcmeta")).unwrap(), b"{}");
        assert_eq!(std::fs::read(dst.join("assets").join("x.txt")).unwrap(), b"x");
        std::fs::remove_dir_all(&base).ok();
    }
}
