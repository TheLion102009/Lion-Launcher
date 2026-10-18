//! Update-Pruefung und Einzel-Updates fuer die Mods eines Profils.
//!
//! Installierte Dateien werden ueber ihren SHA1-Hash bei Modrinth zugeordnet
//! (`/version_files` und `/version_files/update`). Das ist exakt - kein Raten ueber
//! Dateinamen oder Versions-Strings - und braucht pro Pruefung nur eine Handvoll
//! Requests, egal wie viele Mods im Profil liegen.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

const MODRINTH_API: &str = "https://api.modrinth.com/v2";
const HASHES_PER_REQUEST: usize = 300;
const PROJECTS_PER_REQUEST: usize = 100;

/// SHA1 einer Mod-Datei, gueltig solange Groesse und Aenderungszeit gleich bleiben.
#[derive(Clone, Serialize, Deserialize)]
struct HashCacheEntry {
    len: u64,
    modified: Option<(u64, u32)>,
    sha1: String,
}

/// Pfad -> Hash. Wird auf Platte gespeichert, damit grosse Modpacks auch nach einem
/// Neustart nicht erneut komplett gelesen werden muessen (kalter Cache: mehrere Sekunden).
static HASH_CACHE: Lazy<Mutex<HashMap<String, HashCacheEntry>>> =
    Lazy::new(|| Mutex::new(load_hash_cache()));
static HASH_CACHE_DIRTY: AtomicBool = AtomicBool::new(false);
static HASH_CACHE_SAVE_LOCK: Mutex<()> = Mutex::new(());

/// project_id -> Titel/Icon. Aendert sich praktisch nie, daher ohne Ablaufzeit.
static PROJECT_CACHE: Lazy<Mutex<HashMap<String, ProjectInfo>>> = Lazy::new(Default::default);

#[derive(Clone, Deserialize)]
struct ProjectInfo {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    icon_url: Option<String>,
}

#[derive(Clone, Deserialize)]
struct VersionFileHashes {
    #[serde(default)]
    sha1: Option<String>,
}

#[derive(Clone, Deserialize)]
struct VersionFile {
    url: String,
    filename: String,
    #[serde(default)]
    primary: bool,
    hashes: VersionFileHashes,
}

#[derive(Clone, Deserialize)]
struct Version {
    id: String,
    project_id: String,
    version_number: String,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    files: Vec<VersionFile>,
}

impl Version {
    fn primary_file(&self) -> Option<&VersionFile> {
        self.files
            .iter()
            .find(|f| f.primary)
            .or_else(|| self.files.first())
    }

    fn supports(&self, mc_version: &str, loaders: &[String]) -> bool {
        self.game_versions.iter().any(|g| g == mc_version)
            && self
                .loaders
                .iter()
                .any(|l| loaders.iter().any(|wanted| wanted.eq_ignore_ascii_case(l)))
    }
}

#[derive(Serialize)]
pub struct ModUpdateStatus {
    pub filename: String,
    /// "update" | "up_to_date" | "incompatible" | "unknown" (nicht auf Modrinth)
    pub status: &'static str,
    pub project_id: Option<String>,
    pub mod_name: Option<String>,
    pub icon_url: Option<String>,
    pub current_version: Option<String>,
    pub current_game_versions: Vec<String>,
    pub current_loaders: Vec<String>,
    /// Laut Modrinth laeuft die installierte Datei auf MC-Version + Loader des Profils.
    pub current_compatible: bool,
    pub latest_version: Option<String>,
    pub latest_version_id: Option<String>,
    pub latest_game_versions: Vec<String>,
}

#[derive(Serialize)]
pub struct ModUpdateCheck {
    pub minecraft_version: String,
    pub loader: String,
    pub results: Vec<ModUpdateStatus>,
}

async fn load_profile(profile_id: &str) -> Result<crate::types::profile::Profile, String> {
    let profiles = crate::core::profiles::ProfileManager::new()
        .map_err(|e| e.to_string())?
        .load_profiles()
        .await
        .map_err(|e| e.to_string())?;
    profiles
        .get_profile(profile_id)
        .cloned()
        .ok_or_else(|| "Profile not found".to_string())
}

fn validate_mod_filename(filename: &str) -> Result<(), String> {
    if filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains("..")
    {
        return Err(format!("Ungueltiger Dateiname: {}", filename));
    }
    Ok(())
}

/// Pfad der Metadaten in `modinfos/` - identisch zur Logik in `get_installed_mods`
/// (`foo.jar` und `foo.jar.disabled` teilen sich `foo.json`).
pub(crate) fn mod_meta_path(game_dir: &Path, filename: &str) -> PathBuf {
    let base = filename
        .trim_end_matches(".disabled")
        .trim_end_matches(".jar");
    game_dir.join("modinfos").join(format!("{}.json", base))
}

/// Quilt laedt auch Fabric-Mods, alle anderen Loader nur ihre eigenen.
fn compatible_loaders(loader: &str) -> Vec<String> {
    match loader {
        "quilt" => vec!["quilt".to_string(), "fabric".to_string()],
        "" | "vanilla" => Vec::new(),
        other => vec![other.to_string()],
    }
}

fn list_mod_files(mods_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(mods_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| {
            let lower = name.to_lowercase();
            lower.ends_with(".jar") || lower.ends_with(".disabled")
        })
        .collect()
}

fn sha1_file(path: &Path) -> Option<String> {
    use sha1::{Digest, Sha1};
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha1::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(hex::encode(hasher.finalize()))
}

fn hash_cache_file() -> PathBuf {
    crate::config::defaults::launcher_dir()
        .join("cache")
        .join("mod-hashes.json")
}

fn load_hash_cache() -> HashMap<String, HashCacheEntry> {
    let Ok(content) = std::fs::read_to_string(hash_cache_file()) else {
        return HashMap::new();
    };
    let entries: HashMap<String, HashCacheEntry> =
        serde_json::from_str(&content).unwrap_or_default();
    // Eintraege geloeschter Mods nicht ewig mitschleppen
    entries
        .into_iter()
        .filter(|(path, _)| Path::new(path).exists())
        .collect()
}

fn save_hash_cache() {
    if !HASH_CACHE_DIRTY.swap(false, Ordering::SeqCst) {
        return;
    }
    let _guard = HASH_CACHE_SAVE_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let snapshot = HASH_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let Ok(content) = serde_json::to_vec(&snapshot) else {
        return;
    };
    let path = hash_cache_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let temp_path = path.with_extension("json.tmp");
    if std::fs::write(&temp_path, content).is_ok() {
        let _ = std::fs::rename(&temp_path, &path);
    }
}

fn cached_sha1(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let len = meta.len();
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| (d.as_secs(), d.subsec_nanos()));
    let key = path.to_string_lossy().to_string();

    if let Some(entry) = HASH_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        if entry.len == len && entry.modified == modified {
            return Some(entry.sha1.clone());
        }
    }

    let sha1 = sha1_file(path)?;
    HASH_CACHE.lock().unwrap_or_else(|e| e.into_inner()).insert(
        key,
        HashCacheEntry {
            len,
            modified,
            sha1: sha1.clone(),
        },
    );
    HASH_CACHE_DIRTY.store(true, Ordering::SeqCst);
    Some(sha1)
}

async fn post_version_lookup(
    client: &reqwest::Client,
    endpoint: &str,
    body: &serde_json::Value,
) -> Result<HashMap<String, Version>, String> {
    let response = client
        .post(format!("{}/{}", MODRINTH_API, endpoint))
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Modrinth nicht erreichbar: {}", e))?;
    if !response.status().is_success() {
        return Err(format!("Modrinth antwortet mit {}", response.status()));
    }
    response
        .json()
        .await
        .map_err(|e| format!("Ungueltige Modrinth-Antwort: {}", e))
}

/// Die installierte Version je Hash.
async fn current_versions(
    client: &reqwest::Client,
    hashes: &[String],
) -> Result<HashMap<String, Version>, String> {
    let mut all = HashMap::new();
    for chunk in hashes.chunks(HASHES_PER_REQUEST) {
        let body = serde_json::json!({ "hashes": chunk, "algorithm": "sha1" });
        all.extend(post_version_lookup(client, "version_files", &body).await?);
    }
    Ok(all)
}

/// Die neueste Version je Hash fuer MC-Version + Loader des Profils.
async fn latest_versions(
    client: &reqwest::Client,
    hashes: &[String],
    mc_version: &str,
    loaders: &[String],
    releases_only: bool,
) -> Result<HashMap<String, Version>, String> {
    let mut all = HashMap::new();
    for chunk in hashes.chunks(HASHES_PER_REQUEST) {
        let mut body = serde_json::json!({
            "hashes": chunk,
            "algorithm": "sha1",
            "loaders": loaders,
            "game_versions": [mc_version],
        });
        if releases_only {
            body["version_types"] = serde_json::json!(["release"]);
        }
        all.extend(post_version_lookup(client, "version_files/update", &body).await?);
    }
    Ok(all)
}

/// Titel und Icons der Projekte. Fehler sind hier nicht kritisch (nur Anzeige).
async fn project_infos(
    client: &reqwest::Client,
    ids: impl IntoIterator<Item = String>,
) -> HashMap<String, ProjectInfo> {
    let mut found = HashMap::new();
    let mut missing = Vec::new();
    {
        let cache = PROJECT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        for id in ids {
            match cache.get(&id) {
                Some(info) => {
                    found.insert(id, info.clone());
                }
                None => missing.push(id),
            }
        }
    }

    for chunk in missing.chunks(PROJECTS_PER_REQUEST) {
        let Ok(ids_json) = serde_json::to_string(chunk) else {
            continue;
        };
        let response = client
            .get(format!("{}/projects", MODRINTH_API))
            .query(&[("ids", ids_json)])
            .send()
            .await;
        let Ok(response) = response else { continue };
        if !response.status().is_success() {
            continue;
        }
        let Ok(list) = response.json::<Vec<ProjectInfo>>().await else {
            continue;
        };
        let mut cache = PROJECT_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        for info in list {
            cache.insert(info.id.clone(), info.clone());
            found.insert(info.id.clone(), info);
        }
    }
    found
}

/// Entscheidet, welche Version (falls ueberhaupt) als Update angeboten wird.
///
/// - Installierte Version passt zum Profil: nur neuere Versionen fuer genau diese
///   MC-Version + Loader. Wer auf einem Release ist, bekommt nur Releases angeboten.
/// - Installierte Version passt nicht (z.B. nach MC-Versionswechsel des Profils):
///   die passende Version fuer das Profil, bevorzugt ein Release.
/// - Gibt es fuer das Profil gar keine Version, ist die Mod wirklich inkompatibel.
fn pick_update<'a>(
    current: &Version,
    current_hash: &str,
    current_compatible: bool,
    latest_any: Option<&'a Version>,
    latest_release: Option<&'a Version>,
) -> (&'static str, Option<&'a Version>) {
    let target = if current_compatible && current.version_type == "release" {
        latest_release
    } else if current_compatible {
        latest_any
    } else {
        latest_release.or(latest_any)
    };

    match target {
        Some(target) => {
            let same_file = target
                .primary_file()
                .and_then(|f| f.hashes.sha1.as_deref())
                .is_some_and(|h| h.eq_ignore_ascii_case(current_hash));
            if target.id == current.id || same_file {
                ("up_to_date", None)
            } else {
                ("update", Some(target))
            }
        }
        None if current_compatible => ("up_to_date", None),
        None => ("incompatible", None),
    }
}

/// Schreibt fehlende `modinfos/`-Metadaten fuer Mods, die per Hash eindeutig
/// zugeordnet wurden (z.B. aus Modpacks). Danach zeigt die Mod-Liste echte Namen,
/// Icons und Versionen, und `.lion`-Exporte kennen die richtige Projekt-ID.
fn write_missing_metadata(game_dir: &Path, results: &[ModUpdateStatus]) {
    for result in results {
        // Nur vollstaendige Eintraege schreiben (Projekt-Infos koennen ausfallen)
        let (Some(project_id), Some(_)) = (&result.project_id, &result.mod_name) else {
            continue;
        };
        let meta_path = mod_meta_path(game_dir, &result.filename);
        if meta_path.exists() {
            continue;
        }
        if let Some(parent) = meta_path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let metadata = serde_json::json!({
            "mod_id": project_id,
            "mod_name": result.mod_name,
            "icon_url": result.icon_url,
            "version": result.current_version,
            "source": "modrinth",
            "filename": result.filename.trim_end_matches(".disabled"),
        });
        if let Ok(content) = serde_json::to_string_pretty(&metadata) {
            if let Err(e) = std::fs::write(&meta_path, content) {
                tracing::warn!("Failed to write mod metadata {:?}: {}", meta_path, e);
            }
        }
    }
}

/// Prueft die Mods eines Profils auf Updates. Ohne `filenames` werden alle Mods
/// geprueft; das Frontend schickt zuerst die sichtbaren und danach den Rest.
#[tauri::command]
pub async fn check_mod_updates(
    profile_id: String,
    filenames: Option<Vec<String>>,
) -> Result<ModUpdateCheck, String> {
    let profile = load_profile(&profile_id).await?;
    let minecraft_version = profile.minecraft_version.clone();
    let loader = profile.loader.loader.as_str().to_string();
    let loaders = compatible_loaders(&loader);
    let mods_dir = profile.game_dir.join("mods");

    let files: Vec<String> = match filenames {
        Some(list) => list
            .into_iter()
            .filter(|f| validate_mod_filename(f).is_ok())
            .collect(),
        None => list_mod_files(&mods_dir),
    };
    if files.is_empty() || loaders.is_empty() {
        return Ok(ModUpdateCheck {
            minecraft_version,
            loader,
            results: Vec::new(),
        });
    }

    let hashed: Vec<(String, Option<String>)> = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;
        files
            .into_par_iter()
            .map(|name| {
                let hash = cached_sha1(&mods_dir.join(&name));
                (name, hash)
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(save_hash_cache);

    let hashes: Vec<String> = hashed
        .iter()
        .filter_map(|(_, hash)| hash.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let client = crate::core::http::HTTP_CLIENT.clone();
    let (current, latest_any, latest_release) = tokio::try_join!(
        current_versions(&client, &hashes),
        latest_versions(&client, &hashes, &minecraft_version, &loaders, false),
        latest_versions(&client, &hashes, &minecraft_version, &loaders, true),
    )?;

    let projects = project_infos(
        &client,
        current
            .values()
            .map(|v| v.project_id.clone())
            .collect::<HashSet<_>>(),
    )
    .await;

    let results: Vec<ModUpdateStatus> = hashed
        .into_iter()
        .map(|(filename, hash)| {
            let current_version = hash.as_ref().and_then(|h| current.get(h));
            let (Some(hash), Some(current_version)) = (hash.as_ref(), current_version) else {
                return ModUpdateStatus {
                    filename,
                    status: "unknown",
                    project_id: None,
                    mod_name: None,
                    icon_url: None,
                    current_version: None,
                    current_game_versions: Vec::new(),
                    current_loaders: Vec::new(),
                    current_compatible: false,
                    latest_version: None,
                    latest_version_id: None,
                    latest_game_versions: Vec::new(),
                };
            };

            let current_compatible = current_version.supports(&minecraft_version, &loaders);
            let (status, target) = pick_update(
                current_version,
                hash,
                current_compatible,
                latest_any.get(hash),
                latest_release.get(hash),
            );
            let project = projects.get(&current_version.project_id);

            ModUpdateStatus {
                filename,
                status,
                project_id: Some(current_version.project_id.clone()),
                mod_name: project.and_then(|p| p.title.clone()),
                icon_url: project.and_then(|p| p.icon_url.clone()),
                current_version: Some(current_version.version_number.clone()),
                current_game_versions: current_version.game_versions.clone(),
                current_loaders: current_version.loaders.clone(),
                current_compatible,
                latest_version: target.map(|t| t.version_number.clone()),
                latest_version_id: target.map(|t| t.id.clone()),
                latest_game_versions: target.map(|t| t.game_versions.clone()).unwrap_or_default(),
            }
        })
        .collect();

    write_missing_metadata(&profile.game_dir, &results);

    Ok(ModUpdateCheck {
        minecraft_version,
        loader,
        results,
    })
}

/// Ersetzt eine installierte Mod-Datei durch die angegebene Modrinth-Version.
/// Deaktivierte Mods bleiben deaktiviert. Gibt den neuen Dateinamen zurueck.
#[tauri::command]
pub async fn update_mod(
    profile_id: String,
    filename: String,
    version_id: String,
) -> Result<String, String> {
    validate_mod_filename(&filename)?;
    let profile = load_profile(&profile_id).await?;
    replace_mod_file(&profile.game_dir, &filename, &version_id).await
}

async fn replace_mod_file(
    game_dir: &Path,
    filename: &str,
    version_id: &str,
) -> Result<String, String> {
    let mods_dir = game_dir.join("mods");
    let old_path = mods_dir.join(filename);
    if !old_path.exists() {
        return Err(format!("Mod-Datei nicht gefunden: {}", filename));
    }

    let client = crate::core::http::HTTP_CLIENT.clone();
    let version: Version = client
        .get(format!(
            "{}/version/{}",
            MODRINTH_API,
            urlencoding::encode(version_id)
        ))
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Version konnte nicht geladen werden: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Ungueltige Modrinth-Antwort: {}", e))?;

    let file = version
        .primary_file()
        .ok_or_else(|| "Diese Version enthaelt keine Datei".to_string())?;
    validate_mod_filename(&file.filename)?;

    let new_filename = if filename.ends_with(".disabled") {
        format!("{}.disabled", file.filename)
    } else {
        file.filename.clone()
    };
    let new_path = mods_dir.join(&new_filename);

    // Erst in eine temporaere Datei laden: schlaegt der Download fehl, bleibt die
    // installierte Version unangetastet (auch wenn der Dateiname gleich bleibt).
    let temp_path = mods_dir.join(format!(".{}.lion-download", file.filename));
    crate::core::download::DownloadManager::new()
        .map_err(|e| e.to_string())?
        .download_with_hash(&file.url, &temp_path, file.hashes.sha1.as_deref())
        .await
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            format!("Download fehlgeschlagen: {}", e)
        })?;

    tokio::fs::rename(&temp_path, &new_path).await.map_err(|e| {
        let _ = std::fs::remove_file(&temp_path);
        format!("{} konnte nicht gespeichert werden: {}", new_filename, e)
    })?;

    if old_path != new_path {
        if let Err(e) = tokio::fs::remove_file(&old_path).await {
            // Lieber die neue Datei wieder entfernen als zwei Versionen derselben Mod
            // im Ordner zu lassen (Minecraft wuerde abstuerzen).
            let _ = tokio::fs::remove_file(&new_path).await;
            return Err(format!(
                "Alte Version konnte nicht entfernt werden (laeuft Minecraft noch?): {}",
                e
            ));
        }
    }
    HASH_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(old_path.to_string_lossy().as_ref());

    let old_meta_path = mod_meta_path(game_dir, filename);
    let old_meta: Option<serde_json::Value> = std::fs::read_to_string(&old_meta_path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok());
    let old_meta_field = |key: &str| {
        old_meta
            .as_ref()
            .and_then(|m| m.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let project = project_infos(&client, [version.project_id.clone()])
        .await
        .remove(&version.project_id);

    let new_meta_path = mod_meta_path(game_dir, &new_filename);
    if old_meta_path != new_meta_path {
        let _ = std::fs::remove_file(&old_meta_path);
    }
    let metadata = serde_json::json!({
        "mod_id": version.project_id,
        "mod_name": project.as_ref().and_then(|p| p.title.clone()).or_else(|| old_meta_field("mod_name")),
        "icon_url": project.as_ref().and_then(|p| p.icon_url.clone()).or_else(|| old_meta_field("icon_url")),
        "version": version.version_number,
        "source": "modrinth",
        "filename": file.filename,
    });
    if let Some(parent) = new_meta_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(
        &new_meta_path,
        serde_json::to_string_pretty(&metadata).unwrap_or_default(),
    ) {
        tracing::warn!("Failed to write mod metadata {:?}: {}", new_meta_path, e);
    }

    tracing::info!(
        "Mod updated: {} -> {} ({})",
        filename,
        new_filename,
        version.version_number
    );
    Ok(new_filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(id: &str, version_type: &str, game_versions: &[&str], sha1: &str) -> Version {
        Version {
            id: id.to_string(),
            project_id: "project".to_string(),
            version_number: id.to_string(),
            version_type: version_type.to_string(),
            game_versions: game_versions.iter().map(|s| s.to_string()).collect(),
            loaders: vec!["neoforge".to_string()],
            files: vec![VersionFile {
                url: String::new(),
                filename: format!("{}.jar", id),
                primary: true,
                hashes: VersionFileHashes {
                    sha1: Some(sha1.to_string()),
                },
            }],
        }
    }

    #[test]
    fn installed_compatible_version_is_up_to_date_even_if_newer_mc_versions_exist() {
        // WorldEdit 7.3.8 auf 1.21.1: neuere Versionen gibt es nur fuer neuere MC-Versionen
        let current = version("7.3.8", "release", &["1.21", "1.21.1"], "aaa");
        let loaders = compatible_loaders("neoforge");
        assert!(current.supports("1.21.1", &loaders));
        let (status, target) = pick_update(&current, "aaa", true, Some(&current), Some(&current));
        assert_eq!(status, "up_to_date");
        assert!(target.is_none());
    }

    #[test]
    fn profile_on_new_mc_version_gets_matching_version_as_update() {
        let current = version("7.3.8", "release", &["1.21", "1.21.1"], "aaa");
        let newer = version("7.3.11", "release", &["1.21.4"], "bbb");
        let loaders = compatible_loaders("neoforge");
        let compatible = current.supports("1.21.4", &loaders);
        assert!(!compatible);
        let (status, target) = pick_update(&current, "aaa", compatible, Some(&newer), Some(&newer));
        assert_eq!(status, "update");
        assert_eq!(target.map(|t| t.id.as_str()), Some("7.3.11"));
    }

    #[test]
    fn no_version_for_profile_means_incompatible() {
        let current = version("7.3.8", "release", &["1.21.1"], "aaa");
        let (status, _) = pick_update(&current, "aaa", false, None, None);
        assert_eq!(status, "incompatible");
    }

    #[test]
    fn release_users_are_not_moved_to_betas() {
        let current = version("1.4.1", "release", &["1.21.1"], "aaa");
        let beta = version("2.4.0-beta.5", "beta", &["1.21.1"], "bbb");
        let (status, _) = pick_update(&current, "aaa", true, Some(&beta), Some(&current));
        assert_eq!(status, "up_to_date");

        let beta_user = version("1.1.0-beta.42", "beta", &["1.21.1"], "ccc");
        let (status, target) = pick_update(&beta_user, "ccc", true, Some(&beta), Some(&current));
        assert_eq!(status, "update");
        assert_eq!(target.map(|t| t.id.as_str()), Some("2.4.0-beta.5"));
    }

    #[test]
    fn identical_file_in_newer_version_is_not_an_update() {
        let current = version("1.0.0", "release", &["1.21.1"], "aaa");
        let republished = version("1.0.0+1.21.1", "release", &["1.21.1"], "aaa");
        let (status, _) = pick_update(&current, "aaa", true, Some(&republished), Some(&republished));
        assert_eq!(status, "up_to_date");
    }

    #[test]
    fn quilt_profiles_accept_fabric_mods() {
        let mut current = version("1.0.0", "release", &["1.21.1"], "aaa");
        current.loaders = vec!["fabric".to_string()];
        assert!(current.supports("1.21.1", &compatible_loaders("quilt")));
        assert!(!current.supports("1.21.1", &compatible_loaders("neoforge")));
    }

    #[test]
    fn meta_path_is_shared_between_enabled_and_disabled_file() {
        let dir = Path::new("/profile");
        assert_eq!(
            mod_meta_path(dir, "sodium-0.6.jar"),
            mod_meta_path(dir, "sodium-0.6.jar.disabled")
        );
        assert_eq!(
            mod_meta_path(dir, "sodium-0.6.jar"),
            Path::new("/profile/modinfos/sodium-0.6.json")
        );
    }
}
