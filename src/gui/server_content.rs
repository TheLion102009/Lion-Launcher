//! Plugin- und Mod-Verwaltung fuer lokale Server (Modrinth-Suche, Installation,
//! Aktivieren/Deaktivieren, Update-Pruefung).

use crate::api::modrinth::ModrinthClient;
use crate::gui::server_manager::{download_to_path, load_server_profile_owned};
use crate::types::mod_info::{DependencyType, ModInfo, ModSource, ModVersion};
use crate::types::server_profile::{ServerMode, ServerProfile};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};

const DISABLED_SUFFIX: &str = ".disabled";

/// Loader-Tags, die Modrinth in `categories` mischt. Werden fuer die Anzeige getrennt.
const KNOWN_LOADERS: &[&str] = &[
    "paper", "purpur", "folia", "spigot", "bukkit", "sponge", "velocity", "bungeecord",
    "waterfall", "fabric", "quilt", "forge", "neoforge", "liteloader", "rift", "babric",
    "bta-babric", "java-agent", "legacy-fabric", "modloader", "nilloader", "ornithe", "datapack",
    "minecraft", "iris", "optifine", "canvas", "vanilla",
];

#[derive(Serialize)]
pub struct ServerContentContext {
    /// "plugins" | "mods" | "none" (Vanilla)
    pub content_type: String,
    /// Modrinth project_type: "plugin" | "mod"
    pub project_type: String,
    pub loaders: Vec<String>,
    pub minecraft_version: String,
}

#[derive(Serialize)]
pub struct ServerContentItem {
    pub filename: String,
    pub name: String,
    pub version: Option<String>,
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub icon_url: Option<String>,
    pub disabled: bool,
    pub size: u64,
}

#[derive(Serialize)]
pub struct ServerContentUpdate {
    pub filename: String,
    pub project_id: String,
    pub name: String,
    pub icon_url: Option<String>,
    pub current_version: Option<String>,
    pub latest_version: String,
    pub latest_version_id: String,
}

#[derive(Serialize)]
pub struct ServerContentInstallResult {
    pub installed: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
struct ContentMeta {
    project_id: Option<String>,
    version_id: Option<String>,
    version_number: Option<String>,
    title: Option<String>,
    icon_url: Option<String>,
    filename: Option<String>,
    source: Option<String>,
}

#[derive(Deserialize)]
struct SearchResponse {
    hits: Vec<SearchHit>,
}

#[derive(Deserialize)]
struct SearchHit {
    project_id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    author: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    display_categories: Vec<String>,
    #[serde(default)]
    versions: Vec<String>,
    #[serde(default)]
    date_modified: String,
    #[serde(default)]
    client_side: Option<String>,
    #[serde(default)]
    server_side: Option<String>,
}

fn content_folder(profile: &ServerProfile) -> &'static str {
    match profile.mode {
        ServerMode::Plugins => "plugins",
        ServerMode::Modded => "mods",
    }
}

fn project_type(profile: &ServerProfile) -> &'static str {
    match profile.mode {
        ServerMode::Plugins => "plugin",
        ServerMode::Modded => "mod",
    }
}

/// Welche Modrinth-Loader auf der jeweiligen Server-Software lauffaehig sind.
fn compatible_loaders(profile: &ServerProfile) -> Vec<&'static str> {
    match profile.software.as_str() {
        "paper" => vec!["paper", "spigot", "bukkit"],
        "purpur" => vec!["purpur", "paper", "spigot", "bukkit"],
        "folia" => vec!["folia"],
        "spigot" => vec!["spigot", "bukkit"],
        "bukkit" => vec!["bukkit"],
        "fabric" => vec!["fabric"],
        "quilt" => vec!["quilt", "fabric"],
        "forge" => vec!["forge"],
        "neoforge" if profile.minecraft_version == "1.20.1" => vec!["neoforge", "forge"],
        "neoforge" => vec!["neoforge"],
        _ => Vec::new(),
    }
}

fn is_plugin_server(profile: &ServerProfile) -> bool {
    profile.mode == ServerMode::Plugins
}

fn validate_file_name(file_name: &str) -> Result<(), String> {
    if file_name.is_empty()
        || file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains("..")
    {
        return Err(format!("Ungueltiger Dateiname: {}", file_name));
    }
    Ok(())
}

fn base_file_name(file_name: &str) -> &str {
    file_name.strip_suffix(DISABLED_SUFFIX).unwrap_or(file_name)
}

fn meta_dir(profile: &ServerProfile) -> PathBuf {
    profile.server_dir.join(".lion").join(content_folder(profile))
}

fn meta_path(profile: &ServerProfile, file_name: &str) -> PathBuf {
    meta_dir(profile).join(format!("{}.json", base_file_name(file_name)))
}

fn read_meta(profile: &ServerProfile, file_name: &str) -> Option<ContentMeta> {
    let content = std::fs::read_to_string(meta_path(profile, file_name)).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_meta(profile: &ServerProfile, file_name: &str, meta: &ContentMeta) -> Result<(), String> {
    std::fs::create_dir_all(meta_dir(profile)).map_err(|e| e.to_string())?;
    let content = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    std::fs::write(meta_path(profile, file_name), content).map_err(|e| e.to_string())
}

fn is_content_file(file_name: &str) -> bool {
    base_file_name(&file_name.to_lowercase()).ends_with(".jar")
}

fn clean_value(raw: &str) -> Option<String> {
    let value = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if value.is_empty() || value.contains("${") {
        None
    } else {
        Some(value.to_string())
    }
}

fn read_zip_text(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut file = archive.by_name(name).ok()?;
    let mut content = String::new();
    file.read_to_string(&mut content).ok()?;
    Some(content)
}

/// Liest Name und Version direkt aus der JAR (plugin.yml, fabric.mod.json, mods.toml, ...),
/// falls die Datei nicht ueber den Launcher installiert wurde.
fn read_jar_metadata(path: &Path) -> (Option<String>, Option<String>) {
    let Ok(file) = std::fs::File::open(path) else {
        return (None, None);
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return (None, None);
    };

    for yml in ["paper-plugin.yml", "plugin.yml", "bungee.yml"] {
        if let Some(content) = read_zip_text(&mut archive, yml) {
            let mut name = None;
            let mut version = None;
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("name:") {
                    name = name.or_else(|| clean_value(rest));
                } else if let Some(rest) = line.strip_prefix("version:") {
                    version = version.or_else(|| clean_value(rest));
                }
            }
            if name.is_some() {
                return (name, version);
            }
        }
    }

    if let Some(content) = read_zip_text(&mut archive, "fabric.mod.json") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            let name = json.get("name").and_then(|v| v.as_str()).and_then(clean_value);
            let version = json.get("version").and_then(|v| v.as_str()).and_then(clean_value);
            if name.is_some() {
                return (name, version);
            }
        }
    }

    if let Some(content) = read_zip_text(&mut archive, "quilt.mod.json") {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            let loader = json.get("quilt_loader");
            let name = loader
                .and_then(|l| l.get("metadata"))
                .and_then(|m| m.get("name"))
                .and_then(|v| v.as_str())
                .and_then(clean_value);
            let version = loader
                .and_then(|l| l.get("version"))
                .and_then(|v| v.as_str())
                .and_then(clean_value);
            if name.is_some() {
                return (name, version);
            }
        }
    }

    for toml in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Some(content) = read_zip_text(&mut archive, toml) {
            let mut in_mods = false;
            let mut name = None;
            let mut version = None;
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("[[") {
                    if in_mods && name.is_some() {
                        break;
                    }
                    in_mods = line == "[[mods]]";
                    continue;
                }
                if !in_mods {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let value = value.split('#').next().unwrap_or("");
                    match key.trim() {
                        "displayName" => name = name.or_else(|| clean_value(value)),
                        "version" => version = version.or_else(|| clean_value(value)),
                        _ => {}
                    }
                }
            }
            if version.is_none() {
                if let Some(manifest) = read_zip_text(&mut archive, "META-INF/MANIFEST.MF") {
                    version = manifest
                        .lines()
                        .find_map(|l| l.strip_prefix("Implementation-Version:"))
                        .and_then(clean_value);
                }
            }
            if name.is_some() {
                return (name, version);
            }
        }
    }

    (None, None)
}

fn file_display_name(file_name: &str) -> String {
    base_file_name(file_name)
        .trim_end_matches(".jar")
        .to_string()
}

fn scan_installed_content(profile: &ServerProfile) -> Result<Vec<ServerContentItem>, String> {
    let dir = profile.server_dir.join(content_folder(profile));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let mut items = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !is_content_file(&file_name) {
            continue;
        }

        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let meta = read_meta(profile, &file_name).unwrap_or_default();
        let (jar_name, jar_version) = if meta.title.is_none() || meta.version_number.is_none() {
            read_jar_metadata(&entry.path())
        } else {
            (None, None)
        };

        items.push(ServerContentItem {
            name: meta
                .title
                .clone()
                .or(jar_name)
                .unwrap_or_else(|| file_display_name(&file_name)),
            version: meta.version_number.clone().or(jar_version),
            project_id: meta.project_id.clone(),
            version_id: meta.version_id.clone(),
            icon_url: meta.icon_url.clone(),
            disabled: file_name.ends_with(DISABLED_SUFFIX),
            filename: file_name,
            size,
        });
    }

    items.sort_by_key(|item| item.name.to_lowercase());
    Ok(items)
}

/// Minecraft "major.minor" (1.21.4 -> 1.21, 26.1.2 -> 26.1)
fn minecraft_minor(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Waehlt die passende Version: exakte MC-Version bevorzugt, Releases vor Betas.
/// Plugins sind meist aufwaertskompatibel, deshalb gibt es fuer sie Fallbacks.
fn pick_version<'a>(
    versions: &'a [ModVersion],
    minecraft_version: &str,
    loaders: &[&str],
    lenient: bool,
) -> Option<&'a ModVersion> {
    let mut candidates: Vec<&ModVersion> = versions
        .iter()
        .filter(|v| v.loaders.iter().any(|l| loaders.contains(&l.to_lowercase().as_str())))
        .collect();
    candidates.sort_by(|a, b| b.published.cmp(&a.published));

    let prefer_release = |list: Vec<&'a ModVersion>| -> Option<&'a ModVersion> {
        list.iter()
            .find(|v| v.version_type.as_deref().unwrap_or("release") == "release")
            .or_else(|| list.first())
            .copied()
    };

    let exact: Vec<&ModVersion> = candidates
        .iter()
        .filter(|v| v.game_versions.iter().any(|g| g == minecraft_version))
        .copied()
        .collect();
    if let Some(version) = prefer_release(exact) {
        return Some(version);
    }

    if !lenient {
        return None;
    }

    let minor = minecraft_minor(minecraft_version);
    let same_minor: Vec<&ModVersion> = candidates
        .iter()
        .filter(|v| {
            v.game_versions
                .iter()
                .any(|g| g == &minor || g.starts_with(&format!("{}.", minor)))
        })
        .copied()
        .collect();
    if let Some(version) = prefer_release(same_minor) {
        return Some(version);
    }

    prefer_release(candidates)
}

fn sha1_hex(path: &Path) -> Option<String> {
    use sha1::{Digest, Sha1};
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = Sha1::new();
    hasher.update(&bytes);
    Some(hex::encode(hasher.finalize()))
}

#[tauri::command]
pub async fn get_server_content_context(server_id: String) -> Result<ServerContentContext, String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let loaders = compatible_loaders(&profile);
    Ok(ServerContentContext {
        content_type: if loaders.is_empty() {
            "none".to_string()
        } else {
            content_folder(&profile).to_string()
        },
        project_type: project_type(&profile).to_string(),
        loaders: loaders.into_iter().map(|l| l.to_string()).collect(),
        minecraft_version: profile.minecraft_version,
    })
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn search_server_content(
    project_type: String,
    query: String,
    game_version: Option<String>,
    loaders: Option<Vec<String>>,
    categories: Option<Vec<String>>,
    sort_by: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<Vec<ModInfo>, String> {
    let project_type = match project_type.as_str() {
        "plugin" => "plugin",
        _ => "mod",
    };

    let mut facets: Vec<Vec<String>> = vec![vec![format!("project_type:{}", project_type)]];
    if project_type == "mod" {
        // Reine Client-Mods haben auf einem Server nichts verloren
        facets.push(vec!["server_side!=unsupported".to_string()]);
    }
    if let Some(version) = game_version.filter(|v| !v.trim().is_empty()) {
        facets.push(vec![format!("versions:{}", version)]);
    }
    let loaders: Vec<String> = loaders
        .unwrap_or_default()
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .collect();
    if !loaders.is_empty() {
        facets.push(loaders.iter().map(|l| format!("categories:{}", l)).collect());
    }
    for category in categories.unwrap_or_default() {
        if !category.trim().is_empty() {
            facets.push(vec![format!("categories:{}", category)]);
        }
    }

    let index = match sort_by.as_deref() {
        Some("downloads") => "downloads",
        Some("updated") => "updated",
        Some("newest") => "newest",
        Some("follows") => "follows",
        _ => "relevance",
    };

    let facets_json = serde_json::to_string(&facets).map_err(|e| e.to_string())?;
    let response: SearchResponse = crate::core::http::HTTP_CLIENT
        .get("https://api.modrinth.com/v2/search")
        .query(&[
            ("query", query.as_str()),
            ("facets", facets_json.as_str()),
            ("index", index),
            ("offset", &offset.unwrap_or(0).to_string()),
            ("limit", &limit.unwrap_or(20).min(100).to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("Modrinth-Suche fehlgeschlagen: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Modrinth-Suche fehlgeschlagen: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Modrinth-Antwort ungueltig: {}", e))?;

    Ok(response
        .hits
        .into_iter()
        .map(|hit| {
            let hit_loaders: Vec<String> = hit
                .categories
                .iter()
                .filter(|c| KNOWN_LOADERS.contains(&c.as_str()))
                .cloned()
                .collect();
            let display_categories: Vec<String> = hit
                .display_categories
                .iter()
                .filter(|c| !KNOWN_LOADERS.contains(&c.as_str()))
                .cloned()
                .collect();

            ModInfo {
                id: hit.project_id,
                slug: hit.slug.clone(),
                name: hit.title,
                description: hit.description,
                body: None,
                icon_url: hit.icon_url.filter(|u| !u.is_empty()),
                author: hit.author,
                downloads: hit.downloads,
                followers: None,
                categories: display_categories,
                source: ModSource::Modrinth,
                versions: hit.versions.clone(),
                game_versions: hit.versions,
                loaders: hit_loaders,
                project_url: format!("https://modrinth.com/{}/{}", project_type, hit.slug),
                updated_at: hit.date_modified,
                client_side: hit.client_side,
                server_side: hit.server_side,
                source_url: None,
                issues_url: None,
                wiki_url: None,
                discord_url: None,
                gallery: vec![],
                project_type: None,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn get_server_installed_content(server_id: String) -> Result<Vec<ServerContentItem>, String> {
    let profile = load_server_profile_owned(&server_id).await?;
    tokio::task::spawn_blocking(move || scan_installed_content(&profile))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn toggle_server_content(
    server_id: String,
    filenames: Vec<String>,
    enable: bool,
) -> Result<(), String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let dir = profile.server_dir.join(content_folder(&profile));

    for file_name in filenames {
        validate_file_name(&file_name)?;
        let base = base_file_name(&file_name).to_string();
        let enabled_path = dir.join(&base);
        let disabled_path = dir.join(format!("{}{}", base, DISABLED_SUFFIX));

        let (from, to) = if enable {
            (disabled_path, enabled_path)
        } else {
            (enabled_path, disabled_path)
        };
        if from.exists() {
            tokio::fs::rename(&from, &to)
                .await
                .map_err(|e| format!("{} konnte nicht umbenannt werden: {}", base, e))?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_server_content(server_id: String, filenames: Vec<String>) -> Result<(), String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let dir = profile.server_dir.join(content_folder(&profile));

    for file_name in filenames {
        validate_file_name(&file_name)?;
        let target = dir.join(&file_name);
        if target.exists() {
            tokio::fs::remove_file(&target)
                .await
                .map_err(|e| format!("{} konnte nicht geloescht werden: {}", file_name, e))?;
        }
        let _ = tokio::fs::remove_file(meta_path(&profile, &file_name)).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn install_server_content(
    server_id: String,
    project_id: String,
    version_id: Option<String>,
) -> Result<ServerContentInstallResult, String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let loaders = compatible_loaders(&profile);
    if loaders.is_empty() {
        return Err("Vanilla-Server unterstuetzen keine Plugins oder Mods".to_string());
    }

    let lenient = is_plugin_server(&profile);
    let content_dir = profile.server_dir.join(content_folder(&profile));
    tokio::fs::create_dir_all(&content_dir)
        .await
        .map_err(|e| e.to_string())?;

    // project_id -> Dateiname der bereits installierten Dateien
    let scan_profile = profile.clone();
    let existing = tokio::task::spawn_blocking(move || scan_installed_content(&scan_profile))
        .await
        .map_err(|e| e.to_string())??;
    let mut installed_by_project: HashMap<String, Vec<String>> = HashMap::new();
    for item in &existing {
        if let Some(pid) = &item.project_id {
            installed_by_project
                .entry(pid.clone())
                .or_default()
                .push(item.filename.clone());
        }
    }

    let client = ModrinthClient::new().map_err(|e| e.to_string())?;
    let mut result = ServerContentInstallResult {
        installed: Vec::new(),
        warnings: Vec::new(),
    };
    let mut queue: VecDeque<(String, Option<String>, bool)> = VecDeque::new();
    queue.push_back((project_id, version_id, true));
    let mut seen = HashSet::new();

    while let Some((pid, requested_version, is_root)) = queue.pop_front() {
        if !seen.insert(pid.clone()) {
            continue;
        }
        // Abhaengigkeiten, die schon installiert sind, nicht erneut anfassen
        if !is_root && installed_by_project.contains_key(&pid) {
            continue;
        }

        let versions = match client.get_versions(&pid).await {
            Ok(v) => v,
            Err(e) if is_root => return Err(format!("Versionen konnten nicht geladen werden: {}", e)),
            Err(e) => {
                result
                    .warnings
                    .push(format!("Abhaengigkeit {} konnte nicht geladen werden: {}", pid, e));
                continue;
            }
        };

        let version = match &requested_version {
            Some(vid) => versions.iter().find(|v| &v.id == vid),
            None => pick_version(&versions, &profile.minecraft_version, &loaders, lenient),
        };
        let Some(version) = version else {
            let message = format!(
                "Keine passende Version fuer Minecraft {} ({}) gefunden",
                profile.minecraft_version,
                loaders.join("/")
            );
            if is_root {
                return Err(message);
            }
            result.warnings.push(format!("Abhaengigkeit {}: {}", pid, message));
            continue;
        };

        let file = version
            .files
            .iter()
            .find(|f| f.primary)
            .or_else(|| version.files.first())
            .ok_or_else(|| "Version enthaelt keine Datei".to_string())?;
        validate_file_name(&file.filename)?;
        if !file.filename.to_lowercase().ends_with(".jar") {
            let message = format!("{} ist keine JAR-Datei und kann nicht installiert werden", file.filename);
            if is_root {
                return Err(message);
            }
            result.warnings.push(message);
            continue;
        }

        let target = content_dir.join(&file.filename);
        download_to_path(&file.url, &target).await?;

        if let Some(expected) = &file.hashes.sha1 {
            let path = target.clone();
            let actual = tokio::task::spawn_blocking(move || sha1_hex(&path))
                .await
                .map_err(|e| e.to_string())?;
            if actual.as_deref() != Some(expected.as_str()) {
                let _ = tokio::fs::remove_file(&target).await;
                return Err(format!("Pruefsumme von {} stimmt nicht", file.filename));
            }
        }

        // Alte Versionen desselben Projekts entfernen
        if let Some(old_files) = installed_by_project.get(&pid) {
            for old in old_files {
                if base_file_name(old) == file.filename {
                    // Gleicher Name: evtl. deaktivierte Kopie aufraeumen
                    if old != &file.filename {
                        let _ = tokio::fs::remove_file(content_dir.join(old)).await;
                    }
                    continue;
                }
                let _ = tokio::fs::remove_file(content_dir.join(old)).await;
                let _ = tokio::fs::remove_file(meta_path(&profile, old)).await;
            }
        }

        let project = client.get_mod(&pid).await.ok();
        let title = project
            .as_ref()
            .map(|p| p.name.clone())
            .unwrap_or_else(|| file_display_name(&file.filename));
        let meta = ContentMeta {
            project_id: Some(pid.clone()),
            version_id: Some(version.id.clone()),
            version_number: Some(version.version_number.clone()),
            title: Some(title.clone()),
            icon_url: project.and_then(|p| p.icon_url),
            filename: Some(file.filename.clone()),
            source: Some("modrinth".to_string()),
        };
        write_meta(&profile, &file.filename, &meta)?;

        if lenient && !version.game_versions.iter().any(|g| g == &profile.minecraft_version) {
            result.warnings.push(format!(
                "{} {} ist offiziell nur fuer {} markiert",
                title,
                version.version_number,
                version.game_versions.join(", ")
            ));
        }
        result.installed.push(title);

        for dependency in &version.dependencies {
            if dependency.dependency_type == DependencyType::Required && !dependency.mod_id.is_empty() {
                queue.push_back((dependency.mod_id.clone(), None, false));
            }
        }
    }

    Ok(result)
}

#[tauri::command]
pub async fn check_server_content_updates(server_id: String) -> Result<Vec<ServerContentUpdate>, String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let loaders: Vec<&'static str> = compatible_loaders(&profile);
    if loaders.is_empty() {
        return Ok(Vec::new());
    }

    let scan_profile = profile.clone();
    let items = tokio::task::spawn_blocking(move || scan_installed_content(&scan_profile))
        .await
        .map_err(|e| e.to_string())??;

    let lenient = is_plugin_server(&profile);
    let minecraft_version = profile.minecraft_version.clone();
    let client = std::sync::Arc::new(ModrinthClient::new().map_err(|e| e.to_string())?);

    let updates = futures_util::stream::iter(items.into_iter().filter(|i| i.project_id.is_some()))
        .map(|item| {
            let client = client.clone();
            let loaders = loaders.clone();
            let minecraft_version = minecraft_version.clone();
            async move {
                let project_id = item.project_id.clone()?;
                let versions = client.get_versions(&project_id).await.ok()?;
                let latest = pick_version(&versions, &minecraft_version, &loaders, lenient)?;
                let is_newer = match &item.version_id {
                    Some(current) => current != &latest.id,
                    None => item.version.as_deref() != Some(latest.version_number.as_str()),
                };
                if !is_newer {
                    return None;
                }
                Some(ServerContentUpdate {
                    filename: item.filename,
                    project_id,
                    name: item.name,
                    icon_url: item.icon_url,
                    current_version: item.version,
                    latest_version: latest.version_number.clone(),
                    latest_version_id: latest.id.clone(),
                })
            }
        })
        .buffer_unordered(6)
        .filter_map(|update| async move { update })
        .collect::<Vec<_>>()
        .await;

    Ok(updates)
}
