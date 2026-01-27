# Lion Launcher 🦁

Ein minimalistischer Minecraft Launcher für Linux und Windows, geschrieben in Rust mit Tauri.

## Features

✨ **Profilverwaltung**
- Erstelle mehrere Profile mit verschiedenen Minecraft-Versionen
- Unterstützt Vanilla, Fabric, Forge, NeoForge und Quilt
- Jedes Profil kann eigene Mods haben

🔍 **Mod Browser**
- Durchsuche Modrinth (CurseForge Support geplant)
- Filtere nach Minecraft-Version und Mod-Loader
- Sortiere nach Relevanz, Downloads, Updates
- Installiere Mods mit einem Klick

🔐 **Microsoft Account**
- Anmeldung mit Microsoft/Xbox Account
- Device Code Flow für einfache Authentifizierung
- Zeigt deinen Minecraft-Skin an
- Offline-Account Option verfügbar

🎨 **Skins**
- 3D-Skin-Vorschau
- Suche nach Spieler-Skins
- Zuletzt angesehene Skins speichern

⚙️ **Einstellungen**
- Dark/Light Mode
- Anpassbare Akzentfarben (Gold, Blau, Grün, Lila, Rot, Orange, Pink, Cyan)
- Konfiguriere Speicher und Java-Pfad

🎮 **Design**
- Minimalistisches UI in Grau mit anpassbaren Akzentfarben
- Schnelle und reaktionsschnelle Oberfläche
- Kleiner Bundle-Size (~10-15MB)

## System-Anforderungen

- **Linux**: GTK3, WebKit2GTK
- **Windows**: WebView2
- **Java**: Java 17+ (für moderne Minecraft-Versionen)
- **Rust**: 1.70+ (nur für Build)

## Installation

### Aus Source bauen

```bash
# Repository klonen
git clone https://github.com/yourusername/Lion-Launcher.git
cd Lion-Launcher

# Build
cargo build --release

# Starten
./target/release/Lion-Launcher
```

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

## Technologie-Stack

- **Frontend**: HTML, CSS, JavaScript
- **Backend**: Rust
- **Framework**: Tauri 2.0
- **APIs**: Modrinth, Mojang, Microsoft/Xbox Live

## Lizenz

MIT License

---