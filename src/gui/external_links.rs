//! Externe Links (Modrinth, GitHub, Discord, ...) dürfen NIE im Launcher-Fenster geöffnet
//! werden, sondern immer im Standard-Browser des Systems.
//!
//! Drei Ebenen sorgen dafür:
//! 1. Das Frontend fängt Klicks auf externe Links ab und ruft `open_external_url` auf.
//! 2. [`navigation_guard`] blockiert jede Navigation des Launcher-Webviews weg von der
//!    eigenen Oberfläche und öffnet das Ziel stattdessen im Browser.
//! 3. Neue Fenster (`target="_blank"`, Mittelklick, `window.open`) werden in `main.rs`
//!    über `on_new_window` abgelehnt und ebenfalls im Browser geöffnet.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Runtime, Url};

/// Hosts, unter denen der Launcher selbst seine Oberfläche ausliefert.
const INTERNAL_HOSTS: &[&str] = &[
    "tauri.localhost",
    "ipc.localhost",
    "asset.localhost",
    "localhost",
    "127.0.0.1",
];

/// `true`, wenn die URL zur Launcher-Oberfläche gehört (darf im Webview geladen werden).
fn is_internal_url(url: &Url) -> bool {
    match url.scheme() {
        "tauri" | "asset" | "ipc" | "about" | "data" | "blob" => true,
        "http" | "https" => url
            .host_str()
            .is_some_and(|host| INTERNAL_HOSTS.contains(&host)),
        _ => false,
    }
}

/// `true` für URLs, die im System-Browser bzw. Mail-Programm geöffnet werden dürfen.
fn is_openable_external(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "mailto") && !is_internal_url(url)
}

/// Öffnet eine externe URL im Standard-Browser. Andere Schemata (file:, javascript:, ...)
/// werden abgelehnt.
pub fn open_in_browser(url: &Url) -> Result<(), String> {
    if !is_openable_external(url) {
        return Err(format!("URL wird nicht extern geöffnet: {}", url));
    }
    // Nutzt ShellExecute / xdg-open / open direkt – ohne Umweg über `cmd /C start`,
    // damit Zeichen wie `&` in der URL nicht als Shell-Befehle interpretiert werden.
    #[allow(deprecated)]
    tauri_plugin_shell::open::open(None, url.as_str(), None)
        .map_err(|e| format!("Konnte Browser nicht öffnen: {}", e))
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let parsed = Url::parse(url.trim()).map_err(|e| format!("Ungültige URL: {}", e))?;
    open_in_browser(&parsed)
}

/// Wird für Popup-/Neues-Fenster-Anfragen des Webviews aufgerufen.
pub fn handle_new_window_request(url: &Url) {
    if is_openable_external(url) {
        if let Err(e) = open_in_browser(url) {
            tracing::warn!("{}", e);
        }
    } else {
        tracing::warn!("Blocked new window request for {}", url);
    }
}

/// Plugin, das jede Navigation des Webviews weg von der Launcher-Oberfläche verhindert.
pub fn navigation_guard<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("navigation-guard")
        .on_navigation(|_webview, url| {
            if is_internal_url(url) {
                return true;
            }
            if is_openable_external(url) {
                tracing::info!("Opening external link in browser: {}", url);
                if let Err(e) = open_in_browser(url) {
                    tracing::warn!("{}", e);
                }
            } else {
                tracing::warn!("Blocked navigation to {}", url);
            }
            false
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn launcher_ui_is_internal() {
        assert!(is_internal_url(&url("http://tauri.localhost/index.html")));
        assert!(is_internal_url(&url("https://tauri.localhost/")));
        assert!(is_internal_url(&url("tauri://localhost/index.html")));
        assert!(is_internal_url(&url("about:blank")));
    }

    #[test]
    fn web_links_are_external() {
        assert!(is_openable_external(&url(
            "https://modrinth.com/mod/sodium"
        )));
        assert!(is_openable_external(&url("http://example.com/?a=1&b=2")));
        assert!(is_openable_external(&url("mailto:someone@example.com")));
        assert!(!is_internal_url(&url("https://modrinth.com/mod/sodium")));
    }

    #[test]
    fn dangerous_schemes_are_rejected() {
        assert!(!is_openable_external(&url(
            "file:///C:/Windows/System32/calc.exe"
        )));
        assert!(!is_openable_external(&url("javascript:alert(1)")));
        assert!(!is_internal_url(&url(
            "file:///C:/Windows/System32/calc.exe"
        )));
    }
}
