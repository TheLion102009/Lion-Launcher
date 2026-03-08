# Lion Launcher v0.1.0 - Installation

## 📦 Verfügbare Pakete

### Debian/Ubuntu (.deb)
```bash
sudo dpkg -i "Lion Launcher_0.1.0_amd64.deb"
sudo apt-get install -f  # Falls Abhängigkeiten fehlen
```

### Fedora/RHEL/openSUSE (.rpm)
```bash
sudo rpm -i "Lion Launcher-0.1.0-1.x86_64.rpm"
```

Oder mit dnf (Fedora):
```bash
sudo dnf install "Lion Launcher-0.1.0-1.x86_64.rpm"
```

### Standalone Binary (Alle Linux-Distributionen)
```bash
chmod +x Lion-Launcher-linux-x64
./Lion-Launcher-linux-x64
```

## 🔐 Checksums (SHA256)

```
7e882f5633ac8b842d62c8f982bc0563f9639b71458ffc0456f1bd752257b831  Lion Launcher_0.1.0_amd64.deb
75c55e5248530e38f67bf7af2e411ed9a6572f2dba16509dbf1f200e19590d3f  Lion Launcher-0.1.0-1.x86_64.rpm
```

Verifizierung:
```bash
sha256sum -c checksums-packages.txt
```

## ✨ Features

- ✅ **Minecraft Support**: 1.20.2 - 1.21.11
- ✅ **Mod-Loader**: Fabric, Quilt, NeoForge, Forge (experimentell)
- ✅ **Microsoft Account Login**
- ✅ **Mod Browser** (Modrinth)
- ✅ **Profile Management**
- ✅ **Dark/Light Theme** mit 8 Accent Colors
- ✅ **3D Skin Preview**

## 🐛 Bekannte Probleme

### Forge 1.20.x - 1.21.5
⚠️ **BEHOBEN**: Der Fehler "Hauptklasse ALL-MODULE-PATH konnte nicht gefunden oder geladen werden" wurde in diesem Build behoben!

**Was wurde geändert:**
- Korrekte JVM-Argumente für das Java Module System
- `--add-modules ALL-MODULE-PATH` wird jetzt korrekt nach `-p` (Module-Path) gesetzt
- Module-Path wird für ForgeBootstrap korrekt aufgebaut

### NeoForge
✅ **Funktioniert stabil** für Minecraft 1.20.2+

### Fabric & Quilt
✅ **Vollständig unterstützt** für alle Versionen

## 📋 Systemanforderungen

- **OS**: Linux (64-bit)
- **RAM**: min. 4 GB, empfohlen 8 GB
- **Disk**: 500 MB + Minecraft Daten
- **Java**: Java 21 (wird automatisch erkannt)
- **Abhängigkeiten**: 
  - GTK 3
  - WebKit2GTK 4.1

## 🚀 Erster Start

1. Installiere das Paket (siehe oben)
2. Starte Lion Launcher über:
   - Anwendungsmenü → Spiele → Lion Launcher
   - Terminal: `lion-launcher`
3. Melde dich mit deinem Microsoft Account an
4. Erstelle ein Profil
5. Wähle Minecraft Version und Mod-Loader
6. Klicke auf "Play"!

## 🔧 Deinstallation

### Debian/Ubuntu
```bash
sudo apt remove lion-launcher
```

### Fedora/RHEL
```bash
sudo rpm -e lion-launcher
```

## 📝 Changelog

### v0.1.0 (2026-02-09)

**🐛 Bugfixes:**
- Behoben: Forge startet nicht mit Fehler "Hauptklasse ALL-MODULE-PATH konnte nicht gefunden oder geladen werden"
- Behoben: JVM-Module-Argumente werden jetzt in der korrekten Reihenfolge übergeben
- Verbessert: ForgeBootstrap (1.21.6+) Module-System Integration

**✨ Neue Features:**
- `.deb` und `.rpm` Paket-Support
- Automatische Abhängigkeitsauflösung für Linux-Distributionen
- Desktop-Integration mit Icon und Menüeintrag

**📦 Build-Info:**
- Compiler: rustc 1.85+
- Build-Datum: 2026-02-09
- Pakete: .deb (Debian/Ubuntu), .rpm (Fedora/RHEL), Standalone Binary

## 🐧 Distribution Support

| Distribution | Paket | Status |
|--------------|-------|--------|
| Ubuntu 20.04+ | .deb | ✅ Getestet |
| Debian 11+ | .deb | ✅ Getestet |
| Pop!_OS 22.04+ | .deb | ✅ Getestet |
| Linux Mint 20+ | .deb | ✅ Empfohlen |
| Fedora 38+ | .rpm | ✅ Funktioniert |
| RHEL 9+ | .rpm | ⚠️ Nicht getestet |
| openSUSE Tumbleweed | .rpm | ⚠️ Nicht getestet |
| Arch Linux | Binary | ✅ Funktioniert |
| Andere | Binary | ⚠️ Eventuell |

## 📞 Support

Bei Problemen erstelle bitte ein Issue auf GitHub mit:
- Linux-Distribution und Version
- Minecraft-Version und Mod-Loader
- Log-Dateien aus `~/.config/lion-launcher/logs/`

## 📄 Lizenz

© 2026 Lion Launcher Team

