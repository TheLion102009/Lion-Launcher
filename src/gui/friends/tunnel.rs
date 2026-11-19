//! P2P-Tunnel für „Zusammen spielen“: Minecraft-Verbindungen eines Freundes laufen über eine
//! verschlüsselte iroh-Verbindung direkt zu meinem Rechner.
//!
//! **Gast:** Der Launcher öffnet einen lokalen Port (`127.0.0.1:<port>`), Minecraft verbindet
//! sich dorthin. Jede TCP-Verbindung wird zu einem eigenen QUIC-Stream zum Gastgeber.
//!
//! **Gastgeber:** Jeder Stream beginnt mit einer Kopfzeile (`{"session_id": …}\n`). Ist der
//! Freund eingeladen und die Welt offen (bzw. der Server gestartet), verbindet der Launcher den
//! Stream mit dem lokalen Port des Spiels; sonst antwortet er mit einem Fehlertext.
//! Antwort: 1 Status-Byte (0 = ok, 1 = Fehler + Text), danach die rohen Spieldaten.
//!
//! Minecraft selbst prüft weiterhin jeden Spieler über Mojang (Online-Modus) – der Tunnel ersetzt
//! nur Portfreigabe und Router-Einstellungen.

use super::FriendsService;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};

pub const ALPN: &[u8] = b"lion-launcher/tunnel/1";
const MAX_HEADER: usize = 1024;
const STATUS_OK: u8 = 0;
const STATUS_ERROR: u8 = 1;

#[derive(Serialize, Deserialize)]
struct Header {
    session_id: String,
    /// Nur prüfen, ob beitreten möglich ist (keine Spielverbindung)
    #[serde(default)]
    probe: bool,
}

async fn read_header(recv: &mut RecvStream) -> Result<Header, String> {
    let mut line = Vec::new();
    loop {
        let byte = recv.read_u8().await.map_err(|e| e.to_string())?;
        if byte == b'\n' {
            break;
        }
        line.push(byte);
        if line.len() > MAX_HEADER {
            return Err("Kopfzeile zu lang".into());
        }
    }
    serde_json::from_slice(&line).map_err(|e| e.to_string())
}

async fn reply_error(send: &mut SendStream, message: &str) {
    let mut bytes = vec![STATUS_ERROR];
    bytes.extend_from_slice(message.as_bytes());
    if send.write_all(&bytes).await.is_ok() {
        send.finish().ok();
    }
}

// ── Gastgeber ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(super) struct TunnelProtocol;

impl ProtocolHandler for TunnelProtocol {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let Ok(service) = super::service() else {
            connection.close(1u32.into(), b"starting");
            return Ok(());
        };
        let remote = connection.remote_id().to_string();
        while let Ok((send, recv)) = connection.accept_bi().await {
            let service = service.clone();
            let remote = remote.clone();
            tokio::spawn(async move {
                if let Err(e) = serve_stream(&service, &remote, send, recv).await {
                    tracing::debug!("Tunnel stream from {} ended: {}", remote, e);
                }
            });
        }
        Ok(())
    }
}

async fn serve_stream(
    service: &FriendsService,
    remote: &str,
    send: SendStream,
    recv: RecvStream,
) -> Result<(), String> {
    serve_resolved(send, recv, |session_id| async move {
        super::play::tunnel_target(service, remote, &session_id).await
    })
    .await
}

/// Liest die Kopfzeile, fragt `resolve` nach dem lokalen Port und verbindet den Stream damit.
async fn serve_resolved<F, Fut>(
    mut send: SendStream,
    mut recv: RecvStream,
    resolve: F,
) -> Result<(), String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<u16, String>>,
{
    let header = read_header(&mut recv).await?;
    let port = match resolve(header.session_id.clone()).await {
        Ok(port) => port,
        Err(e) => {
            reply_error(&mut send, &e).await;
            return Err(e);
        }
    };
    if header.probe {
        send.write_all(&[STATUS_OK])
            .await
            .map_err(|e| e.to_string())?;
        send.finish().ok();
        return Ok(());
    }
    let mut tcp = match TcpStream::connect(("127.0.0.1", port)).await {
        Ok(tcp) => tcp,
        Err(e) => {
            let message = format!("Das Spiel nimmt keine Verbindung an: {e}");
            reply_error(&mut send, &message).await;
            return Err(message);
        }
    };
    tcp.set_nodelay(true).ok();
    send.write_all(&[STATUS_OK])
        .await
        .map_err(|e| e.to_string())?;
    let mut quic = tokio::io::join(recv, send);
    tokio::io::copy_bidirectional(&mut tcp, &mut quic)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ── Gast ─────────────────────────────────────────────────────────────────────

/// Offene Tunnel: Sitzung → (lokaler Port, Aufgabe).
static TUNNELS: Mutex<Option<HashMap<String, (u16, tokio::task::AbortHandle)>>> = Mutex::new(None);

/// Baut einen Stream zum Gastgeber auf und liest dessen Antwort.
async fn open_stream(
    connection: &Connection,
    session_id: &str,
    probe: bool,
) -> Result<(SendStream, RecvStream), String> {
    let (mut send, mut recv) = connection.open_bi().await.map_err(|e| e.to_string())?;
    let mut header = serde_json::to_vec(&Header {
        session_id: session_id.to_string(),
        probe,
    })
    .map_err(|e| e.to_string())?;
    header.push(b'\n');
    send.write_all(&header).await.map_err(|e| e.to_string())?;
    let status = recv.read_u8().await.map_err(|e| e.to_string())?;
    if status != STATUS_OK {
        let message = recv.read_to_end(MAX_HEADER).await.unwrap_or_default();
        return Err(String::from_utf8_lossy(&message).trim().to_string());
    }
    Ok((send, recv))
}

async fn connect(service: &FriendsService, host: &str) -> Result<Connection, String> {
    let id = super::parse_endpoint_id(host)?;
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        service.endpoint.connect(id, ALPN),
    )
    .await
    .map_err(|_| "Dein Freund ist nicht erreichbar".to_string())?
    .map_err(|e| format!("Dein Freund ist nicht erreichbar: {e}"))
}

/// Prüft, ob der Gastgeber die Welt bzw. den Server gerade für mich offen hat.
pub(super) async fn probe(
    service: &FriendsService,
    host: &str,
    session_id: &str,
) -> Result<(), String> {
    let connection = connect(service, host).await?;
    let result = open_stream(&connection, session_id, true).await.map(|_| ());
    connection.close(0u32.into(), b"probe");
    result
}

/// Öffnet (oder nutzt) den lokalen Port, über den Minecraft zum Gastgeber verbindet.
pub(super) async fn open(
    service: Arc<FriendsService>,
    host: &str,
    session_id: &str,
) -> Result<u16, String> {
    if let Some((port, task)) = TUNNELS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .get(session_id)
    {
        if !task.is_finished() {
            return Ok(*port);
        }
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("Lokaler Port konnte nicht geöffnet werden: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let host = host.to_string();
    let session = session_id.to_string();
    let connection: Arc<tokio::sync::Mutex<Option<Connection>>> = Arc::default();

    let task = tokio::spawn(async move {
        while let Ok((mut tcp, _)) = listener.accept().await {
            tcp.set_nodelay(true).ok();
            let (service, host, session, connection) = (
                service.clone(),
                host.clone(),
                session.clone(),
                connection.clone(),
            );
            tokio::spawn(async move {
                // Eine QUIC-Verbindung für alle Spielverbindungen zu diesem Gastgeber
                let conn = {
                    let mut cached = connection.lock().await;
                    match cached.as_ref().filter(|c| c.close_reason().is_none()) {
                        Some(conn) => conn.clone(),
                        None => match connect(&service, &host).await {
                            Ok(conn) => {
                                *cached = Some(conn.clone());
                                conn
                            }
                            Err(e) => {
                                tracing::warn!("Tunnel to {} failed: {}", host, e);
                                return;
                            }
                        },
                    }
                };
                match open_stream(&conn, &session, false).await {
                    Ok((send, recv)) => {
                        let mut quic = tokio::io::join(recv, send);
                        tokio::io::copy_bidirectional(&mut tcp, &mut quic)
                            .await
                            .ok();
                    }
                    Err(e) => tracing::warn!("Tunnel refused by {}: {}", host, e),
                }
            });
        }
    });
    TUNNELS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(session_id.to_string(), (port, task.abort_handle()));
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::endpoint::presets;
    use iroh::protocol::Router;
    use iroh::Endpoint;

    #[derive(Debug, Clone)]
    struct TestHost(u16);

    impl ProtocolHandler for TestHost {
        async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
            while let Ok((send, recv)) = connection.accept_bi().await {
                let port = self.0;
                tokio::spawn(serve_resolved(send, recv, move |session| async move {
                    if session == "offen" {
                        Ok(port)
                    } else {
                        Err("Welt ist nicht offen".to_string())
                    }
                }));
            }
            Ok(())
        }
    }

    /// Zwei Endpoints auf diesem Rechner: Daten laufen durch den Tunnel zu einem TCP-Server
    /// und zurück; eine nicht offene Sitzung wird mit ihrem Fehlertext abgelehnt.
    /// `cargo test --bin Lion-Launcher tunnel_carries -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "öffnet Netzwerkverbindungen"]
    async fn tunnel_carries_tcp_both_ways() {
        // Stellvertreter für Minecrafts LAN-Port: schickt alles zurück
        let game = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let game_port = game.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = game.accept().await {
                tokio::spawn(async move {
                    let (mut read, mut write) = socket.split();
                    tokio::io::copy(&mut read, &mut write).await.ok();
                });
            }
        });

        let host = Endpoint::bind(presets::N0).await.unwrap();
        let router = Router::builder(host.clone())
            .accept(ALPN, TestHost(game_port))
            .spawn();
        let guest = Endpoint::bind(presets::N0).await.unwrap();
        let connection = guest.connect(host.addr(), ALPN).await.unwrap();

        let refused = open_stream(&connection, "zu", false).await.unwrap_err();
        assert_eq!(refused, "Welt ist nicht offen");

        let (mut send, mut recv) = open_stream(&connection, "offen", false).await.unwrap();
        send.write_all(b"Hallo Welt").await.unwrap();
        send.finish().unwrap();
        let echoed = recv.read_to_end(1024).await.unwrap();
        assert_eq!(echoed, b"Hallo Welt");

        connection.close(0u32.into(), b"done");
        router.shutdown().await.ok();
        guest.close().await;
    }
}
