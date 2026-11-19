//! „Zusammen spielen“: Freunde in eine Einzelspieler-Welt oder auf einen lokalen Server einladen.
//!
//! 1. Der Gastgeber lädt einen Freund ein. Der Launcher stellt das **Mindest-Modpaket** zusammen:
//!    nur Mods, die laut Modrinth auf dem Client nötig sind und auch auf dem Server laufen
//!    (reine Client-Mods wie Sodium oder Minimaps fehlen bewusst). Mods, die nicht auf Modrinth
//!    sind, werden direkt vom Gastgeber übertragen. Ohne Mods bekommt der Freund nur das Profil
//!    (Minecraft-Version und Loader).
//! 2. Die Einladung erscheint im Chat. Nimmt der Freund an, legt sein Launcher ein passendes
//!    Profil an bzw. bringt es auf den Stand des Gastgebers.
//! 3. Die Spielverbindung läuft über den P2P-Tunnel ([`super::tunnel`]): keine Portfreigabe,
//!    kein Router. Für Welten muss der Gastgeber sie im Spiel „im LAN öffnen“.

use super::protocol::{Request, Response};
use super::share::{self, ApplyResult};
use super::store::{
    self, ChatMessage, HostedSession, JoinedSession, PlayInviteInfo, ShareManifest,
};
use super::{service, FriendsService};
use crate::core::profiles::ProfileManager;
use crate::gui::content_share::SharedContentEntry;
use crate::types::profile::Profile;
use crate::types::version::ModLoader;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub const KIND_WORLD: &str = "world";
pub const KIND_SERVER: &str = "server";

// ── Gastgeber ────────────────────────────────────────────────────────────────

/// Port, über den Minecraft die Welt im LAN anbietet – aus `logs/latest.log`
/// (`Started serving on <port>`), nur solange das Spiel läuft.
fn lan_port_from_log(log: &str) -> Option<u16> {
    let mut port = None;
    for line in log.lines() {
        if let Some((_, rest)) = line.split_once("Started serving on ") {
            port = rest.trim().trim_end_matches('.').parse().ok();
        } else if let Some((_, rest)) = line.split_once("hosted on port ") {
            port = rest
                .trim()
                .trim_end_matches('.')
                .split_whitespace()
                .next()
                .and_then(|p| p.parse().ok());
        } else if line.contains("Stopping singleplayer server") || line.contains("Stopping server")
        {
            port = None;
        }
    }
    port
}

fn lan_port(profile_id: &str, game_dir: &Path) -> Option<u16> {
    if !crate::core::minecraft::get_running_profile_ids()
        .iter()
        .any(|id| id == profile_id)
    {
        return None;
    }
    let log = std::fs::read(game_dir.join("logs").join("latest.log")).ok()?;
    lan_port_from_log(&String::from_utf8_lossy(&log))
}

/// Lokaler Port, mit dem der Tunnel eine Spielverbindung des Freundes verbindet.
pub(super) async fn tunnel_target(
    service: &FriendsService,
    remote: &str,
    session_id: &str,
) -> Result<u16, String> {
    let session = {
        let state = service.state.lock().await;
        if !state.is_accepted_friend(remote) {
            return Err("Nur Freunde können beitreten".into());
        }
        state
            .hosted_sessions
            .iter()
            .find(|s| s.session_id == session_id)
            .cloned()
            .ok_or("Diese Einladung gilt nicht mehr")?
    };
    if !session.invited.iter().any(|f| f == remote) {
        return Err("Du bist nicht eingeladen".into());
    }
    match session.kind.as_str() {
        KIND_WORLD => {
            let profile = crate::gui::content_share::load_profile(&session.target_id).await?;
            lan_port(&profile.id, &profile.game_dir).ok_or_else(|| {
                format!(
                    "„{}“ ist gerade nicht für Freunde offen – dein Freund muss die Welt starten und im Spiel „Im LAN öffnen“ wählen",
                    session.title
                )
            })
        }
        KIND_SERVER => {
            let server =
                crate::gui::server_manager::load_server_profile_owned(&session.target_id).await?;
            if crate::gui::server_manager::is_server_running(&server.id) {
                Ok(server.port)
            } else {
                Err(format!("Der Server „{}“ läuft gerade nicht", server.name))
            }
        }
        _ => Err("Unbekannte Einladung".into()),
    }
}

#[derive(Deserialize)]
struct ProjectSides {
    id: String,
    #[serde(default)]
    client_side: String,
    #[serde(default)]
    server_side: String,
}

/// Nur Mods, die ein beitretender Spieler braucht: auf dem Client erforderlich und auf dem
/// Server lauffähig. Unbekannte Mods (nicht auf Modrinth) bleiben sicherheitshalber drin.
async fn needed_by_guest(entries: Vec<SharedContentEntry>) -> Vec<SharedContentEntry> {
    let entries: Vec<SharedContentEntry> = entries.into_iter().filter(|e| !e.disabled).collect();
    let project = |e: &SharedContentEntry| e.project_id.clone().or_else(|| e.mod_id.clone());
    let ids: Vec<String> = entries.iter().filter_map(project).collect();
    let mut sides: HashMap<String, ProjectSides> = HashMap::new();
    for chunk in ids.chunks(100) {
        let Ok(ids_json) = serde_json::to_string(chunk) else {
            continue;
        };
        let response = crate::core::http::HTTP_CLIENT
            .get("https://api.modrinth.com/v2/projects")
            .query(&[("ids", ids_json)])
            .send()
            .await;
        if let Ok(list) = match response {
            Ok(r) => r.json::<Vec<ProjectSides>>().await,
            Err(e) => Err(e),
        } {
            sides.extend(list.into_iter().map(|p| (p.id.clone(), p)));
        }
    }
    entries
        .into_iter()
        .filter(|e| match project(e).and_then(|id| sides.get(&id)) {
            Some(p) => p.client_side == "required" && p.server_side != "unsupported",
            None => true,
        })
        .collect()
}

/// Manifest für den Gast: Version, Loader und Mindest-Modpaket (keine Packs, keine Konfigs).
async fn session_manifest(
    kind: &str,
    target_id: &str,
    title: &str,
    session_id: &str,
    revision: u64,
) -> Result<ShareManifest, String> {
    let profile = match kind {
        KIND_WORLD => crate::gui::content_share::load_profile(target_id).await?,
        KIND_SERVER => {
            let server = crate::gui::server_manager::load_server_profile_owned(target_id).await?;
            let loader = match server.software.as_str() {
                "fabric" => ModLoader::Fabric,
                "quilt" => ModLoader::Quilt,
                "forge" => ModLoader::Forge,
                "neoforge" => ModLoader::NeoForge,
                // Plugin-Server und Vanilla: Spieler brauchen ein normales Minecraft
                _ => ModLoader::Vanilla,
            };
            let version = if loader == ModLoader::Vanilla {
                String::new()
            } else {
                server.software_version.clone()
            };
            let mut profile = Profile::new(
                server.name.clone(),
                server.minecraft_version.clone(),
                loader,
                version,
            );
            profile.game_dir = server.server_dir.clone();
            profile
        }
        _ => return Err("Unbekannte Einladung".into()),
    };
    if profile.loader.loader == ModLoader::Horizon {
        return Err("Horizon-Welten lassen sich (noch) nicht so teilen".into());
    }
    let mut manifest = share::build_manifest(&profile, session_id, revision, &[], true).await?;
    manifest.name = title.to_string();
    manifest.resourcepacks.clear();
    manifest.shaderpacks.clear();
    manifest.configs.clear();
    if profile.loader.loader == ModLoader::Vanilla {
        manifest.mods.clear();
    } else {
        manifest.mods = needed_by_guest(std::mem::take(&mut manifest.mods)).await;
    }
    Ok(manifest)
}

#[derive(Serialize)]
pub struct PlayStatus {
    /// Freunde können gerade beitreten
    pub open: bool,
    /// Welt bzw. Server läuft (dann kein „Starten“-Knopf)
    pub running: bool,
    pub detail: String,
}

/// Ist die Welt (bzw. der Server) gerade für eingeladene Freunde erreichbar?
#[tauri::command]
pub async fn play_status(kind: String, target_id: String) -> Result<PlayStatus, String> {
    match kind.as_str() {
        KIND_WORLD => {
            let profile = crate::gui::content_share::load_profile(&target_id).await?;
            let running = crate::core::minecraft::get_running_profile_ids().contains(&profile.id);
            Ok(match lan_port(&profile.id, &profile.game_dir) {
                Some(port) => PlayStatus {
                    open: true,
                    running,
                    detail: format!("Offen für Freunde (LAN-Port {port})"),
                },
                None if running => PlayStatus {
                    open: false,
                    running,
                    detail:
                        "Öffne die Welt im Spiel mit „Im LAN öffnen“, dann können Freunde beitreten"
                            .into(),
                },
                None => PlayStatus {
                    open: false,
                    running,
                    detail: "Starte die Welt und wähle im Spiel „Im LAN öffnen“".into(),
                },
            })
        }
        KIND_SERVER => {
            let running = crate::gui::server_manager::is_server_running(&target_id);
            Ok(PlayStatus {
                open: running,
                running,
                detail: if running {
                    "Der Server läuft – Freunde können beitreten".into()
                } else {
                    "Starte den Server, damit Freunde beitreten können".into()
                },
            })
        }
        _ => Err("Unbekannte Einladung".into()),
    }
}

/// Lädt einen Freund in eine Welt (`kind = "world"`, Ziel = Profil, Titel = Weltordner) oder
/// auf einen lokalen Server (`kind = "server"`, Ziel = Server-Profil) ein.
#[tauri::command]
pub async fn play_invite(
    kind: String,
    target_id: String,
    title: String,
    friend_id: String,
) -> Result<String, String> {
    let service = service()?;
    if !service.state.lock().await.is_accepted_friend(&friend_id) {
        return Err("Nur Freunde können eingeladen werden".into());
    }
    let existing = {
        let state = service.state.lock().await;
        state
            .hosted_sessions
            .iter()
            .find(|s| s.kind == kind && s.target_id == target_id && s.title == title)
            .map(|s| (s.session_id.clone(), s.manifest.revision))
    };
    let (session_id, revision) = match existing {
        Some((id, revision)) => (id, revision + 1),
        None => (uuid::Uuid::new_v4().simple().to_string(), 1),
    };
    // Bei jeder Einladung neu: Mods können sich geändert haben
    let manifest = session_manifest(&kind, &target_id, &title, &session_id, revision).await?;
    let (minecraft_version, loader) = (manifest.minecraft_version.clone(), manifest.loader.clone());
    {
        let mut state = service.state.lock().await;
        let invited = match state
            .hosted_sessions
            .iter()
            .position(|s| s.session_id == session_id)
        {
            Some(index) => state.hosted_sessions.remove(index).invited,
            None => Vec::new(),
        };
        let mut invited = invited;
        if !invited.contains(&friend_id) {
            invited.push(friend_id.clone());
        }
        state.hosted_sessions.push(HostedSession {
            session_id: session_id.clone(),
            kind: kind.clone(),
            target_id,
            title: title.clone(),
            invited,
            manifest,
        });
        state.save();
    }

    let request = Request::PlayInvite {
        session_id: session_id.clone(),
        kind: kind.clone(),
        title: title.clone(),
        minecraft_version,
        loader,
    };
    let delivered = service.send(&friend_id, &request).await.is_ok();
    {
        let mut state = service.state.lock().await;
        state.push_message(ChatMessage {
            id: uuid::Uuid::new_v4().to_string(),
            friend_id: friend_id.clone(),
            outgoing: true,
            text: String::new(),
            sent_at: store::now_rfc3339(),
            delivered,
            read: true,
            share_id: None,
            share_name: None,
            play: Some(PlayInviteInfo {
                session_id,
                title,
                kind,
            }),
        });
        state.save();
    }
    service.notify_changed();
    let name = service.state.lock().await.friend_name(&friend_id);
    if delivered {
        Ok(format!("{name} wurde eingeladen"))
    } else {
        Err(format!(
            "{name} ist gerade nicht erreichbar – zum Zusammenspielen müsst ihr beide online sein"
        ))
    }
}

/// Eingehende Einladung eines Freundes: erscheint im Chat.
pub(super) async fn on_invite(
    service: &FriendsService,
    remote: &str,
    session_id: String,
    kind: String,
    title: String,
) -> Result<(), String> {
    let mut state = service.state.lock().await;
    if !state.is_accepted_friend(remote) {
        return Err("Nur Freunde können einladen".into());
    }
    state.push_message(ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        friend_id: remote.to_string(),
        outgoing: false,
        text: String::new(),
        sent_at: store::now_rfc3339(),
        delivered: true,
        read: false,
        share_id: None,
        share_name: None,
        play: Some(PlayInviteInfo {
            session_id,
            title,
            kind,
        }),
    });
    state.save();
    drop(state);
    service.notify_changed();
    Ok(())
}

/// Manifest einer Sitzung für einen eingeladenen Freund.
pub(super) async fn manifest_for(
    service: &FriendsService,
    remote: &str,
    session_id: &str,
) -> Result<Response, String> {
    let state = service.state.lock().await;
    let session = state
        .hosted_sessions
        .iter()
        .find(|s| s.session_id == session_id)
        .ok_or("Diese Einladung gilt nicht mehr")?;
    if !session.invited.iter().any(|f| f == remote) {
        return Err("Du bist nicht eingeladen".into());
    }
    Ok(Response::Manifest {
        manifest: Box::new(session.manifest.clone()),
    })
}

/// Darf `remote` diese Datei einer Sitzung laden? (für `GetBlob`)
pub(super) fn may_fetch(
    state: &store::FriendsState,
    remote: &str,
    session_id: &str,
    sha1: &str,
) -> bool {
    state.hosted_sessions.iter().any(|s| {
        s.session_id == session_id
            && s.invited.iter().any(|f| f == remote)
            && s.manifest
                .blob_hashes()
                .any(|h| h.eq_ignore_ascii_case(sha1))
    })
}

// ── Gast ─────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct PlayJoin {
    pub profile_id: String,
    /// Adresse für Minecraft (`127.0.0.1:<port>`, über den Tunnel)
    pub address: String,
    pub result: ApplyResult,
}

/// Nimmt eine Einladung an: Profil anlegen bzw. aktualisieren, Mindest-Modpaket installieren,
/// Tunnel öffnen. Gestartet wird Minecraft danach von der Oberfläche (Schnellstart auf `address`).
#[tauri::command]
pub async fn play_join(host_id: String, session_id: String) -> Result<PlayJoin, String> {
    let service = service()?;
    let manifest = match service
        .send(
            &host_id,
            &Request::GetPlayManifest {
                session_id: session_id.clone(),
            },
        )
        .await?
    {
        Response::Manifest { manifest } => *manifest,
        _ => return Err("Unerwartete Antwort".into()),
    };
    let host_name = service.state.lock().await.friend_name(&host_id);
    let previous = {
        let state = service.state.lock().await;
        state
            .joined_sessions
            .iter()
            .find(|j| j.host_id == host_id && j.session_id == session_id)
            .cloned()
    };
    let existing_profile = match &previous {
        Some(joined) => crate::gui::content_share::load_profile(&joined.profile_id)
            .await
            .ok(),
        None => None,
    };
    let (profile, result) = match (existing_profile, &previous) {
        (Some(profile), Some(joined)) => {
            let result = share::apply_manifest(
                &service,
                &host_id,
                profile.clone(),
                Some(&joined.manifest),
                &manifest,
            )
            .await?;
            (profile, result)
        }
        _ => {
            let loader = share::parse_loader(&manifest.loader)?;
            let mut profile = Profile::new(
                format!("{} ({})", manifest.name, host_name),
                manifest.minecraft_version.clone(),
                loader,
                manifest.loader_version.clone(),
            );
            profile.icon_path = manifest.icon.clone().map(std::path::PathBuf::from);
            ProfileManager::new()
                .map_err(|e| e.to_string())?
                .create_profile(profile.clone())
                .await
                .map_err(|e| e.to_string())?;
            let result =
                share::apply_manifest(&service, &host_id, profile.clone(), None, &manifest).await?;
            (profile, result)
        }
    };
    {
        let mut state = service.state.lock().await;
        state
            .joined_sessions
            .retain(|j| !(j.host_id == host_id && j.session_id == session_id));
        state.joined_sessions.push(JoinedSession {
            session_id: session_id.clone(),
            host_id: host_id.clone(),
            profile_id: profile.id.clone(),
            manifest,
        });
        state.save();
    }

    // Erst prüfen, ob der Gastgeber gerade offen hat – sonst startet Minecraft umsonst
    super::tunnel::probe(&service, &host_id, &session_id).await?;
    let port = super::tunnel::open(service.clone(), &host_id, &session_id).await?;
    Ok(PlayJoin {
        profile_id: profile.id,
        address: format!("127.0.0.1:{port}"),
        result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_lan_port_in_log() {
        let log = "[12:00:00] [Server thread/INFO]: Starting integrated minecraft server version 1.21.4\n\
                   [12:01:00] [Server thread/INFO]: Started serving on 51234\n";
        assert_eq!(lan_port_from_log(log), Some(51234));
        let closed = format!("{log}[12:05:00] [Render thread/INFO]: Stopping singleplayer server as player logged out\n");
        assert_eq!(lan_port_from_log(&closed), None);
        let chat = "[Render thread/INFO]: [System] [CHAT] Local game hosted on port 61000\n";
        assert_eq!(lan_port_from_log(chat), Some(61000));
        assert_eq!(lan_port_from_log("nothing here"), None);
    }
}
