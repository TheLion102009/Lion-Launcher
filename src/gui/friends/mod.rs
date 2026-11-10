//! Freunde & geteilte Instanzen – komplett ohne eigenen Server.
//!
//! Jeder Launcher ist ein iroh-Endpoint, identifiziert durch einen öffentlichen Schlüssel
//! (Freundescode). Verbindungen laufen Ende-zu-Ende-verschlüsselt per QUIC direkt zwischen
//! den Launchern (Hole-Punching). Nur zum gegenseitigen Auffinden und als Notfall-Weiterleitung
//! werden die kostenlosen öffentlichen iroh-Relays genutzt – sie sehen keine Inhalte.
//!
//! Online-Status: Jeder Launcher meldet sich regelmäßig bei seinen Freunden ("Hello").
//! Wer innerhalb von [`ONLINE_TIMEOUT`] erreichbar war, gilt als online.

mod protocol;
pub mod share;
mod store;

use iroh::endpoint::{presets, Connection};
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointId, SecretKey};
use protocol::{Request, Response};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use store::{ChatMessage, Friend, FriendState, FriendsState, ShareInvite};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, OnceCell};

/// Wie lange ein Freund nach dem letzten Lebenszeichen als online gilt.
const ONLINE_TIMEOUT: Duration = Duration::from_secs(70);
/// Abstand zwischen zwei Runden des Online-Checks.
const PRESENCE_INTERVAL: Duration = Duration::from_secs(25);
/// Zeitlimit für kurze Anfragen (Hello, Einladungen, ...).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);

/// Event an das Frontend, wenn sich Freunde, Status oder Einladungen geändert haben.
const CHANGED_EVENT: &str = "friends-changed";

/// Maximale Länge einer Chat-Nachricht (Zeichen).
const MAX_CHAT_CHARS: usize = 2000;

static SERVICE: OnceCell<Arc<FriendsService>> = OnceCell::const_new();

pub(crate) struct FriendsService {
    endpoint: Endpoint,
    /// Hält die Annahme eingehender Verbindungen am Leben
    _router: Router,
    app: AppHandle,
    pub(crate) state: Mutex<FriendsState>,
    last_seen: std::sync::Mutex<HashMap<String, Instant>>,
    connections: Mutex<HashMap<EndpointId, Connection>>,
    /// Zuletzt bekannte eigene Minecraft-Identität (Name, UUID)
    identity: std::sync::Mutex<(String, Option<String>)>,
}

/// Startet den Dienst im Hintergrund (aufgerufen beim App-Start).
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // In den Einstellungen abschaltbar (Datenschutz): dann keinerlei P2P-Verbindungen
        let enabled = crate::gui::settings::load_config()
            .await
            .map(|c| c.privacy.friends_enabled)
            .unwrap_or(true);
        if !enabled {
            tracing::info!("Friends service disabled in settings");
            return;
        }
        match FriendsService::bind(app).await {
            Ok(service) => {
                let service = SERVICE.get_or_init(|| async { service }).await.clone();
                tracing::info!("Friends service online as {}", service.endpoint.id());
                service.run_presence_loop().await;
            }
            Err(e) => tracing::error!("Friends service could not start: {}", e),
        }
    });
}

pub(crate) fn service() -> Result<Arc<FriendsService>, String> {
    SERVICE.get().cloned().ok_or_else(|| {
        "Das Freunde-System startet noch – bitte gleich nochmal versuchen".to_string()
    })
}

fn load_or_create_secret_key() -> Result<SecretKey, String> {
    let path = store::identity_file();
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(arr) = <[u8; 32]>::try_from(bytes.as_slice()) {
            return Ok(SecretKey::from_bytes(&arr));
        }
        tracing::warn!("Invalid friends identity file – creating a new identity");
    }
    let key = SecretKey::generate();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, key.to_bytes()).map_err(|e| e.to_string())?;
    Ok(key)
}

fn parse_endpoint_id(id: &str) -> Result<EndpointId, String> {
    id.trim()
        .parse::<EndpointId>()
        .map_err(|_| "Ungültiger Freundescode".to_string())
}

/// Liest den Freundescode aus `lion://friend/<id>` oder der nackten ID.
fn parse_friend_code(code: &str) -> Result<String, String> {
    let code = code.trim();
    let id = code
        .strip_prefix("lion://friend/")
        .unwrap_or(code)
        .trim_end_matches('/');
    parse_endpoint_id(id).map(|id| id.to_string())
}

#[derive(Debug, Clone)]
struct FriendsProtocol;

impl ProtocolHandler for FriendsProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok(service) = service() else {
            connection.close(1u32.into(), b"starting");
            return Ok(());
        };
        let remote = connection.remote_id().to_string();
        // Jede Anfrage kommt auf einem eigenen Stream
        while let Ok((mut send, mut recv)) = connection.accept_bi().await {
            let service = service.clone();
            let remote = remote.clone();
            tokio::spawn(async move {
                let response = match recv.read_to_end(protocol::MAX_REQUEST_BYTES).await {
                    Ok(bytes) => match serde_json::from_slice::<Request>(&bytes) {
                        Ok(request) => service.handle_request(&remote, request).await,
                        Err(e) => protocol::encode_error(&format!("Ungültige Anfrage: {}", e)),
                    },
                    Err(e) => protocol::encode_error(&e.to_string()),
                };
                if send.write_all(&response).await.is_ok() {
                    send.finish().ok();
                }
            });
        }
        Ok(())
    }
}

#[derive(Serialize)]
pub struct MeView {
    pub id: String,
    pub code: String,
    pub name: String,
    pub uuid: Option<String>,
}

#[derive(Serialize)]
pub struct FriendView {
    pub id: String,
    pub name: String,
    pub uuid: Option<String>,
    pub state: FriendState,
    pub online: bool,
    /// Ungelesene Nachrichten/Einladungen
    pub unread: usize,
}

#[derive(Serialize)]
pub struct FriendsView {
    pub me: MeView,
    pub friends: Vec<FriendView>,
    pub invites: Vec<ShareInvite>,
}

impl FriendsService {
    async fn bind(app: AppHandle) -> Result<Arc<Self>, String> {
        let secret_key = load_or_create_secret_key()?;
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .bind()
            .await
            .map_err(|e| e.to_string())?;
        let router = Router::builder(endpoint.clone())
            .accept(protocol::ALPN, FriendsProtocol)
            .spawn();
        Ok(Arc::new(Self {
            endpoint,
            _router: router,
            app,
            state: Mutex::new(FriendsState::load()),
            last_seen: std::sync::Mutex::new(HashMap::new()),
            connections: Mutex::new(HashMap::new()),
            identity: std::sync::Mutex::new(("Spieler".to_string(), None)),
        }))
    }

    pub(crate) fn my_id(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// Eigener Minecraft-Name und UUID (vom aktiven Account).
    pub(crate) fn my_identity(&self) -> (String, Option<String>) {
        let mut cached = self.identity.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((uuid, name, _)) = crate::gui::auth::get_active_access_token() {
            *cached = (name, Some(uuid));
        }
        cached.clone()
    }

    pub(crate) fn notify_changed(&self) {
        let _ = self.app.emit(CHANGED_EVENT, ());
    }

    fn touch(&self, id: &str) {
        let became_online = {
            let mut seen = self.last_seen.lock().unwrap_or_else(|e| e.into_inner());
            let was_online = seen.get(id).is_some_and(|t| t.elapsed() < ONLINE_TIMEOUT);
            seen.insert(id.to_string(), Instant::now());
            !was_online
        };
        if became_online {
            self.notify_changed();
        }
    }

    pub(crate) fn is_online(&self, id: &str) -> bool {
        self.last_seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .is_some_and(|t| t.elapsed() < ONLINE_TIMEOUT)
    }

    // ── Ausgehende Anfragen ──────────────────────────────────────────────────

    async fn connection(&self, id: EndpointId, fresh: bool) -> Result<Connection, String> {
        let mut connections = self.connections.lock().await;
        if !fresh {
            if let Some(conn) = connections.get(&id) {
                if conn.close_reason().is_none() {
                    return Ok(conn.clone());
                }
            }
        }
        let conn = self
            .endpoint
            .connect(id, protocol::ALPN)
            .await
            .map_err(|e| format!("Nicht erreichbar: {}", e))?;
        connections.insert(id, conn.clone());
        Ok(conn)
    }

    async fn exchange(
        &self,
        conn: &Connection,
        body: &[u8],
        limit: usize,
    ) -> Result<Vec<u8>, String> {
        let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        send.write_all(body).await.map_err(|e| e.to_string())?;
        send.finish().map_err(|e| e.to_string())?;
        recv.read_to_end(limit).await.map_err(|e| e.to_string())
    }

    /// Sendet eine Anfrage und liefert die rohe Antwort (inkl. Status-Byte).
    async fn send_raw(
        &self,
        peer: &str,
        request: &Request,
        limit: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        let id = parse_endpoint_id(peer)?;
        let body = serde_json::to_vec(request).map_err(|e| e.to_string())?;
        let attempt = async {
            let conn = self.connection(id, false).await?;
            match self.exchange(&conn, &body, limit).await {
                Ok(bytes) => Ok(bytes),
                // Zwischengespeicherte Verbindung war tot → einmal neu verbinden
                Err(_) => {
                    let conn = self.connection(id, true).await?;
                    self.exchange(&conn, &body, limit).await
                }
            }
        };
        let bytes = tokio::time::timeout(timeout, attempt)
            .await
            .map_err(|_| "Zeitüberschreitung – der Freund ist vermutlich offline".to_string())??;
        self.touch(peer);
        Ok(bytes)
    }

    pub(crate) async fn send(&self, peer: &str, request: &Request) -> Result<Response, String> {
        let bytes = self
            .send_raw(
                peer,
                request,
                protocol::MAX_JSON_RESPONSE_BYTES,
                REQUEST_TIMEOUT,
            )
            .await?;
        protocol::decode_response(bytes)
    }

    pub(crate) async fn fetch_blob(
        &self,
        peer: &str,
        share_id: &str,
        sha1: &str,
    ) -> Result<Vec<u8>, String> {
        let request = Request::GetBlob {
            share_id: share_id.to_string(),
            sha1: sha1.to_string(),
        };
        let bytes = self
            .send_raw(
                peer,
                &request,
                protocol::MAX_BLOB_BYTES,
                Duration::from_secs(600),
            )
            .await?;
        let payload = protocol::decode_payload(bytes)?;
        if !crate::gui::content_share::sha1_hex(&payload).eq_ignore_ascii_case(sha1) {
            return Err("Prüfsumme der übertragenen Datei stimmt nicht".to_string());
        }
        Ok(payload)
    }

    fn hello_request(&self) -> Request {
        let (name, uuid) = self.my_identity();
        Request::Hello { name, uuid }
    }

    // ── Eingehende Anfragen ──────────────────────────────────────────────────

    async fn handle_request(&self, remote: &str, request: Request) -> Vec<u8> {
        self.touch(remote);
        match request {
            Request::Hello { name, uuid } => {
                self.update_friend_profile(remote, name, uuid).await;
                let (name, uuid) = self.my_identity();
                protocol::encode_ok(&Response::Hello { name, uuid })
            }
            Request::AddFriend { name, uuid } => {
                self.on_friend_request(remote, name, uuid).await;
                protocol::encode_ok(&Response::Ok)
            }
            Request::AcceptFriend { name, uuid } => {
                let mut state = self.state.lock().await;
                if let Some(friend) = state.friend_mut(remote) {
                    if friend.state == FriendState::Outgoing {
                        friend.state = FriendState::Accepted;
                    }
                    friend.name = name;
                    friend.uuid = uuid;
                    state.save();
                    drop(state);
                    self.notify_changed();
                }
                protocol::encode_ok(&Response::Ok)
            }
            Request::RemoveFriend => {
                let mut state = self.state.lock().await;
                state.friends.retain(|f| f.id != remote);
                state.invites.retain(|i| i.owner_id != remote);
                state.save();
                drop(state);
                self.notify_changed();
                protocol::encode_ok(&Response::Ok)
            }
            Request::ShareInvite {
                share_id,
                name,
                icon,
                owner_name,
            } => {
                let mut state = self.state.lock().await;
                if !state.is_accepted_friend(remote) {
                    return protocol::encode_error("Nur Freunde können Einladungen senden");
                }
                state
                    .invites
                    .retain(|i| !(i.share_id == share_id && i.owner_id == remote));
                // Einladung erscheint zusätzlich im Chat mit dem Freund
                state.push_message(ChatMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    friend_id: remote.to_string(),
                    outgoing: false,
                    text: String::new(),
                    sent_at: store::now_rfc3339(),
                    delivered: true,
                    read: false,
                    share_id: Some(share_id.clone()),
                    share_name: Some(name.clone()),
                });
                state.invites.push(ShareInvite {
                    share_id,
                    owner_id: remote.to_string(),
                    owner_name,
                    name,
                    icon,
                    received_at: store::now_rfc3339(),
                });
                state.save();
                drop(state);
                self.notify_changed();
                protocol::encode_ok(&Response::Ok)
            }
            Request::Chat { id, text } => {
                let mut state = self.state.lock().await;
                if !state.is_accepted_friend(remote) {
                    return protocol::encode_error("Nur Freunde können Nachrichten senden");
                }
                // Wiederholte Zustellung derselben Nachricht ignorieren
                if !state.messages.iter().any(|m| m.id == id) {
                    state.push_message(ChatMessage {
                        id,
                        friend_id: remote.to_string(),
                        outgoing: false,
                        text: text.chars().take(MAX_CHAT_CHARS).collect(),
                        sent_at: store::now_rfc3339(),
                        delivered: true,
                        read: false,
                        share_id: None,
                        share_name: None,
                    });
                    state.save();
                    drop(state);
                    self.notify_changed();
                }
                protocol::encode_ok(&Response::Ok)
            }
            other => share::handle_share_request(self, remote, other).await,
        }
    }

    async fn update_friend_profile(&self, id: &str, name: String, uuid: Option<String>) {
        let mut state = self.state.lock().await;
        if let Some(friend) = state.friend_mut(id) {
            if friend.name != name || friend.uuid != uuid {
                friend.name = name;
                friend.uuid = uuid;
                state.save();
                drop(state);
                self.notify_changed();
            }
        }
    }

    async fn on_friend_request(&self, remote: &str, name: String, uuid: Option<String>) {
        let mut state = self.state.lock().await;
        match state.friend_mut(remote) {
            Some(friend) => {
                // Beide haben sich gegenseitig angefragt → befreundet
                if friend.state == FriendState::Outgoing {
                    friend.state = FriendState::Accepted;
                }
                friend.name = name;
                friend.uuid = uuid;
            }
            None => state.friends.push(Friend {
                id: remote.to_string(),
                name,
                uuid,
                state: FriendState::Incoming,
                added_at: store::now_rfc3339(),
            }),
        }
        state.save();
        drop(state);
        self.notify_changed();
    }

    // ── Online-Status ────────────────────────────────────────────────────────

    async fn run_presence_loop(self: Arc<Self>) {
        // Erst warten, bis der Endpoint mit einem Relay verbunden ist
        let _ = tokio::time::timeout(Duration::from_secs(20), self.endpoint.online()).await;
        loop {
            self.presence_round().await;
            tokio::time::sleep(PRESENCE_INTERVAL).await;
        }
    }

    async fn presence_round(self: &Arc<Self>) {
        let friends: Vec<Friend> = self.state.lock().await.friends.clone();
        let mut tasks = Vec::new();
        for friend in friends {
            let service = self.clone();
            tasks.push(tokio::spawn(async move {
                match friend.state {
                    FriendState::Accepted => {
                        if let Ok(Response::Hello { name, uuid }) =
                            service.send(&friend.id, &service.hello_request()).await
                        {
                            service.update_friend_profile(&friend.id, name, uuid).await;
                            // Erreichbar → Liegengebliebenes nachholen
                            service.deliver_pending_messages(&friend.id).await;
                            share::on_friend_online(&service, &friend.id).await;
                        }
                    }
                    // Noch nicht zugestellte Anfrage erneut senden
                    FriendState::Outgoing => {
                        let (name, uuid) = service.my_identity();
                        let _ = service
                            .send(&friend.id, &Request::AddFriend { name, uuid })
                            .await;
                    }
                    FriendState::Incoming => {}
                }
            }));
        }
        for task in tasks {
            let _ = task.await;
        }
        share::deliver_pending_invites(self).await;
        // Abgelaufene Online-Status an die Oberfläche melden
        self.notify_changed();
    }

    /// Sendet noch nicht zugestellte Nachrichten an einen Freund (in Reihenfolge).
    async fn deliver_pending_messages(&self, friend_id: &str) {
        let pending: Vec<(String, String)> = {
            let state = self.state.lock().await;
            state
                .messages
                .iter()
                .filter(|m| {
                    m.friend_id == friend_id && m.outgoing && !m.delivered && m.share_id.is_none()
                })
                .map(|m| (m.id.clone(), m.text.clone()))
                .collect()
        };
        for (id, text) in pending {
            if self
                .send(
                    friend_id,
                    &Request::Chat {
                        id: id.clone(),
                        text,
                    },
                )
                .await
                .is_err()
            {
                break;
            }
            let mut state = self.state.lock().await;
            if let Some(message) = state.messages.iter_mut().find(|m| m.id == id) {
                message.delivered = true;
            }
            state.save();
        }
    }

    pub(crate) async fn view(&self) -> FriendsView {
        let (name, uuid) = self.my_identity();
        let id = self.my_id();
        let state = self.state.lock().await;
        let mut friends: Vec<FriendView> = state
            .friends
            .iter()
            .map(|f| FriendView {
                id: f.id.clone(),
                name: f.name.clone(),
                uuid: f.uuid.clone(),
                state: f.state,
                online: f.state == FriendState::Accepted && self.is_online(&f.id),
                unread: state.unread_count(&f.id),
            })
            .collect();
        friends.sort_by_key(|f| (!f.online, f.name.to_lowercase()));
        FriendsView {
            me: MeView {
                code: format!("lion://friend/{}", id),
                id,
                name,
                uuid,
            },
            friends,
            invites: state.invites.clone(),
        }
    }
}

// ==================== Befehle: Freunde ====================

#[tauri::command]
pub async fn friends_state() -> Result<FriendsView, String> {
    Ok(service()?.view().await)
}

/// Fügt einen Freund per Freundescode hinzu. Ist er offline, wird die Anfrage zugestellt,
/// sobald er online kommt.
#[tauri::command]
pub async fn friends_add(code: String) -> Result<String, String> {
    let service = service()?;
    let id = parse_friend_code(&code)?;
    if id == service.my_id() {
        return Err("Das ist dein eigener Freundescode".to_string());
    }

    // Hat er mich schon angefragt, wird daraus direkt eine Freundschaft
    let accept_existing = {
        let mut state = service.state.lock().await;
        let accept_existing = match state.friend_mut(&id) {
            Some(friend) if friend.state == FriendState::Accepted => {
                return Ok("Ihr seid bereits befreundet".to_string());
            }
            Some(friend) if friend.state == FriendState::Incoming => {
                friend.state = FriendState::Accepted;
                true
            }
            Some(_) => false,
            None => {
                state.friends.push(Friend {
                    name: store::short_id(&id),
                    id: id.clone(),
                    uuid: None,
                    state: FriendState::Outgoing,
                    added_at: store::now_rfc3339(),
                });
                false
            }
        };
        state.save();
        accept_existing
    };
    service.notify_changed();

    let (name, uuid) = service.my_identity();
    let request = if accept_existing {
        Request::AcceptFriend { name, uuid }
    } else {
        Request::AddFriend { name, uuid }
    };
    match service.send(&id, &request).await {
        Ok(_) => {
            // Namen direkt abholen
            if let Ok(Response::Hello { name, uuid }) =
                service.send(&id, &service.hello_request()).await
            {
                service.update_friend_profile(&id, name, uuid).await;
            }
            Ok(if accept_existing {
                "Freundschaftsanfrage angenommen".to_string()
            } else {
                "Freundschaftsanfrage gesendet".to_string()
            })
        }
        Err(_) => Ok(
            "Anfrage gespeichert – sie wird zugestellt, sobald dein Freund online ist".to_string(),
        ),
    }
}

#[tauri::command]
pub async fn friends_accept(id: String) -> Result<(), String> {
    let service = service()?;
    {
        let mut state = service.state.lock().await;
        let friend = state
            .friend_mut(&id)
            .ok_or_else(|| "Anfrage nicht gefunden".to_string())?;
        friend.state = FriendState::Accepted;
        state.save();
    }
    service.notify_changed();
    let (name, uuid) = service.my_identity();
    // Ist der Freund gerade offline, erfährt er es beim nächsten Hello/AddFriend
    let _ = service
        .send(&id, &Request::AcceptFriend { name, uuid })
        .await;
    Ok(())
}

/// Entfernt einen Freund bzw. lehnt eine Anfrage ab / zieht sie zurück.
#[tauri::command]
pub async fn friends_remove(id: String) -> Result<(), String> {
    let service = service()?;
    {
        let mut state = service.state.lock().await;
        state.friends.retain(|f| f.id != id);
        state.invites.retain(|i| i.owner_id != id);
        state.messages.retain(|m| m.friend_id != id);
        state.save();
    }
    service.notify_changed();
    let _ = service.send(&id, &Request::RemoveFriend).await;
    Ok(())
}

/// Chatverlauf mit einem Freund; markiert eingehende Nachrichten als gelesen.
#[tauri::command]
pub async fn friends_messages(friend_id: String) -> Result<Vec<ChatMessage>, String> {
    let service = service()?;
    let mut state = service.state.lock().await;
    let mut changed = false;
    for message in state
        .messages
        .iter_mut()
        .filter(|m| m.friend_id == friend_id && !m.read)
    {
        message.read = true;
        changed = true;
    }
    let messages = state
        .messages
        .iter()
        .filter(|m| m.friend_id == friend_id)
        .cloned()
        .collect();
    if changed {
        state.save();
        drop(state);
        service.notify_changed();
    }
    Ok(messages)
}

/// Sendet eine Nachricht. Ist der Freund offline, wird sie zugestellt, sobald er online kommt.
#[tauri::command]
pub async fn friends_send_message(friend_id: String, text: String) -> Result<ChatMessage, String> {
    let service = service()?;
    let text: String = text.trim().chars().take(MAX_CHAT_CHARS).collect();
    if text.is_empty() {
        return Err("Leere Nachricht".to_string());
    }
    let mut message = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        friend_id: friend_id.clone(),
        outgoing: true,
        text: text.clone(),
        sent_at: store::now_rfc3339(),
        delivered: false,
        read: true,
        share_id: None,
        share_name: None,
    };
    {
        let mut state = service.state.lock().await;
        if !state.is_accepted_friend(&friend_id) {
            return Err("Nur Freunden kann geschrieben werden".to_string());
        }
        state.push_message(message.clone());
        state.save();
    }
    let request = Request::Chat {
        id: message.id.clone(),
        text,
    };
    if service.send(&friend_id, &request).await.is_ok() {
        message.delivered = true;
        let mut state = service.state.lock().await;
        if let Some(stored) = state.messages.iter_mut().find(|m| m.id == message.id) {
            stored.delivered = true;
        }
        state.save();
    }
    service.notify_changed();
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_friend_codes() {
        let key = SecretKey::generate().public().to_string();
        assert_eq!(parse_friend_code(&key).unwrap(), key);
        assert_eq!(
            parse_friend_code(&format!("lion://friend/{key}")).unwrap(),
            key
        );
        assert_eq!(
            parse_friend_code(&format!("  lion://friend/{key}/ ")).unwrap(),
            key
        );
        assert!(parse_friend_code("lion://friend/nonsense").is_err());
    }

    /// Zwei Endpoints finden sich nur über die Endpoint-ID (Freundescode) – über die
    /// öffentlichen iroh-Relays/DNS, ohne IP-Adressen auszutauschen.
    /// `cargo test --bin Lion-Launcher dial_by_id_only -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "benötigt Internet"]
    async fn dial_by_id_only() {
        const TEST_ALPN: &[u8] = b"lion-launcher/test/0";
        let server = Endpoint::builder(presets::N0)
            .alpns(vec![TEST_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let client = Endpoint::bind(presets::N0).await.unwrap();
        tokio::time::timeout(Duration::from_secs(30), server.online())
            .await
            .expect("server online");
        let server_id = server.id();

        let accept = tokio::spawn(async move {
            let conn = server.accept().await.unwrap().await.unwrap();
            let (mut send, mut recv) = conn.accept_bi().await.unwrap();
            let msg = recv.read_to_end(1024).await.unwrap();
            send.write_all(&msg).await.unwrap();
            send.finish().unwrap();
            conn.closed().await;
            server
        });

        let started = Instant::now();
        // Nur die ID – Adresse wird über DNS/Relay gefunden (dauert nach dem Start evtl. etwas)
        let conn = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                match client.connect(server_id, TEST_ALPN).await {
                    Ok(conn) => break conn,
                    Err(_) => tokio::time::sleep(Duration::from_secs(2)).await,
                }
            }
        })
        .await
        .expect("connect by id");
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        send.write_all(b"hallo freund").await.unwrap();
        send.finish().unwrap();
        let echo = recv.read_to_end(1024).await.unwrap();
        assert_eq!(echo, b"hallo freund");
        println!("connected by id in {:?}", started.elapsed());
        conn.close(0u32.into(), b"done");
        let server = accept.await.unwrap();
        server.close().await;
        client.close().await;
    }
}
