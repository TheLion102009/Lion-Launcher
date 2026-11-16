//! Eigene Geräte: Die Freundesliste wird zwischen allen Launchern abgeglichen, auf denen
//! derselbe Minecraft-Account (Microsoft) angemeldet ist – ohne eigenen Server.
//!
//! **Auffinden:** Jedes Gerät trägt seine Endpoint-ID in einen signierten pkarr-Eintrag beim
//! öffentlichen iroh-DNS-Dienst ein. Dessen Schlüssel wird aus der Minecraft-UUID abgeleitet,
//! so finden sich die Geräte desselben Accounts gegenseitig.
//!
//! **Vertrauen:** Der abgeleitete Schlüssel ist nicht geheim. Ein dort gefundenes Gerät gilt
//! deshalb erst als eigenes, wenn beide Seiten über den Mojang-Sitzungsserver bewiesen haben,
//! mit demselben Account angemeldet zu sein (`join`/`hasJoined` – dasselbe Verfahren wie beim
//! Betreten eines Minecraft-Servers). Zugangsdaten verlassen dabei nie das Gerät.
//!
//! **Freunde:** Übernommene Freunde kennen das neue Gerät noch nicht. Es stellt sich ihnen mit
//! einer Bestätigung (Signatur) eines bereits bekannten Geräts vor ([`Request::IntroduceDevice`]).

use super::protocol::{Request, Response, SyncData, SyncedFriend};
use super::store::{self, DeviceVoucher, Friend, FriendState, FriendsState, LinkedDevice};
use super::FriendsService;
use iroh::{PublicKey, SecretKey, Signature};
use iroh_dns::pkarr::SignedPacket;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Öffentlicher pkarr-Relay von n0 (derselbe Dienst, über den sich iroh-Endpoints finden).
const PKARR_RELAY: &str = "https://dns.iroh.link/pkarr";
/// DNS-Name der Einträge im pkarr-Paket.
const TXT_NAME: &str = "_lion-devices";
/// Mehr Geräte passen nicht sicher in ein pkarr-Paket (max. 1000 Byte).
const MAX_RECORD_DEVICES: usize = 8;
const RECORD_TTL: u32 = 300;
/// Abstand zwischen zwei Runden des Geräte-Auffindens.
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Erneuter Verknüpfungsversuch mit einem Gerät frühestens nach dieser Zeit.
const LINK_RETRY: Duration = Duration::from_secs(30 * 60);
/// Gültigkeit einer Verknüpfungs-Herausforderung.
const CHALLENGE_TTL: Duration = Duration::from_secs(120);
/// Mindestabstand zwischen zwei Herausforderungen derselben Gegenseite.
const CHALLENGE_COOLDOWN: Duration = Duration::from_secs(20);
const VOUCHER_CONTEXT: &str = "lion-launcher/device-link/v1:";
const SESSION_SERVER: &str = "https://sessionserver.mojang.com/session/minecraft";

/// Flüchtiger Zustand des Geräte-Abgleichs.
#[derive(Default)]
pub(crate) struct DeviceRuntime {
    /// Offene Herausforderungen: Gegenseite → (Nonce, ausgestellt)
    challenges: Mutex<HashMap<String, (String, Instant)>>,
    /// Letzter Verknüpfungsversuch je Gerät
    attempts: Mutex<HashMap<String, Instant>>,
    /// Geräte, deren Verknüpfung fehlgeschlagen ist (werden nicht weiter veröffentlicht)
    failed: Mutex<HashSet<String>>,
    last_discovery: Mutex<Option<Instant>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Serialize)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub last_sync: Option<String>,
}

pub(crate) fn device_name() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "Unbekanntes Gerät".to_string())
}

fn normalize_uuid(uuid: &str) -> String {
    uuid.replace('-', "").to_lowercase()
}

fn random_nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Schlüssel des Geräte-Eintrags eines Accounts (aus der UUID abgeleitet, nicht geheim).
fn discovery_key(uuid: &str) -> SecretKey {
    let digest = Sha256::digest(format!("lion-launcher/devices/v1:{}", normalize_uuid(uuid)));
    SecretKey::from_bytes(&digest.into())
}

/// Server-ID für die Mojang-Sitzung: bindet den Beweis an die Herausforderung und beide Geräte.
fn session_server_id(nonce: &str, prover: &str, verifier: &str) -> String {
    crate::gui::content_share::sha1_hex(
        format!("lion-device-link:{nonce}:{prover}:{verifier}").as_bytes(),
    )
}

fn voucher_message(target: &str) -> Vec<u8> {
    format!("{VOUCHER_CONTEXT}{target}").into_bytes()
}

pub(crate) fn sign_voucher(secret: &SecretKey, target: &str) -> String {
    hex::encode(secret.sign(&voucher_message(target)).to_bytes())
}

/// Prüft, ob `signer` bestätigt hat, dass `target` zum selben Account gehört.
pub(crate) fn verify_voucher(signer: &str, target: &str, signature: &str) -> bool {
    let Ok(key) = signer.parse::<PublicKey>() else {
        return false;
    };
    let Some(bytes) = hex::decode(signature)
        .ok()
        .and_then(|b| <[u8; 64]>::try_from(b.as_slice()).ok())
    else {
        return false;
    };
    key.verify(&voucher_message(target), &Signature::from_bytes(&bytes))
        .is_ok()
}

// ── Mojang-Sitzung ───────────────────────────────────────────────────────────

/// Aktiver Microsoft-Account: (UUID ohne Bindestriche, Name, Access-Token).
async fn microsoft_account() -> Option<(String, String, String)> {
    if !crate::gui::auth::active_account_is_microsoft().await {
        return None;
    }
    let (uuid, name, token) = crate::gui::auth::get_active_access_token_refreshed().await?;
    Some((normalize_uuid(&uuid), name, token))
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("Lion-Launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

async fn mojang_join(token: &str, uuid: &str, server_id: &str) -> Result<(), String> {
    let response = http()
        .post(format!("{SESSION_SERVER}/join"))
        .json(&serde_json::json!({
            "accessToken": token,
            "selectedProfile": uuid,
            "serverId": server_id,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "Mojang-Sitzung fehlgeschlagen ({})",
            response.status()
        ))
    }
}

/// Liefert die UUID des Accounts, der sich mit `server_id` angemeldet hat.
async fn mojang_has_joined(name: &str, server_id: &str) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Profile {
        id: String,
    }
    let response = http()
        .get(format!("{SESSION_SERVER}/hasJoined"))
        .query(&[("username", name), ("serverId", server_id)])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if response.status() != reqwest::StatusCode::OK {
        return Err("Gerät konnte seinen Minecraft-Account nicht nachweisen".to_string());
    }
    let profile: Profile = response.json().await.map_err(|e| e.to_string())?;
    Ok(normalize_uuid(&profile.id))
}

// ── Geräte-Eintrag (pkarr) ───────────────────────────────────────────────────

async fn resolve_record(key: &PublicKey) -> Vec<String> {
    let url = format!("{PKARR_RELAY}/{}", key.to_z32());
    let Ok(response) = http().get(url).send().await else {
        return Vec::new();
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let Ok(payload) = response.bytes().await else {
        return Vec::new();
    };
    SignedPacket::from_relay_payload(key, &payload)
        .map(|packet| packet.txt_records(TXT_NAME))
        .unwrap_or_default()
        .into_iter()
        .filter(|id| id.parse::<PublicKey>().is_ok())
        .collect()
}

async fn publish_record(secret: &SecretKey, ids: &[String]) -> Result<(), String> {
    let packet = SignedPacket::from_txt_strings(secret, TXT_NAME, ids, RECORD_TTL)
        .map_err(|e| e.to_string())?;
    let url = format!("{PKARR_RELAY}/{}", secret.public().to_z32());
    let response = http()
        .put(url)
        .body(packet.to_relay_payload())
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("pkarr: {}", response.status()))
    }
}

/// Neuer Inhalt des Geräte-Eintrags: ich zuerst, dann verknüpfte, dann noch unbekannte Geräte.
fn record_entries(
    my_id: &str,
    linked: &[String],
    found: &[String],
    failed: &HashSet<String>,
) -> Vec<String> {
    let mut ids = vec![my_id.to_string()];
    for id in linked
        .iter()
        .chain(found.iter().filter(|id| !failed.contains(*id)))
    {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids.truncate(MAX_RECORD_DEVICES);
    ids
}

/// Sucht nach weiteren Geräten desselben Accounts (läuft mit dem Online-Check, aber höchstens
/// alle [`DISCOVERY_INTERVAL`]).
pub(crate) async fn discovery_round(service: &Arc<FriendsService>) {
    {
        let mut last = lock(&service.devices.last_discovery);
        if last.is_some_and(|t| t.elapsed() < DISCOVERY_INTERVAL) {
            return;
        }
        *last = Some(Instant::now());
    }
    let Some((uuid, _, _)) = microsoft_account().await else {
        return;
    };
    let secret = discovery_key(&uuid);
    let my_id = service.my_id();
    let found = resolve_record(&secret.public()).await;
    let (linked, blocked): (Vec<String>, Vec<String>) = {
        let state = service.state.lock().await;
        (
            state.devices.iter().map(|d| d.id.clone()).collect(),
            state.blocked_devices.clone(),
        )
    };

    let entries = {
        let failed = lock(&service.devices.failed);
        let found: Vec<String> = found
            .iter()
            .filter(|id| !blocked.contains(id))
            .cloned()
            .collect();
        record_entries(&my_id, &linked, &found, &failed)
    };
    if entries != found {
        if let Err(e) = publish_record(&secret, &entries).await {
            tracing::debug!("Device record not published: {}", e);
        }
    }

    for id in found {
        if id == my_id || linked.contains(&id) || blocked.contains(&id) {
            continue;
        }
        {
            let mut attempts = lock(&service.devices.attempts);
            if attempts.get(&id).is_some_and(|t| t.elapsed() < LINK_RETRY) {
                continue;
            }
            attempts.insert(id.clone(), Instant::now());
        }
        let service = service.clone();
        tokio::spawn(async move {
            match link_with(&service, &id).await {
                Ok(()) => {
                    lock(&service.devices.failed).remove(&id);
                    tracing::info!("Linked own device {}", id);
                }
                Err(e) => {
                    lock(&service.devices.failed).insert(id.clone());
                    tracing::debug!("Device {} not linked: {}", id, e);
                }
            }
        });
    }
}

// ── Verknüpfen ───────────────────────────────────────────────────────────────

/// Verknüpft dieses Gerät mit `peer` (beide Seiten beweisen denselben Account).
async fn link_with(service: &Arc<FriendsService>, peer: &str) -> Result<(), String> {
    let (uuid, name, token) = microsoft_account()
        .await
        .ok_or("Kein Microsoft-Account aktiv")?;
    let my_id = service.my_id();
    let my_nonce = random_nonce();

    let Response::Challenge {
        nonce: peer_nonce,
        name: peer_name,
        device_name: peer_device,
    } = service
        .send(
            peer,
            &Request::LinkChallenge {
                nonce: my_nonce.clone(),
            },
        )
        .await?
    else {
        return Err("Unerwartete Antwort".to_string());
    };

    // Erst den Beweis der Gegenseite prüfen, dann selbst etwas preisgeben
    let peer_uuid =
        mojang_has_joined(&peer_name, &session_server_id(&my_nonce, peer, &my_id)).await?;
    if peer_uuid != uuid {
        return Err("Anderer Minecraft-Account".to_string());
    }
    mojang_join(&token, &uuid, &session_server_id(&peer_nonce, &my_id, peer)).await?;

    let request = Request::LinkDevice {
        nonce: peer_nonce,
        name,
        device_name: device_name(),
        voucher: sign_voucher(service.endpoint.secret_key(), peer),
    };
    let Response::Linked { voucher } = service.send(peer, &request).await? else {
        return Err("Unerwartete Antwort".to_string());
    };
    if !verify_voucher(peer, &my_id, &voucher) {
        return Err("Ungültige Bestätigung".to_string());
    }
    add_device(service, peer, peer_device, voucher).await;
    sync_with(service, peer).await
}

pub(crate) async fn handle_link_challenge(
    service: &FriendsService,
    remote: &str,
    nonce: String,
) -> Result<Response, String> {
    if service
        .state
        .lock()
        .await
        .blocked_devices
        .iter()
        .any(|d| d == remote)
    {
        return Err("Gerät wurde getrennt".to_string());
    }
    {
        let challenges = lock(&service.devices.challenges);
        if challenges
            .get(remote)
            .is_some_and(|(_, t)| t.elapsed() < CHALLENGE_COOLDOWN)
        {
            return Err("Bitte kurz warten".to_string());
        }
    }
    let (uuid, name, token) = microsoft_account()
        .await
        .ok_or("Kein Microsoft-Account aktiv")?;
    let my_id = service.my_id();
    mojang_join(&token, &uuid, &session_server_id(&nonce, &my_id, remote)).await?;

    let my_nonce = random_nonce();
    let mut challenges = lock(&service.devices.challenges);
    challenges.retain(|_, (_, t)| t.elapsed() < CHALLENGE_TTL);
    challenges.insert(remote.to_string(), (my_nonce.clone(), Instant::now()));
    Ok(Response::Challenge {
        nonce: my_nonce,
        name,
        device_name: device_name(),
    })
}

pub(crate) async fn handle_link_device(
    service: &FriendsService,
    remote: &str,
    nonce: String,
    name: String,
    device_name: String,
    voucher: String,
) -> Result<Response, String> {
    let expected = lock(&service.devices.challenges).remove(remote);
    match expected {
        Some((expected, issued)) if expected == nonce && issued.elapsed() < CHALLENGE_TTL => {}
        _ => return Err("Herausforderung abgelaufen".to_string()),
    }
    let (uuid, _, _) = microsoft_account()
        .await
        .ok_or("Kein Microsoft-Account aktiv")?;
    let my_id = service.my_id();
    let peer_uuid = mojang_has_joined(&name, &session_server_id(&nonce, remote, &my_id)).await?;
    if peer_uuid != uuid {
        return Err("Anderer Minecraft-Account".to_string());
    }
    if !verify_voucher(remote, &my_id, &voucher) {
        return Err("Ungültige Bestätigung".to_string());
    }
    add_device(service, remote, device_name, voucher).await;
    Ok(Response::Linked {
        voucher: sign_voucher(service.endpoint.secret_key(), remote),
    })
}

async fn add_device(service: &FriendsService, id: &str, name: String, voucher: String) {
    let mut state = service.state.lock().await;
    match state.devices.iter_mut().find(|d| d.id == id) {
        Some(device) => device.name = name,
        None => state.devices.push(LinkedDevice {
            id: id.to_string(),
            name,
            linked_at: store::now_rfc3339(),
            last_sync: None,
        }),
    }
    state.vouchers.retain(|v| v.signer != id);
    state.vouchers.push(DeviceVoucher {
        signer: id.to_string(),
        signature: voucher,
    });
    // Ein eigenes Gerät ist kein Freund
    state.friends.retain(|f| f.id != id);
    state.save();
    drop(state);
    service.notify_changed();
}

/// Entfernt ein eigenes Gerät und verhindert, dass es automatisch wieder verknüpft wird.
async fn forget_device(service: &FriendsService, id: &str) {
    let mut state = service.state.lock().await;
    state.devices.retain(|d| d.id != id);
    state.vouchers.retain(|v| v.signer != id);
    if !state.blocked_devices.iter().any(|d| d == id) {
        state.blocked_devices.push(id.to_string());
    }
    state.save();
    drop(state);
    service.notify_changed();
}

pub(crate) async fn handle_unlink(service: &FriendsService, remote: &str) -> Response {
    forget_device(service, remote).await;
    Response::Ok
}

// ── Abgleich der Freundesliste ───────────────────────────────────────────────

fn sync_data(state: &FriendsState, my_id: &str) -> SyncData {
    SyncData {
        friends: state
            .friends
            .iter()
            .filter(|f| {
                f.state == FriendState::Accepted && f.id != my_id && !state.is_device(&f.id)
            })
            .map(|f| SyncedFriend {
                id: f.id.clone(),
                name: f.name.clone(),
                uuid: f.uuid.clone(),
                added_at: f.added_at.clone(),
                linked_to: f.linked_to.clone(),
            })
            .collect(),
        removed: state.removed_friends.clone(),
    }
}

fn is_after(a: &str, b: &str) -> bool {
    match (
        chrono::DateTime::parse_from_rfc3339(a),
        chrono::DateTime::parse_from_rfc3339(b),
    ) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

/// Übernimmt den Stand eines eigenen Geräts. Liefert die IDs der dabei entfernten Freunde.
fn merge_sync(state: &mut FriendsState, my_id: &str, data: SyncData) -> (bool, Vec<String>) {
    let mut changed = false;
    let mut removed_ids = Vec::new();

    for removed in data.removed {
        let still_valid = state
            .friend(&removed.id)
            .is_some_and(|f| is_after(&f.added_at, &removed.removed_at));
        if state.friend(&removed.id).is_some() && !still_valid {
            state.friends.retain(|f| f.id != removed.id);
            state.invites.retain(|i| i.owner_id != removed.id);
            removed_ids.push(removed.id.clone());
            changed = true;
        }
        match state
            .removed_friends
            .iter_mut()
            .find(|r| r.id == removed.id)
        {
            Some(existing) => {
                if is_after(&removed.removed_at, &existing.removed_at) {
                    existing.removed_at = removed.removed_at;
                    changed = true;
                }
            }
            None => {
                state.removed_friends.push(removed);
                changed = true;
            }
        }
    }

    for friend in data.friends {
        if friend.id == my_id || state.is_device(&friend.id) {
            continue;
        }
        let removed_later = state
            .removed_friends
            .iter()
            .any(|r| r.id == friend.id && !is_after(&friend.added_at, &r.removed_at));
        if removed_later {
            continue;
        }
        match state.friend_mut(&friend.id) {
            Some(existing) => {
                if existing.state != FriendState::Accepted {
                    existing.state = FriendState::Accepted;
                    existing.needs_intro = true;
                    changed = true;
                }
                if existing.linked_to.is_none() && friend.linked_to.is_some() {
                    existing.linked_to = friend.linked_to;
                    changed = true;
                }
            }
            None => {
                state.friends.push(Friend {
                    id: friend.id,
                    name: friend.name,
                    uuid: friend.uuid,
                    state: FriendState::Accepted,
                    added_at: friend.added_at,
                    linked_to: friend.linked_to,
                    needs_intro: true,
                });
                changed = true;
            }
        }
    }
    state.prune_removed_friends();
    (changed, removed_ids)
}

async fn apply_sync(service: &FriendsService, device: &str, data: SyncData) {
    let my_id = service.my_id();
    let removed_ids = {
        let mut state = service.state.lock().await;
        let (changed, removed_ids) = merge_sync(&mut state, &my_id, data);
        if let Some(d) = state.devices.iter_mut().find(|d| d.id == device) {
            d.last_sync = Some(store::now_rfc3339());
        }
        state.save();
        if changed {
            drop(state);
            service.notify_changed();
        }
        removed_ids
    };
    // Auf einem anderen Gerät entfernte Freunde: auch von diesem Gerät abmelden
    for id in removed_ids {
        let _ = service.send(&id, &Request::RemoveFriend).await;
    }
}

/// Gleicht die Freundesliste mit einem eigenen Gerät ab (in beide Richtungen).
pub(crate) async fn sync_with(service: &FriendsService, device: &str) -> Result<(), String> {
    let data = {
        let state = service.state.lock().await;
        sync_data(&state, &service.my_id())
    };
    match service.send(device, &Request::SyncFriends { data }).await? {
        Response::Sync { data } => {
            service.touch(device);
            apply_sync(service, device, data).await;
            Ok(())
        }
        _ => Err("Unerwartete Antwort".to_string()),
    }
}

pub(crate) async fn handle_sync(
    service: &FriendsService,
    remote: &str,
    data: SyncData,
) -> Result<Response, String> {
    if !service.state.lock().await.is_device(remote) {
        return Err("Kein verknüpftes Gerät".to_string());
    }
    apply_sync(service, remote, data).await;
    let state = service.state.lock().await;
    Ok(Response::Sync {
        data: sync_data(&state, &service.my_id()),
    })
}

/// Stellt dieses Gerät einem Freund vor, der bisher nur meine anderen Geräte kennt.
pub(crate) async fn introduce_to(service: &FriendsService, friend_id: &str) -> bool {
    let vouchers = service.state.lock().await.vouchers.clone();
    if vouchers.is_empty() {
        return false;
    }
    let (name, uuid) = service.my_identity();
    let request = Request::IntroduceDevice {
        name,
        uuid,
        vouchers,
    };
    if service.send(friend_id, &request).await.is_err() {
        return false;
    }
    let mut state = service.state.lock().await;
    if let Some(friend) = state.friend_mut(friend_id) {
        friend.needs_intro = false;
    }
    state.save();
    true
}

/// Ein Gerät eines Freundes stellt sich vor – angenommen, wenn ein mir bekannter Freund
/// bestätigt, dass es zu ihm gehört.
pub(crate) async fn handle_introduce(
    service: &FriendsService,
    remote: &str,
    name: String,
    uuid: Option<String>,
    vouchers: Vec<DeviceVoucher>,
) -> Result<Response, String> {
    let mut state = service.state.lock().await;
    let signer = vouchers
        .iter()
        .find(|v| {
            v.signer != remote
                && state.is_accepted_friend(&v.signer)
                && verify_voucher(&v.signer, remote, &v.signature)
        })
        .map(|v| v.signer.clone())
        .ok_or("Unbekanntes Gerät")?;
    let root = state.group_root(&signer);
    match state.friend_mut(remote) {
        Some(friend) => {
            friend.state = FriendState::Accepted;
            friend.name = name;
            friend.uuid = uuid;
            if friend.linked_to.is_none() && friend.id != root {
                friend.linked_to = Some(root);
            }
        }
        None => state.friends.push(Friend {
            id: remote.to_string(),
            name,
            uuid,
            state: FriendState::Accepted,
            added_at: store::now_rfc3339(),
            linked_to: Some(root),
            needs_intro: false,
        }),
    }
    state.removed_friends.retain(|r| r.id != remote);
    state.save();
    drop(state);
    service.notify_changed();
    Ok(Response::Ok)
}

pub(crate) async fn device_views(service: &FriendsService) -> Vec<DeviceView> {
    let state = service.state.lock().await;
    state
        .devices
        .iter()
        .map(|d| DeviceView {
            id: d.id.clone(),
            name: d.name.clone(),
            online: service.is_online(&d.id),
            last_sync: d.last_sync.clone(),
        })
        .collect()
}

// ==================== Befehle ====================

/// Trennt ein eigenes Gerät (die Freundesliste wird nicht mehr abgeglichen).
#[tauri::command]
pub async fn friends_unlink_device(id: String) -> Result<(), String> {
    let service = super::service()?;
    let _ = service.send(&id, &Request::UnlinkDevice).await;
    forget_device(&service, &id).await;
    Ok(())
}

/// Sucht sofort nach weiteren Geräten desselben Accounts.
#[tauri::command]
pub async fn friends_find_devices() -> Result<(), String> {
    let service = super::service()?;
    if microsoft_account().await.is_none() {
        return Err("Der Abgleich zwischen Geräten benötigt einen Microsoft-Account".to_string());
    }
    *lock(&service.devices.last_discovery) = None;
    lock(&service.devices.attempts).clear();
    lock(&service.devices.failed).clear();
    discovery_round(&service).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::friends::store::RemovedFriend;

    fn friend(id: &str, added_at: &str) -> Friend {
        Friend {
            id: id.to_string(),
            name: id.to_string(),
            uuid: None,
            state: FriendState::Accepted,
            added_at: added_at.to_string(),
            linked_to: None,
            needs_intro: false,
        }
    }

    #[test]
    fn vouchers_verify_only_for_target() {
        let secret = SecretKey::generate();
        let signer = secret.public().to_string();
        let target = SecretKey::generate().public().to_string();
        let other = SecretKey::generate().public().to_string();
        let voucher = sign_voucher(&secret, &target);
        assert!(verify_voucher(&signer, &target, &voucher));
        assert!(!verify_voucher(&signer, &other, &voucher));
        assert!(!verify_voucher(&other, &target, &voucher));
        assert!(!verify_voucher(&signer, &target, "zz"));
    }

    #[test]
    fn discovery_key_ignores_uuid_format() {
        let a = discovery_key("0f4e4b2c-1111-2222-3333-444455556666");
        let b = discovery_key("0F4E4B2C111122223333444455556666");
        assert_eq!(a.public(), b.public());
    }

    #[test]
    fn record_keeps_me_and_linked_first() {
        let failed: HashSet<String> = ["bad".to_string()].into();
        let found: Vec<String> = (0..10)
            .map(|i| format!("f{i}"))
            .chain(["bad".into()])
            .collect();
        let entries = record_entries("me", &["l1".into()], &found, &failed);
        assert_eq!(entries.len(), MAX_RECORD_DEVICES);
        assert_eq!(&entries[..2], &["me".to_string(), "l1".to_string()]);
        assert!(!entries.contains(&"bad".to_string()));
    }

    #[test]
    fn merge_adds_friends_and_respects_removals() {
        let mut state = FriendsState::default();
        state.friends.push(friend("old", "2026-01-01T00:00:00Z"));
        let data = SyncData {
            friends: vec![
                SyncedFriend {
                    id: "new".into(),
                    name: "Neu".into(),
                    uuid: None,
                    added_at: "2026-03-01T00:00:00Z".into(),
                    linked_to: None,
                },
                // Wurde hier nach dem Hinzufügen entfernt → nicht zurückholen
                SyncedFriend {
                    id: "gone".into(),
                    name: "Weg".into(),
                    uuid: None,
                    added_at: "2026-01-01T00:00:00Z".into(),
                    linked_to: None,
                },
                // Eigene ID nie als Freund übernehmen
                SyncedFriend {
                    id: "me".into(),
                    name: "Ich".into(),
                    uuid: None,
                    added_at: "2026-01-01T00:00:00Z".into(),
                    linked_to: None,
                },
            ],
            removed: vec![RemovedFriend {
                id: "old".into(),
                removed_at: "2026-02-01T00:00:00Z".into(),
            }],
        };
        state.removed_friends.push(RemovedFriend {
            id: "gone".into(),
            removed_at: chrono::Utc::now().to_rfc3339(),
        });
        let (changed, removed) = merge_sync(&mut state, "me", data);
        assert!(changed);
        assert_eq!(removed, vec!["old".to_string()]);
        let ids: Vec<&str> = state.friends.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["new"]);
        assert!(state.friends[0].needs_intro);
    }

    #[test]
    fn readding_after_removal_wins() {
        let mut state = FriendsState::default();
        state.removed_friends.push(RemovedFriend {
            id: "f".into(),
            removed_at: chrono::Utc::now().to_rfc3339(),
        });
        let later = (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();
        let data = SyncData {
            friends: vec![SyncedFriend {
                id: "f".into(),
                name: "F".into(),
                uuid: None,
                added_at: later,
                linked_to: None,
            }],
            removed: Vec::new(),
        };
        merge_sync(&mut state, "me", data);
        assert_eq!(state.friends.len(), 1);
    }

    #[test]
    fn groups_devices_of_one_friend() {
        let mut state = FriendsState::default();
        state.friends.push(friend("a", "2026-01-01T00:00:00Z"));
        let mut b = friend("b", "2026-01-01T00:00:00Z");
        b.linked_to = Some("a".into());
        state.friends.push(b);
        state.friends.push(friend("c", "2026-01-01T00:00:00Z"));
        assert_eq!(state.group_root("b"), "a");
        let mut group = state.group_ids("b");
        group.sort();
        assert_eq!(group, vec!["a".to_string(), "b".to_string()]);
        let removed = state.remove_friend_group("a");
        assert_eq!(removed.len(), 2);
        assert_eq!(state.friends.len(), 1);
        assert_eq!(state.removed_friends.len(), 2);
    }

    /// Geräte-Eintrag beim öffentlichen pkarr-Relay schreiben und wieder lesen.
    /// `cargo test --bin Lion-Launcher pkarr_record_roundtrip -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "benötigt Internet"]
    async fn pkarr_record_roundtrip() {
        let secret = discovery_key(&random_nonce());
        let ids: Vec<String> = (0..MAX_RECORD_DEVICES)
            .map(|_| SecretKey::generate().public().to_string())
            .collect();
        publish_record(&secret, &ids).await.expect("publish");
        let found = resolve_record(&secret.public()).await;
        assert_eq!(found, ids);
    }
}
