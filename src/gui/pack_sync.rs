//! Abgleich von Resource Packs und Shader Packs zwischen Profilen mit aktivem Settings-Sync.
//!
//! Früher wurde nur vereinigt: Ein in Profil A gelöschtes Pack lag noch in Profil B und kam
//! beim nächsten Start zurück. Jetzt merkt sich `sync_state.json`, welche Packs nach dem letzten
//! Abgleich in allen beteiligten Profilen lagen. Fehlt eines davon in einem Profil, das damals
//! dabei war, wurde es gelöscht – und verschwindet überall. Wurde es nach dem letzten Abgleich
//! irgendwo neu hinzugefügt oder geändert, gewinnt dagegen das Hinzufügen.

use crate::types::profile::Profile;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Pack-Ordner und der zugehörige Metadaten-Ordner des Launchers
pub(crate) const SYNCED_PACK_FOLDERS: [(&str, &str); 2] = [
    ("resourcepacks", "resourcepackinfos"),
    ("shaderpacks", "shaderpackinfos"),
];

#[derive(Default, Serialize, Deserialize)]
struct PackSyncState {
    /// Zeitpunkt des letzten Abgleichs (Millisekunden seit Epoch)
    synced_at_ms: u64,
    /// Profile, die beim letzten Abgleich dabei waren
    members: Vec<String>,
    /// Ordner -> Packs, die danach in allen beteiligten Profilen lagen
    packs: BTreeMap<String, BTreeSet<String>>,
}

fn state_path() -> PathBuf {
    crate::config::defaults::launcher_dir().join("sync_state.json")
}

fn load_state(path: &Path) -> PackSyncState {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_state(path: &Path, state: &PackSyncState) {
    if let Ok(content) = serde_json::to_string_pretty(state) {
        if let Err(e) = std::fs::write(path, content) {
            tracing::warn!("Failed to save pack sync state: {}", e);
        }
    }
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Ein Pack in einem Profil
#[derive(Clone, Debug)]
struct PackFile {
    path: PathBuf,
    modified_ms: u64,
    size: u64,
}

fn list_packs(dir: &Path) -> HashMap<String, PackFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            // Versteckte Dateien und halb kopierte Reste nicht synchronisieren
            if name.starts_with('.') {
                return None;
            }
            let meta = entry.metadata().ok()?;
            Some((
                name,
                PackFile {
                    path: entry.path(),
                    modified_ms: meta.modified().map(millis).unwrap_or(0),
                    size: if meta.is_dir() { 0 } else { meta.len() },
                },
            ))
        })
        .collect()
}

/// Entscheidet, welche Packs gelöscht werden.
///
/// `listings[i]` gehört zu `member_ids[i]`. Gelöscht wird ein Pack, das nach dem letzten
/// Abgleich überall lag, jetzt aber in einem damals beteiligten Profil fehlt – außer es wurde
/// seitdem irgendwo neu hinzugefügt oder geändert.
fn find_deleted(
    known: &BTreeSet<String>,
    previous_members: &[String],
    synced_at_ms: u64,
    member_ids: &[String],
    listings: &[HashMap<String, PackFile>],
) -> BTreeSet<String> {
    known
        .iter()
        .filter(|name| {
            let removed_somewhere = member_ids
                .iter()
                .zip(listings)
                .any(|(id, listing)| previous_members.contains(id) && !listing.contains_key(*name));
            let changed_since = listings
                .iter()
                .any(|listing| listing.get(*name).is_some_and(|f| f.modified_ms > synced_at_ms));
            removed_somewhere && !changed_since
        })
        .cloned()
        .collect()
}

fn remove_pack(game_dir: &Path, folder: &str, meta_folder: &str, name: &str) {
    let path = game_dir.join(folder).join(name);
    let result = if path.is_dir() {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    };
    match result {
        Ok(()) => tracing::info!("Sync: removed {} from {:?}", name, game_dir.join(folder)),
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            tracing::warn!("Sync: failed to remove {:?}: {}", path, e)
        }
        Err(_) => {}
    }
    let _ = std::fs::remove_file(game_dir.join(meta_folder).join(format!("{}.json", name)));
}

fn copy_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(src, dst).map(|_| ())
    }
}

/// Kopiert ein Pack samt Launcher-Metadaten (Icon, Titel) in ein anderes Profil.
fn copy_pack(source_dir: &Path, target_dir: &Path, folder: &str, meta_folder: &str, name: &str) {
    let src = source_dir.join(folder).join(name);
    let dst = target_dir.join(folder).join(name);
    if let Err(e) = std::fs::create_dir_all(target_dir.join(folder)) {
        tracing::warn!("Sync: cannot create {:?}: {}", target_dir.join(folder), e);
        return;
    }
    // Erst unter temporärem Namen kopieren, damit kein halbes Pack übrig bleibt
    let temp = target_dir.join(folder).join(format!(".{}.lion-sync", name));
    let _ = std::fs::remove_dir_all(&temp);
    let _ = std::fs::remove_file(&temp);
    if let Err(e) = copy_recursive(&src, &temp) {
        tracing::warn!("Sync: failed to copy {:?}: {}", src, e);
        let _ = std::fs::remove_dir_all(&temp);
        let _ = std::fs::remove_file(&temp);
        return;
    }
    if dst.is_dir() {
        let _ = std::fs::remove_dir_all(&dst);
    }
    if let Err(e) = std::fs::rename(&temp, &dst) {
        tracing::warn!("Sync: failed to place {:?}: {}", dst, e);
        return;
    }
    let meta_src = source_dir.join(meta_folder).join(format!("{}.json", name));
    if meta_src.exists() {
        let meta_dst_dir = target_dir.join(meta_folder);
        if std::fs::create_dir_all(&meta_dst_dir).is_ok() {
            let _ = std::fs::copy(&meta_src, meta_dst_dir.join(format!("{}.json", name)));
        }
    }
}

/// Gleicht Resource- und Shader Packs aller Profile mit aktivem Sync ab (vor dem Spielstart).
pub(crate) fn sync_pack_folders(profiles: &[Profile]) {
    sync_with_state_file(profiles, &state_path());
}

fn sync_with_state_file(profiles: &[Profile], state_file: &Path) {
    let members: Vec<&Profile> = profiles.iter().filter(|p| p.settings_sync).collect();
    if members.is_empty() {
        return;
    }
    let member_ids: Vec<String> = members.iter().map(|p| p.id.clone()).collect();
    let mut state = load_state(state_file);

    for (folder, meta_folder) in SYNCED_PACK_FOLDERS {
        let listings: Vec<HashMap<String, PackFile>> = members
            .iter()
            .map(|p| list_packs(&p.game_dir.join(folder)))
            .collect();
        let known = state.packs.get(folder).cloned().unwrap_or_default();

        let deleted = find_deleted(
            &known,
            &state.members,
            state.synced_at_ms,
            &member_ids,
            &listings,
        );
        for (profile, listing) in members.iter().zip(&listings) {
            for name in deleted.iter().filter(|n| listing.contains_key(*n)) {
                remove_pack(&profile.game_dir, folder, meta_folder, name);
            }
        }

        // Neueste Fassung jedes verbleibenden Packs bestimmen
        let mut newest: HashMap<&str, (usize, &PackFile)> = HashMap::new();
        for (index, listing) in listings.iter().enumerate() {
            for (name, file) in listing {
                if deleted.contains(name) {
                    continue;
                }
                let better = newest
                    .get(name.as_str())
                    .is_none_or(|(_, best)| file.modified_ms > best.modified_ms);
                if better {
                    newest.insert(name.as_str(), (index, file));
                }
            }
        }

        // Fehlende oder veraltete Packs in alle Profile verteilen. Gleich große Dateien gelten
        // als identisch (Kopien bekommen eine neue Änderungszeit und sollen nicht hin- und
        // herwandern).
        let mut copied = 0usize;
        for (name, (source_index, source)) in &newest {
            for (index, profile) in members.iter().enumerate() {
                if index == *source_index {
                    continue;
                }
                // Entpackte Ordner-Packs werden nur verteilt, wenn sie fehlen
                let up_to_date = listings[index].get(*name).is_some_and(|existing| {
                    source.path.is_dir()
                        || existing.size == source.size
                        || existing.modified_ms >= source.modified_ms
                });
                if up_to_date {
                    continue;
                }
                copy_pack(
                    &members[*source_index].game_dir,
                    &profile.game_dir,
                    folder,
                    meta_folder,
                    name,
                );
                copied += 1;
            }
        }
        if copied > 0 || !deleted.is_empty() {
            tracing::info!(
                "Sync {}: {} copied, {} removed everywhere",
                folder,
                copied,
                deleted.len()
            );
        }

        state
            .packs
            .insert(folder.to_string(), newest.keys().map(|n| n.to_string()).collect());
    }

    state.members = member_ids;
    state.synced_at_ms = millis(SystemTime::now());
    save_state(state_file, &state);
}

/// Wird im Launcher ein Pack gelöscht, verschwindet es sofort auch in den anderen Profilen mit
/// aktivem Sync (statt erst beim nächsten Spielstart).
pub(crate) fn propagate_deletion(profiles: &[Profile], source: &Profile, folder: &str, name: &str) {
    if !source.settings_sync {
        return;
    }
    let Some((_, meta_folder)) = SYNCED_PACK_FOLDERS.iter().find(|(f, _)| *f == folder) else {
        return;
    };
    for profile in profiles
        .iter()
        .filter(|p| p.settings_sync && p.id != source.id)
    {
        if profile.game_dir.join(folder).join(name).exists() {
            remove_pack(&profile.game_dir, folder, meta_folder, name);
        }
    }
    let state_file = state_path();
    let mut state = load_state(&state_file);
    if let Some(known) = state.packs.get_mut(folder) {
        known.remove(name);
        save_state(&state_file, &state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(entries: &[(&str, u64)]) -> HashMap<String, PackFile> {
        entries
            .iter()
            .map(|(name, modified_ms)| {
                (
                    name.to_string(),
                    PackFile {
                        path: PathBuf::from(name),
                        modified_ms: *modified_ms,
                        size: 1,
                    },
                )
            })
            .collect()
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn known(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn pack_deleted_in_one_profile_is_deleted_everywhere() {
        let deleted = find_deleted(
            &known(&["a.zip", "b.zip"]),
            &ids(&["p1", "p2"]),
            1_000,
            &ids(&["p1", "p2"]),
            &[listing(&[("b.zip", 500)]), listing(&[("a.zip", 500), ("b.zip", 500)])],
        );
        assert_eq!(deleted, known(&["a.zip"]));
    }

    #[test]
    fn new_sync_member_does_not_delete_anything() {
        let deleted = find_deleted(
            &known(&["a.zip"]),
            &ids(&["p1"]),
            1_000,
            &ids(&["p1", "p2"]),
            &[listing(&[("a.zip", 500)]), listing(&[])],
        );
        assert!(deleted.is_empty());
    }

    #[test]
    fn readded_after_last_sync_wins_over_deletion() {
        let deleted = find_deleted(
            &known(&["a.zip"]),
            &ids(&["p1", "p2"]),
            1_000,
            &ids(&["p1", "p2"]),
            &[listing(&[]), listing(&[("a.zip", 2_000)])],
        );
        assert!(deleted.is_empty());
    }

    #[test]
    fn sync_on_disk_propagates_additions_and_deletions() {
        let root = std::env::temp_dir().join(format!("lion-pack-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let state_file = root.join("sync_state.json");
        let make = |id: &str| {
            let mut p = Profile::new(id.into(), "1.21.1".into(), crate::types::version::ModLoader::Fabric, String::new());
            p.id = id.into();
            p.game_dir = root.join(id);
            std::fs::create_dir_all(p.game_dir.join("resourcepacks")).unwrap();
            std::fs::create_dir_all(p.game_dir.join("shaderpacks")).unwrap();
            p
        };
        let (p1, p2) = (make("p1"), make("p2"));
        let rp = |p: &Profile, n: &str| p.game_dir.join("resourcepacks").join(n);
        std::fs::write(rp(&p1, "a.zip"), "a").unwrap();
        std::fs::write(rp(&p1, "b.zip"), "b").unwrap();
        std::fs::write(p1.game_dir.join("shaderpacks").join("s.zip"), "s").unwrap();
        let profiles = vec![p1.clone(), p2.clone()];

        sync_with_state_file(&profiles, &state_file);
        assert!(rp(&p2, "a.zip").exists() && rp(&p2, "b.zip").exists());
        assert!(p2.game_dir.join("shaderpacks").join("s.zip").exists());

        // In Profil 2 gelöscht -> nach dem Abgleich auch in Profil 1 weg, b bleibt
        std::fs::remove_file(rp(&p2, "a.zip")).unwrap();
        std::fs::remove_file(p2.game_dir.join("shaderpacks").join("s.zip")).unwrap();
        sync_with_state_file(&profiles, &state_file);
        assert!(!rp(&p1, "a.zip").exists() && !rp(&p2, "a.zip").exists());
        assert!(!p1.game_dir.join("shaderpacks").join("s.zip").exists());
        assert!(rp(&p1, "b.zip").exists() && rp(&p2, "b.zip").exists());

        // Neues Pack in Profil 2 kommt in Profil 1 an
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(rp(&p2, "c.zip"), "c").unwrap();
        sync_with_state_file(&profiles, &state_file);
        assert!(rp(&p1, "c.zip").exists());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn first_sync_without_state_only_merges() {
        let deleted = find_deleted(
            &BTreeSet::new(),
            &[],
            0,
            &ids(&["p1", "p2"]),
            &[listing(&[("a.zip", 500)]), listing(&[])],
        );
        assert!(deleted.is_empty());
    }
}
