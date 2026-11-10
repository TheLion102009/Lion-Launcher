//! Persistenter Zustand des Freunde-Systems (`<launcher>/friends/state.json`).

use crate::gui::content_share::{ContentKind, SharedContentEntry};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(crate) fn friends_dir() -> PathBuf {
    crate::config::defaults::launcher_dir().join("friends")
}

fn state_file() -> PathBuf {
    friends_dir().join("state.json")
}

pub(crate) fn identity_file() -> PathBuf {
    friends_dir().join("identity.key")
}

/// Content-adressierter Speicher (Dateiname = SHA-1) für geteilte Dateien, die es nicht auf
/// Modrinth gibt, und für geteilte Konfig-Dateien. So liefert der Besitzer immer exakt den
/// veröffentlichten Stand aus, auch wenn er lokal schon weitergearbeitet hat.
pub(crate) fn blobs_dir() -> PathBuf {
    friends_dir().join("blobs")
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FriendState {
    /// Beide Seiten haben zugestimmt
    Accepted,
    /// Ich habe eine Anfrage gesendet
    Outgoing,
    /// Ich habe eine Anfrage erhalten
    Incoming,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Friend {
    /// iroh-Endpoint-ID (öffentlicher Schlüssel, hex)
    pub id: String,
    /// Minecraft-Name
    pub name: String,
    /// Minecraft-UUID (für den Kopf als Profilbild)
    #[serde(default)]
    pub uuid: Option<String>,
    pub state: FriendState,
    pub added_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigEntry {
    /// Relativer Pfad im Spielordner, z.B. `config/sodium-options.json`
    pub path: String,
    pub sha1: String,
    pub size: u64,
}

/// Veröffentlichter Stand einer geteilten Instanz.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ShareManifest {
    pub share_id: String,
    pub revision: u64,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub minecraft_version: String,
    pub loader: String,
    #[serde(default)]
    pub loader_version: String,
    #[serde(default)]
    pub mods: Vec<SharedContentEntry>,
    #[serde(default)]
    pub resourcepacks: Vec<SharedContentEntry>,
    #[serde(default)]
    pub shaderpacks: Vec<SharedContentEntry>,
    #[serde(default)]
    pub configs: Vec<ConfigEntry>,
}

impl ShareManifest {
    pub fn content(&self, kind: ContentKind) -> &Vec<SharedContentEntry> {
        match kind {
            ContentKind::Mods => &self.mods,
            ContentKind::ResourcePacks => &self.resourcepacks,
            ContentKind::ShaderPacks => &self.shaderpacks,
        }
    }

    pub fn content_mut(&mut self, kind: ContentKind) -> &mut Vec<SharedContentEntry> {
        match kind {
            ContentKind::Mods => &mut self.mods,
            ContentKind::ResourcePacks => &mut self.resourcepacks,
            ContentKind::ShaderPacks => &mut self.shaderpacks,
        }
    }

    /// Prüfsummen aller Dateien, die der Besitzer selbst ausliefert.
    pub fn blob_hashes(&self) -> impl Iterator<Item = &str> {
        ALL_KINDS
            .iter()
            .flat_map(|kind| self.content(*kind).iter())
            .filter(|e| e.source.as_deref() == Some("share"))
            .filter_map(|e| e.sha1.as_deref())
            .chain(self.configs.iter().map(|c| c.sha1.as_str()))
    }
}

pub(crate) const ALL_KINDS: [ContentKind; 3] = [
    ContentKind::Mods,
    ContentKind::ResourcePacks,
    ContentKind::ShaderPacks,
];

/// Eine Instanz, die ich mit Freunden teile.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OwnedShare {
    pub share_id: String,
    pub profile_id: String,
    /// Geheimnis im Einladungslink – wer den Link hat, darf beitreten
    pub link_token: String,
    #[serde(default)]
    pub members: Vec<String>,
    /// Eingeladene Freunde, die noch nicht beigetreten sind
    #[serde(default)]
    pub invited: Vec<String>,
    /// Einladungen, die noch nicht zugestellt werden konnten (Freund war offline)
    #[serde(default)]
    pub pending_invites: Vec<String>,
    /// Vom Besitzer ausgewählte Konfig-Dateien
    #[serde(default)]
    pub config_files: Vec<String>,
    /// Mitglieder, die vom letzten Update noch nichts wissen (waren offline)
    #[serde(default)]
    pub pending_notify: Vec<String>,
    pub manifest: ShareManifest,
}

/// Eine Instanz, die ein Freund mit mir teilt.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JoinedShare {
    pub share_id: String,
    pub owner_id: String,
    pub owner_name: String,
    pub profile_id: String,
    /// Zuletzt angewendeter Stand (für Änderungsübersicht und gesperrte Inhalte)
    pub manifest: ShareManifest,
    /// Neueste bekannte Revision des Besitzers
    #[serde(default)]
    pub latest_revision: u64,
    /// Besitzer hat das Teilen beendet
    #[serde(default)]
    pub ended: bool,
    /// Vorab geladener, noch nicht übernommener Stand (inkl. Dateien im Blob-Speicher) –
    /// so lässt sich ein Update auch übernehmen, wenn der Besitzer gerade offline ist
    #[serde(default)]
    pub pending: Option<ShareManifest>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShareInvite {
    pub share_id: String,
    pub owner_id: String,
    pub owner_name: String,
    pub name: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub received_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FriendsState {
    #[serde(default)]
    pub friends: Vec<Friend>,
    #[serde(default)]
    pub owned_shares: Vec<OwnedShare>,
    #[serde(default)]
    pub joined_shares: Vec<JoinedShare>,
    #[serde(default)]
    pub invites: Vec<ShareInvite>,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
}

/// Chat-Nachricht mit einem Freund. Einladungen zu geteilten Instanzen erscheinen ebenfalls
/// als Nachricht (`share_id` gesetzt).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub friend_id: String,
    /// `true` = von mir gesendet
    pub outgoing: bool,
    #[serde(default)]
    pub text: String,
    pub sent_at: String,
    /// Ausgehend: beim Freund angekommen
    #[serde(default)]
    pub delivered: bool,
    /// Eingehend: schon angesehen
    #[serde(default)]
    pub read: bool,
    /// Einladung zu dieser geteilten Instanz
    #[serde(default)]
    pub share_id: Option<String>,
    #[serde(default)]
    pub share_name: Option<String>,
}

/// Maximale Anzahl gespeicherter Nachrichten pro Freund.
pub const MAX_MESSAGES_PER_FRIEND: usize = 500;

impl FriendsState {
    pub fn load() -> Self {
        std::fs::read_to_string(state_file())
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    /// Schreibt atomar (erst Temp-Datei, dann umbenennen), damit ein Absturz nichts zerstört.
    pub fn save(&self) {
        let path = state_file();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let tmp = path.with_extension("json.tmp");
        match serde_json::to_string_pretty(self) {
            Ok(content) => {
                if std::fs::write(&tmp, content).is_ok() {
                    std::fs::remove_file(&path).ok();
                    if let Err(e) = std::fs::rename(&tmp, &path) {
                        tracing::warn!("Failed to save friends state: {}", e);
                    }
                }
            }
            Err(e) => tracing::warn!("Failed to serialize friends state: {}", e),
        }
    }

    pub fn friend(&self, id: &str) -> Option<&Friend> {
        self.friends.iter().find(|f| f.id == id)
    }

    pub fn friend_mut(&mut self, id: &str) -> Option<&mut Friend> {
        self.friends.iter_mut().find(|f| f.id == id)
    }

    pub fn is_accepted_friend(&self, id: &str) -> bool {
        self.friend(id)
            .is_some_and(|f| f.state == FriendState::Accepted)
    }

    pub fn friend_name(&self, id: &str) -> String {
        self.friend(id)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| short_id(id))
    }

    /// Speichert eine Nachricht; pro Freund bleiben nur die neuesten erhalten.
    pub fn push_message(&mut self, message: ChatMessage) {
        let friend_id = message.friend_id.clone();
        self.messages.push(message);
        let count = self
            .messages
            .iter()
            .filter(|m| m.friend_id == friend_id)
            .count();
        if count > MAX_MESSAGES_PER_FRIEND {
            let mut excess = count - MAX_MESSAGES_PER_FRIEND;
            self.messages.retain(|m| {
                if excess > 0 && m.friend_id == friend_id {
                    excess -= 1;
                    false
                } else {
                    true
                }
            });
        }
    }

    pub fn unread_count(&self, friend_id: &str) -> usize {
        self.messages
            .iter()
            .filter(|m| m.friend_id == friend_id && !m.outgoing && !m.read)
            .count()
    }

    pub fn owned_by_profile(&self, profile_id: &str) -> Option<&OwnedShare> {
        self.owned_shares
            .iter()
            .find(|s| s.profile_id == profile_id)
    }

    pub fn owned_by_profile_mut(&mut self, profile_id: &str) -> Option<&mut OwnedShare> {
        self.owned_shares
            .iter_mut()
            .find(|s| s.profile_id == profile_id)
    }

    pub fn owned_by_id_mut(&mut self, share_id: &str) -> Option<&mut OwnedShare> {
        self.owned_shares
            .iter_mut()
            .find(|s| s.share_id == share_id)
    }

    pub fn joined_by_profile(&self, profile_id: &str) -> Option<&JoinedShare> {
        self.joined_shares
            .iter()
            .find(|s| s.profile_id == profile_id)
    }

    pub fn joined_by_profile_mut(&mut self, profile_id: &str) -> Option<&mut JoinedShare> {
        self.joined_shares
            .iter_mut()
            .find(|s| s.profile_id == profile_id)
    }

    pub fn joined_by_share_mut(
        &mut self,
        owner_id: &str,
        share_id: &str,
    ) -> Option<&mut JoinedShare> {
        self.joined_shares
            .iter_mut()
            .find(|s| s.share_id == share_id && s.owner_id == owner_id)
    }
}

/// Kurzform einer Endpoint-ID für die Anzeige, falls kein Name bekannt ist.
pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}
