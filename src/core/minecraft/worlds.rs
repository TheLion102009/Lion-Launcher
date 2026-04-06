use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldInfo {
    pub name: String,
    pub folder_name: String,
    pub icon_base64: Option<String>,
    pub last_played: i64,
    pub game_mode: String,
    pub difficulty: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub ip: String,
    pub icon_base64: Option<String>,
    pub motd: Option<String>,
    /// MOTD als HTML mit Minecraft-Farben und Formatierung (bold, italic etc.)
    pub motd_html: Option<Vec<String>>,
    pub online_players: Option<u32>,
    pub max_players: Option<u32>,
    pub online: Option<bool>,
}

/// Liest alle Welten aus dem saves-Ordner eines Profils
pub async fn get_worlds(game_dir: &Path) -> Result<Vec<WorldInfo>> {
    let saves_dir = game_dir.join("saves");

    if !saves_dir.exists() {
        return Ok(Vec::new());
    }

    let mut worlds = Vec::new();
    let mut entries = fs::read_dir(&saves_dir).await
        .context("Failed to read saves directory")?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let level_dat = path.join("level.dat");
        if !level_dat.exists() {
            continue;
        }

        let folder_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        // Versuche level.dat zu lesen (NBT Format)
        let world_info = read_world_info(&path, &folder_name).await
            .unwrap_or_else(|_| WorldInfo {
                name: folder_name.clone(),
                folder_name: folder_name.clone(),
                icon_base64: None,
                last_played: 0,
                game_mode: "Unknown".to_string(),
                difficulty: "Unknown".to_string(),
                size_bytes: 0,
            });

        worlds.push(world_info);
    }

    // Sortiere nach letzter Spielzeit (neueste zuerst)
    worlds.sort_by(|a, b| b.last_played.cmp(&a.last_played));

    Ok(worlds)
}

/// Liest World-Info aus level.dat
async fn read_world_info(world_path: &Path, folder_name: &str) -> Result<WorldInfo> {
    use std::io::Read;
    use flate2::read::GzDecoder;

    let level_dat_path = world_path.join("level.dat");
    let data = fs::read(&level_dat_path).await?;

    // level.dat ist gzip-komprimiert
    let mut decoder = GzDecoder::new(&data[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;

    // Parse NBT (vereinfacht - wir suchen nach bekannten Strings)
    let name = extract_nbt_string(&decompressed, "LevelName")
        .unwrap_or_else(|| folder_name.to_string());

    let last_played = extract_nbt_long(&decompressed, "LastPlayed")
        .unwrap_or(0);

    let game_mode = match extract_nbt_int(&decompressed, "GameType") {
        Some(0) => "Survival",
        Some(1) => "Creative",
        Some(2) => "Adventure",
        Some(3) => "Spectator",
        _ => "Unknown",
    }.to_string();

    let difficulty = match extract_nbt_int(&decompressed, "Difficulty") {
        Some(0) => "Peaceful",
        Some(1) => "Easy",
        Some(2) => "Normal",
        Some(3) => "Hard",
        _ => "Normal",
    }.to_string();

    // Versuche Icon zu laden
    let icon_path = world_path.join("icon.png");
    let icon_base64 = if icon_path.exists() {
        fs::read(&icon_path).await.ok().map(|data| {
            use base64::{Engine as _, engine::general_purpose};
            format!("data:image/png;base64,{}", general_purpose::STANDARD.encode(&data))
        })
    } else {
        None
    };

    // Berechne Ordnergröße (vereinfacht)
    let size_bytes = calculate_dir_size(world_path).await.unwrap_or(0);

    Ok(WorldInfo {
        name,
        folder_name: folder_name.to_string(),
        icon_base64,
        last_played,
        game_mode,
        difficulty,
        size_bytes,
    })
}

/// Extrahiert einen String aus NBT-Daten (vereinfachte Methode)
fn extract_nbt_string(data: &[u8], key: &str) -> Option<String> {
    let key_bytes = key.as_bytes();

    // Suche nach dem Key im NBT
    for i in 0..data.len().saturating_sub(key_bytes.len() + 4) {
        if &data[i..i + key_bytes.len()] == key_bytes {
            // Nach dem Key kommt die String-Länge (2 bytes, big-endian) und dann der String
            let offset = i + key_bytes.len();
            if offset + 2 < data.len() {
                let len = ((data[offset] as usize) << 8) | (data[offset + 1] as usize);
                if offset + 2 + len <= data.len() {
                    return String::from_utf8(data[offset + 2..offset + 2 + len].to_vec()).ok();
                }
            }
        }
    }
    None
}

/// Extrahiert einen Long aus NBT-Daten (vereinfachte Methode)
fn extract_nbt_long(data: &[u8], key: &str) -> Option<i64> {
    let key_bytes = key.as_bytes();

    for i in 0..data.len().saturating_sub(key_bytes.len() + 8) {
        if &data[i..i + key_bytes.len()] == key_bytes {
            let offset = i + key_bytes.len();
            if offset + 8 <= data.len() {
                let bytes: [u8; 8] = data[offset..offset + 8].try_into().ok()?;
                return Some(i64::from_be_bytes(bytes));
            }
        }
    }
    None
}

/// Extrahiert einen Int aus NBT-Daten (vereinfachte Methode)
fn extract_nbt_int(data: &[u8], key: &str) -> Option<i32> {
    let key_bytes = key.as_bytes();

    for i in 0..data.len().saturating_sub(key_bytes.len() + 4) {
        if &data[i..i + key_bytes.len()] == key_bytes {
            let offset = i + key_bytes.len();
            if offset + 4 <= data.len() {
                let bytes: [u8; 4] = data[offset..offset + 4].try_into().ok()?;
                return Some(i32::from_be_bytes(bytes));
            }
        }
    }
    None
}

/// Berechnet die Größe eines Verzeichnisses
async fn calculate_dir_size(path: &Path) -> Result<u64> {
    let mut size = 0u64;
    let mut stack = vec![path.to_path_buf()];

    while let Some(current) = stack.pop() {
        if let Ok(mut entries) = fs::read_dir(&current).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if let Ok(metadata) = fs::metadata(&path).await {
                    if metadata.is_file() {
                        size += metadata.len();
                    } else if metadata.is_dir() {
                        stack.push(path);
                    }
                }
            }
        }
    }

    Ok(size)
}

/// Liest Server aus servers.dat
pub async fn get_servers(game_dir: &Path) -> Result<Vec<ServerInfo>> {
    let servers_dat = game_dir.join("servers.dat");

    if !servers_dat.exists() {
        return Ok(Vec::new());
    }

    let data = fs::read(&servers_dat).await?;

    // servers.dat ist unkomprimiertes NBT
    let mut servers = parse_servers_dat(&data)?;

    tracing::info!("Parsed {} servers from servers.dat", servers.len());

    // Versuche live Status (Icon + MOTD) für alle Server PARALLEL zu holen
    // So dauert es max 5s statt 5s × Anzahl Server
    let status_futures: Vec<_> = servers.iter()
        .map(|s| query_server_status(&s.ip))
        .collect();

    let results = futures_util::future::join_all(status_futures).await;

    for (server, result) in servers.iter_mut().zip(results.into_iter()) {
        match result {
            Ok(status) => {
                if server.icon_base64.is_none() {
                    server.icon_base64 = status.icon_base64;
                }
                if server.motd.is_none() || server.motd.as_deref() == Some("") {
                    server.motd = status.motd;
                }
                server.motd_html = status.motd_html;
                server.online_players = status.online_players;
                server.max_players = status.max_players;
                server.online = Some(true);
            }
            Err(_) => {
                server.online = Some(false);
            }
        }
    }

    Ok(servers)
}

/// Fragt den Server-Status über die mcsrvstat.us API ab
async fn query_server_status(address: &str) -> Result<ServerStatusResponse> {
    // Adresse bereinigen: Leerzeichen entfernen
    let address = address.trim();

    // API v2 ist stabiler und braucht weniger Parsing
    let url = format!("https://api.mcsrvstat.us/2/{}", address);
    tracing::info!("Querying server status: {}", url);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Lion-Launcher/1.0")
        .build()?;

    let resp = client.get(&url).send().await
        .map_err(|e| {
            tracing::warn!("Server status request failed for '{}': {}", address, e);
            e
        })?;

    let status_code = resp.status();
    if !status_code.is_success() {
        tracing::warn!("Server status API returned {} for '{}'", status_code, address);
        anyhow::bail!("API returned {}", status_code);
    }

    let body = resp.text().await?;
    tracing::debug!("Server status response for '{}': {}...chars", address, body.len());

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| {
            tracing::warn!("Failed to parse server status JSON for '{}': {}", address, e);
            e
        })?;

    let online = json.get("online").and_then(|v| v.as_bool()).unwrap_or(false);
    if !online {
        tracing::info!("Server '{}' reports as offline", address);
        anyhow::bail!("Server offline");
    }

    // Icon: API v2 gibt base64-String OHNE data:-Prefix zurück
    let icon_base64 = json.get("icon")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with("data:") {
                s.to_string()
            } else {
                format!("data:image/png;base64,{}", s)
            }
        });

    // MOTD: clean-Array (ohne Farb-Codes) als Fallback
    let motd = json.get("motd")
        .and_then(|v| v.get("clean"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ")
        )
        .filter(|s| !s.trim().is_empty());

    // MOTD als HTML-Array: Jede Zeile ist ein HTML-String mit Minecraft-Farben und Bold/Italic
    let motd_html = json.get("motd")
        .and_then(|v| v.get("html"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<String>>()
        )
        .filter(|v| !v.is_empty());

    let online_players = json.get("players")
        .and_then(|v| v.get("online"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    let max_players = json.get("players")
        .and_then(|v| v.get("max"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    tracing::info!("Server '{}': online, icon={}, motd={}, motd_html={}, players={:?}/{:?}",
        address, icon_base64.is_some(), motd.is_some(), motd_html.is_some(), online_players, max_players);

    Ok(ServerStatusResponse {
        icon_base64,
        motd,
        motd_html,
        online_players,
        max_players,
    })
}

struct ServerStatusResponse {
    icon_base64: Option<String>,
    motd: Option<String>,
    motd_html: Option<Vec<String>>,
    online_players: Option<u32>,
    max_players: Option<u32>,
}

/// Fügt einen Server zur servers.dat eines Profils hinzu
pub async fn add_server(game_dir: &Path, name: &str, ip: &str) -> Result<()> {
    let servers_dat = game_dir.join("servers.dat");

    // Lies existierende Server
    let mut existing_servers = if servers_dat.exists() {
        let data = fs::read(&servers_dat).await?;
        parse_servers_dat(&data)?
    } else {
        Vec::new()
    };

    // Prüfe auf Duplikate
    if existing_servers.iter().any(|s| s.ip == ip) {
        anyhow::bail!("Server mit dieser IP existiert bereits");
    }

    existing_servers.push(ServerInfo {
        name: name.to_string(),
        ip: ip.to_string(),
        icon_base64: None,
        motd: None,
        motd_html: None,
        online_players: None,
        max_players: None,
        online: None,
    });

    // Schreibe neue servers.dat
    let nbt_data = build_servers_dat(&existing_servers);
    tokio::fs::create_dir_all(game_dir).await?;
    fs::write(&servers_dat, &nbt_data).await?;

    tracing::info!("Server '{}' ({}) zur servers.dat hinzugefügt", name, ip);
    Ok(())
}

/// Entfernt einen Server aus servers.dat
pub async fn remove_server(game_dir: &Path, ip: &str) -> Result<()> {
    let servers_dat = game_dir.join("servers.dat");

    if !servers_dat.exists() {
        anyhow::bail!("servers.dat nicht gefunden");
    }

    let data = fs::read(&servers_dat).await?;
    let mut servers = parse_servers_dat(&data)?;

    let before = servers.len();
    servers.retain(|s| s.ip != ip);

    if servers.len() == before {
        anyhow::bail!("Server nicht gefunden");
    }

    let nbt_data = build_servers_dat(&servers);
    fs::write(&servers_dat, &nbt_data).await?;

    tracing::info!("Server '{}' aus servers.dat entfernt", ip);
    Ok(())
}

/// Baut eine servers.dat im NBT-Format
/// Format:
/// TAG_Compound(""):
///   TAG_List("servers", TAG_Compound):
///     TAG_Compound(""):
///       TAG_String("name"): ...
///       TAG_String("ip"): ...
fn build_servers_dat(servers: &[ServerInfo]) -> Vec<u8> {
    let mut data = Vec::new();

    // Root TAG_Compound (unnamed)
    data.push(0x0A); // TAG_Compound type
    write_nbt_string_tag_name(&mut data, ""); // leerer Name

    // TAG_List "servers"
    data.push(0x09); // TAG_List type
    write_nbt_string_tag_name(&mut data, "servers");
    data.push(0x0A); // Element-Typ: TAG_Compound
    // Anzahl der Einträge (big-endian i32)
    let count = servers.len() as i32;
    data.extend_from_slice(&count.to_be_bytes());

    for server in servers {
        // TAG_String "name"
        data.push(0x08); // TAG_String type
        write_nbt_string_tag_name(&mut data, "name");
        write_nbt_string_value(&mut data, &server.name);

        // TAG_String "ip"
        data.push(0x08); // TAG_String type
        write_nbt_string_tag_name(&mut data, "ip");
        write_nbt_string_value(&mut data, &server.ip);

        // TAG_End (Ende des Compound)
        data.push(0x00);
    }

    // TAG_End (Ende des Root-Compound)
    data.push(0x00);

    data
}

fn write_nbt_string_tag_name(data: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    data.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    data.extend_from_slice(bytes);
}

fn write_nbt_string_value(data: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    data.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    data.extend_from_slice(bytes);
}

/// Parst servers.dat (NBT Format)
///
/// find_sequence() gibt die Position NACH dem Suchstring zurück.
/// D.h. nach b"name" zeigt pos direkt auf die String-Länge (2 Bytes BE) des Werts.
fn parse_servers_dat(data: &[u8]) -> Result<Vec<ServerInfo>> {
    let mut servers = Vec::new();

    let mut i = 0;
    while i < data.len() {
        // Suche nach "name" Tag gefolgt von String
        if let Some(pos) = find_sequence(data, i, b"name") {
            // pos zeigt direkt auf die Wert-Länge (2 Bytes) nach "name"
            if pos + 2 > data.len() { i = pos; continue; }
            let name_len = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
            if pos + 2 + name_len > data.len() { i = pos; continue; }
            let name = String::from_utf8_lossy(&data[pos + 2..pos + 2 + name_len]).to_string();

            // Suche nach "ip" in der Nähe (innerhalb 200 Bytes)
            let search_start = pos + 2 + name_len;
            let search_end = (search_start + 200).min(data.len());

            if let Some(ip_pos) = find_sequence(data, search_start, b"ip") {
                if ip_pos < search_end {
                    // ip_pos zeigt direkt auf die Wert-Länge (2 Bytes) nach "ip"
                    if ip_pos + 2 > data.len() { i = ip_pos; continue; }
                    let ip_len = ((data[ip_pos] as usize) << 8) | (data[ip_pos + 1] as usize);
                    if ip_pos + 2 + ip_len > data.len() { i = ip_pos; continue; }
                    let ip = String::from_utf8_lossy(&data[ip_pos + 2..ip_pos + 2 + ip_len]).to_string();

                    // Vermeide Duplikate
                    if !servers.iter().any(|s: &ServerInfo| s.ip == ip) {
                        servers.push(ServerInfo {
                            name,
                            ip,
                            icon_base64: None,
                            motd: None,
                            motd_html: None,
                            online_players: None,
                            max_players: None,
                            online: None,
                        });
                    }

                    i = ip_pos + 2 + ip_len;
                    continue;
                }
            }
            i = search_start;
        } else {
            break; // Kein "name" mehr gefunden → fertig
        }
    }

    Ok(servers)
}

fn find_sequence(data: &[u8], start: usize, seq: &[u8]) -> Option<usize> {
    for i in start..data.len().saturating_sub(seq.len()) {
        if &data[i..i + seq.len()] == seq {
            return Some(i + seq.len());
        }
    }
    None
}

/// Formatiert Bytes in lesbare Größe
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
