# 🦁 Lion Launcher - Distribution

## Version 0.1.0 - Build 2026-02-08

Ein moderner, minimalistischer Minecraft Launcher für Linux und Windows.

---

## 📦 Downloads

### Windows (64-bit)
- **Datei**: `Lion-Launcher-windows-x64.exe`
- **Größe**: 7.1 MB
- **SHA256**: `ad5a6ecc2d1168db299324e402c702f296c1d5c04f280f56b201ce2a272a2b71`

### Linux (64-bit)
- **Datei**: `Lion-Launcher-linux-x64`
- **Größe**: 7.7 MB
- **SHA256**: `072c82d29dca1e93dac64ad031e0af6ea06cf28a7dd69cac777e5e021527098b`

---

## 🚀 Installation

### Windows
1. Lade `Lion-Launcher-windows-x64.exe` herunter
2. Führe die EXE-Datei aus
3. Fertig! Der Launcher startet automatisch

### Linux
1. Lade `Lion-Launcher-linux-x64` herunter
2. Mache die Datei ausführbar:
   ```bash
   chmod +x Lion-Launcher-linux-x64
   ```
3. Starte den Launcher:
   ```bash
   ./Lion-Launcher-linux-x64
   ```

---

## ✨ Features

### Mod-Loader Support
✅ **Vanilla** - Pures Minecraft ohne Mods
✅ **Fabric** - Leichtgewichtig und schnell (Empfohlen)
✅ **Quilt** - Fabric-Fork mit erweiterten Features
✅ **Forge** - Vollständige Unterstützung (experimentell)
✅ **NeoForge** - Vollständige Unterstützung für MC 1.20.2+

### Profilverwaltung
- Erstelle mehrere Profile mit verschiedenen MC-Versionen
- Jedes Profil kann eigene Mods, Einstellungen und Resource Packs haben
- Automatisches Laden und Verwalten der Loader-Versionen
- Unterstützt MC Versionen 1.20.2 - 1.21.11

### Mod Browser
- Durchsuche Modrinth direkt im Launcher
- Filtere nach MC-Version und Mod-Loader
- Sortiere nach Relevanz, Downloads, Updates
- Installiere Mods mit einem Klick
- Unterstützt auch Resource Packs & Shader Packs

### Account System
- Microsoft/Xbox Account Login
- Device Code Flow für einfache Authentifizierung
- Zeigt deinen Minecraft-Skin an
- Offline-Account Option verfügbar

### Design
- Dark/Light Mode
- Anpassbare Akzentfarben (Gold, Blau, Grün, Lila, Rot, Orange, Pink, Cyan)
- Minimalistisches UI
- Schnelle und reaktionsschnelle Oberfläche

---

## 🔒 Checksums Verifizieren

Um die Integrität der Downloads zu überprüfen:

### Linux/macOS:
```bash
sha256sum Lion-Launcher-linux-x64
# Erwarteter Hash: 072c82d29dca1e93dac64ad031e0af6ea06cf28a7dd69cac777e5e021527098b

sha256sum Lion-Launcher-windows-x64.exe
# Erwarteter Hash: ad5a6ecc2d1168db299324e402c702f296c1d5c04f280f56b201ce2a272a2b71
```

### Windows (PowerShell):
```powershell
Get-FileHash Lion-Launcher-windows-x64.exe -Algorithm SHA256
# Erwarteter Hash: ad5a6ecc2d1168db299324e402c702f296c1d5c04f280f56b201ce2a272a2b71
```

Die Checksums müssen mit denen in `SHA256SUMS.txt` übereinstimmen.

---

## 💻 Systemanforderungen

### Minimum:
- **OS**: Windows 10/11 (64-bit) oder Linux (64-bit)
- **RAM**: 4 GB (für Minecraft selbst werden mindestens 2 GB empfohlen)
- **Festplatte**: 500 MB für den Launcher + Speicher für Minecraft und Mods
- **Java**: Java 21 (wird automatisch erkannt, falls installiert)

### Empfohlen:
- **RAM**: 8 GB oder mehr
- **Festplatte**: 5 GB oder mehr (für Mods und Welten)
- **Java**: Java 21 (OpenJDK oder Oracle JDK)

### Linux Zusätzlich:
- GTK 3
- WebKit2GTK 4.1
- X11 oder Wayland

---

## 🎮 Erste Schritte

1. **Starte den Launcher**
2. **Melde dich an** (Microsoft oder Offline)
3. **Erstelle ein Profil** mit deiner gewünschten Minecraft-Version
4. **Wähle einen Mod-Loader** (Fabric, Quilt, NeoForge oder Vanilla)
5. **Füge Mods hinzu** über den Mod Browser (optional)
6. **Starte Minecraft!**

---

## 🛠️ Technologie

- **Frontend**: HTML, CSS, JavaScript
- **Backend**: Rust
- **Framework**: Tauri 2.0
- **APIs**: Modrinth, Mojang, Microsoft/Xbox Live

---

## 📝 Lizenz

MIT License

---

## 🐛 Support & Feedback

Bei Problemen oder Fragen:
- GitHub Issues: [github.com/your-repo/issues](https://github.com/your-repo/issues)
- Discord: [Your Discord Server]

---

**Build**: 2026-02-08 01:27 UTC  
**Version**: 0.1.0  
**Compiler**: Rust 1.85+ mit Cross-Compilation (MinGW-w64)

