//! Backups fuer lokale Server (ZIP im Ordner `backups/` des Servers).

use crate::gui::server_manager::{
    append_console_line, console_buffer, is_server_installing, is_server_running,
    load_server_profile_owned, write_server_stdin,
};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use tokio::time::{sleep, Duration};

const BACKUPS_DIR: &str = "backups";

#[derive(Serialize)]
pub struct ServerBackupEntry {
    pub file_name: String,
    pub size: u64,
    pub created_unix: u64,
    /// "full" | "worlds"
    pub kind: String,
}

fn backups_dir(server_dir: &Path) -> PathBuf {
    server_dir.join(BACKUPS_DIR)
}

fn validate_backup_name(file_name: &str) -> Result<(), String> {
    if file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains("..")
        || !file_name.to_lowercase().ends_with(".zip")
    {
        return Err("Ungueltiger Backup-Name".to_string());
    }
    Ok(())
}

fn backup_kind(file_name: &str) -> &'static str {
    if file_name.to_lowercase().ends_with("-worlds.zip") {
        "worlds"
    } else {
        "full"
    }
}

fn entry_for(path: &Path) -> Option<ServerBackupEntry> {
    let metadata = std::fs::metadata(path).ok()?;
    let file_name = path.file_name()?.to_string_lossy().to_string();
    let created_unix = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Some(ServerBackupEntry {
        kind: backup_kind(&file_name).to_string(),
        file_name,
        size: metadata.len(),
        created_unix,
    })
}

/// Welt-Ordner erkennt man an der level.dat
fn is_world_dir(path: &Path) -> bool {
    path.is_dir() && path.join("level.dat").is_file()
}

/// Packt den Server-Ordner in ein ZIP. Gibt die Anzahl uebersprungener (gesperrter) Dateien zurueck.
fn write_backup_zip(server_dir: &Path, target: &Path, worlds_only: bool) -> Result<usize, String> {
    let tmp_target = target.with_extension("zip.part");
    let file = std::fs::File::create(&tmp_target)
        .map_err(|e| format!("Backup-Datei konnte nicht erstellt werden: {}", e))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let options = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);

    let mut skipped = 0usize;
    let walker = walkdir::WalkDir::new(server_dir)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| {
            if entry.depth() != 1 {
                return entry.file_name() != "session.lock";
            }
            let name = entry.file_name().to_string_lossy();
            if name == BACKUPS_DIR || name == ".buildtools" {
                return false;
            }
            if worlds_only {
                return is_world_dir(entry.path());
            }
            true
        });

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let relative = entry
            .path()
            .strip_prefix(server_dir)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");

        if entry.file_type().is_dir() {
            zip.add_directory(format!("{}/", relative), options)
                .map_err(|e| e.to_string())?;
            continue;
        }

        // Dateien, die der laufende Server exklusiv sperrt, ueberspringen statt abzubrechen
        let mut source = match std::fs::File::open(entry.path()) {
            Ok(file) => file,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        zip.start_file(relative, options).map_err(|e| e.to_string())?;
        if std::io::copy(&mut source, &mut zip).is_err() {
            skipped += 1;
        }
    }

    let mut writer = zip.finish().map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    drop(writer);

    std::fs::rename(&tmp_target, target).map_err(|e| e.to_string())?;
    Ok(skipped)
}

fn restore_backup_zip(server_dir: &Path, backup_path: &Path) -> Result<(), String> {
    let file = std::fs::File::open(backup_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Backup ist beschaedigt: {}", e))?;

    // Top-Level-Eintraege des Backups ermitteln
    let mut top_level = std::collections::HashSet::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if let Some(path) = entry.enclosed_name() {
            if let Some(first) = path.components().next() {
                top_level.insert(first.as_os_str().to_string_lossy().to_string());
            }
        }
    }

    let worlds_only = backup_kind(&backup_path.file_name().unwrap_or_default().to_string_lossy()) == "worlds";

    // Vorhandene Daten entfernen: bei Welt-Backups nur die enthaltenen Welten, sonst alles ausser backups/
    for entry in std::fs::read_dir(server_dir).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == BACKUPS_DIR {
            continue;
        }
        if worlds_only && !top_level.contains(&name) {
            continue;
        }
        let path = entry.path();
        let removed = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        removed.map_err(|e| format!("{} konnte nicht entfernt werden: {}", name, e))?;
    }

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(relative) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
            continue;
        };
        let out_path = server_dir.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&out_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub async fn list_server_backups(server_id: String) -> Result<Vec<ServerBackupEntry>, String> {
    let profile = load_server_profile_owned(&server_id).await?;
    let dir = backups_dir(&profile.server_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut backups: Vec<ServerBackupEntry> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|ext| ext == "zip").unwrap_or(false))
        .filter_map(|p| entry_for(&p))
        .collect();
    backups.sort_by(|a, b| b.created_unix.cmp(&a.created_unix));
    Ok(backups)
}

#[tauri::command]
pub async fn create_server_backup(
    server_id: String,
    worlds_only: Option<bool>,
) -> Result<ServerBackupEntry, String> {
    if is_server_installing(&server_id) {
        return Err("Server wird gerade installiert/gestartet".to_string());
    }

    let profile = load_server_profile_owned(&server_id).await?;
    let worlds_only = worlds_only.unwrap_or(false);
    let dir = backups_dir(&profile.server_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;

    let file_name = format!(
        "backup-{}-{}.zip",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"),
        if worlds_only { "worlds" } else { "full" }
    );
    let target = dir.join(&file_name);

    // Laufender Server: Autosave pausieren und alles auf die Platte schreiben lassen
    let running = is_server_running(&server_id);
    let console = console_buffer(&server_id);
    if running {
        append_console_line(&console, "[Lion] Backup: speichere Welten...".to_string()).await;
        write_server_stdin(&server_id, "save-off").await;
        write_server_stdin(&server_id, "save-all flush").await;
        sleep(Duration::from_secs(3)).await;
    }

    let server_dir = profile.server_dir.clone();
    let zip_target = target.clone();
    let result = tokio::task::spawn_blocking(move || write_backup_zip(&server_dir, &zip_target, worlds_only))
        .await
        .map_err(|e| e.to_string());

    if running {
        write_server_stdin(&server_id, "save-on").await;
    }

    match result {
        Ok(Ok(skipped)) => {
            let message = if skipped > 0 {
                format!("[Lion] Backup {} erstellt ({} gesperrte Dateien uebersprungen)", file_name, skipped)
            } else {
                format!("[Lion] Backup {} erstellt", file_name)
            };
            append_console_line(&console, message).await;
            entry_for(&target).ok_or_else(|| "Backup wurde nicht gefunden".to_string())
        }
        Ok(Err(err)) | Err(err) => {
            let _ = tokio::fs::remove_file(target.with_extension("zip.part")).await;
            append_console_line(&console, format!("[Lion] Backup fehlgeschlagen: {}", err)).await;
            Err(err)
        }
    }
}

#[tauri::command]
pub async fn restore_server_backup(server_id: String, file_name: String) -> Result<(), String> {
    validate_backup_name(&file_name)?;
    if is_server_running(&server_id) || is_server_installing(&server_id) {
        return Err("Bitte stoppe den Server, bevor du ein Backup wiederherstellst".to_string());
    }

    let profile = load_server_profile_owned(&server_id).await?;
    let backup_path = backups_dir(&profile.server_dir).join(&file_name);
    if !backup_path.is_file() {
        return Err("Backup wurde nicht gefunden".to_string());
    }

    let server_dir = profile.server_dir.clone();
    tokio::task::spawn_blocking(move || restore_backup_zip(&server_dir, &backup_path))
        .await
        .map_err(|e| e.to_string())??;

    append_console_line(
        &console_buffer(&server_id),
        format!("[Lion] Backup {} wiederhergestellt", file_name),
    )
    .await;
    Ok(())
}

#[tauri::command]
pub async fn delete_server_backup(server_id: String, file_name: String) -> Result<(), String> {
    validate_backup_name(&file_name)?;
    let profile = load_server_profile_owned(&server_id).await?;
    let path = backups_dir(&profile.server_dir).join(&file_name);
    if path.exists() {
        tokio::fs::remove_file(path)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
