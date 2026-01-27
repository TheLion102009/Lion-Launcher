#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc, Duration};

// Azure AD App - MultiMC's öffentliche Client ID (funktioniert mit Device Code Flow)
const AZURE_CLIENT_ID: &str = "499c8d36-be2a-4231-9ebd-ef291b7bb64c";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftAccount {
    pub uuid: String,
    pub username: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub skin_url: Option<String>,
    pub cape_url: Option<String>,
    pub is_microsoft: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub accounts: Vec<MinecraftAccount>,
    pub active_account: Option<String>, // UUID des aktiven Accounts
}

impl Default for AuthState {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            active_account: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeFlow {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    pub message: String,
}

// Device Code Response
#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
    message: String,
}

// Token Response
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    #[serde(default)]
    error: Option<String>,
}

// Xbox Live Token Response
#[derive(Debug, Deserialize)]
struct XboxLiveResponse {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: HashMap<String, serde_json::Value>,
}

// Minecraft Auth Response
#[derive(Debug, Deserialize)]
struct MinecraftAuthResponse {
    access_token: String,
    expires_in: u64,
}

// Minecraft Profile Response
#[derive(Debug, Deserialize)]
struct MinecraftProfileResponse {
    id: String,
    name: String,
    skins: Option<Vec<SkinInfo>>,
    capes: Option<Vec<CapeInfo>>,
}

#[derive(Debug, Deserialize)]
struct SkinInfo {
    url: String,
    state: String,
}

#[derive(Debug, Deserialize)]
struct CapeInfo {
    url: String,
    state: String,
}

pub struct MinecraftAuth {
    client: reqwest::Client,
}

impl MinecraftAuth {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("Lion-Launcher/1.0")
                .build()
                .unwrap(),
        }
    }

    /// Startet den Device Code Flow - gibt Code zurück den der User eingeben muss
    pub async fn begin_device_code_flow(&self) -> Result<DeviceCodeFlow> {
        let params = [
            ("client_id", AZURE_CLIENT_ID),
            ("scope", SCOPE),
        ];

        let response = self.client
            .post(DEVICE_CODE_URL)
            .form(&params)
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await?;
        tracing::info!("Device code response (status {}): {}", status, text);

        // Prüfe auf Fehler
        if text.contains("error") {
            #[derive(Deserialize)]
            struct ErrorResponse {
                error: String,
                error_description: Option<String>,
            }

            if let Ok(err) = serde_json::from_str::<ErrorResponse>(&text) {
                return Err(anyhow::anyhow!(
                    "Microsoft Auth Fehler: {} - {}",
                    err.error,
                    err.error_description.unwrap_or_default()
                ));
            }
        }

        let device_code: DeviceCodeResponse = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("Fehler beim Parsen der Response: {} - Raw: {}", e, text))?;

        Ok(DeviceCodeFlow {
            user_code: device_code.user_code,
            device_code: device_code.device_code,
            verification_uri: device_code.verification_uri,
            expires_in: device_code.expires_in,
            interval: device_code.interval,
            message: device_code.message,
        })
    }

    /// Pollt für Token nachdem User den Code eingegeben hat
    pub async fn poll_for_token(&self, device_code: &str) -> Result<Option<MinecraftAccount>> {
        let params = [
            ("client_id", AZURE_CLIENT_ID),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ];

        let response = self.client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await?;

        let text = response.text().await?;

        // Prüfe auf "authorization_pending" Fehler
        if text.contains("authorization_pending") {
            return Ok(None); // Noch nicht autorisiert, weiter pollen
        }

        if text.contains("expired_token") {
            return Err(anyhow::anyhow!("Device code abgelaufen"));
        }

        if text.contains("access_denied") {
            return Err(anyhow::anyhow!("Zugriff verweigert"));
        }

        let token: TokenResponse = serde_json::from_str(&text)?;

        if token.error.is_some() {
            return Ok(None); // Noch nicht fertig
        }

        // Token erfolgreich - jetzt Xbox Live Auth
        let account = self.complete_auth(&token.access_token, token.refresh_token).await?;

        Ok(Some(account))
    }

    /// Komplettiert die Auth nach Erhalt des Microsoft Tokens
    async fn complete_auth(&self, ms_access_token: &str, refresh_token: Option<String>) -> Result<MinecraftAccount> {
        tracing::info!("Got Microsoft token, getting Xbox Live token...");

        // 1. Xbox Live Token
        let (xbl_token, user_hash) = self.get_xbox_live_token(ms_access_token).await?;
        tracing::info!("Got Xbox Live token");

        // 2. XSTS Token
        let xsts_token = self.get_xsts_token(&xbl_token).await?;
        tracing::info!("Got XSTS token");

        // 3. Minecraft Token
        let mc_token = self.get_minecraft_token(&xsts_token, &user_hash).await?;
        tracing::info!("Got Minecraft token");

        // 4. Minecraft Profil
        let profile = self.get_minecraft_profile(&mc_token.access_token).await?;
        tracing::info!("Got Minecraft profile: {}", profile.name);

        let skin_url = profile.skins
            .as_ref()
            .and_then(|s| s.iter().find(|skin| skin.state == "ACTIVE"))
            .map(|s| s.url.clone());

        let cape_url = profile.capes
            .as_ref()
            .and_then(|c| c.iter().find(|cape| cape.state == "ACTIVE"))
            .map(|c| c.url.clone());

        Ok(MinecraftAccount {
            uuid: profile.id,
            username: profile.name,
            access_token: mc_token.access_token,
            refresh_token,
            expires_at: Some(Utc::now() + Duration::seconds(mc_token.expires_in as i64)),
            skin_url,
            cape_url,
            is_microsoft: true,
        })
    }

    async fn get_xbox_live_token(&self, access_token: &str) -> Result<(String, String)> {
        let body = serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={}", access_token)
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        });

        let response: XboxLiveResponse = self.client
            .post("https://user.auth.xboxlive.com/user/authenticate")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        let user_hash = response.display_claims
            .get("xui")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|obj| obj.get("uhs"))
            .and_then(|uhs| uhs.as_str())
            .ok_or_else(|| anyhow::anyhow!("No user hash in Xbox Live response"))?
            .to_string();

        Ok((response.token, user_hash))
    }

    async fn get_xsts_token(&self, xbl_token: &str) -> Result<String> {
        let body = serde_json::json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl_token]
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        });

        let response: XboxLiveResponse = self.client
            .post("https://xsts.auth.xboxlive.com/xsts/authorize")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        Ok(response.token)
    }

    async fn get_minecraft_token(&self, xsts_token: &str, user_hash: &str) -> Result<MinecraftAuthResponse> {
        let body = serde_json::json!({
            "identityToken": format!("XBL3.0 x={};{}", user_hash, xsts_token)
        });

        let response = self.client
            .post("https://api.minecraftservices.com/authentication/login_with_xbox")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        Ok(response)
    }

    async fn get_minecraft_profile(&self, access_token: &str) -> Result<MinecraftProfileResponse> {
        let response = self.client
            .get("https://api.minecraftservices.com/minecraft/profile")
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await?
            .json()
            .await?;

        Ok(response)
    }

    /// Refresh Token verwenden um neuen Access Token zu bekommen
    pub async fn refresh_auth(&self, refresh_token: &str) -> Result<MinecraftAccount> {
        let params = [
            ("client_id", AZURE_CLIENT_ID),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
            ("scope", SCOPE),
        ];

        let token_response: TokenResponse = self.client
            .post(TOKEN_URL)
            .form(&params)
            .send()
            .await?
            .json()
            .await?;

        self.complete_auth(&token_response.access_token, token_response.refresh_token).await
    }

    /// Offline Account erstellen
    pub fn create_offline_account(username: &str) -> MinecraftAccount {
        // Generiere eine konsistente UUID basierend auf dem Username
        let uuid = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, username.as_bytes());

        MinecraftAccount {
            uuid: uuid.to_string().replace("-", ""),
            username: username.to_string(),
            access_token: "0".to_string(),
            refresh_token: None,
            expires_at: None,
            skin_url: None,
            cape_url: None,
            is_microsoft: false,
        }
    }
}

/// Skin-URL für Kopf-Avatar generieren (via mc-heads.net - zuverlässiger als Crafatar)
pub fn get_head_url(uuid: &str, size: u32) -> String {
    // mc-heads.net ist zuverlässiger als crafatar
    format!("https://mc-heads.net/avatar/{}/{}", uuid, size)
}

/// Skin-URL für 3D-Render generieren (via mc-heads.net)
pub fn get_skin_render_url(uuid: &str) -> String {
    format!("https://mc-heads.net/body/{}/100", uuid)
}

/// Vollständige Skin-URL
pub fn get_full_skin_url(uuid: &str) -> String {
    format!("https://mc-heads.net/skin/{}", uuid)
}
