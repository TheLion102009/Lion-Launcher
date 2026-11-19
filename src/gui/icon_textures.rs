//! Texturen für den Icon-Editor (3D-Blöcke, Mob-Köpfe und Items als Profil-Icon).
//!
//! Mojangs Texturen werden nicht mit dem Launcher ausgeliefert: Sie werden zur Laufzeit aus dem
//! Client-JAR gelesen, das der Launcher für den Spieler ohnehin geladen hat (neueste vorhandene
//! Version). Gibt es noch keines, wird nur das Client-JAR der neuesten Version geladen.

use base64::{engine::general_purpose, Engine as _};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// Bereits gelesene Texturen (Pfad → Data-URL).
static CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

const MAX_PATHS: usize = 400;

/// Erlaubt nur Pfade wie `block/grass_block_top` oder `entity/creeper/creeper`.
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 100
        && !path.contains("..")
        && !path.starts_with('/')
        && path
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '/')
}

/// Versionsnummer eines Ordnernamens (`1.21.4`, `26.3`); andere (Loader, Snapshots) → None.
fn release_number(name: &str) -> Option<Vec<u32>> {
    let parts: Option<Vec<u32>> = name.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| p.len() >= 2)
}

/// Neuestes vorhandenes Vanilla-Client-JAR.
fn newest_client_jar() -> Option<PathBuf> {
    let versions = crate::config::defaults::versions_dir();
    std::fs::read_dir(&versions)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let number = release_number(&name)?;
            let jar = e.path().join(format!("{name}.jar"));
            jar.is_file().then_some((number, jar))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, jar)| jar)
}

fn read_textures(jar: &PathBuf, paths: &[String]) -> Result<HashMap<String, String>, String> {
    let file = std::fs::File::open(jar).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut found = HashMap::new();
    for path in paths {
        let Ok(mut entry) = zip.by_name(&format!("assets/minecraft/textures/{path}.png")) else {
            continue;
        };
        let mut bytes = Vec::new();
        if std::io::Read::read_to_end(&mut entry, &mut bytes).is_ok() {
            found.insert(
                path.clone(),
                format!(
                    "data:image/png;base64,{}",
                    general_purpose::STANDARD.encode(&bytes)
                ),
            );
        }
    }
    Ok(found)
}

/// Liefert die angefragten Texturen als Data-URLs; fehlende (andere Version) werden weggelassen.
#[tauri::command]
pub async fn get_icon_textures(paths: Vec<String>) -> Result<HashMap<String, String>, String> {
    let paths: Vec<String> = paths
        .into_iter()
        .filter(|p| valid_path(p))
        .take(MAX_PATHS)
        .collect();
    let mut result = HashMap::new();
    let missing: Vec<String> = {
        let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let cache = cache.as_ref();
        paths
            .iter()
            .filter(|p| match cache.and_then(|c| c.get(*p)) {
                Some(url) => {
                    result.insert((*p).clone(), url.clone());
                    false
                }
                None => true,
            })
            .cloned()
            .collect()
    };
    if missing.is_empty() {
        return Ok(result);
    }

    let jar = match newest_client_jar() {
        Some(jar) => jar,
        None => crate::core::minecraft::MinecraftLauncher::new()
            .map_err(|e| e.to_string())?
            .ensure_latest_release_client()
            .await
            .map_err(|e| format!("Minecraft-Texturen konnten nicht geladen werden: {e}"))?,
    };
    let found = tokio::task::spawn_blocking(move || read_textures(&jar, &missing))
        .await
        .map_err(|e| e.to_string())??;

    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::new);
    for (path, url) in found {
        cache.insert(path.clone(), url.clone());
        result.insert(path, url);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_texture_paths_are_allowed() {
        assert!(valid_path("block/grass_block_top"));
        assert!(valid_path("entity/creeper/creeper"));
        assert!(!valid_path("../secret"));
        assert!(!valid_path("/etc/passwd"));
        assert!(!valid_path("block/Grass"));
        assert!(!valid_path("block/a.png"));
    }

    #[test]
    fn picks_release_versions_only() {
        assert_eq!(release_number("26.3"), Some(vec![26, 3]));
        assert_eq!(release_number("1.21.4"), Some(vec![1, 21, 4]));
        assert_eq!(release_number("neoforge-21.1.233"), None);
        assert_eq!(release_number("26.4-snapshot-8"), None);
        assert!(release_number("26.3") > release_number("1.21.4"));
    }
}
