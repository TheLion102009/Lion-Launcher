//! Horizons Inhalte-Verzeichnis (`horizon-rust/horizon-mods`, `index.json` Schema 1).
//!
//! Angeboten wird nur, was der installierte Build lädt (`content` aus `--launch-api`):
//! Content-Mods (`.hmod`) mit passender Mod-API und nur der Fähigkeit `registries`, native
//! Mods (`.hnmod`, Rust-Quellcode – beim Start kompiliert, siehe [`super::native`]),
//! Resource Packs und Shader Packs.
//!
//! Vor der Installation wird jede Datei geprüft (`mod-index.md`, „Before a file is installed“):
//! Herkunft, Größe, SHA-256, Minisign-Signatur des Autors (falls er einen Schlüssel hat),
//! zurückgezogene Dateien und die ID im Manifest der Mod.

use crate::types::mod_info::{FileHashes, GalleryImage, ModFile, ModInfo, ModSource, ModVersion};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const INDEX_URL: &str = "https://horizon-rust.github.io/horizon-mods/index.json";
/// Dateien der Registry selbst.
const FILE_HOST: &str = "https://github.com/horizon-rust/horizon-mods/releases/download/";
const INDEX_SCHEMA: u64 = 1;
const INDEX_MAX_AGE: Duration = Duration::from_secs(10 * 60);
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Ordner und Schalter-Datei der Content-Mods im Spielordner.
pub const MODS_DIR: &str = "horizon-mods";
/// Ordner der nativen Mods (Rust-Quellcode, beim Start kompiliert).
pub const NATIVE_DIR: &str = "horizon-native-mods";
const MODS_SWITCHES: &str = "horizon-mods.txt";

static INDEX: Mutex<Option<(Instant, Arc<Index>)>> = Mutex::new(None);

#[derive(Debug, Default, Deserialize)]
pub struct Index {
    #[serde(default)]
    pub schema: u64,
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub revoked: Vec<Revoked>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Project {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    /// Minisign-Schlüssel des Autors: dann muss jede Version damit signiert sein
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub stars: Option<u64>,
    #[serde(default)]
    pub updated: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub screenshots: Vec<String>,
    #[serde(default)]
    pub versions: Vec<Version>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Version {
    pub version: String,
    #[serde(default)]
    pub published: Option<String>,
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub game: Option<String>,
    /// Native Mods: Anzahl der Hook-Punkte, die der Build mindestens bieten muss
    #[serde(default)]
    pub hooks: Option<u64>,
    pub file: VersionFile,
}

impl Version {
    /// Native Mods: Horizon-Commit, auf den die Version festgelegt ist (`native-1/<horizon>/<mixins>`).
    fn native_commit(&self) -> Option<&str> {
        self.api
            .as_deref()?
            .strip_prefix("native-1/")?
            .split('/')
            .next()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct VersionFile {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Revoked {
    pub id: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Inhaltsarten, wie sie der Content Browser kennt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Mod,
    ResourcePack,
    ShaderPack,
}

impl ContentKind {
    fn matches(self, kind: &str) -> bool {
        match self {
            ContentKind::Mod => kind == "mod" || kind == "native-mod",
            ContentKind::ResourcePack => kind == "resourcepack",
            ContentKind::ShaderPack => kind == "shader" || kind == "shaderpack",
        }
    }

    fn folder(self) -> &'static str {
        match self {
            ContentKind::Mod => MODS_DIR,
            ContentKind::ResourcePack => "resourcepacks",
            ContentKind::ShaderPack => "shaderpacks",
        }
    }

    /// Ordner der Lion-Metadaten (gleich wie bei Modrinth-Inhalten).
    fn metadata_folder(self) -> &'static str {
        match self {
            ContentKind::Mod => "modinfos",
            ContentKind::ResourcePack => "resourcepackinfos",
            ContentKind::ShaderPack => "shaderpackinfos",
        }
    }
}

/// Was der installierte Build lädt.
#[derive(Debug, Clone)]
pub struct BuildContent {
    pub minecraft: String,
    pub mod_api: Vec<String>,
    pub shader_api: Vec<String>,
    /// Hook-Punkte für native Mods
    pub native_hooks: Option<u64>,
    /// Kurzer Commit des Builds (aus seiner ID, z.B. `9f42064`)
    pub commit: Option<String>,
}

impl Default for BuildContent {
    /// Stand des Nightlys vom 2026-11-13 – gilt nur, bis ein Build installiert ist.
    fn default() -> Self {
        Self {
            minecraft: super::MINECRAFT_VERSION.to_string(),
            mod_api: vec!["0.2".into(), "0.3".into()],
            shader_api: vec!["0.1".into()],
            native_hooks: None,
            commit: None,
        }
    }
}

pub async fn build_content() -> BuildContent {
    let Some(id) = super::build::installed_builds().into_iter().next() else {
        return BuildContent::default();
    };
    match super::build::client_api(&id).await {
        Ok(api) => {
            let defaults = BuildContent::default();
            BuildContent {
                minecraft: if api.minecraft.is_empty() {
                    defaults.minecraft
                } else {
                    api.minecraft.clone()
                },
                mod_api: api.content_list("mod", "api"),
                shader_api: api.content_list("shader", "api"),
                native_hooks: api.content["native_mod"]["hooks"].as_u64(),
                commit: id.rsplit('-').next().map(String::from),
            }
        }
        Err(_) => BuildContent::default(),
    }
}

fn index_cache_file() -> PathBuf {
    super::build::horizon_dir().join("index.json")
}

fn parse_index(bytes: &[u8]) -> Result<Index, String> {
    let index: Index =
        serde_json::from_slice(bytes).map_err(|e| format!("Ungültiger Horizon-Index: {e}"))?;
    if index.schema > INDEX_SCHEMA {
        return Err(
            "Der Horizon-Index ist neuer als dieser Launcher – bitte Lion Launcher aktualisieren"
                .into(),
        );
    }
    Ok(index)
}

/// Fehlermeldung, wenn Horizons Server (GitHub) nicht antworten.
fn unreachable_message(error: &reqwest::Error) -> String {
    match error.status() {
        Some(status) if status.as_u16() == 404 => {
            "Horizons Mod-Verzeichnis ist gerade nicht erreichbar (GitHub meldet 404 – das Horizon-Projekt ist dort zurzeit nicht verfügbar)".into()
        }
        Some(status) => format!("Horizons Mod-Verzeichnis antwortet nicht ({status})"),
        None => format!("Horizons Mod-Verzeichnis ist nicht erreichbar: {error}"),
    }
}

/// Index laden (höchstens alle zehn Minuten neu; offline die letzte gespeicherte Kopie).
pub async fn index() -> Result<Arc<Index>, String> {
    if let Some((loaded, index)) = INDEX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if loaded.elapsed() < INDEX_MAX_AGE {
            return Ok(index.clone());
        }
    }
    let fetched = crate::core::http::HTTP_CLIENT
        .get(INDEX_URL)
        .send()
        .await
        .and_then(|r| r.error_for_status());
    let index = match fetched {
        Ok(response) => {
            let bytes = response.bytes().await.map_err(|e| e.to_string())?;
            let index = parse_index(&bytes)?;
            if let Some(parent) = index_cache_file().parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(index_cache_file(), &bytes).ok();
            index
        }
        Err(e) => {
            let cached = std::fs::read(index_cache_file()).map_err(|_| unreachable_message(&e))?;
            parse_index(&cached)?
        }
    };
    let index = Arc::new(index);
    *INDEX.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), index.clone()));
    Ok(index)
}

impl Index {
    fn is_revoked(&self, id: &str, sha256: &str) -> Option<String> {
        self.revoked
            .iter()
            .find(|r| {
                r.id == id
                    && r.sha256
                        .as_deref()
                        .is_none_or(|s| s.eq_ignore_ascii_case(sha256))
            })
            .map(|r| r.reason.clone().unwrap_or_else(|| "zurückgezogen".into()))
    }

    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }
}

/// Läuft diese Version auf dem Build? (`content.md`, „What a build loads“)
fn version_runs(project: &Project, version: &Version, content: &BuildContent) -> bool {
    if version
        .game
        .as_deref()
        .is_some_and(|g| g != content.minecraft)
    {
        return false;
    }
    let api = version.api.as_deref().unwrap_or_default();
    match project.kind.as_str() {
        // Content-Mods mit anderen Fähigkeiten als `registries` installiert auch der
        // Horizon-Launcher nicht einzeln
        "mod" => {
            content.mod_api.iter().any(|a| a == api)
                && version.capabilities.iter().all(|c| c == "registries")
        }
        "shader" => content.shader_api.iter().any(|a| a == api),
        "resourcepack" | "shaderpack" => true,
        // Ob eine Version für einen anderen Build passt, prüft der Bau selbst; vorher lässt sich
        // nur ausschließen, was mehr Hook-Punkte braucht, als der Build hat
        "native-mod" => {
            version.native_commit().is_some()
                && match (version.hooks, content.native_hooks) {
                    (Some(needed), Some(offered)) => needed <= offered,
                    _ => true,
                }
        }
        _ => false,
    }
}

/// Native Mods: zuerst die Version, die genau für diesen Build gemacht ist.
fn preferred_version<'a>(versions: &[&'a Version], content: &BuildContent) -> Option<&'a Version> {
    let exact = content.commit.as_deref().and_then(|commit| {
        versions
            .iter()
            .find(|v| v.native_commit().is_some_and(|c| c.starts_with(commit)))
    });
    exact.or(versions.first()).copied()
}

/// Kategorien eines Projekts; native Mods tragen zusätzlich `native` (zum Filtern und als Hinweis).
fn project_categories(project: &Project) -> Vec<String> {
    let mut categories = project.categories.clone();
    if project.kind == "native-mod" {
        categories.push("native".into());
    }
    categories
}

/// Lauffähige Versionen eines Projekts, neueste zuerst (wie im Index).
fn runnable_versions<'a>(
    index: &Index,
    project: &'a Project,
    content: &BuildContent,
) -> Vec<&'a Version> {
    project
        .versions
        .iter()
        .filter(|v| version_runs(project, v, content))
        .filter(|v| index.is_revoked(&project.id, &v.file.sha256).is_none())
        .collect()
}

fn to_mod_info(project: &Project, versions: &[&Version]) -> ModInfo {
    let source = project.source.clone();
    ModInfo {
        id: project.id.clone(),
        slug: project.id.clone(),
        name: if project.name.is_empty() {
            project.id.clone()
        } else {
            project.name.clone()
        },
        description: project.summary.clone(),
        body: Some(project.description.clone()),
        icon_url: project.icon.clone(),
        author: project.authors.join(", "),
        downloads: project.downloads,
        followers: project.stars,
        categories: project_categories(project),
        source: ModSource::Horizon,
        versions: versions.iter().map(|v| v.version.clone()).collect(),
        game_versions: versions
            .iter()
            .filter_map(|v| v.game.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect(),
        loaders: vec!["horizon".into()],
        project_url: format!("https://playhorizon.dev/mods/{}", project.id),
        updated_at: project.updated.clone().unwrap_or_default(),
        client_side: None,
        server_side: None,
        source_url: source,
        issues_url: None,
        wiki_url: None,
        discord_url: None,
        gallery: project
            .screenshots
            .iter()
            .map(|url| GalleryImage {
                url: url.clone(),
                raw_url: Some(url.clone()),
                title: None,
                description: None,
            })
            .collect(),
        project_type: Some(
            if project.kind == "shaderpack" {
                "shader"
            } else {
                project.kind.as_str()
            }
            .to_string(),
        ),
    }
}

fn to_mod_version(project: &Project, version: &Version) -> ModVersion {
    ModVersion {
        id: version.version.clone(),
        mod_id: project.id.clone(),
        name: version.version.clone(),
        version_number: version.version.clone(),
        game_versions: version.game.clone().into_iter().collect(),
        loaders: vec!["horizon".into()],
        files: vec![ModFile {
            url: version.file.url.clone(),
            filename: version.file.name.clone(),
            primary: true,
            size: version.file.size,
            hashes: FileHashes {
                sha1: None,
                sha512: None,
            },
        }],
        dependencies: Vec::new(),
        published: version.published.clone().unwrap_or_default(),
        version_type: Some("release".into()),
        downloads: None,
    }
}

/// Suche wie im Content Browser: Suchbegriff (Name, ID, Zusammenfassung, Tags), Kategorien
/// (alle gewählten müssen passen), Sortierung und Seiten.
pub async fn search(
    kind: ContentKind,
    query: &str,
    categories: &[String],
    sort_by: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<Vec<ModInfo>, String> {
    let index = index().await?;
    let content = build_content().await;
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();

    let mut hits: Vec<(i64, &Project, Vec<&Version>)> = index
        .projects
        .iter()
        .filter(|p| kind.matches(&p.kind))
        .filter(|p| {
            let own = project_categories(p);
            categories.iter().all(|c| own.contains(c))
        })
        // Ganz zurückgezogene Projekte (Eintrag ohne Prüfsumme)
        .filter(|p| index.is_revoked(&p.id, "").is_none())
        .filter_map(|p| {
            let versions = runnable_versions(&index, p, &content);
            if versions.is_empty() {
                return None;
            }
            let haystack = format!(
                "{} {} {} {}",
                p.name.to_lowercase(),
                p.id,
                p.summary.to_lowercase(),
                p.tags.join(" ")
            );
            if !words.iter().all(|w| haystack.contains(w.as_str())) {
                return None;
            }
            let name = p.name.to_lowercase();
            let score = words
                .iter()
                .map(|w| if name.contains(w.as_str()) { 10 } else { 1 })
                .sum::<i64>();
            Some((score, p, versions))
        })
        .collect();

    match sort_by {
        Some("updated") => hits.sort_by(|a, b| b.1.updated.cmp(&a.1.updated)),
        Some("newest") => hits.sort_by(|a, b| {
            let first = |p: &Project| p.versions.last().and_then(|v| v.published.clone());
            first(b.1).cmp(&first(a.1))
        }),
        Some("downloads") => hits.sort_by_key(|a| std::cmp::Reverse(a.1.downloads)),
        _ if !words.is_empty() => {
            hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.downloads.cmp(&a.1.downloads)))
        }
        _ => hits.sort_by_key(|a| std::cmp::Reverse(a.1.downloads)),
    }

    Ok(hits
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|(_, p, versions)| to_mod_info(p, &versions))
        .collect())
}

pub async fn project_info(id: &str) -> Result<ModInfo, String> {
    let index = index().await?;
    let content = build_content().await;
    let project = index
        .project(id)
        .ok_or("Projekt nicht im Horizon-Verzeichnis")?;
    Ok(to_mod_info(
        project,
        &runnable_versions(&index, project, &content),
    ))
}

pub async fn project_versions(id: &str) -> Result<Vec<ModVersion>, String> {
    let index = index().await?;
    let content = build_content().await;
    let project = index
        .project(id)
        .ok_or("Projekt nicht im Horizon-Verzeichnis")?;
    Ok(runnable_versions(&index, project, &content)
        .into_iter()
        .map(|v| to_mod_version(project, v))
        .collect())
}

/// Kategorien einer Inhaltsart (wie Modrinths `/tag/category`, für die Filter-Leiste).
pub async fn categories(kind: ContentKind) -> Result<Vec<String>, String> {
    let index = index().await?;
    let mut categories: Vec<String> = index
        .projects
        .iter()
        .filter(|p| kind.matches(&p.kind))
        .flat_map(project_categories)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    categories.sort();
    Ok(categories)
}

// ── Prüfen und Installieren ──────────────────────────────────────────────────

/// Nur Release-Dateien der Registry oder des eigenen GitHub-Repos des Projekts.
fn allowed_url(url: &str) -> bool {
    if url.starts_with(FILE_HOST) {
        return true;
    }
    let Some(rest) = url.strip_prefix("https://github.com/") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('/').collect();
    parts.len() == 6
        && parts[2] == "releases"
        && parts[3] == "download"
        && parts.iter().all(|p| !p.is_empty() && *p != "..")
}

fn allowed_file_name(name: &str, kind: &str) -> bool {
    let extensions: &[&str] = match kind {
        "mod" => &[".hmod"],
        // Native Mods: früher .hnmod, inzwischen ebenfalls .hmod
        "native-mod" => &[".hmod", ".hnmod"],
        "shader" => &[".hshader"],
        "resourcepack" | "shaderpack" => &[".zip"],
        _ => return false,
    };
    !name.is_empty()
        && name.len() <= 128
        && extensions.iter().any(|ext| name.ends_with(ext))
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
}

/// `id = "…"` aus `horizon-mod.toml` (ohne vollständigen TOML-Parser: nur Schlüssel auf
/// oberster Ebene vor der ersten Tabelle).
fn manifest_value(manifest: &str, key: &str) -> Option<String> {
    manifest
        .lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .find_map(|line| {
            let (k, v) = line.split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
        })
}

/// Was ein Mod-Paket über sich sagt: ID und ob es nativer Rust-Code ist.
struct PackageInfo {
    id: Option<String>,
    native: bool,
}

fn read_entry<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut zip.by_name(name).ok()?, &mut text).ok()?;
    Some(text)
}

/// Liest ein Paket: nativ ist es mit einer `[native]`-Tabelle in `horizon-mod.toml` oder im
/// alten Format (`horizon-native.json`). WebAssembly-Mods (`[wasm]`) enthalten zwar auch ein
/// Rust-Projekt, laufen aber fertig kompiliert direkt aus `horizon-mods/`.
fn package_info<R: std::io::Read + std::io::Seek>(reader: R) -> Option<PackageInfo> {
    let mut zip = zip::ZipArchive::new(reader).ok()?;
    if let Some(text) = read_entry(&mut zip, "horizon-native.json") {
        let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
        return Some(PackageInfo {
            id: manifest["id"].as_str().map(String::from),
            native: true,
        });
    }
    let manifest = read_entry(&mut zip, "horizon-mod.toml")?;
    let native = manifest
        .lines()
        .any(|line| line.trim_start().starts_with("[native"));
    Some(PackageInfo {
        id: manifest_value(&manifest, "id"),
        native,
    })
}

fn package_info_of_file(path: &Path) -> Option<PackageInfo> {
    package_info(std::fs::File::open(path).ok()?)
}

/// Natives Paket: muss nativ sein und die ID des Eintrags tragen.
fn check_native_manifest(bytes: &[u8], id: &str) -> Result<(), String> {
    let info = package_info(std::io::Cursor::new(bytes))
        .ok_or_else(|| "Die Mod-Datei ist kein gültiges Paket".to_string())?;
    if !info.native {
        return Err("Das Paket enthält keinen nativen Mod".into());
    }
    match info.id {
        Some(found) if found == id => Ok(()),
        found => Err(format!(
            "Die Mod-Datei passt nicht zum Eintrag (ID {:?} statt {id})",
            found.unwrap_or_default()
        )),
    }
}

fn check_mod_manifest(bytes: &[u8], id: &str) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|_| "Die Mod-Datei ist kein gültiges Paket".to_string())?;
    let mut manifest = String::new();
    std::io::Read::read_to_string(
        &mut zip
            .by_name("horizon-mod.toml")
            .map_err(|_| "Der Mod fehlt horizon-mod.toml".to_string())?,
        &mut manifest,
    )
    .map_err(|e| e.to_string())?;
    if manifest
        .lines()
        .any(|line| line.trim_start().starts_with("[native"))
    {
        return Err(
            "Das Paket ist ein nativer Mod, im Verzeichnis aber als Content-Mod eingetragen".into(),
        );
    }
    match manifest_value(&manifest, "id") {
        Some(found) if found == id => Ok(()),
        found => Err(format!(
            "Die Mod-Datei passt nicht zum Eintrag (ID {:?} statt {id})",
            found.unwrap_or_default()
        )),
    }
}

/// Lädt eine Version und prüft sie vollständig, bevor sie den Spielordner erreicht.
async fn download_checked(
    index: &Index,
    project: &Project,
    version: &Version,
) -> Result<Vec<u8>, String> {
    let file = &version.file;
    if let Some(reason) = index.is_revoked(&project.id, &file.sha256) {
        return Err(format!("Diese Datei wurde zurückgezogen: {reason}"));
    }
    if !allowed_url(&file.url) {
        return Err("Die Datei liegt nicht an einer erlaubten Adresse".into());
    }
    if !allowed_file_name(&file.name, &project.kind) {
        return Err(format!("Ungültiger Dateiname: {}", file.name));
    }
    if file.size > MAX_FILE_BYTES {
        return Err("Die Datei ist größer als 64 MiB".into());
    }
    let bytes = reqwest::Client::builder()
        .user_agent(concat!("Lion-Launcher/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?
        .get(&file.url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Download fehlgeschlagen: {}", unreachable_message(&e)))?
        .bytes()
        .await
        .map_err(|e| format!("Download fehlgeschlagen: {e}"))?;
    if bytes.len() as u64 != file.size {
        return Err("Die Dateigröße stimmt nicht mit dem Verzeichnis überein".into());
    }
    if !hex::encode(Sha256::digest(&bytes)).eq_ignore_ascii_case(&file.sha256) {
        return Err("Die Prüfsumme (SHA-256) stimmt nicht – Installation abgebrochen".into());
    }
    if let Some(key) = project.key.as_deref() {
        let signature = file
            .signature
            .as_deref()
            .ok_or("Die Version ist nicht signiert, obwohl das Projekt einen Schlüssel hat")?;
        let key = minisign_verify::PublicKey::from_base64(key.trim())
            .map_err(|_| "Ungültiger Schlüssel im Verzeichnis".to_string())?;
        let signature = minisign_verify::Signature::decode(signature)
            .map_err(|_| "Ungültige Signatur im Verzeichnis".to_string())?;
        key.verify(&bytes, &signature, true).map_err(|_| {
            "Die Signatur des Autors ist ungültig – Installation abgebrochen".to_string()
        })?;
    }
    match project.kind.as_str() {
        "mod" => check_mod_manifest(&bytes, &project.id)?,
        "native-mod" => check_native_manifest(&bytes, &project.id)?,
        _ => {}
    }
    Ok(bytes.to_vec())
}

fn content_kind_of(project: &Project) -> Option<ContentKind> {
    match project.kind.as_str() {
        "mod" | "native-mod" => Some(ContentKind::Mod),
        "resourcepack" => Some(ContentKind::ResourcePack),
        "shader" | "shaderpack" => Some(ContentKind::ShaderPack),
        _ => None,
    }
}

/// Dateiname im Spielordner: Mods heißen wie ihre ID (`horizon-mods/<id>.hmod`,
/// native in `horizon-native-mods/<id>.hmod` – wie im Horizon-Launcher).
fn target_file_name(project: &Project, version: &Version) -> String {
    match project.kind.as_str() {
        "mod" => format!("{}.hmod", project.id),
        "native-mod" if version.file.name.ends_with(".hnmod") => format!("{}.hnmod", project.id),
        "native-mod" => format!("{}.hmod", project.id),
        _ => version.file.name.clone(),
    }
}

/// Ordner für eine neue Datei: native Mods getrennt von Content-Mods.
fn folder_for_project(kind: ContentKind, project: &Project) -> &'static str {
    if project.kind == "native-mod" {
        NATIVE_DIR
    } else {
        kind.folder()
    }
}

/// Ordner, in dem eine installierte Mod-Datei liegt (native zuerst).
fn mod_folder_of(game_dir: &Path, file_name: &str) -> &'static str {
    if game_dir.join(NATIVE_DIR).join(file_name).is_file() {
        NATIVE_DIR
    } else {
        MODS_DIR
    }
}

/// Native Mods, die (z.B. von einer älteren Version oder von Hand) in `horizon-mods/` gelandet
/// sind, nach `horizon-native-mods/` verschieben – das Spiel lädt dort nur Content-Mods.
pub fn move_misplaced_native_mods(game_dir: &Path) {
    let content_dir = game_dir.join(MODS_DIR);
    for file_name in mod_files(&content_dir, ".hmod") {
        let path = content_dir.join(&file_name);
        if !package_info_of_file(&path).is_some_and(|info| info.native) {
            continue;
        }
        let native_dir = game_dir.join(NATIVE_DIR);
        if std::fs::create_dir_all(&native_dir).is_ok() {
            let target = native_dir.join(&file_name);
            std::fs::remove_file(&target).ok();
            if std::fs::rename(&path, &target).is_ok() {
                tracing::info!("Moved native Horizon mod {} to {}", file_name, NATIVE_DIR);
            }
        }
    }
}

/// Ist das Projekt ein nativer Mod (Rust-Quellcode, läuft ohne Sandbox)?
pub async fn is_native(project_id: &str) -> bool {
    index()
        .await
        .ok()
        .and_then(|index| index.project(project_id).map(|p| p.kind == "native-mod"))
        .unwrap_or(false)
}

/// Installiert ein Projekt (bestimmte Version oder die neueste lauffähige) in ein Profil.
pub async fn install(
    game_dir: &Path,
    project_id: &str,
    version: Option<&str>,
) -> Result<String, String> {
    let index = index().await?;
    let content = build_content().await;
    let project = index
        .project(project_id)
        .ok_or("Projekt nicht im Horizon-Verzeichnis")?;
    let kind =
        content_kind_of(project).ok_or("Diese Inhaltsart kann Lion Launcher nicht installieren")?;
    let versions = runnable_versions(&index, project, &content);
    let chosen = match version {
        Some(wanted) => versions.iter().find(|v| v.version == wanted).copied(),
        None => preferred_version(&versions, &content),
    }
    .ok_or("Keine Version läuft auf dem installierten Horizon-Build")?;

    let bytes = download_checked(&index, project, chosen).await?;
    let file_name = target_file_name(project, chosen);
    let folder = game_dir.join(folder_for_project(kind, project));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;

    // Ältere Version desselben Projekts entfernen
    if let Some(old) = installed_file_of(game_dir, kind, &project.id) {
        let old_folder = if kind == ContentKind::Mod {
            mod_folder_of(game_dir, &old)
        } else {
            kind.folder()
        };
        let old_path = game_dir.join(old_folder).join(&old);
        if old_path.is_file() {
            std::fs::remove_file(&old_path).ok();
        }
        std::fs::remove_file(metadata_path(game_dir, kind, &old)).ok();
    }

    let target = folder.join(&file_name);
    let part = folder.join(format!(".{file_name}.part"));
    std::fs::write(&part, &bytes).map_err(|e| e.to_string())?;
    std::fs::remove_file(&target).ok();
    std::fs::rename(&part, &target).map_err(|e| e.to_string())?;

    write_metadata(game_dir, kind, project, chosen, &file_name)?;
    if kind == ContentKind::Mod {
        set_mod_enabled(game_dir, &project.id, true)?;
    }
    Ok(file_name)
}

fn metadata_path(game_dir: &Path, kind: ContentKind, file_name: &str) -> PathBuf {
    let base = match kind {
        // Wie bei Java-Mods: `foo.hmod` → `modinfos/foo.json`
        ContentKind::Mod => mod_id_of(file_name).to_string(),
        _ => file_name.to_string(),
    };
    game_dir
        .join(kind.metadata_folder())
        .join(format!("{base}.json"))
}

fn write_metadata(
    game_dir: &Path,
    kind: ContentKind,
    project: &Project,
    version: &Version,
    file_name: &str,
) -> Result<(), String> {
    let path = metadata_path(game_dir, kind, file_name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let name = if project.name.is_empty() {
        &project.id
    } else {
        &project.name
    };
    let metadata = match kind {
        ContentKind::Mod => serde_json::json!({
            "mod_id": project.id,
            "mod_name": name,
            "icon_url": project.icon,
            "version": version.version,
            "version_id": version.version,
            "source": "horizon",
            "filename": file_name,
            "native": project.kind == "native-mod",
        }),
        _ => serde_json::json!({
            "project_id": project.id,
            "slug": project.id,
            "title": name,
            "icon_url": project.icon,
            "filename": file_name,
            "version_id": version.version,
            "source": "horizon",
        }),
    };
    std::fs::write(
        path,
        serde_json::to_string_pretty(&metadata).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Datei eines installierten Projekts (über die Lion-Metadaten).
fn installed_file_of(game_dir: &Path, kind: ContentKind, project_id: &str) -> Option<String> {
    let id_key = if kind == ContentKind::Mod {
        "mod_id"
    } else {
        "project_id"
    };
    std::fs::read_dir(game_dir.join(kind.metadata_folder()))
        .ok()?
        .filter_map(|e| e.ok())
        .find_map(|entry| {
            let meta: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(entry.path()).ok()?).ok()?;
            (meta["source"] == "horizon" && meta[id_key] == project_id)
                .then(|| meta["filename"].as_str().map(String::from))
                .flatten()
        })
}

// ── Content-Mods im Spielordner ──────────────────────────────────────────────

/// `horizon-mods.txt`: eine Zeile `mod.<id>:true` schaltet eine Mod ein; ohne Zeile ist sie aus.
fn read_switches(game_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(game_dir.join(MODS_SWITCHES))
        .map(|s| s.lines().map(String::from).collect())
        .unwrap_or_default()
}

pub fn is_mod_enabled(game_dir: &Path, id: &str) -> bool {
    let wanted = format!("mod.{id}:true");
    read_switches(game_dir).iter().any(|l| l.trim() == wanted)
}

pub fn set_mod_enabled(game_dir: &Path, id: &str, enabled: bool) -> Result<(), String> {
    let prefix = format!("mod.{id}:");
    let mut lines: Vec<String> = read_switches(game_dir)
        .into_iter()
        .filter(|l| !l.trim().starts_with(&prefix))
        .collect();
    lines.push(format!("{prefix}{enabled}"));
    std::fs::write(game_dir.join(MODS_SWITCHES), lines.join("\n") + "\n").map_err(|e| e.to_string())
}

fn remove_switch(game_dir: &Path, id: &str) {
    let prefix = format!("mod.{id}:");
    let lines: Vec<String> = read_switches(game_dir)
        .into_iter()
        .filter(|l| !l.trim().starts_with(&prefix))
        .collect();
    std::fs::write(game_dir.join(MODS_SWITCHES), lines.join("\n") + "\n").ok();
}

/// Mod-ID einer Datei in `horizon-mods/` oder `horizon-native-mods/` (Name ohne Endung).
pub fn mod_id_of(file_name: &str) -> &str {
    file_name
        .strip_suffix(".hnmod")
        .unwrap_or_else(|| file_name.trim_end_matches(".hmod"))
}

pub struct InstalledMod {
    pub file_name: String,
    pub id: String,
    pub enabled: bool,
    /// Nativer Mod (wird beim Start kompiliert)
    pub native: bool,
    pub metadata: Option<serde_json::Value>,
}

fn mod_files(dir: &Path, extension: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|name| name.ends_with(extension) && !name.starts_with('.'))
                .collect()
        })
        .unwrap_or_default()
}

pub fn installed_mods(game_dir: &Path) -> Vec<InstalledMod> {
    move_misplaced_native_mods(game_dir);
    let content = mod_files(&game_dir.join(MODS_DIR), ".hmod")
        .into_iter()
        .map(|f| (f, false));
    let native_dir = game_dir.join(NATIVE_DIR);
    let native = mod_files(&native_dir, ".hmod")
        .into_iter()
        .chain(mod_files(&native_dir, ".hnmod"))
        .map(|f| (f, true));
    let mut mods: Vec<InstalledMod> = content
        .chain(native)
        .map(|(file_name, native)| {
            let id = mod_id_of(&file_name).to_string();
            let metadata =
                std::fs::read_to_string(metadata_path(game_dir, ContentKind::Mod, &file_name))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok());
            InstalledMod {
                enabled: is_mod_enabled(game_dir, &id),
                file_name,
                id,
                native,
                metadata,
            }
        })
        .collect();
    mods.sort_by_key(|m| m.id.clone());
    mods
}

/// Eingeschaltete native Mods (Dateien in `horizon-native-mods/`).
pub fn enabled_native_mods(game_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = installed_mods(game_dir)
        .into_iter()
        .filter(|m| m.native && m.enabled)
        .map(|m| game_dir.join(NATIVE_DIR).join(m.file_name))
        .collect();
    files.sort();
    files
}

pub fn delete_mod(game_dir: &Path, file_name: &str) -> Result<(), String> {
    let path = game_dir
        .join(mod_folder_of(game_dir, file_name))
        .join(file_name);
    if !path.is_file() {
        return Err(format!("Mod-Datei nicht gefunden: {file_name}"));
    }
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    std::fs::remove_file(metadata_path(game_dir, ContentKind::Mod, file_name)).ok();
    remove_switch(game_dir, mod_id_of(file_name));
    Ok(())
}

/// Update-Status installierter Mods: (Dateiname, Projekt, installierte Version, neueste Version).
pub struct ModUpdate {
    pub file_name: String,
    pub project_id: Option<String>,
    pub name: Option<String>,
    pub icon_url: Option<String>,
    pub current: Option<String>,
    pub latest: Option<String>,
}

pub async fn mod_updates(game_dir: &Path, files: &[String]) -> Result<Vec<ModUpdate>, String> {
    let index = index().await?;
    let content = build_content().await;
    Ok(installed_mods(game_dir)
        .into_iter()
        .filter(|m| files.is_empty() || files.contains(&m.file_name))
        .map(|m| {
            let project = index
                .project(&m.id)
                .filter(|p| p.kind == "mod" || p.kind == "native-mod");
            let current = m
                .metadata
                .as_ref()
                .and_then(|meta| meta["version"].as_str().map(String::from));
            let latest = project.and_then(|p| {
                preferred_version(&runnable_versions(&index, p, &content), &content)
                    .map(|v| v.version.clone())
            });
            ModUpdate {
                file_name: m.file_name,
                project_id: project.map(|p| p.id.clone()),
                name: project.map(|p| p.name.clone()),
                icon_url: project.and_then(|p| p.icon.clone()),
                current,
                latest,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(kind: &str, versions: Vec<Version>) -> Project {
        Project {
            id: "coords_hud".into(),
            kind: kind.into(),
            name: "Coordinates HUD".into(),
            versions,
            ..Default::default()
        }
    }

    fn version(api: &str, capabilities: &[&str], sha: &str) -> Version {
        Version {
            version: "0.1.0".into(),
            api: Some(api.into()),
            capabilities: capabilities.iter().map(|c| c.to_string()).collect(),
            game: Some("26.3".into()),
            file: VersionFile {
                name: "coords_hud-0.1.0.hmod".into(),
                url: format!("{FILE_HOST}coords_hud-0.1.0/coords_hud-0.1.0.hmod"),
                size: 1,
                sha256: sha.into(),
                signature: None,
            },
            ..Default::default()
        }
    }

    #[test]
    fn offers_only_what_the_build_loads() {
        let content = BuildContent::default();
        let ok = project("mod", vec![version("0.2", &["registries"], "aa")]);
        assert!(version_runs(&ok, &ok.versions[0], &content));
        let other_api = project("mod", vec![version("0.1", &["registries"], "aa")]);
        assert!(!version_runs(&other_api, &other_api.versions[0], &content));
        let capable = project("mod", vec![version("0.2", &["registries", "chat"], "aa")]);
        assert!(!version_runs(&capable, &capable.versions[0], &content));
        let native = project("native-mod", vec![version("native-1/a/b", &[], "aa")]);
        assert!(version_runs(&native, &native.versions[0], &content));
        let hungry = Version {
            hooks: Some(999),
            ..version("native-1/a/b", &[], "aa")
        };
        let limited = BuildContent {
            native_hooks: Some(415),
            ..BuildContent::default()
        };
        assert!(!version_runs(&native, &hungry, &limited));
        let shader = project("shader", vec![version("0.1", &[], "aa")]);
        assert!(version_runs(&shader, &shader.versions[0], &content));
    }

    #[test]
    fn revoked_files_are_skipped() {
        let index = Index {
            schema: 1,
            projects: vec![project(
                "mod",
                vec![version("0.2", &[], "bad"), version("0.2", &[], "good")],
            )],
            revoked: vec![Revoked {
                id: "coords_hud".into(),
                sha256: Some("BAD".into()),
                reason: None,
            }],
        };
        let versions = runnable_versions(&index, &index.projects[0], &BuildContent::default());
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].file.sha256, "good");
    }

    #[test]
    fn rejects_foreign_urls_and_names() {
        assert!(allowed_url(&format!("{FILE_HOST}x-1/x.hmod")));
        assert!(allowed_url(
            "https://github.com/me/repo/releases/download/v1/x.hmod"
        ));
        assert!(!allowed_url("https://example.com/x.hmod"));
        assert!(!allowed_url("https://github.com/me/repo/raw/main/x.hmod"));
        assert!(allowed_file_name("x-1.0.hmod", "mod"));
        assert!(allowed_file_name("create.hmod", "native-mod"));
        assert!(allowed_file_name("old.hnmod", "native-mod"));
        assert!(!allowed_file_name("../x.hmod", "mod"));
        assert!(!allowed_file_name("x.zip", "mod"));
        assert!(allowed_file_name("pack.zip", "resourcepack"));
    }

    #[test]
    fn prefers_native_version_for_this_build() {
        let other = version("native-1/aaaaaaa1111/m", &[], "a");
        let exact = version("native-1/9f42064c11/m", &[], "b");
        let versions = vec![&other, &exact];
        let content = BuildContent {
            commit: Some("9f42064".into()),
            ..BuildContent::default()
        };
        assert_eq!(
            preferred_version(&versions, &content).unwrap().file.sha256,
            "b"
        );
        assert_eq!(
            preferred_version(&versions, &BuildContent::default())
                .unwrap()
                .file
                .sha256,
            "a"
        );
        assert_eq!(
            mod_id_of("wayfarer_stones_native.hnmod"),
            "wayfarer_stones_native"
        );
        assert_eq!(mod_id_of("coords_hud.hmod"), "coords_hud");
    }

    #[test]
    fn reads_manifest_id() {
        let manifest =
            "api = \"0.2\"\nid = \"coords_hud\"\nversion = \"0.1.0\"\n[native]\nid = \"x\"";
        assert_eq!(
            manifest_value(manifest, "id").as_deref(),
            Some("coords_hud")
        );
        assert_eq!(manifest_value(manifest, "missing"), None);
    }

    fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            for (name, content) in files {
                zip.start_file(*name, zip::write::FileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut zip, content.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    #[test]
    fn recognizes_native_packages_by_content() {
        let native = zip_with(&[
            (
                "horizon-mod.toml",
                "id = \"create\"\nversion = \"1.0\"\n[native]\nhorizon = \"abc\"\n",
            ),
            ("Cargo.toml", "[package]"),
        ]);
        let info = package_info(std::io::Cursor::new(&native)).unwrap();
        assert!(info.native);
        assert_eq!(info.id.as_deref(), Some("create"));
        assert!(check_native_manifest(&native, "create").is_ok());
        assert!(check_mod_manifest(&native, "create").is_err());

        let content = zip_with(&[("horizon-mod.toml", "id = \"zoom\"\napi = \"0.2\"\n")]);
        let info = package_info(std::io::Cursor::new(&content)).unwrap();
        assert!(!info.native);
        assert!(check_mod_manifest(&content, "zoom").is_ok());

        let wasm = zip_with(&[
            ("horizon-mod.toml", "id = \"zoom\"\napi = \"0.2\"\n[wasm]\nsha512 = \"x\"\n"),
            ("Cargo.toml", "[package]"),
            ("mod.wasm", "wasm"),
        ]);
        assert!(!package_info(std::io::Cursor::new(&wasm)).unwrap().native);

        let old = zip_with(&[("horizon-native.json", r#"{"id":"wayfarer"}"#)]);
        assert!(package_info(std::io::Cursor::new(&old)).unwrap().native);
    }

    #[test]
    fn moves_native_mods_out_of_the_content_folder() {
        let dir = std::env::temp_dir().join(format!("lion-horizon-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join(MODS_DIR)).unwrap();
        std::fs::write(
            dir.join(MODS_DIR).join("create.hmod"),
            zip_with(&[("horizon-mod.toml", "id = \"create\"\n[native]\n")]),
        )
        .unwrap();
        std::fs::write(
            dir.join(MODS_DIR).join("zoom.hmod"),
            zip_with(&[("horizon-mod.toml", "id = \"zoom\"\n")]),
        )
        .unwrap();
        let mods = installed_mods(&dir);
        assert!(dir.join(NATIVE_DIR).join("create.hmod").is_file());
        assert!(dir.join(MODS_DIR).join("zoom.hmod").is_file());
        assert!(mods.iter().any(|m| m.id == "create" && m.native));
        assert!(mods.iter().any(|m| m.id == "zoom" && !m.native));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn mod_switches_roundtrip() {
        let dir = std::env::temp_dir().join(format!("lion-horizon-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MODS_SWITCHES), "zoomKey:c\nmod.other:true\n").unwrap();
        set_mod_enabled(&dir, "coords_hud", true).unwrap();
        assert!(is_mod_enabled(&dir, "coords_hud"));
        set_mod_enabled(&dir, "coords_hud", false).unwrap();
        assert!(!is_mod_enabled(&dir, "coords_hud"));
        assert!(is_mod_enabled(&dir, "other"));
        // Fremde Einstellungen bleiben erhalten
        assert!(std::fs::read_to_string(dir.join(MODS_SWITCHES))
            .unwrap()
            .contains("zoomKey:c"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// `cargo test --bin Lion-Launcher live_index_parses -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "benötigt Internet"]
    async fn live_index_parses() {
        let mods = search(ContentKind::Mod, "", &[], None, 0, 100)
            .await
            .unwrap();
        let shaders = search(ContentKind::ShaderPack, "", &[], None, 0, 100)
            .await
            .unwrap();
        println!("{} mods, {} shaders", mods.len(), shaders.len());
        assert!(!mods.is_empty());
        assert!(!shaders.is_empty());
    }
}
