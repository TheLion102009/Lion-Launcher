//! Discord Rich Presence: zeigt in Discord, welches Profil gerade läuft, wo (Welt oder Server)
//! und seit wann gespielt wird.
//!
//! Alles läuft lokal über Discords IPC-Schnittstelle (Named Pipe unter Windows, Unix-Socket
//! sonst) – der Launcher baut dafür keine Netzwerkverbindung auf. Läuft Discord nicht, passiert
//! nichts; es wird regelmäßig erneut versucht. Abschaltbar in den Einstellungen.
//!
//! Welt und Server stehen im Spiel-Log des Profils (`logs/latest.log`), z.B.
//! `Connecting to mc.example.net, 25565` oder `Starting integrated minecraft server`.

use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Anwendung „Lion Launcher“ im Discord Developer Portal.
const CLIENT_ID: &str = "1572598345938116618";
const LOGO_URL: &str =
    "https://raw.githubusercontent.com/TheLion102009/Lion-Launcher/master/ui/lionlogo.png";
const PROJECT_URL: &str = "https://github.com/TheLion102009/Lion-Launcher";
const TICK: Duration = Duration::from_secs(2);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(15);
const CONFIG_INTERVAL: Duration = Duration::from_secs(6);
/// So lange nach dem Start einer Welt wird ihr Ordnername noch nachgeschlagen.
const WORLD_LOOKUP_WINDOW: Duration = Duration::from_secs(60);

// ── IPC ──────────────────────────────────────────────────────────────────────

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;

struct Ipc {
    pipe: Box<dyn Pipe>,
}

#[cfg(windows)]
fn open_pipe() -> Option<Box<dyn Pipe>> {
    (0..10).find_map(|i| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!(r"\\.\pipe\discord-ipc-{i}"))
            .ok()
            .map(|file| Box::new(file) as Box<dyn Pipe>)
    })
}

#[cfg(unix)]
fn open_pipe() -> Option<Box<dyn Pipe>> {
    let bases: Vec<PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .chain(std::iter::once(PathBuf::from("/tmp")))
        .collect();
    // Auch Flatpak- und Snap-Installationen von Discord
    let subdirs = [
        "",
        "app/com.discordapp.Discord",
        "snap.discord",
        "app/dev.vencord.Vesktop",
    ];
    for base in &bases {
        for sub in subdirs {
            for i in 0..10 {
                let path = base.join(sub).join(format!("discord-ipc-{i}"));
                if let Ok(stream) = std::os::unix::net::UnixStream::connect(&path) {
                    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
                    return Some(Box::new(stream));
                }
            }
        }
    }
    None
}

impl Ipc {
    fn connect() -> Option<Self> {
        let mut ipc = Ipc { pipe: open_pipe()? };
        ipc.send(OP_HANDSHAKE, &json!({ "v": 1, "client_id": CLIENT_ID }))
            .ok()?;
        ipc.receive().ok()?;
        Some(ipc)
    }

    fn send(&mut self, opcode: u32, payload: &Value) -> std::io::Result<()> {
        let data = serde_json::to_vec(payload)?;
        let mut frame = Vec::with_capacity(8 + data.len());
        frame.extend_from_slice(&opcode.to_le_bytes());
        frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
        frame.extend_from_slice(&data);
        self.pipe.write_all(&frame)?;
        self.pipe.flush()
    }

    fn receive(&mut self) -> std::io::Result<Value> {
        let mut header = [0u8; 8];
        self.pipe.read_exact(&mut header)?;
        let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        if length > 1024 * 1024 {
            return Err(std::io::Error::other("Antwort zu groß"));
        }
        let mut data = vec![0u8; length];
        self.pipe.read_exact(&mut data)?;
        if opcode == OP_CLOSE {
            return Err(std::io::Error::other("Discord hat die Verbindung beendet"));
        }
        Ok(serde_json::from_slice(&data).unwrap_or(Value::Null))
    }

    /// `None` entfernt den Status. `Err` = Verbindung verloren; ein Status, den Discord ablehnt,
    /// wird nur protokolliert (die Verbindung bleibt).
    fn set_activity(&mut self, activity: Option<&Value>) -> std::io::Result<()> {
        self.send(
            OP_FRAME,
            &json!({
                "cmd": "SET_ACTIVITY",
                "args": { "pid": std::process::id(), "activity": activity },
                "nonce": uuid::Uuid::new_v4().to_string(),
            }),
        )?;
        let response = self.receive()?;
        if let Some(message) = rejection(&response) {
            tracing::warn!("Discord rejected the presence: {}", message);
        }
        Ok(())
    }
}

/// Fehlermeldung, falls Discord den Status abgelehnt hat (`"evt": "ERROR"`).
fn rejection(response: &Value) -> Option<String> {
    (response["evt"] == "ERROR").then(|| {
        response["data"]["message"]
            .as_str()
            .unwrap_or("unbekannter Fehler")
            .to_string()
    })
}

// ── Wo gerade gespielt wird ──────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum Place {
    Menu,
    World(Option<String>),
    Server(String),
    Lan,
}

/// Neue Position aus einer Zeile des Spiel-Logs.
fn place_from_log(line: &str) -> Option<Place> {
    if let Some((_, rest)) = line.split_once("Connecting to ") {
        // `Connecting to <host>, <port>` (Sprachchat-Mods schreiben ein anderes Format)
        let (host, port) = rest.trim().split_once(", ")?;
        let port: u16 = port.trim().parse().ok()?;
        let host = host.trim();
        if host.is_empty() || host.contains(char::is_whitespace) {
            return None;
        }
        if is_local_address(host) {
            return Some(Place::Lan);
        }
        return Some(Place::Server(if port == 25565 {
            host.to_string()
        } else {
            format!("{host}:{port}")
        }));
    }
    if line.contains("Starting integrated minecraft server") {
        return Some(Place::World(None));
    }
    if line.contains("Stopping singleplayer server") || line.contains("Disconnected from server") {
        return Some(Place::Menu);
    }
    None
}

fn is_local_address(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_private() || ip.is_loopback() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// Zuletzt geöffnete Welt: deren `session.lock` wird beim Öffnen geschrieben.
fn current_world(game_dir: &Path) -> Option<String> {
    std::fs::read_dir(game_dir.join("saves"))
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let modified = e
                .path()
                .join("session.lock")
                .metadata()
                .ok()?
                .modified()
                .ok()?;
            Some((modified, e.file_name().to_string_lossy().to_string()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, name)| name)
}

/// Liest neue Zeilen aus `logs/latest.log` (das Spiel legt die Datei bei jedem Start neu an).
struct LogTail {
    path: PathBuf,
    offset: u64,
    created: Option<SystemTime>,
    partial: String,
}

impl LogTail {
    fn new(path: PathBuf, started: SystemTime) -> Self {
        let metadata = path.metadata().ok();
        // Log einer früheren Sitzung überspringen; das aktuelle von Anfang an lesen
        let stale = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .is_some_and(|modified| modified + Duration::from_secs(2) < started);
        Self {
            offset: if stale {
                metadata.as_ref().map_or(0, |m| m.len())
            } else {
                0
            },
            created: metadata.and_then(|m| m.created().ok()),
            path,
            partial: String::new(),
        }
    }

    fn read_lines(&mut self) -> Vec<String> {
        let Ok(metadata) = self.path.metadata() else {
            return Vec::new();
        };
        let created = metadata.created().ok();
        // Neue Datei (neu angelegt oder kürzer geworden) → von vorn
        if metadata.len() < self.offset || (created.is_some() && created != self.created) {
            self.offset = 0;
            self.partial.clear();
            self.created = created;
        }
        if metadata.len() == self.offset {
            return Vec::new();
        }
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        // Höchstens 4 MB pro Runde (riesige Modpack-Logs)
        if file.take(4 * 1024 * 1024).read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        self.offset += bytes.len() as u64;
        self.partial.push_str(&String::from_utf8_lossy(&bytes));
        let mut lines: Vec<String> = self.partial.split('\n').map(String::from).collect();
        // Letzte (evtl. unvollständige) Zeile aufheben
        self.partial = lines.pop().unwrap_or_default();
        lines
    }
}

// ── Sitzungen ────────────────────────────────────────────────────────────────

struct Session {
    profile_id: String,
    profile_name: String,
    loader: String,
    minecraft_version: String,
    game_dir: PathBuf,
    started_ms: i64,
    place: Place,
    world_since: Option<Instant>,
    log: LogTail,
}

impl Session {
    fn update(&mut self) {
        for line in self.log.read_lines() {
            if let Some(place) = place_from_log(&line) {
                self.world_since = matches!(place, Place::World(_)).then(Instant::now);
                self.place = place;
            }
        }
        // Welt-Ordner nachschlagen, sobald ihr session.lock geschrieben wurde
        if let (Place::World(name), Some(since)) = (&self.place, self.world_since) {
            if since.elapsed() < WORLD_LOOKUP_WINDOW {
                let found = current_world(&self.game_dir);
                if found.is_some() && found != *name {
                    self.place = Place::World(found);
                }
            } else {
                self.world_since = None;
            }
        }
    }
}

/// Schnellstart in eine Welt / auf einen Server: Ziel schon vor dem ersten Log-Eintrag bekannt.
static HINTS: Mutex<Vec<(String, Place, Instant)>> = Mutex::new(Vec::new());

pub fn hint_world(profile_id: &str, world: &str) {
    push_hint(profile_id, Place::World(Some(world.to_string())));
}

pub fn hint_server(profile_id: &str, address: &str) {
    let place = place_from_log(&format!("Connecting to {}, 25565", address.trim()))
        .unwrap_or_else(|| Place::Server(address.trim().to_string()));
    push_hint(profile_id, place);
}

fn push_hint(profile_id: &str, place: Place) {
    let mut hints = HINTS.lock().unwrap_or_else(|e| e.into_inner());
    hints.retain(|(id, _, at)| id != profile_id && at.elapsed() < Duration::from_secs(600));
    hints.push((profile_id.to_string(), place, Instant::now()));
}

fn take_hint(profile_id: &str) -> Option<Place> {
    let mut hints = HINTS.lock().unwrap_or_else(|e| e.into_inner());
    let index = hints.iter().position(|(id, _, _)| id == profile_id)?;
    Some(hints.remove(index).1)
}

fn loader_label(loader: &str) -> &str {
    match loader {
        "fabric" => "Fabric",
        "forge" => "Forge",
        "neoforge" => "NeoForge",
        "quilt" => "Quilt",
        "horizon" => "Horizon",
        _ => "Vanilla",
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Öffnet Sitzungen für neu gestartete Spiele und schließt die beendeter.
fn sync_sessions(sessions: &mut Vec<Session>) {
    let running = crate::core::minecraft::running_instances();
    sessions.retain(|s| running.iter().any(|(id, _)| *id == s.profile_id));
    let missing: Vec<(String, u64)> = running
        .into_iter()
        .filter(|(id, _)| !sessions.iter().any(|s| s.profile_id == *id))
        .collect();
    if missing.is_empty() {
        return;
    }
    let Ok(profiles) = crate::core::profiles::ProfileManager::new()
        .map_err(|e| e.to_string())
        .and_then(|m| tauri::async_runtime::block_on(m.load_profiles()).map_err(|e| e.to_string()))
    else {
        return;
    };
    for (id, start_secs) in missing {
        let Some(profile) = profiles.get_profile(&id) else {
            continue;
        };
        let started_ms = if start_secs > 0 {
            start_secs as i64 * 1000
        } else {
            now_ms()
        };
        let started = UNIX_EPOCH + Duration::from_millis(started_ms as u64);
        let place = take_hint(&id).unwrap_or(Place::Menu);
        sessions.push(Session {
            profile_name: profile.name.clone(),
            loader: profile.loader.loader.as_str().to_string(),
            minecraft_version: profile.minecraft_version.clone(),
            game_dir: profile.game_dir.clone(),
            started_ms,
            world_since: matches!(place, Place::World(_)).then(Instant::now),
            place,
            log: LogTail::new(profile.game_dir.join("logs").join("latest.log"), started),
            profile_id: id,
        });
    }
}

// ── Anzeige ──────────────────────────────────────────────────────────────────

struct Texts {
    in_launcher: &'static str,
    menu: &'static str,
    world: &'static str,
    singleplayer: &'static str,
    server: &'static str,
    lan: &'static str,
}

fn texts() -> Texts {
    let german = crate::gui::settings::get_ui_state()
        .get("language")
        .and_then(|v| v.as_str())
        .is_some_and(|lang| lang == "de");
    if german {
        Texts {
            in_launcher: "Im Launcher",
            menu: "Im Hauptmenü",
            world: "Einzelspieler: ",
            singleplayer: "Einzelspieler",
            server: "Auf ",
            lan: "In einer LAN-Welt",
        }
    } else {
        Texts {
            in_launcher: "In the launcher",
            menu: "In the main menu",
            world: "Singleplayer: ",
            singleplayer: "Singleplayer",
            server: "On ",
            lan: "In a LAN world",
        }
    }
}

/// Discord erlaubt 2–128 Zeichen pro Textfeld.
fn clamp(text: String) -> String {
    let mut text: String = text.chars().take(128).collect();
    while text.chars().count() < 2 {
        text.push(' ');
    }
    text
}

fn player() -> Option<(String, String)> {
    crate::gui::auth::get_active_access_token().map(|(uuid, name, _)| (uuid, name))
}

fn activity(session: Option<&Session>) -> Value {
    let texts = texts();
    let mut assets = json!({ "large_image": LOGO_URL, "large_text": "Lion Launcher" });
    if let Some((uuid, name)) = player() {
        assets["small_image"] = json!(format!(
            "https://mc-heads.net/avatar/{}/64",
            uuid.replace('-', "")
        ));
        assets["small_text"] = json!(clamp(name));
    }
    let buttons = json!([{ "label": "Lion Launcher", "url": PROJECT_URL }]);
    let Some(session) = session else {
        return json!({
            "type": 0,
            "details": texts.in_launcher,
            "assets": assets,
            "buttons": buttons,
        });
    };
    let state = match &session.place {
        Place::Menu => texts.menu.to_string(),
        Place::World(Some(name)) => format!("{}{}", texts.world, name),
        Place::World(None) => texts.singleplayer.to_string(),
        Place::Server(address) => format!("{}{}", texts.server, address),
        Place::Lan => texts.lan.to_string(),
    };
    assets["large_text"] = json!(clamp(format!(
        "{} {} · Lion Launcher",
        loader_label(&session.loader),
        session.minecraft_version
    )));
    json!({
        "type": 0,
        "details": clamp(format!(
            "{} ({} {})",
            session.profile_name,
            loader_label(&session.loader),
            session.minecraft_version
        )),
        "state": clamp(state),
        "timestamps": { "start": session.started_ms },
        "assets": assets,
        "buttons": buttons,
    })
}

fn enabled() -> bool {
    tauri::async_runtime::block_on(crate::gui::settings::load_config())
        .map(|c| c.privacy.discord_rich_presence)
        .unwrap_or(true)
}

/// Startet den Hintergrund-Thread (beim App-Start).
pub fn start() {
    std::thread::Builder::new()
        .name("discord-presence".into())
        .spawn(run)
        .ok();
}

fn run() {
    let mut ipc: Option<Ipc> = None;
    let mut last_connect: Option<Instant> = None;
    let mut last_config: Option<Instant> = None;
    let mut is_enabled = true;
    let mut shown: Option<Value> = None;
    let mut sessions: Vec<Session> = Vec::new();

    loop {
        if last_config.is_none_or(|t| t.elapsed() >= CONFIG_INTERVAL) {
            is_enabled = enabled();
            last_config = Some(Instant::now());
        }
        if !is_enabled {
            if let Some(mut client) = ipc.take() {
                client.set_activity(None).ok();
            }
            shown = None;
            std::thread::sleep(TICK);
            continue;
        }

        sync_sessions(&mut sessions);
        sessions.iter_mut().for_each(Session::update);

        if ipc.is_none() && last_connect.is_none_or(|t| t.elapsed() >= RECONNECT_INTERVAL) {
            last_connect = Some(Instant::now());
            ipc = Ipc::connect();
            shown = None;
        }
        if let Some(client) = ipc.as_mut() {
            // Zuletzt gestartetes Spiel anzeigen
            let current = sessions.iter().max_by_key(|s| s.started_ms);
            let wanted = activity(current);
            if shown.as_ref() != Some(&wanted) {
                match client.set_activity(Some(&wanted)) {
                    Ok(()) => shown = Some(wanted),
                    Err(e) => {
                        tracing::debug!("Discord presence lost: {}", e);
                        ipc = None;
                    }
                }
            }
        }
        std::thread::sleep(TICK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_places_from_log() {
        assert_eq!(
            place_from_log(
                "[12:51:26] [Server Connector #1/INFO]: Connecting to mc.lion-craft.net, 25565"
            ),
            Some(Place::Server("mc.lion-craft.net".into()))
        );
        assert_eq!(
            place_from_log("[Render thread/INFO] [net.minecraft.client.gui.screens.ConnectScreen/]: Connecting to srv17.godlike.club, 26640"),
            Some(Place::Server("srv17.godlike.club:26640".into()))
        );
        assert_eq!(
            place_from_log(
                "[voicechat/]: [voicechat] Connecting to voice chat server: '134.19.177.228:26370'"
            ),
            None
        );
        assert_eq!(
            place_from_log("[Render thread/INFO]: Connecting to 192.168.1.20, 51234"),
            Some(Place::Lan)
        );
        assert_eq!(
            place_from_log(
                "[Server thread/INFO]: Starting integrated minecraft server version 1.21.4"
            ),
            Some(Place::World(None))
        );
        assert_eq!(
            place_from_log(
                "[Render thread/INFO]: Stopping singleplayer server as player logged out"
            ),
            Some(Place::Menu)
        );
        assert_eq!(
            place_from_log("[Render thread/INFO]: Loaded 202 advancements"),
            None
        );
    }

    #[test]
    fn tails_new_log_lines_only() {
        let dir = std::env::temp_dir().join(format!("lion-discord-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("latest.log");
        std::fs::write(&path, "old line\n").unwrap();
        let mut tail = LogTail::new(path.clone(), SystemTime::now() + Duration::from_secs(60));
        assert!(tail.read_lines().is_empty());
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        write!(file, "first\nsecond (half").unwrap();
        assert_eq!(tail.read_lines(), vec!["first".to_string()]);
        writeln!(file, ")").unwrap();
        assert_eq!(tail.read_lines(), vec!["second (half)".to_string()]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn texts_fit_discord_limits() {
        assert_eq!(clamp("x".into()).chars().count(), 2);
        assert_eq!(clamp("y".repeat(300)).chars().count(), 128);
    }

    /// Zeigt kurz einen Status in Discord (Discord muss laufen).
    /// `cargo test --bin Lion-Launcher discord_presence_live -- --ignored --nocapture`
    #[test]
    #[ignore = "benötigt ein laufendes Discord"]
    fn discord_presence_live() {
        let mut ipc = Ipc::connect().expect("Discord läuft nicht");
        let session = Session {
            profile_id: "test".into(),
            profile_name: "Testprofil".into(),
            loader: "fabric".into(),
            minecraft_version: "1.21.4".into(),
            game_dir: std::env::temp_dir(),
            started_ms: now_ms() - 754_000,
            place: Place::Server("mc.lion-craft.net".into()),
            world_since: None,
            log: LogTail::new(std::env::temp_dir().join("none.log"), SystemTime::now()),
        };
        ipc.send(
            OP_FRAME,
            &json!({
                "cmd": "SET_ACTIVITY",
                "args": { "pid": std::process::id(), "activity": activity(Some(&session)) },
                "nonce": "test",
            }),
        )
        .unwrap();
        let response = ipc.receive().unwrap();
        println!("Discord: {response}");
        assert_eq!(rejection(&response), None);
        std::thread::sleep(Duration::from_secs(20));
        ipc.set_activity(None).unwrap();
    }
}
