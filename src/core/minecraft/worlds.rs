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
    let servers = parse_servers_dat(&data)?;

    Ok(servers)
}

/// Parst servers.dat (NBT Format)
fn parse_servers_dat(data: &[u8]) -> Result<Vec<ServerInfo>> {
    let mut servers = Vec::new();

    // Suche nach Server-Einträgen
    // Das NBT hat eine Liste "servers" mit Compound-Tags
    // Jeder Server hat "name", "ip" und optional "icon"

    let mut i = 0;
    while i < data.len() {
        // Suche nach "name" Tag gefolgt von String
        if let Some(pos) = find_sequence(data, i, b"name") {
            let name_start = pos + 4; // "name" length
            if name_start + 2 < data.len() {
                let name_len = ((data[name_start] as usize) << 8) | (data[name_start + 1] as usize);
                if name_start + 2 + name_len <= data.len() {
                    let name = String::from_utf8_lossy(&data[name_start + 2..name_start + 2 + name_len]).to_string();

                    // Suche nach "ip" in der Nähe
                    let search_start = name_start + 2 + name_len;
                    let search_end = (search_start + 200).min(data.len());

                    if let Some(ip_pos) = find_sequence(data, search_start, b"ip") {
                        if ip_pos < search_end {
                            let ip_start = ip_pos + 2; // "ip" length
                            if ip_start + 2 < data.len() {
                                let ip_len = ((data[ip_start] as usize) << 8) | (data[ip_start + 1] as usize);
                                if ip_start + 2 + ip_len <= data.len() {
                                    let ip = String::from_utf8_lossy(&data[ip_start + 2..ip_start + 2 + ip_len]).to_string();

                                    // Vermeide Duplikate
                                    if !servers.iter().any(|s: &ServerInfo| s.ip == ip) {
                                        servers.push(ServerInfo {
                                            name,
                                            ip,
                                            icon_base64: None, // TODO: Icon aus NBT extrahieren
                                            motd: None, // Wird zur Laufzeit geholt
                                        });
                                    }

                                    i = ip_start + 2 + ip_len;
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }
        i += 1;
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
