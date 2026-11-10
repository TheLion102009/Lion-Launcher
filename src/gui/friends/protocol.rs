//! Nachrichten zwischen zwei Launchern.
//!
//! Jede Anfrage läuft über einen eigenen bidirektionalen QUIC-Stream:
//! Anfrage = JSON, danach wird der Sendekanal geschlossen.
//! Antwort = 1 Status-Byte (0 = ok, 1 = Fehler) + Nutzdaten
//! (JSON-[`Response`], bei Dateien die rohen Bytes, bei Fehlern der Fehlertext).

use super::store::ShareManifest;
use serde::{Deserialize, Serialize};

/// Protokollkennung (ALPN). Bei inkompatiblen Änderungen die Versionsnummer erhöhen.
pub const ALPN: &[u8] = b"lion-launcher/friends/1";

/// Maximale Größe einer Anfrage (Einladungen enthalten das Profil-Icon als Data-URL).
pub const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
/// Maximale Größe einer JSON-Antwort (Manifest).
pub const MAX_JSON_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
/// Maximale Größe einer übertragenen Datei.
pub const MAX_BLOB_BYTES: usize = 1024 * 1024 * 1024;

pub const STATUS_OK: u8 = 0;
pub const STATUS_ERROR: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Lebenszeichen + Austausch von Minecraft-Name/UUID
    Hello {
        name: String,
        uuid: Option<String>,
    },
    AddFriend {
        name: String,
        uuid: Option<String>,
    },
    AcceptFriend {
        name: String,
        uuid: Option<String>,
    },
    RemoveFriend,
    /// Chat-Nachricht (`id` verhindert doppelte Zustellung bei Wiederholungen)
    Chat {
        id: String,
        text: String,
    },
    ShareInvite {
        share_id: String,
        name: String,
        icon: Option<String>,
        owner_name: String,
    },
    /// Inhalt ansehen, ohne beizutreten (Vorschau vor dem Annehmen)
    PreviewShare {
        share_id: String,
        token: Option<String>,
    },
    JoinShare {
        share_id: String,
        token: Option<String>,
    },
    GetManifest {
        share_id: String,
    },
    GetBlob {
        share_id: String,
        sha1: String,
    },
    ShareUpdated {
        share_id: String,
        revision: u64,
    },
    LeaveShare {
        share_id: String,
    },
    ShareRemoved {
        share_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Hello { name: String, uuid: Option<String> },
    Manifest { manifest: Box<ShareManifest> },
}

pub fn encode_ok(response: &Response) -> Vec<u8> {
    let mut out = vec![STATUS_OK];
    out.extend(serde_json::to_vec(response).unwrap_or_default());
    out
}

pub fn encode_raw(bytes: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    out.push(STATUS_OK);
    out.extend(bytes);
    out
}

pub fn encode_error(message: &str) -> Vec<u8> {
    let mut out = vec![STATUS_ERROR];
    out.extend(message.as_bytes());
    out
}

/// Trennt Status und Nutzdaten; Fehler der Gegenseite werden zu `Err`.
pub fn decode_payload(mut bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.is_empty() {
        return Err("Leere Antwort".to_string());
    }
    let status = bytes.remove(0);
    if status == STATUS_OK {
        Ok(bytes)
    } else {
        Err(String::from_utf8_lossy(&bytes).to_string())
    }
}

pub fn decode_response(bytes: Vec<u8>) -> Result<Response, String> {
    let payload = decode_payload(bytes)?;
    serde_json::from_slice(&payload).map_err(|e| format!("Ungültige Antwort: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip() {
        let req = Request::GetBlob {
            share_id: "s".into(),
            sha1: "abc".into(),
        };
        let json = serde_json::to_vec(&req).unwrap();
        let back: Request = serde_json::from_slice(&json).unwrap();
        assert!(matches!(back, Request::GetBlob { sha1, .. } if sha1 == "abc"));
    }

    #[test]
    fn response_encoding() {
        let ok = decode_response(encode_ok(&Response::Ok)).unwrap();
        assert!(matches!(ok, Response::Ok));
        assert_eq!(
            decode_payload(encode_raw(vec![1, 2, 3])).unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(decode_payload(encode_error("nope")).unwrap_err(), "nope");
    }
}
