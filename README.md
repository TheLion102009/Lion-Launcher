# Lion Launcher 🦁

Ein minimalistischer Minecraft Launcher für Linux und Windows, geschrieben in Rust mit Tauri.

## Features

**Profilverwaltung**
- Erstelle mehrere Profile mit verschiedenen Minecraft-Versionen
- Vollständige Unterstützung für alle Mod-Loader:
  -  **Vanilla**
  -  **Fabric** 
  -  **Quilt** 
  -  **Forge**
  -  **Neoforge**

- Jedes Profil kann eigene Mods, Einstellungen und Resource Packs haben
- Automatisches Laden und Verwalten der Loader-Versionen

**Mod Browser**
- Durchsuche Modrinth
- Filtere nach Minecraft-Version und Mod-Loader
- Sortiere nach Relevanz, Downloads, Updates
- Installiere Mods mit einem Klick
- Eine große Detail übersicht 

**Microsoft Account**
- Anmeldung mit Microsoft/Xbox Account
- Device Code Flow für einfache Authentifizierung
- Zeigt deinen Minecraft-Skin an
- Offline-Account Option verfügbar

**Skins**
- 3D-Skin-Vorschau
- Suche nach Spieler-Skins
- Zuletzt angesehene Skins speichern

**Einstellungen**
- Dark/Light Mode
- Anpassbare Akzentfarben 
- Konfiguriere Speicher und Java-Pfad

 **Design**
- Minimalistisches UI in Grau mit anpassbaren Akzentfarben
- Schnelle und reaktionsschnelle Oberfläche
- Kleiner Bundle-Size (~10-15MB)

## System-Anforderungen

- **Linux**: GTK3, WebKit2GTK
- **Windows**: WebView2
- **Java**: Java 17+ (für moderne Minecraft-Versionen)
- **Rust**: 1.70+ (nur für Build)

## Installation

### Releases

Lade die neueste Version von diesem Repository runter bei nighly builds.



### Aus Source bauen

```bash
# Repository klonen
git clone https://github.com/yourusername/Lion-Launcher.git
cd Lion-Launcher

# Tauri CLI installieren
cargo install tauri-cli --version "^2.0.0"

# Linux: Dependencies installieren
sudo apt install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
    libayatana-appindicator3-dev librsvg2-dev patchelf

# Release Build erstellen
cargo tauri build --bundles appimage,deb

# Oder verwende das Build-Skript
./build-release.sh
```

**Ausgabe:** `target/release/bundle/appimage/*.AppImage`

**Detaillierte Build-Anleitung:** Siehe [BUILD.md](BUILD.md) oder [QUICKBUILD.md](QUICKBUILD.md)

### Linux Dependencies (Ubuntu/Debian)

```bash
sudo apt update
sudo apt install -y libwebkit2gtk-4.1-dev \
    build-essential \
    curl \
    wget \
    file \
    libssl-dev \
    libgtk-3-dev \
    librsvg2-dev
```

## Verwendung

1. **Starte den Launcher**
2. **Melde dich an** (Microsoft oder Offline)
3. **Erstelle ein Profil** mit deiner gewünschten Minecraft-Version
4. **Füge Mods hinzu** über den Mod Browser
5. **Starte Minecraft!**

## Windows-Test unter Linux (Wine)

Wenn du Loader-Probleme unter Windows reproduzieren willst, kannst du den Windows-Launcher direkt unter Wine starten:

```bash
bash scripts/wine-loader-test.sh --prepare-prefix --winetricks
```

Nutzt standardmaessig das neueste `release-output/*_x64-setup.zip`, extrahiert `Lion-Launcher.exe` und schreibt Logs nach `release-output/wine-logs/`.

Weitere Optionen:

```bash
bash scripts/wine-loader-test.sh --help
```

## Technologie-Stack

- **Frontend**: HTML, CSS, JavaScript
- **Backend**: Rust
- **Framework**: Tauri 2.0
- **APIs**: Modrinth, Mojang, Microsoft/Xbox Live

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org)

Signiert werden nur die Windows-Installer der offiziellen Releases (Git-Tags `v*`). Sie werden
ausschließlich auf GitHub-gehosteten Runnern aus dem Quellcode dieses Repositorys gebaut; jede
Signatur wird vor dem Signieren manuell freigegeben.

- Committers and reviewers: [TheLion102009](https://github.com/TheLion102009)
- Approvers: [TheLion102009](https://github.com/TheLion102009)

Pull Requests von Personen außerhalb des Teams werden vor dem Zusammenführen von einem Reviewer geprüft.

## Datenschutz / Privacy

Der Launcher hat keinen eigenen Server und sammelt keine Telemetrie. Automatische Verbindungen
(Update-Prüfung bei GitHub, P2P-Freunde-System über die iroh-Relays) sind unter
**Einstellungen → Datenschutz** abschaltbar. Details: [PRIVACY.md](PRIVACY.md)

## Lizenz

[MIT](LICENSE) – Copyright (c) 2026 Thorben Hess
