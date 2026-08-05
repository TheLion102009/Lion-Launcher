//! Zentraler, geteilter HTTP-Client für alle API-Aufrufe (Modrinth, Mojang, Auth-Server etc.).
//!
//! Vorher hat fast jede Funktion ihren eigenen `reqwest::Client::new()` erzeugt.
//! Das Problem: jeder neue Client baut einen eigenen Connection-Pool auf, TLS-Handshakes
//! und Keep-Alive-Verbindungen werden also NICHT wiederverwendet -> jeder Request zahlt
//! erneut den vollen Verbindungsaufbau (spürbar bei API-lastigen Views wie Mod-Browser
//! oder Update-Checks mit vielen Requests hintereinander).
//!
//! Diese Instanz wird einmal lazy beim ersten Zugriff gebaut und danach für den
//! gesamten Programmlauf wiederverwendet. `reqwest::Client` ist intern ein `Arc`,
//! `.clone()` ist also günstig (nur Referenzzähler hoch).
//!
//! Für große Datei-Downloads (Mod-JARs, Server-JARs, Resourcepacks) weiterhin den
//! `DownloadManager` aus `core::download` nutzen -> der hat bewusst ein längeres
//! Timeout (300s) und ein eigenes Profil für Streaming-Downloads.

use once_cell::sync::Lazy;
use std::time::Duration;

/// Geteilter Client für kurze API-Aufrufe (Modrinth-Suche, Mojang-Manifest,
/// Auth-Refresh, Version-Checks etc.).
pub static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        // Genug Idle-Connections pro Host, damit aufeinanderfolgende Requests
        // (z.B. 50x Modrinth-Version-Check) die bestehende Verbindung wiederverwenden.
        .pool_max_idle_per_host(8)
        .pool_idle_timeout(Duration::from_secs(30))
        // API-Calls sollen nicht ewig hängen - 30s reicht für JSON-Antworten locker.
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!(
        "Lion-Launcher/",
        env!("CARGO_PKG_VERSION"),
        " (+https://lion-craft.net)"
        ))
        .build()
        .expect("HTTP-Client konnte nicht initialisiert werden")
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_is_reusable_and_cheap_to_clone() {
        // Zwei Zugriffe müssen denselben zugrunde liegenden Client referenzieren.
        let a = HTTP_CLIENT.clone();
        let b = HTTP_CLIENT.clone();
        // reqwest::Client hat keinen PartialEq, aber der Zugriff selbst darf nicht panicen.
        drop(a);
        drop(b);
    }
}