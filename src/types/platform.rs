#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Windows,
    Linux,
    MacOS,
}

impl Platform {
    pub fn current() -> Self {
        #[cfg(target_os = "windows")]
        return Platform::Windows;

        #[cfg(target_os = "linux")]
        return Platform::Linux;

        #[cfg(target_os = "macos")]
        return Platform::MacOS;
    }

    pub fn os_name(&self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Linux => "linux",
            Platform::MacOS => "osx",
        }
    }

    pub fn arch() -> &'static str {
        if cfg!(target_arch = "x86_64") {
            "x64"
        } else if cfg!(target_arch = "x86") {
            "x86"
        } else if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "unknown"
        }
    }
}
