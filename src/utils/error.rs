#![allow(dead_code)]

use thiserror::Error;

#[derive(Error, Debug)]
pub enum LauncherError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Profile not found: {0}")]
    ProfileNotFound(String),

    #[error("Mod not found: {0}")]
    ModNotFound(String),

    #[error("Version not found: {0}")]
    VersionNotFound(String),

    #[error("Download failed: {0}")]
    DownloadFailed(String),

    #[error("Launch failed: {0}")]
    LaunchFailed(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, LauncherError>;
