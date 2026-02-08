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

