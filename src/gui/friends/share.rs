//! Geteilte Instanzen (angelehnt an "Shared Instances" der Modrinth App).
//!
//! * Der Besitzer veröffentlicht einen Stand ("Revision"): Name, Icon, Minecraft-Version,
//!   Loader, alle Mods/Packs (exakte Versionen) und ausgewählte Konfig-Dateien.
//! * Dateien, die es nicht auf Modrinth gibt, sowie Konfig-Dateien liefert der Besitzer
//!   selbst aus – aus einem Snapshot, damit immer genau der veröffentlichte Stand ankommt.
//! * Eingeladene sehen vor dem Annehmen eine Vorschau (inkl. Hinweis auf nicht auf Modrinth
//!   veröffentlichte Dateien). Updates werden beim Spielstart geprüft und erst nach
//!   Bestätigung angewendet.
//! * Inhalte des Besitzers sind für Mitglieder gesperrt; eigene Inhalte dürfen sie ergänzen.

use super::protocol::{self, Request, Response};
use super::store::{
    self, blobs_dir, ConfigEntry, FriendsState, JoinedShare, OwnedShare, ShareManifest, ALL_KINDS,
};
use super::{service, FriendsService};
use crate::core::profiles::ProfileManager;
use crate::gui::content_share::{self, ContentKind, SharedContentEntry};
use crate::types::profile::Profile;
use crate::types::version::ModLoader;
use base64::{engine::general_purpose, Engine as _};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Ordner, aus denen Konfig-Dateien geteilt werden können.
const CONFIG_ROOTS: [&str; 3] = ["config", "defaultconfigs", "kubejs"];
/// Einzelne Dateien im Spielordner, die geteilt werden können.
const CONFIG_ROOT_FILES: [&str; 3] = ["options.txt", "optionsof.txt", "optionsshaders.txt"];
const MAX_CONFIG_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_CONFIG_FILES_LISTED: usize = 3000;

// ==================== Hilfen ====================

fn share_link(owner_id: &str, share: &OwnedShare) -> String {
    format!(
        "lion://share/{}/{}/{}",
        owner_id, share.share_id, share.link_token
    )
}

fn parse_loader(loader: &str) -> Result<ModLoader, String> {
    match loader.to_lowercase().as_str() {
        "vanilla" => Ok(ModLoader::Vanilla),
        "fabric" => Ok(ModLoader::Fabric),
        "forge" => Ok(ModLoader::Forge),
        "neoforge" => Ok(ModLoader::NeoForge),
        "quilt" => Ok(ModLoader::Quilt),
        other => Err(format!("Unbekannter Mod-Loader: {}", other)),
    }
}

/// Erlaubt nur relative Pfade innerhalb der freigegebenen Konfig-Orte (kein `..`, kein
/// absoluter Pfad). Die Pfade stammen von einem anderen Rechner.
fn safe_config_path(path: &str) -> Option<PathBuf> {
    let rel = Path::new(path);
    let components: Vec<_> = rel.components().collect();
    if components.is_empty() || !components.iter().all(|c| matches!(c, Component::Normal(_))) {
        return None;
    }
    let first = components[0].as_os_str().to_str()?;
    let allowed = if components.len() == 1 {
        CONFIG_ROOT_FILES.contains(&first)
    } else {
        CONFIG_ROOTS.contains(&first)
    };
    allowed.then(|| rel.to_path_buf())
}

fn is_sha1_hex(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn blob_path(sha1: &str) -> PathBuf {
    blobs_dir().join(sha1.to_lowercase())
}

fn write_blob(sha1: &str, bytes: &[u8]) -> Result<(), String> {
    let path = blob_path(sha1);
    if path.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(blobs_dir()).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn content_path(game_dir: &Path, kind: ContentKind, entry: &SharedContentEntry) -> Option<PathBuf> {
    let filename = content_share::safe_file_name(entry.filename.as_deref()?)?;
    let dir = game_dir.join(kind.as_str());
    Some(if entry.disabled {
        dir.join(format!("{}.disabled", filename))
    } else {
        dir.join(filename)
    })
}

fn entry_key(entry: &SharedContentEntry) -> String {
    entry
        .project_id
        .clone()
        .or_else(|| entry.mod_id.clone())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| entry.filename.clone().unwrap_or_default().to_lowercase())
}

fn entry_name(entry: &SharedContentEntry) -> String {
    entry
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .or_else(|| entry.filename.clone())
        .unwrap_or_else(|| "?".to_string())
}

fn entry_version(entry: &SharedContentEntry) -> Option<String> {
    entry
        .version_number
        .clone()
        .or_else(|| entry.filename.clone())
}

/// Nicht auf Modrinth veröffentlicht → kommt direkt vom Besitzer.
fn is_unpublished(entry: &SharedContentEntry) -> bool {
    entry.source.as_deref() != Some("modrinth")
}

fn same_entry(a: &SharedContentEntry, b: &SharedContentEntry) -> bool {
    a.sha1 == b.sha1 && a.disabled == b.disabled && a.filename == b.filename
}

async fn load_state_snapshot<T>(f: impl FnOnce(&FriendsState) -> T) -> Result<T, String> {
    let service = service()?;
    let state = service.state.lock().await;
    Ok(f(&state))
}

// ==================== Manifest erstellen ====================

/// Erstellt den aktuellen Stand einer Instanz. Mit `write_blobs` werden Dateien, die der
/// Besitzer selbst ausliefert, in den Snapshot-Speicher kopiert (beim Veröffentlichen).
async fn build_manifest(
    profile: &Profile,
    share_id: &str,
    revision: u64,
    config_files: &[String],
    write_blobs: bool,
) -> Result<ShareManifest, String> {
    let mut manifest = ShareManifest {
        share_id: share_id.to_string(),
        revision,
        name: profile.name.clone(),
        icon: profile
            .icon_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string()),
        minecraft_version: profile.minecraft_version.clone(),
        loader: profile.loader.loader.to_string().to_lowercase(),
        loader_version: profile.loader.version.clone(),
        ..Default::default()
    };

    for kind in ALL_KINDS {
        let mut entries =
            content_share::export_content_from(&profile.game_dir, kind, false).await?;
        let game_dir = profile.game_dir.clone();
        entries =
            tokio::task::spawn_blocking(move || -> Result<Vec<SharedContentEntry>, String> {
                for entry in entries
                    .iter_mut()
                    .filter(|e| e.source.as_deref() == Some("local"))
                {
                    let path = content_path(&game_dir, kind, entry)
                        .ok_or_else(|| "Ungültiger Dateiname".to_string())?;
                    let bytes = if entry.is_folder {
                        content_share::zip_folder(&path).map_err(|e| e.to_string())?
                    } else {
                        std::fs::read(&path).map_err(|e| format!("{}: {}", path.display(), e))?
                    };
                    let sha1 = content_share::sha1_hex(&bytes);
                    if write_blobs {
                        write_blob(&sha1, &bytes)?;
                    }
                    entry.size = Some(bytes.len() as u64);
                    entry.sha1 = Some(sha1);
                    entry.source = Some("share".to_string());
                }
                Ok(entries)
            })
            .await
            .map_err(|e| e.to_string())??;
        *manifest.content_mut(kind) = entries;
    }

    let game_dir = profile.game_dir.clone();
    let config_files = config_files.to_vec();
    manifest.configs = tokio::task::spawn_blocking(move || -> Result<Vec<ConfigEntry>, String> {
        let mut configs = Vec::new();
        for path in config_files {
            let Some(rel) = safe_config_path(&path) else {
                continue;
            };
            // Gelöschte Dateien werden einfach nicht mehr geteilt
            let Ok(bytes) = std::fs::read(game_dir.join(&rel)) else {
                continue;
            };
            let sha1 = content_share::sha1_hex(&bytes);
            if write_blobs {
                write_blob(&sha1, &bytes)?;
            }
            configs.push(ConfigEntry {
                path,
                sha1,
                size: bytes.len() as u64,
            });
        }
        Ok(configs)
    })
    .await
    .map_err(|e| e.to_string())??;

    Ok(manifest)
}

// ==================== Änderungen ====================

#[derive(Debug, Serialize)]
pub struct DiffItem {
    pub kind: String,
    pub name: String,
    pub old_version: Option<String>,
    pub new_version: Option<String>,
    /// Datei ist nicht auf Modrinth veröffentlicht (kommt direkt vom Besitzer)
    pub unpublished: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct ShareDiff {
    pub added: Vec<DiffItem>,
    pub removed: Vec<DiffItem>,
    pub updated: Vec<DiffItem>,
    pub configs_added: Vec<String>,
    pub configs_removed: Vec<String>,
    pub configs_changed: Vec<String>,
    /// z.B. "1.20.1 fabric → 1.21.1 fabric"
    pub version_change: Option<String>,
}

impl ShareDiff {
    fn has_content_changes(&self) -> bool {
        !self.added.is_empty()
            || !self.removed.is_empty()
            || !self.updated.is_empty()
            || self.version_change.is_some()
    }

    fn has_config_changes(&self) -> bool {
        !self.configs_added.is_empty()
            || !self.configs_removed.is_empty()
            || !self.configs_changed.is_empty()
    }

    fn is_empty(&self) -> bool {
        !self.has_content_changes() && !self.has_config_changes()
    }
}

fn diff_manifests(old: &ShareManifest, new: &ShareManifest) -> ShareDiff {
    let mut diff = ShareDiff::default();

    let old_version = format!(
        "{} {} {}",
        old.minecraft_version, old.loader, old.loader_version
    );
    let new_version = format!(
        "{} {} {}",
        new.minecraft_version, new.loader, new.loader_version
    );
    if old_version != new_version {
        diff.version_change = Some(format!("{} → {}", old_version.trim(), new_version.trim()));
    }

    for kind in ALL_KINDS {
        let old_map: HashMap<String, &SharedContentEntry> = old
            .content(kind)
            .iter()
            .map(|e| (entry_key(e), e))
            .collect();
        let new_map: HashMap<String, &SharedContentEntry> = new
            .content(kind)
            .iter()
            .map(|e| (entry_key(e), e))
            .collect();

        for (key, entry) in &new_map {
            match old_map.get(key) {
                None => diff.added.push(DiffItem {
                    kind: kind.as_str().to_string(),
                    name: entry_name(entry),
                    old_version: None,
                    new_version: entry_version(entry),
                    unpublished: is_unpublished(entry),
                }),
                Some(old_entry) if !same_entry(old_entry, entry) => diff.updated.push(DiffItem {
                    kind: kind.as_str().to_string(),
                    name: entry_name(entry),
                    old_version: entry_version(old_entry),
                    new_version: entry_version(entry),
                    unpublished: is_unpublished(entry),
                }),
                Some(_) => {}
            }
        }
        for (key, entry) in &old_map {
            if !new_map.contains_key(key) {
                diff.removed.push(DiffItem {
                    kind: kind.as_str().to_string(),
                    name: entry_name(entry),
                    old_version: entry_version(entry),
                    new_version: None,
                    unpublished: is_unpublished(entry),
                });
            }
        }
    }

    let old_configs: HashMap<&str, &str> = old
        .configs
        .iter()
        .map(|c| (c.path.as_str(), c.sha1.as_str()))
        .collect();
    let new_configs: HashMap<&str, &str> = new
        .configs
        .iter()
        .map(|c| (c.path.as_str(), c.sha1.as_str()))
        .collect();
    for (path, sha1) in &new_configs {
        match old_configs.get(path) {
            None => diff.configs_added.push(path.to_string()),
            Some(old_sha1) if old_sha1 != sha1 => diff.configs_changed.push(path.to_string()),
            Some(_) => {}
        }
    }
    for path in old_configs.keys() {
        if !new_configs.contains_key(path) {
            diff.configs_removed.push(path.to_string());
        }
    }

    for list in [&mut diff.added, &mut diff.removed, &mut diff.updated] {
        list.sort_by_key(|item| item.name.to_lowercase());
    }
    diff.configs_added.sort();
    diff.configs_removed.sort();
    diff.configs_changed.sort();
    diff
}

// ==================== Anwenden (Mitglied) ====================

#[derive(Debug, Default, Serialize)]
pub struct ApplyResult {
    pub profile_id: String,
    pub installed: usize,
    pub failed: Vec<String>,
    pub warnings: Vec<String>,
}

async fn update_profile_version(
    profile_id: &str,
    manifest: &ShareManifest,
) -> Result<Profile, String> {
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    let mut profiles = manager.load_profiles().await.map_err(|e| e.to_string())?;
    let profile = profiles
        .get_profile_mut(profile_id)
        .ok_or_else(|| "Profile not found".to_string())?;
    profile.minecraft_version = manifest.minecraft_version.clone();
    profile.loader.loader = parse_loader(&manifest.loader)?;
    profile.loader.version = manifest.loader_version.clone();
    profile.loader.minecraft_version = manifest.minecraft_version.clone();
    let updated = profile.clone();
    manager
        .save_profiles(&profiles)
        .await
        .map_err(|e| e.to_string())?;
    Ok(updated)
}

/// Bringt eine Instanz vom Stand `old` (None = neu) auf den Stand `new`.
/// Eigene Inhalte des Mitglieds bleiben unangetastet.
async fn apply_manifest(
    service: &FriendsService,
    owner_id: &str,
    mut profile: Profile,
    old: Option<&ShareManifest>,
    new: &ShareManifest,
) -> Result<ApplyResult, String> {
    let mut result = ApplyResult {
        profile_id: profile.id.clone(),
        ..Default::default()
    };

    if let Some(old) = old {
        if old.minecraft_version != new.minecraft_version
            || old.loader != new.loader
            || old.loader_version != new.loader_version
        {
            profile = update_profile_version(&profile.id, new).await?;
        }
    }
    let loader = profile.loader.loader.to_string().to_lowercase();

    for kind in ALL_KINDS {
        let old_entries: HashMap<String, &SharedContentEntry> = old
            .map(|m| m.content(kind).iter().map(|e| (entry_key(e), e)).collect())
            .unwrap_or_default();
        let new_keys: std::collections::HashSet<String> =
            new.content(kind).iter().map(entry_key).collect();

        // Entfernte oder geänderte Inhalte des Besitzers löschen
        for (key, old_entry) in &old_entries {
            let changed = new
                .content(kind)
                .iter()
                .find(|e| entry_key(e) == *key)
                .is_none_or(|e| !same_entry(old_entry, e));
            if !new_keys.contains(key) || changed {
                remove_content(&profile.game_dir, kind, old_entry);
            }
        }

        // Neue/geänderte Inhalte installieren (exakte Versionen bzw. Dateien vom Besitzer)
        let mut to_install = Vec::new();
        for entry in new.content(kind) {
            let unchanged = old_entries
                .get(&entry_key(entry))
                .is_some_and(|old_entry| same_entry(old_entry, entry));
            let present = content_path(&profile.game_dir, kind, entry).is_some_and(|p| p.exists());
            if unchanged && present {
                continue;
            }
            let mut entry = entry.clone();
            if entry.source.as_deref() == Some("share") {
                let sha1 = entry.sha1.clone().unwrap_or_default();
                match load_blob(service, owner_id, &new.share_id, &sha1).await {
                    Ok(bytes) => entry.data_base64 = Some(general_purpose::STANDARD.encode(bytes)),
                    Err(e) => {
                        result.failed.push(format!("{}: {}", entry_name(&entry), e));
                        continue;
                    }
                }
            }
            to_install.push(entry);
        }

        if !to_install.is_empty() {
            let (report, _) = content_share::import_content_into(
                &profile.game_dir,
                &profile.minecraft_version,
                &loader,
                kind,
                to_install,
                true,
                |_| {},
            )
            .await?;
            result.installed += report.installed;
            result.failed.extend(
                report
                    .failed
                    .into_iter()
                    .map(|f| format!("{}: {}", f.name, f.reason)),
            );
            result.warnings.extend(report.warnings);
        }
    }

    // Konfig-Dateien schreiben, wenn sie sich vom lokalen Stand unterscheiden
    for config in &new.configs {
        let Some(rel) = safe_config_path(&config.path) else {
            result
                .failed
                .push(format!("{}: ungültiger Pfad", config.path));
            continue;
        };
        let target = profile.game_dir.join(rel);
        let local_sha1 = std::fs::read(&target)
            .ok()
            .map(|b| content_share::sha1_hex(&b));
        if local_sha1.as_deref() == Some(config.sha1.as_str()) {
            continue;
        }
        match load_blob(service, owner_id, &new.share_id, &config.sha1).await {
            Ok(bytes) => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                if let Err(e) = std::fs::write(&target, bytes) {
                    result.failed.push(format!("{}: {}", config.path, e));
                }
            }
            Err(e) => result.failed.push(format!("{}: {}", config.path, e)),
        }
    }

    Ok(result)
}

/// Liefert eine Datei des Besitzers: zuerst aus dem lokalen Speicher (vorab geladen),
/// sonst direkt vom Besitzer – und legt sie dabei für später ab.
async fn load_blob(
    service: &FriendsService,
    owner_id: &str,
    share_id: &str,
    sha1: &str,
) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = tokio::fs::read(blob_path(sha1)).await {
        if content_share::sha1_hex(&bytes).eq_ignore_ascii_case(sha1) {
            return Ok(bytes);
        }
    }
    let bytes = service.fetch_blob(owner_id, share_id, sha1).await?;
    write_blob(sha1, &bytes)?;
    Ok(bytes)
}

/// Lädt den neuesten Stand des Besitzers samt seiner Dateien vorab, damit das Update später
/// auch ohne den Besitzer übernommen werden kann. Gibt den vorgemerkten Stand zurück.
async fn prefetch_update(
    service: &FriendsService,
    owner_id: &str,
    share_id: &str,
) -> Result<Option<ShareManifest>, String> {
    let manifest = manifest_from(
        service,
        owner_id,
        Request::GetManifest {
            share_id: share_id.to_string(),
        },
    )
    .await?;
    let applied = load_state_snapshot(|state| {
        state
            .joined_shares
            .iter()
            .find(|j| j.owner_id == owner_id && j.share_id == share_id)
            .map(|j| j.manifest.revision)
    })
    .await?;
    let Some(applied) = applied else {
        return Ok(None);
    };
    if manifest.revision <= applied {
        return Ok(None);
    }

    let hashes: Vec<String> = manifest.blob_hashes().map(str::to_string).collect();
    for sha1 in hashes {
        load_blob(service, owner_id, share_id, &sha1).await?;
    }

    let mut state = service.state.lock().await;
    if let Some(joined) = state.joined_by_share_mut(owner_id, share_id) {
        joined.latest_revision = manifest.revision;
        joined.pending = Some(manifest.clone());
    }
    state.save();
    drop(state);
    service.notify_changed();
    Ok(Some(manifest))
}

/// Zeitpunkt der letzten Vorab-Prüfung je geteilter Instanz (nicht bei jedem Online-Check).
static LAST_PREFETCH: once_cell::sync::Lazy<std::sync::Mutex<HashMap<String, std::time::Instant>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(HashMap::new()));
const PREFETCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(300);

/// Wird aufgerufen, sobald ein Freund erreichbar ist: offene Update-Hinweise an ihn zustellen
/// und neue Stände seiner geteilten Instanzen vorab laden.
pub(super) async fn on_friend_online(service: &Arc<FriendsService>, friend_id: &str) {
    let (notify, joined): (Vec<(String, u64)>, Vec<String>) = {
        let state = service.state.lock().await;
        (
            state
                .owned_shares
                .iter()
                .filter(|s| s.pending_notify.iter().any(|m| m == friend_id))
                .map(|s| (s.share_id.clone(), s.manifest.revision))
                .collect(),
            state
                .joined_shares
                .iter()
                .filter(|j| j.owner_id == friend_id && !j.ended)
                .map(|j| j.share_id.clone())
                .collect(),
        )
    };

    for (share_id, revision) in notify {
        let request = Request::ShareUpdated {
            share_id: share_id.clone(),
            revision,
        };
        if service.send(friend_id, &request).await.is_ok() {
            let mut state = service.state.lock().await;
            if let Some(share) = state.owned_by_id_mut(&share_id) {
                share.pending_notify.retain(|m| m != friend_id);
            }
            state.save();
        }
    }

    for share_id in joined {
        let key = format!("{friend_id}/{share_id}");
        let due = {
            let mut last = LAST_PREFETCH.lock().unwrap_or_else(|e| e.into_inner());
            let due = last
                .get(&key)
                .is_none_or(|t| t.elapsed() >= PREFETCH_INTERVAL);
            if due {
                last.insert(key, std::time::Instant::now());
            }
            due
        };
        if due {
            if let Err(e) = prefetch_update(service, friend_id, &share_id).await {
                tracing::warn!("Prefetch of shared instance {} failed: {}", share_id, e);
            }
        }
    }
}

fn remove_content(game_dir: &Path, kind: ContentKind, entry: &SharedContentEntry) {
    let Some(filename) = entry
        .filename
        .as_deref()
        .and_then(content_share::safe_file_name)
    else {
        return;
    };
    let dir = game_dir.join(kind.as_str());
    let path = dir.join(&filename);
    if entry.is_folder {
        std::fs::remove_dir_all(&path).ok();
        return;
    }
    std::fs::remove_file(&path).ok();
    std::fs::remove_file(dir.join(format!("{}.disabled", filename))).ok();
}

// ==================== Eingehende Anfragen ====================

pub(super) async fn handle_share_request(
    service: &FriendsService,
    remote: &str,
    request: Request,
) -> Vec<u8> {
    match request {
        Request::PreviewShare { share_id, token } => {
            let state = service.state.lock().await;
            match state.owned_shares.iter().find(|s| s.share_id == share_id) {
                Some(share) if may_join(share, remote, token.as_deref()) => {
                    protocol::encode_ok(&Response::Manifest {
                        manifest: Box::new(share.manifest.clone()),
                    })
                }
                Some(_) => protocol::encode_error("Keine Einladung für diese Instanz"),
                None => protocol::encode_error("Diese Instanz wird nicht mehr geteilt"),
            }
        }
        Request::JoinShare { share_id, token } => {
            let mut state = service.state.lock().await;
            let Some(share) = state.owned_by_id_mut(&share_id) else {
                return protocol::encode_error("Diese Instanz wird nicht mehr geteilt");
            };
            if !may_join(share, remote, token.as_deref()) {
                return protocol::encode_error("Keine Einladung für diese Instanz");
            }
            if !share.members.iter().any(|m| m == remote) {
                share.members.push(remote.to_string());
            }
            share.invited.retain(|m| m != remote);
            share.pending_invites.retain(|m| m != remote);
            let manifest = share.manifest.clone();
            state.save();
            drop(state);
            service.notify_changed();
            protocol::encode_ok(&Response::Manifest {
                manifest: Box::new(manifest),
            })
        }
        Request::GetManifest { share_id } => {
            let state = service.state.lock().await;
            match state.owned_shares.iter().find(|s| s.share_id == share_id) {
                Some(share) if share.members.iter().any(|m| m == remote) => {
                    protocol::encode_ok(&Response::Manifest {
                        manifest: Box::new(share.manifest.clone()),
                    })
                }
                Some(_) => protocol::encode_error("Du bist kein Mitglied dieser Instanz mehr"),
                None => protocol::encode_error("Diese Instanz wird nicht mehr geteilt"),
            }
        }
        Request::GetBlob { share_id, sha1 } => {
            let allowed = {
                let state = service.state.lock().await;
                state
                    .owned_shares
                    .iter()
                    .find(|s| s.share_id == share_id)
                    .is_some_and(|share| {
                        share.members.iter().any(|m| m == remote)
                            && share
                                .manifest
                                .blob_hashes()
                                .any(|h| h.eq_ignore_ascii_case(&sha1))
                    })
            };
            if !allowed || !is_sha1_hex(&sha1) {
                return protocol::encode_error("Kein Zugriff auf diese Datei");
            }
            match tokio::fs::read(blob_path(&sha1)).await {
                Ok(bytes) => protocol::encode_raw(bytes),
                Err(_) => protocol::encode_error("Datei beim Besitzer nicht gefunden"),
            }
        }
        Request::ShareUpdated { share_id, revision } => {
            let mut state = service.state.lock().await;
            if let Some(joined) = state.joined_by_share_mut(remote, &share_id) {
                joined.latest_revision = joined.latest_revision.max(revision);
                state.save();
                drop(state);
                service.notify_changed();
                // Neuen Stand gleich vorab laden, solange der Besitzer online ist
                let owner = remote.to_string();
                tokio::spawn(async move {
                    if let Ok(service) = super::service() {
                        if let Err(e) = prefetch_update(&service, &owner, &share_id).await {
                            tracing::warn!(
                                "Prefetch of shared instance {} failed: {}",
                                share_id,
                                e
                            );
                        }
                    }
                });
            }
            protocol::encode_ok(&Response::Ok)
        }
        Request::LeaveShare { share_id } => {
            let mut state = service.state.lock().await;
            if let Some(share) = state.owned_by_id_mut(&share_id) {
                share.members.retain(|m| m != remote);
                state.save();
                drop(state);
                service.notify_changed();
            }
            protocol::encode_ok(&Response::Ok)
        }
        Request::ShareRemoved { share_id } => {
            let mut state = service.state.lock().await;
            if let Some(joined) = state.joined_by_share_mut(remote, &share_id) {
                joined.ended = true;
                state.save();
                drop(state);
                service.notify_changed();
            }
            protocol::encode_ok(&Response::Ok)
        }
        _ => protocol::encode_error("Unbekannte Anfrage"),
    }
}

/// Eingeladene Freunde, bestehende Mitglieder und Besitzer des Einladungslinks dürfen beitreten.
fn may_join(share: &OwnedShare, remote: &str, token: Option<&str>) -> bool {
    share.members.iter().any(|m| m == remote)
        || share.invited.iter().any(|m| m == remote)
        || token.is_some_and(|t| t == share.link_token)
}

/// Stellt Einladungen zu, die beim Versenden nicht ankamen (Freund war offline).
pub(super) async fn deliver_pending_invites(service: &Arc<FriendsService>) {
    let pending: Vec<(String, String, String, Option<String>)> = {
        let state = service.state.lock().await;
        state
            .owned_shares
            .iter()
            .flat_map(|share| {
                share.pending_invites.iter().map(move |friend| {
                    (
                        friend.clone(),
                        share.share_id.clone(),
                        share.manifest.name.clone(),
                        share.manifest.icon.clone(),
                    )
                })
            })
            .collect()
    };
    for (friend, share_id, name, icon) in pending {
        if send_invite(service, &friend, &share_id, &name, icon)
            .await
            .is_ok()
        {
            let mut state = service.state.lock().await;
            if let Some(share) = state.owned_by_id_mut(&share_id) {
                share.pending_invites.retain(|f| f != &friend);
            }
            for message in state.messages.iter_mut().filter(|m| {
                m.friend_id == friend && m.share_id.as_deref() == Some(share_id.as_str())
            }) {
                message.delivered = true;
            }
            state.save();
        }
    }
}

async fn send_invite(
    service: &FriendsService,
    friend: &str,
    share_id: &str,
    name: &str,
    icon: Option<String>,
) -> Result<(), String> {
    let (owner_name, _) = service.my_identity();
    service
        .send(
            friend,
            &Request::ShareInvite {
                share_id: share_id.to_string(),
                name: name.to_string(),
                icon,
                owner_name,
            },
        )
        .await
        .map(|_| ())
}

/// Benachrichtigt Mitglieder (im Hintergrund, Offline-Mitglieder prüfen beim Spielstart).
fn notify_members(service: Arc<FriendsService>, members: Vec<String>, request: Request) {
    for member in members {
        let service = service.clone();
        let request = request.clone();
        tokio::spawn(async move {
            let _ = service.send(&member, &request).await;
        });
    }
}

async fn manifest_from(
    service: &FriendsService,
    owner_id: &str,
    request: Request,
) -> Result<ShareManifest, String> {
    match service.send(owner_id, &request).await? {
        Response::Manifest { manifest } => Ok(*manifest),
        _ => Err("Unerwartete Antwort des Besitzers".to_string()),
    }
}

// ==================== Ansichten ====================

#[derive(Serialize)]
pub struct MemberView {
    pub id: String,
    pub name: String,
    pub uuid: Option<String>,
    pub online: bool,
}

#[derive(Serialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum ShareStatus {
    None,
    Owner {
        share_id: String,
        link: String,
        revision: u64,
        members: Vec<MemberView>,
        invited: Vec<MemberView>,
        config_files: Vec<String>,
    },
    Member {
        share_id: String,
        owner_id: String,
        owner_name: String,
        owner_online: bool,
        revision: u64,
        latest_revision: u64,
        ended: bool,
        /// Vom Besitzer verwaltete Dateien (für Mitglieder gesperrt)
        managed_mods: Vec<String>,
        managed_resourcepacks: Vec<String>,
        managed_shaderpacks: Vec<String>,
        /// Projekt-IDs der Inhalte des Besitzers (keine andere Version installierbar)
        managed_projects: Vec<String>,
    },
}

fn member_view(service: &FriendsService, state: &FriendsState, id: &str) -> MemberView {
    let friend = state.friend(id);
    MemberView {
        id: id.to_string(),
        name: state.friend_name(id),
        uuid: friend.and_then(|f| f.uuid.clone()),
        online: service.is_online(id),
    }
}

fn managed_files(manifest: &ShareManifest, kind: ContentKind) -> Vec<String> {
    manifest
        .content(kind)
        .iter()
        .filter_map(|e| e.filename.clone())
        .collect()
}

async fn status_for(service: &FriendsService, profile_id: &str) -> ShareStatus {
    let state = service.state.lock().await;
    if let Some(share) = state.owned_by_profile(profile_id) {
        return ShareStatus::Owner {
            share_id: share.share_id.clone(),
            link: share_link(&service.my_id(), share),
            revision: share.manifest.revision,
            members: share
                .members
                .iter()
                .map(|m| member_view(service, &state, m))
                .collect(),
            invited: share
                .invited
                .iter()
                .map(|m| member_view(service, &state, m))
                .collect(),
            config_files: share.config_files.clone(),
        };
    }
    if let Some(joined) = state.joined_by_profile(profile_id) {
        return ShareStatus::Member {
            share_id: joined.share_id.clone(),
            owner_id: joined.owner_id.clone(),
            owner_name: joined.owner_name.clone(),
            owner_online: service.is_online(&joined.owner_id),
            revision: joined.manifest.revision,
            latest_revision: joined.latest_revision.max(joined.manifest.revision),
            ended: joined.ended,
            managed_mods: managed_files(&joined.manifest, ContentKind::Mods),
            managed_resourcepacks: managed_files(&joined.manifest, ContentKind::ResourcePacks),
            managed_shaderpacks: managed_files(&joined.manifest, ContentKind::ShaderPacks),
            managed_projects: ALL_KINDS
                .iter()
                .flat_map(|kind| joined.manifest.content(*kind).iter())
                .filter_map(|e| e.project_id.clone())
                .collect(),
        };
    }
    ShareStatus::None
}

// ==================== Befehle: Besitzer ====================

#[tauri::command]
pub async fn share_status(profile_id: String) -> Result<ShareStatus, String> {
    let service = service()?;
    Ok(status_for(&service, &profile_id).await)
}

#[tauri::command]
pub async fn share_create(profile_id: String) -> Result<ShareStatus, String> {
    let service = service()?;
    if !matches!(status_for(&service, &profile_id).await, ShareStatus::None) {
        return Err("Diese Instanz wird bereits geteilt".to_string());
    }
    let profile = content_share::load_profile(&profile_id).await?;
    let share_id = uuid::Uuid::new_v4().to_string();
    let manifest = build_manifest(&profile, &share_id, 1, &[], true).await?;
    {
        let mut state = service.state.lock().await;
        state.owned_shares.push(OwnedShare {
            share_id,
            profile_id: profile_id.clone(),
            link_token: uuid::Uuid::new_v4().simple().to_string(),
            members: Vec::new(),
            invited: Vec::new(),
            pending_invites: Vec::new(),
            pending_notify: Vec::new(),
            config_files: Vec::new(),
            manifest,
        });
        state.save();
    }
    service.notify_changed();
    Ok(status_for(&service, &profile_id).await)
}

#[derive(Serialize)]
pub struct PendingChanges {
    /// Instanz ist den Mitgliedern voraus
    pub ahead: bool,
    pub content_changed: bool,
    pub configs_changed: bool,
    pub diff: ShareDiff,
}

/// Vergleicht den aktuellen Stand der Instanz mit dem zuletzt veröffentlichten.
#[tauri::command]
pub async fn share_pending_changes(profile_id: String) -> Result<PendingChanges, String> {
    let (share_id, config_files, published) = load_state_snapshot(|state| {
        state.owned_by_profile(&profile_id).map(|s| {
            (
                s.share_id.clone(),
                s.config_files.clone(),
                s.manifest.clone(),
            )
        })
    })
    .await?
    .ok_or_else(|| "Diese Instanz wird nicht geteilt".to_string())?;
    let profile = content_share::load_profile(&profile_id).await?;
    let current = build_manifest(
        &profile,
        &share_id,
        published.revision,
        &config_files,
        false,
    )
    .await?;
    let diff = diff_manifests(&published, &current);
    Ok(PendingChanges {
        ahead: !diff.is_empty(),
        content_changed: diff.has_content_changes(),
        configs_changed: diff.has_config_changes(),
        diff,
    })
}

/// Veröffentlicht den aktuellen Stand. `mode = "configs"` überträgt nur die Konfig-Dateien.
#[tauri::command]
pub async fn share_push(profile_id: String, mode: String) -> Result<ShareStatus, String> {
    let service = service()?;
    let (share_id, config_files, published, members) = load_state_snapshot(|state| {
        state.owned_by_profile(&profile_id).map(|s| {
            (
                s.share_id.clone(),
                s.config_files.clone(),
                s.manifest.clone(),
                s.members.clone(),
            )
        })
    })
    .await?
    .ok_or_else(|| "Diese Instanz wird nicht geteilt".to_string())?;

    let profile = content_share::load_profile(&profile_id).await?;
    let revision = published.revision + 1;
    let mut manifest = build_manifest(&profile, &share_id, revision, &config_files, true).await?;
    if mode == "configs" {
        // Inhalte und Version bleiben auf dem veröffentlichten Stand
        let configs = std::mem::take(&mut manifest.configs);
        manifest = ShareManifest {
            revision,
            configs,
            ..published
        };
    }

    {
        let mut state = service.state.lock().await;
        if let Some(share) = state.owned_by_profile_mut(&profile_id) {
            share.manifest = manifest;
            // Bis zur Bestätigung gilt jedes Mitglied als "noch nicht informiert"
            share.pending_notify = members.clone();
        }
        state.save();
    }
    for member in members {
        let service = service.clone();
        let share_id = share_id.clone();
        tokio::spawn(async move {
            let request = Request::ShareUpdated {
                share_id: share_id.clone(),
                revision,
            };
            // Offline-Mitglieder werden nachträglich informiert, sobald sie erreichbar sind
            if service.send(&member, &request).await.is_ok() {
                let mut state = service.state.lock().await;
                if let Some(share) = state.owned_by_id_mut(&share_id) {
                    share.pending_notify.retain(|m| m != &member);
                }
                state.save();
            }
        });
    }
    service.notify_changed();
    Ok(status_for(&service, &profile_id).await)
}

#[tauri::command]
pub async fn share_invite_friend(profile_id: String, friend_id: String) -> Result<String, String> {
    let service = service()?;
    let (share_id, name, icon) = {
        let mut state = service.state.lock().await;
        if !state.is_accepted_friend(&friend_id) {
            return Err("Nur Freunde können eingeladen werden".to_string());
        }
        let share = state
            .owned_by_profile_mut(&profile_id)
            .ok_or_else(|| "Diese Instanz wird nicht geteilt".to_string())?;
        if share.members.contains(&friend_id) {
            return Ok("Ist bereits Mitglied".to_string());
        }
        if !share.invited.contains(&friend_id) {
            share.invited.push(friend_id.clone());
        }
        let result = (
            share.share_id.clone(),
            share.manifest.name.clone(),
            share.manifest.icon.clone(),
        );
        state.save();
        result
    };
    let delivered = send_invite(&service, &friend_id, &share_id, &name, icon)
        .await
        .is_ok();
    {
        let mut state = service.state.lock().await;
        // Einladung erscheint auch im eigenen Chat mit dem Freund
        state.push_message(store::ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            friend_id: friend_id.clone(),
            outgoing: true,
            text: String::new(),
            sent_at: store::now_rfc3339(),
            delivered,
            read: true,
            share_id: Some(share_id.clone()),
            share_name: Some(name),
        });
        if !delivered {
            if let Some(share) = state.owned_by_profile_mut(&profile_id) {
                if !share.pending_invites.contains(&friend_id) {
                    share.pending_invites.push(friend_id);
                }
            }
        }
        state.save();
    }
    service.notify_changed();

    Ok(if delivered {
        "Einladung gesendet".to_string()
    } else {
        "Dein Freund ist offline – die Einladung wird zugestellt, sobald er online ist".to_string()
    })
}

#[tauri::command]
pub async fn share_remove_member(
    profile_id: String,
    member_id: String,
) -> Result<ShareStatus, String> {
    let service = service()?;
    let share_id = {
        let mut state = service.state.lock().await;
        let share = state
            .owned_by_profile_mut(&profile_id)
            .ok_or_else(|| "Diese Instanz wird nicht geteilt".to_string())?;
        share.members.retain(|m| m != &member_id);
        share.invited.retain(|m| m != &member_id);
        share.pending_invites.retain(|m| m != &member_id);
        let id = share.share_id.clone();
        state.save();
        id
    };
    notify_members(
        service.clone(),
        vec![member_id],
        Request::ShareRemoved { share_id },
    );
    service.notify_changed();
    Ok(status_for(&service, &profile_id).await)
}

#[tauri::command]
pub async fn share_stop(profile_id: String) -> Result<(), String> {
    let service = service()?;
    let share = {
        let mut state = service.state.lock().await;
        let index = state
            .owned_shares
            .iter()
            .position(|s| s.profile_id == profile_id)
            .ok_or_else(|| "Diese Instanz wird nicht geteilt".to_string())?;
        let share = state.owned_shares.remove(index);
        state.save();
        share
    };
    notify_members(
        service.clone(),
        share.members,
        Request::ShareRemoved {
            share_id: share.share_id,
        },
    );
    service.notify_changed();
    Ok(())
}

#[derive(Serialize)]
pub struct ConfigFileView {
    pub path: String,
    pub size: u64,
    pub selected: bool,
}

/// Listet die Konfig-Dateien der Instanz, die geteilt werden können.
#[tauri::command]
pub async fn share_list_config_files(profile_id: String) -> Result<Vec<ConfigFileView>, String> {
    let profile = content_share::load_profile(&profile_id).await?;
    let selected: std::collections::HashSet<String> = load_state_snapshot(|state| {
        state
            .owned_by_profile(&profile_id)
            .map(|s| s.config_files.iter().cloned().collect())
            .unwrap_or_default()
    })
    .await?;

    let game_dir = profile.game_dir.clone();
    let mut files = tokio::task::spawn_blocking(move || {
        let mut files = Vec::new();
        for name in CONFIG_ROOT_FILES {
            if let Ok(meta) = std::fs::metadata(game_dir.join(name)) {
                if meta.is_file() {
                    files.push((name.to_string(), meta.len()));
                }
            }
        }
        for root in CONFIG_ROOTS {
            let root_dir = game_dir.join(root);
            for entry in walkdir::WalkDir::new(&root_dir)
                .max_depth(8)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                if files.len() >= MAX_CONFIG_FILES_LISTED {
                    break;
                }
                let Ok(meta) = entry.metadata() else { continue };
                if meta.len() > MAX_CONFIG_FILE_BYTES {
                    continue;
                }
                let Ok(rel) = entry.path().strip_prefix(&game_dir) else {
                    continue;
                };
                files.push((rel.to_string_lossy().replace('\\', "/"), meta.len()));
            }
        }
        files
    })
    .await
    .map_err(|e| e.to_string())?;

    files.sort_by_key(|a| a.0.to_lowercase());
    Ok(files
        .drain(..)
        .map(|(path, size)| ConfigFileView {
            selected: selected.contains(&path),
            path,
            size,
        })
        .collect())
}

#[tauri::command]
pub async fn share_set_config_files(profile_id: String, paths: Vec<String>) -> Result<(), String> {
    let service = service()?;
    let mut paths: Vec<String> = paths
        .into_iter()
        .filter(|p| safe_config_path(p).is_some())
        .collect();
    paths.sort();
    paths.dedup();
    let mut state = service.state.lock().await;
    let share = state
        .owned_by_profile_mut(&profile_id)
        .ok_or_else(|| "Diese Instanz wird nicht geteilt".to_string())?;
    share.config_files = paths;
    state.save();
    Ok(())
}

// ==================== Befehle: Eingeladene / Mitglieder ====================

#[derive(Serialize)]
pub struct PreviewItem {
    pub kind: String,
    pub name: String,
    pub version: Option<String>,
    pub unpublished: bool,
}

#[derive(Serialize)]
pub struct SharePreview {
    pub owner_id: String,
    pub owner_name: String,
    pub share_id: String,
    pub name: String,
    pub icon: Option<String>,
    pub minecraft_version: String,
    pub loader: String,
    pub loader_version: String,
    pub items: Vec<PreviewItem>,
    pub configs: Vec<String>,
    pub unpublished_count: usize,
}

/// Liest einen Einladungslink `lion://share/<besitzer>/<share-id>/<token>`.
fn parse_share_link(link: &str) -> Result<(String, String, String), String> {
    let rest = link
        .trim()
        .strip_prefix("lion://share/")
        .ok_or_else(|| "Kein gültiger Einladungslink".to_string())?;
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    match parts.as_slice() {
        [owner, share_id, token]
            if !owner.is_empty() && !share_id.is_empty() && !token.is_empty() =>
        {
            Ok((owner.to_string(), share_id.to_string(), token.to_string()))
        }
        _ => Err("Kein gültiger Einladungslink".to_string()),
    }
}

/// Vorschau einer Einladung. Entweder per `link` oder per `owner_id` + `share_id`
/// (Einladung eines Freundes).
#[tauri::command]
pub async fn share_preview(
    link: Option<String>,
    owner_id: Option<String>,
    share_id: Option<String>,
) -> Result<SharePreview, String> {
    let service = service()?;
    let (owner_id, share_id, token) = match link {
        Some(link) => {
            let (owner, share, token) = parse_share_link(&link)?;
            (owner, share, Some(token))
        }
        None => (
            owner_id.ok_or_else(|| "Besitzer fehlt".to_string())?,
            share_id.ok_or_else(|| "Instanz fehlt".to_string())?,
            None,
        ),
    };
    if owner_id == service.my_id() {
        return Err("Das ist deine eigene Instanz".to_string());
    }

    let owner_name = match service.send(&owner_id, &service.hello_request()).await? {
        Response::Hello { name, .. } => name,
        _ => load_state_snapshot(|s| s.friend_name(&owner_id)).await?,
    };
    let manifest = manifest_from(
        &service,
        &owner_id,
        Request::PreviewShare {
            share_id: share_id.clone(),
            token,
        },
    )
    .await?;

    let mut items = Vec::new();
    for kind in ALL_KINDS {
        for entry in manifest.content(kind) {
            items.push(PreviewItem {
                kind: kind.as_str().to_string(),
                name: entry_name(entry),
                version: entry.version_number.clone(),
                unpublished: is_unpublished(entry),
            });
        }
    }
    let unpublished_count = items.iter().filter(|i| i.unpublished).count();
    Ok(SharePreview {
        owner_id,
        owner_name,
        share_id,
        name: manifest.name,
        icon: manifest.icon,
        minecraft_version: manifest.minecraft_version,
        loader: manifest.loader,
        loader_version: manifest.loader_version,
        items,
        configs: manifest.configs.into_iter().map(|c| c.path).collect(),
        unpublished_count,
    })
}

/// Nimmt eine Einladung an: tritt bei, legt das Profil an und installiert alles.
#[tauri::command]
pub async fn share_accept(
    link: Option<String>,
    owner_id: Option<String>,
    share_id: Option<String>,
) -> Result<ApplyResult, String> {
    let service = service()?;
    let (owner_id, share_id, token) = match link {
        Some(link) => {
            let (owner, share, token) = parse_share_link(&link)?;
            (owner, share, Some(token))
        }
        None => (
            owner_id.ok_or_else(|| "Besitzer fehlt".to_string())?,
            share_id.ok_or_else(|| "Instanz fehlt".to_string())?,
            None,
        ),
    };

    let existing = load_state_snapshot(|state| {
        state
            .joined_shares
            .iter()
            .find(|j| j.owner_id == owner_id && j.share_id == share_id)
            .map(|j| j.profile_id.clone())
    })
    .await?;
    if let Some(profile_id) = existing {
        return Err(format!(
            "Du bist dieser Instanz bereits beigetreten (Profil {})",
            profile_id
        ));
    }

    let owner_name = match service.send(&owner_id, &service.hello_request()).await? {
        Response::Hello { name, .. } => name,
        _ => load_state_snapshot(|s| s.friend_name(&owner_id)).await?,
    };
    let manifest = manifest_from(
        &service,
        &owner_id,
        Request::JoinShare {
            share_id: share_id.clone(),
            token,
        },
    )
    .await?;

    // Profil im Stil des Besitzers anlegen
    let mut profile = Profile::new(
        manifest.name.clone(),
        manifest.minecraft_version.clone(),
        parse_loader(&manifest.loader)?,
        manifest.loader_version.clone(),
    );
    profile.icon_path = manifest.icon.clone().map(PathBuf::from);
    let manager = ProfileManager::new().map_err(|e| e.to_string())?;
    manager
        .create_profile(profile.clone())
        .await
        .map_err(|e| e.to_string())?;

    let result = apply_manifest(&service, &owner_id, profile.clone(), None, &manifest).await?;

    {
        let mut state = service.state.lock().await;
        state
            .invites
            .retain(|i| !(i.owner_id == owner_id && i.share_id == share_id));
        state.joined_shares.push(JoinedShare {
            share_id,
            owner_id,
            owner_name,
            profile_id: profile.id.clone(),
            latest_revision: manifest.revision,
            manifest,
            ended: false,
            pending: None,
        });
        state.save();
    }
    service.notify_changed();
    Ok(result)
}

#[tauri::command]
pub async fn share_decline_invite(owner_id: String, share_id: String) -> Result<(), String> {
    let service = service()?;
    {
        let mut state = service.state.lock().await;
        state
            .invites
            .retain(|i| !(i.owner_id == owner_id && i.share_id == share_id));
        state.save();
    }
    service.notify_changed();
    Ok(())
}

#[derive(Serialize)]
pub struct UpdateCheck {
    pub available: bool,
    pub owner_online: bool,
    pub ended: bool,
    pub revision: u64,
    pub latest_revision: u64,
    pub diff: Option<ShareDiff>,
    pub message: Option<String>,
}

/// Prüft, ob es für eine geteilte Instanz einen neuen Stand gibt. Ist der Besitzer online, wird
/// sein neuester Stand (samt Dateien) vorab geladen; sonst wird ein früher vorab geladener Stand
/// verwendet – so kommen Updates auch an, wenn der Besitzer gerade offline ist.
async fn check_update_for(
    service: &FriendsService,
    profile_id: &str,
) -> Result<UpdateCheck, String> {
    let joined = load_state_snapshot(|state| state.joined_by_profile(profile_id).cloned())
        .await?
        .ok_or_else(|| "Diese Instanz ist keine geteilte Instanz".to_string())?;
    let revision = joined.manifest.revision;
    let mut check = UpdateCheck {
        available: false,
        owner_online: false,
        ended: joined.ended,
        revision,
        latest_revision: joined.latest_revision.max(revision),
        diff: None,
        message: None,
    };
    if joined.ended {
        check.message = Some(format!(
            "{} teilt diese Instanz nicht mehr",
            joined.owner_name
        ));
        return Ok(check);
    }

    let pending = match prefetch_update(service, &joined.owner_id, &joined.share_id).await {
        Ok(fresh) => {
            check.owner_online = true;
            fresh.or(joined.pending.clone())
        }
        Err(e) => {
            check.message = Some(e);
            joined.pending.clone()
        }
    };

    if let Some(pending) = pending.filter(|p| p.revision > revision) {
        check.latest_revision = pending.revision;
        check.diff = Some(diff_manifests(&joined.manifest, &pending));
        check.available = true;
    }
    Ok(check)
}

/// Prüft eine geteilte Instanz (z.B. vor dem Spielstart).
#[tauri::command]
pub async fn share_check_update(profile_id: String) -> Result<UpdateCheck, String> {
    let service = service()?;
    check_update_for(&service, &profile_id).await
}

#[derive(Serialize)]
pub struct SharedUpdate {
    pub profile_id: String,
    pub name: String,
    pub owner_name: String,
    pub check: UpdateCheck,
}

/// Prüft beim Start alle geteilten Instanzen nacheinander und liefert die mit Update.
#[tauri::command]
pub async fn share_check_all_updates() -> Result<Vec<SharedUpdate>, String> {
    let service = service()?;
    let joined: Vec<(String, String, String)> = load_state_snapshot(|state| {
        state
            .joined_shares
            .iter()
            .filter(|j| !j.ended)
            .map(|j| {
                (
                    j.profile_id.clone(),
                    j.manifest.name.clone(),
                    j.owner_name.clone(),
                )
            })
            .collect()
    })
    .await?;

    let mut updates = Vec::new();
    for (profile_id, name, owner_name) in joined {
        match check_update_for(&service, &profile_id).await {
            Ok(check) if check.available => updates.push(SharedUpdate {
                profile_id,
                name,
                owner_name,
                check,
            }),
            Ok(_) => {}
            Err(e) => tracing::warn!("Update check for shared instance failed: {}", e),
        }
    }
    Ok(updates)
}

/// Übernimmt den vorab geladenen neuen Stand einer geteilten Instanz.
#[tauri::command]
pub async fn share_apply_update(profile_id: String) -> Result<ApplyResult, String> {
    let service = service()?;
    let joined = load_state_snapshot(|state| state.joined_by_profile(&profile_id).cloned())
        .await?
        .ok_or_else(|| "Diese Instanz ist keine geteilte Instanz".to_string())?;
    let new = joined
        .pending
        .clone()
        .filter(|p| p.revision > joined.manifest.revision)
        .ok_or_else(|| "Kein Update vorhanden – bitte erneut prüfen".to_string())?;
    let profile = content_share::load_profile(&profile_id).await?;

    let result = apply_manifest(
        &service,
        &joined.owner_id,
        profile,
        Some(&joined.manifest),
        &new,
    )
    .await?;

    {
        let mut state = service.state.lock().await;
        if let Some(j) = state.joined_by_profile_mut(&profile_id) {
            j.latest_revision = new.revision;
            j.manifest = new;
            j.pending = None;
        }
        state.save();
    }
    service.notify_changed();
    Ok(result)
}

/// Verlässt eine geteilte Instanz. Das Profil bleibt als normale Instanz erhalten.
#[tauri::command]
pub async fn share_leave(profile_id: String) -> Result<(), String> {
    let service = service()?;
    let joined = {
        let mut state = service.state.lock().await;
        let index = state
            .joined_shares
            .iter()
            .position(|j| j.profile_id == profile_id)
            .ok_or_else(|| "Diese Instanz ist keine geteilte Instanz".to_string())?;
        let joined = state.joined_shares.remove(index);
        state.save();
        joined
    };
    let request = Request::LeaveShare {
        share_id: joined.share_id,
    };
    let _ = service.send(&joined.owner_id, &request).await;
    service.notify_changed();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(project: &str, sha1: &str, version: &str) -> SharedContentEntry {
        SharedContentEntry {
            filename: Some(format!("{project}-{version}.jar")),
            project_id: Some(project.to_string()),
            sha1: Some(sha1.to_string()),
            version_number: Some(version.to_string()),
            source: Some("modrinth".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn config_paths_are_restricted() {
        assert!(safe_config_path("config/sodium-options.json").is_some());
        assert!(safe_config_path("options.txt").is_some());
        assert!(safe_config_path("kubejs/server_scripts/a.js").is_some());
        assert!(safe_config_path("../evil.txt").is_none());
        assert!(safe_config_path("config/../../evil").is_none());
        assert!(safe_config_path("/etc/passwd").is_none());
        assert!(safe_config_path("C:/Windows/evil.dll").is_none());
        assert!(safe_config_path("mods/evil.jar").is_none());
        assert!(safe_config_path("evil.txt").is_none());
    }

    #[test]
    fn diff_detects_added_removed_updated() {
        let old = ShareManifest {
            mods: vec![entry("jei", "a", "1.0"), entry("sodium", "b", "0.5")],
            configs: vec![ConfigEntry {
                path: "config/a.json".into(),
                sha1: "x".into(),
                size: 1,
            }],
            ..Default::default()
        };
        let new = ShareManifest {
            mods: vec![entry("jei", "c", "1.1"), entry("iris", "d", "1.7")],
            configs: vec![ConfigEntry {
                path: "config/a.json".into(),
                sha1: "y".into(),
                size: 1,
            }],
            ..Default::default()
        };
        let diff = diff_manifests(&old, &new);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].name, "iris-1.7.jar");
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.updated.len(), 1);
        assert_eq!(diff.updated[0].old_version.as_deref(), Some("1.0"));
        assert_eq!(diff.updated[0].new_version.as_deref(), Some("1.1"));
        assert_eq!(diff.configs_changed, vec!["config/a.json".to_string()]);
        assert!(diff_manifests(&new, &new).is_empty());
    }

    #[test]
    fn parses_share_links() {
        let (owner, share, token) = parse_share_link("lion://share/abc/def/ghi").unwrap();
        assert_eq!(
            (owner.as_str(), share.as_str(), token.as_str()),
            ("abc", "def", "ghi")
        );
        assert!(parse_share_link("lion://share/abc/def").is_err());
        assert!(parse_share_link("https://example.com").is_err());
    }

    #[test]
    fn may_join_requires_invite_or_token() {
        let share = OwnedShare {
            share_id: "s".into(),
            profile_id: "p".into(),
            link_token: "secret".into(),
            members: vec!["m".into()],
            invited: vec!["i".into()],
            pending_invites: vec![],
            pending_notify: vec![],
            config_files: vec![],
            manifest: ShareManifest::default(),
        };
        assert!(may_join(&share, "m", None));
        assert!(may_join(&share, "i", None));
        assert!(may_join(&share, "x", Some("secret")));
        assert!(!may_join(&share, "x", Some("wrong")));
        assert!(!may_join(&share, "x", None));
    }
}
