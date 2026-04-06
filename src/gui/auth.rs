#![allow(dead_code)]

use crate::core::auth::{MinecraftAuth, AuthState, DeviceCodeFlow, get_head_url};
use tokio::sync::Mutex;
use once_cell::sync::Lazy;

// Global Auth State - pub(crate) für interne Verwendung
pub(crate) static AUTH_STATE: Lazy<Mutex<AuthState>> = Lazy::new(|| {
    Mutex::new(load_auth_state().unwrap_or_default())
});

fn get_auth_file_path() -> std::path::PathBuf {
    crate::config::defaults::data_dir().join("auth.json")
}

fn load_auth_state() -> Option<AuthState> {
    let path = get_auth_file_path();
    if path.exists() {
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    } else {
        None
    }
}

fn save_auth_state(state: &AuthState) -> Result<(), String> {
    let path = get_auth_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(serde::Serialize)]
pub struct AccountInfo {
    pub uuid: String,
    pub username: String,
    pub head_url: String,
    pub is_microsoft: bool,
    pub is_active: bool,
}

#[tauri::command]
pub async fn get_accounts() -> Result<Vec<AccountInfo>, String> {
    let state = AUTH_STATE.lock().await;

    let accounts: Vec<AccountInfo> = state.accounts.iter().map(|acc| {
        AccountInfo {
            uuid: acc.uuid.clone(),
            username: acc.username.clone(),
            head_url: get_head_url(&acc.uuid, 64),
            is_microsoft: acc.is_microsoft,
            is_active: state.active_account.as_ref() == Some(&acc.uuid),
        }
    }).collect();

    Ok(accounts)
}

#[tauri::command]
pub async fn get_active_account() -> Result<Option<AccountInfo>, String> {
    let state = AUTH_STATE.lock().await;

    if let Some(active_uuid) = &state.active_account {
        if let Some(acc) = state.accounts.iter().find(|a| &a.uuid == active_uuid) {
            return Ok(Some(AccountInfo {
                uuid: acc.uuid.clone(),
                username: acc.username.clone(),
                head_url: get_head_url(&acc.uuid, 64),
                is_microsoft: acc.is_microsoft,
                is_active: true,
            }));
        }
    }

    Ok(None)
}

#[tauri::command]
pub async fn set_active_account(uuid: String) -> Result<(), String> {
    let mut state = AUTH_STATE.lock().await;

    if state.accounts.iter().any(|a| a.uuid == uuid) {
        state.active_account = Some(uuid);
        save_auth_state(&state)?;
        Ok(())
    } else {
        Err("Account nicht gefunden".to_string())
    }
}

/// Startet den Device Code Flow für Microsoft Login
#[tauri::command]
pub async fn begin_microsoft_login() -> Result<DeviceCodeFlow, String> {
    let auth = MinecraftAuth::new();
    auth.begin_device_code_flow()
        .await
        .map_err(|e| format!("Fehler beim Starten des Logins: {}", e))
}

/// Pollt für Token nachdem User den Code eingegeben hat
#[tauri::command]
pub async fn poll_microsoft_login(device_code: String) -> Result<Option<AccountInfo>, String> {
    let auth = MinecraftAuth::new();

    match auth.poll_for_token(&device_code).await {
        Ok(Some(account)) => {
            let account_info = AccountInfo {
                uuid: account.uuid.clone(),
                username: account.username.clone(),
                head_url: get_head_url(&account.uuid, 64),
                is_microsoft: account.is_microsoft,
                is_active: true,
            };

            // Zum State hinzufügen
            let mut state = AUTH_STATE.lock().await;

            if let Some(existing) = state.accounts.iter_mut().find(|a| a.uuid == account.uuid) {
                *existing = account.clone();
            } else {
                state.accounts.push(account.clone());
            }

            state.active_account = Some(account.uuid);
            save_auth_state(&state)?;

            Ok(Some(account_info))
        }
        Ok(None) => Ok(None), // Noch nicht autorisiert
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn add_offline_account(username: String) -> Result<AccountInfo, String> {
    if username.is_empty() || username.len() > 16 {
        return Err("Username muss zwischen 1 und 16 Zeichen lang sein".to_string());
    }

    let account = MinecraftAuth::create_offline_account(&username);

    let account_info = AccountInfo {
        uuid: account.uuid.clone(),
        username: account.username.clone(),
        head_url: get_head_url(&account.uuid, 64),
        is_microsoft: account.is_microsoft,
        is_active: true,
    };

    let mut state = AUTH_STATE.lock().await;

    if let Some(existing) = state.accounts.iter_mut().find(|a| a.uuid == account.uuid) {
        *existing = account.clone();
    } else {
        state.accounts.push(account.clone());
    }

    state.active_account = Some(account.uuid);
    save_auth_state(&state)?;

    Ok(account_info)
}

#[tauri::command]
pub async fn remove_account(uuid: String) -> Result<(), String> {
    let mut state = AUTH_STATE.lock().await;

    state.accounts.retain(|a| a.uuid != uuid);

    if state.active_account.as_ref() == Some(&uuid) {
        state.active_account = state.accounts.first().map(|a| a.uuid.clone());
    }

    save_auth_state(&state)?;
    Ok(())
}

#[tauri::command]
pub async fn refresh_account(uuid: String) -> Result<AccountInfo, String> {
    let account = {
        let state = AUTH_STATE.lock().await;
        state.accounts.iter()
            .find(|a| a.uuid == uuid)
            .ok_or_else(|| "Account nicht gefunden".to_string())?
            .clone()
    };

    if !account.is_microsoft {
        return Err("Offline-Accounts können nicht aktualisiert werden".to_string());
    }

    let refresh_token = account.refresh_token
        .ok_or_else(|| "Kein Refresh-Token vorhanden".to_string())?;

    let auth = MinecraftAuth::new();
    let new_account = auth.refresh_auth(&refresh_token)
        .await
        .map_err(|e| format!("Refresh fehlgeschlagen: {}", e))?;

    let account_info = AccountInfo {
        uuid: new_account.uuid.clone(),
        username: new_account.username.clone(),
        head_url: get_head_url(&new_account.uuid, 64),
        is_microsoft: new_account.is_microsoft,
        is_active: true,
    };

    let mut state = AUTH_STATE.lock().await;

    if let Some(existing) = state.accounts.iter_mut().find(|a| a.uuid == new_account.uuid) {
        *existing = new_account;
    }

    save_auth_state(&state)?;

    Ok(account_info)
}

#[tauri::command]
pub async fn upload_skin_file(skin_data: String, variant: String) -> Result<(), String> {
    use base64::{Engine as _, engine::general_purpose};

    let (_, _, access_token) = get_active_access_token_refreshed()
        .await
        .ok_or_else(|| "Kein aktiver Microsoft-Account gefunden".to_string())?;

    // Prüfe ob es ein Microsoft-Account ist
    {
        let state = AUTH_STATE.lock().await;
        if let Some(active_uuid) = &state.active_account {
            if let Some(acc) = state.accounts.iter().find(|a| &a.uuid == active_uuid) {
                if !acc.is_microsoft {
                    return Err("Skin-Upload funktioniert nur mit Microsoft-Accounts".to_string());
                }
            }
        }
    }

    // Base64 dekodieren
    let skin_bytes = general_purpose::STANDARD.decode(&skin_data)
        .map_err(|e| format!("Ungültige Skin-Daten: {}", e))?;

    let skin_variant = if variant == "slim" { "slim" } else { "classic" };

    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::bytes(skin_bytes)
        .file_name("skin.png")
        .mime_str("image/png")
        .map_err(|e| e.to_string())?;

    let form = reqwest::multipart::Form::new()
        .text("variant", skin_variant.to_string())
        .part("file", part);

    let response = client
        .post("https://api.minecraftservices.com/minecraft/profile/skins")
        .header("Authorization", format!("Bearer {}", access_token))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Fehler beim Upload: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Skin-Upload fehlgeschlagen ({}): {}", status, body));
    }

    tracing::info!("Skin erfolgreich hochgeladen!");
    Ok(())
}

/// Skin von URL übernehmen (z.B. von einem anderen Spieler)
/// Lädt den Skin erst herunter und sendet ihn dann als Multipart-Upload,
/// da die Mojang-API nur URLs von textures.minecraft.net akzeptiert.
#[tauri::command]
pub async fn apply_skin_from_url(skin_url: String, variant: String) -> Result<(), String> {
    let (_, _, access_token) = get_active_access_token_refreshed()
        .await
        .ok_or_else(|| "Kein aktiver Microsoft-Account gefunden".to_string())?;

    // Prüfe ob es ein Microsoft-Account ist
    {
        let state = AUTH_STATE.lock().await;
        if let Some(active_uuid) = &state.active_account {
            if let Some(acc) = state.accounts.iter().find(|a| &a.uuid == active_uuid) {
                if !acc.is_microsoft {
                    return Err("Skin-Änderung funktioniert nur mit Microsoft-Accounts".to_string());
                }
            }
        }
    }

    let skin_variant = if variant == "slim" { "slim" } else { "classic" };

    let client = reqwest::Client::builder()
        .user_agent("Lion-Launcher/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    // Skin-Textur zuerst herunterladen
    let download_response = client
        .get(&skin_url)
        .send()
        .await
        .map_err(|e| format!("Fehler beim Herunterladen des Skins: {}", e))?;

    if !download_response.status().is_success() {
        return Err(format!("Skin konnte nicht heruntergeladen werden ({})", download_response.status()));
    }

    let skin_bytes = download_response.bytes().await
        .map_err(|e| format!("Fehler beim Lesen der Skin-Daten: {}", e))?;

    // Als Multipart-Upload an Mojang senden
    let part = reqwest::multipart::Part::bytes(skin_bytes.to_vec())
        .file_name("skin.png")
        .mime_str("image/png")
        .map_err(|e| e.to_string())?;

    let form = reqwest::multipart::Form::new()
        .text("variant", skin_variant.to_string())
        .part("file", part);

    let response = client
        .post("https://api.minecraftservices.com/minecraft/profile/skins")
        .header("Authorization", format!("Bearer {}", access_token))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Fehler beim Skin-Wechsel: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(format!("Skin-Wechsel fehlgeschlagen ({}): {}", status, body_text));
    }

    tracing::info!("Skin erfolgreich von URL übernommen!");
    Ok(())
}

/// Skin-Textur als Base64 Data-URL proxen (CORS-Umgehung für skinview3d)
/// Holt den Skin direkt von Mojang's Session-Server (kein Cache wie mc-heads.net),
/// damit nach einem Skin-Wechsel sofort der neue Skin angezeigt wird.
#[tauri::command]
pub async fn get_skin_texture(uuid: String) -> Result<String, String> {
    use base64::{Engine as _, engine::general_purpose};

    let client = reqwest::Client::builder()
        .user_agent("Lion-Launcher/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    // UUID ohne Bindestriche für Mojang API
    let clean_uuid = uuid.replace('-', "");

    // 1. Profil von Mojang's Session-Server holen (kein Cache!)
    let profile_url = format!(
        "https://sessionserver.mojang.com/session/minecraft/profile/{}",
        clean_uuid
    );

    let profile_response = client
        .get(&profile_url)
        .send()
        .await
        .map_err(|e| format!("Fehler beim Laden des Profils: {}", e))?;

    if !profile_response.status().is_success() {
        // Fallback auf mc-heads.net für Offline-/unbekannte UUIDs
        tracing::warn!("Mojang Session-Server gab {} zurück, Fallback auf mc-heads.net", profile_response.status());
        return get_skin_texture_fallback(&client, &uuid).await;
    }

    let profile: serde_json::Value = profile_response.json().await
        .map_err(|e| format!("Ungültige Profil-Antwort: {}", e))?;

    // 2. Textures-Property aus dem Profil extrahieren und dekodieren
    let skin_url = profile["properties"].as_array()
        .and_then(|props| props.iter().find(|p| p["name"].as_str() == Some("textures")))
        .and_then(|prop| prop["value"].as_str())
        .and_then(|b64_value| general_purpose::STANDARD.decode(b64_value).ok())
        .and_then(|decoded_bytes| String::from_utf8(decoded_bytes).ok())
        .and_then(|json_str| serde_json::from_str::<serde_json::Value>(&json_str).ok())
        .and_then(|textures| {
            textures["textures"]["SKIN"]["url"].as_str().map(|s| s.to_string())
        });

    let skin_url = match skin_url {
        Some(url) => url,
        None => {
            tracing::warn!("Keine Skin-URL im Profil gefunden, Fallback auf mc-heads.net");
            return get_skin_texture_fallback(&client, &uuid).await;
        }
    };

    // 3. Skin-Textur direkt von textures.minecraft.net herunterladen
    let skin_response = client
        .get(&skin_url)
        .send()
        .await
        .map_err(|e| format!("Fehler beim Laden der Skin-Textur: {}", e))?;

    if !skin_response.status().is_success() {
        return Err(format!("Skin nicht gefunden ({})", skin_response.status()));
    }

    let bytes = skin_response.bytes().await.map_err(|e| e.to_string())?;
    let encoded = general_purpose::STANDARD.encode(&bytes);

    Ok(format!("data:image/png;base64,{}", encoded))
}

/// Fallback: Skin von mc-heads.net holen (für Offline-Accounts oder wenn Mojang API fehlschlägt)
async fn get_skin_texture_fallback(client: &reqwest::Client, uuid: &str) -> Result<String, String> {
    use base64::{Engine as _, engine::general_purpose};

    let url = format!("https://mc-heads.net/skin/{}", uuid);

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Fehler beim Laden der Skin-Textur: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Skin nicht gefunden ({})", response.status()));
    }

    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    let encoded = general_purpose::STANDARD.encode(&bytes);

    Ok(format!("data:image/png;base64,{}", encoded))
}

/// Skin lokal speichern (wird beim Equip aufgerufen)
/// Speichert die Skin-Textur als PNG unter skins/{name}_{uuid}.png
#[tauri::command]
pub async fn save_skin_locally(uuid: String, name: String, skin_data: String) -> Result<String, String> {
    use base64::{Engine as _, engine::general_purpose};

    let skins_dir = crate::config::defaults::skins_dir();
    std::fs::create_dir_all(&skins_dir).map_err(|e| format!("Konnte Skins-Verzeichnis nicht erstellen: {}", e))?;

    // Base64 data URL dekodieren
    let b64 = if skin_data.contains(",") {
        skin_data.split(',').nth(1).unwrap_or(&skin_data).to_string()
    } else {
        skin_data
    };

    let bytes = general_purpose::STANDARD.decode(&b64)
        .map_err(|e| format!("Ungültige Skin-Daten: {}", e))?;

    let clean_name = name.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");
    let filename = format!("{}_{}.png", clean_name, uuid.replace('-', ""));
    let path = skins_dir.join(&filename);

    std::fs::write(&path, &bytes).map_err(|e| format!("Fehler beim Speichern: {}", e))?;

    tracing::info!("Skin lokal gespeichert: {}", path.display());
    Ok(filename)
}

/// Lokal gespeicherten Skin als Base64 Data-URL laden
#[tauri::command]
pub async fn load_saved_skin(filename: String) -> Result<String, String> {
    use base64::{Engine as _, engine::general_purpose};

    let path = crate::config::defaults::skins_dir().join(&filename);

    if !path.exists() {
        return Err("Skin-Datei nicht gefunden".to_string());
    }

    let bytes = std::fs::read(&path).map_err(|e| format!("Fehler beim Lesen: {}", e))?;
    let encoded = general_purpose::STANDARD.encode(&bytes);

    Ok(format!("data:image/png;base64,{}", encoded))
}

/// Lokal gespeicherten Skin löschen
#[tauri::command]
pub async fn delete_saved_skin(filename: String) -> Result<(), String> {
    let path = crate::config::defaults::skins_dir().join(&filename);

    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Fehler beim Löschen: {}", e))?;
        tracing::info!("Skin gelöscht: {}", path.display());
    }

    Ok(())
}

/// Spieler-UUID über Mojang API auflösen (CORS-Proxy)
#[tauri::command]
pub async fn resolve_player_uuid(username: String) -> Result<(String, String), String> {

    let url = format!("https://api.mojang.com/users/profiles/minecraft/{}", username);

    let client = reqwest::Client::builder()
        .user_agent("Lion-Launcher/1.0")
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Fehler bei Mojang API: {}", e))?;

    if !response.status().is_success() {
        return Err("Spieler nicht gefunden".to_string());
    }

    let data: serde_json::Value = response.json().await
        .map_err(|e| format!("Ungültige Antwort: {}", e))?;

    let uuid = data["id"].as_str()
        .ok_or_else(|| "Keine UUID in Antwort".to_string())?
        .to_string();
    let name = data["name"].as_str()
        .ok_or_else(|| "Kein Name in Antwort".to_string())?
        .to_string();

    Ok((uuid, name))
}

/// Öffnet eine URL im Standard-Browser
#[tauri::command]
pub async fn open_auth_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Konnte Browser nicht öffnen: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
            .map_err(|e| format!("Konnte Browser nicht öffnen: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Konnte Browser nicht öffnen: {}", e))?;
    }

    Ok(())
}

/// Gibt das Access-Token für den aktiven Account zurück (für Minecraft-Start)
pub fn get_active_access_token() -> Option<(String, String, String)> {
    let state = AUTH_STATE.try_lock().ok()?;

    let active_uuid = state.active_account.as_ref()?;
    let account = state.accounts.iter().find(|a| &a.uuid == active_uuid)?;

    Some((account.uuid.clone(), account.username.clone(), account.access_token.clone()))
}

/// Gibt das Access-Token zurück und refreshed es automatisch wenn es abgelaufen ist
pub async fn get_active_access_token_refreshed() -> Option<(String, String, String)> {
    let account_data = {
        let state = AUTH_STATE.try_lock().ok()?;
        let active_uuid = state.active_account.as_ref()?;
        let account = state.accounts.iter().find(|a| &a.uuid == active_uuid)?;
        
        (account.uuid.clone(), 
         account.username.clone(), 
         account.access_token.clone(),
         account.expires_at,
         account.refresh_token.clone(),
         account.is_microsoft)
    };
    
    let (uuid, username, access_token, expires_at, refresh_token, is_microsoft) = account_data;
    
    // Prüfe ob Token abgelaufen ist (oder in den nächsten 5 Minuten abläuft)
    let needs_refresh = if let Some(expires) = expires_at {
        use chrono::Utc;
        let now = Utc::now();
        let threshold = now + chrono::Duration::minutes(5);
        expires < threshold
    } else {
        false
    };
    
    if needs_refresh && is_microsoft {
        if let Some(ref_token) = refresh_token {
            tracing::info!("⚠️  Access-Token ist abgelaufen, refreshe automatisch...");
            
            // Versuche Token zu refreshen
            let auth = crate::core::auth::MinecraftAuth::new();
            match auth.refresh_auth(&ref_token).await {
                Ok(new_account) => {
                    tracing::info!("✅ Token erfolgreich refreshed!");
                    
                    // Update im State
                    let mut state = AUTH_STATE.lock().await;
                    if let Some(existing) = state.accounts.iter_mut().find(|a| a.uuid == uuid) {
                        *existing = new_account.clone();
                    }
                    
                    // Speichere State
                    if let Err(e) = save_auth_state(&state) {
                        tracing::warn!("⚠️  Konnte Auth-State nicht speichern: {}", e);
                    }
                    
                    return Some((new_account.uuid, new_account.username, new_account.access_token));
                }
                Err(e) => {
                    tracing::error!("❌ Token-Refresh fehlgeschlagen: {}", e);
                    tracing::warn!("⚠️  Verwende alten Token, Multiplayer funktioniert eventuell nicht!");
                }
            }
        }
    }
    
    Some((uuid, username, access_token))
}

