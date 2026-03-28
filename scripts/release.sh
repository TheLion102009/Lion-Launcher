#!/bin/bash
# ============================================================
#  Lion Launcher – VOLLSTÄNDIGER RELEASE BUILD
#
#  Erstellt alle Pakete:
#    Linux  →  .deb  |  .rpm  |  .AppImage (separat gebaut)
#    Windows→  .exe (NSIS-Installer, benötigt Cross-Toolchain)
#
#  Hinweis: AppImage wird separat gebaut um FUSE-Probleme
#  auf WSL/Fedora zu umgehen (APPIMAGE_EXTRACT_AND_RUN=1).
# ============================================================

set -e

# ---- Farben ------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

PASS="${GREEN}✓${NC}"
FAIL="${RED}✗${NC}"

echo -e "${BOLD}${CYAN}"
echo "  🦁  Lion Launcher — Full Release Build"
echo "================================================${NC}"
echo ""

# ---- Paket-Verzeichnis (Ausgabe) ---------------------------
OUT_DIR="./release-output"
mkdir -p "$OUT_DIR"

# ---- Umgebungsvariablen ------------------------------------
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export RUST_LOG="info"
# WSL/Fedora: AppImages ohne FUSE-Mount ausführen (kein Kernel-FUSE nötig)
export APPIMAGE_EXTRACT_AND_RUN=1

if [ -z "${DISPLAY}" ]; then
    export DISPLAY=:0
fi

# ---- Voraussetzungen prüfen --------------------------------
echo -e "${YELLOW}[1/7] Prüfe Werkzeuge...${NC}"

if ! command -v cargo &> /dev/null; then
    echo -e "${FAIL} cargo nicht gefunden. Installiere Rust: https://rustup.rs"
    exit 1
fi

if ! cargo tauri --version &> /dev/null; then
    echo -e "${YELLOW}  → tauri-cli nicht gefunden, installiere...${NC}"
    cargo install tauri-cli --version "^2"
fi

echo -e "${PASS} Rust / Cargo / Tauri CLI vorhanden"

# ---- Linux-Pakete (.deb, .rpm) ----------------------------
echo -e "\n${YELLOW}[2/7] Baue Linux-Pakete (.deb / .rpm)...${NC}"
echo -e "  (AppImage wird in Schritt 3 separat gebaut)"

rm -rf ./target/release/bundle/
cargo tauri build 2>&1

echo -e "${PASS} Linux-Pakete (.deb / .rpm) erstellt"

# ---- AppImage separat bauen --------------------------------
echo -e "\n${YELLOW}[3/7] Baue AppImage (mit APPIMAGE_EXTRACT_AND_RUN)...${NC}"

LINUXDEPLOY_CACHE="$HOME/.cache/tauri/linuxdeploy-x86_64.AppImage"
# Eigenes Verzeichnis außerhalb des Tauri-Cache (Tauri überschreibt ~/.cache/tauri/)
LINUXDEPLOY_OWN="$HOME/.local/share/lion-launcher/linuxdeploy-x86_64.AppImage"
APPDIR="./target/release/bundle/appimage/Lion Launcher.AppDir"
BINARY="./target/release/Lion-Launcher"

mkdir -p "$HOME/.local/share/lion-launcher"

# Herunterladen falls noch nicht vorhanden
if [ ! -f "$LINUXDEPLOY_OWN" ]; then
    if [ -f "${LINUXDEPLOY_CACHE}.bak" ]; then
        echo -e "  → Nutze vorhandenes linuxdeploy-Backup..."
        cp "${LINUXDEPLOY_CACHE}.bak" "$LINUXDEPLOY_OWN"
        chmod +x "$LINUXDEPLOY_OWN"
    else
        echo -e "  → Lade linuxdeploy herunter..."
        curl -fsSL -o "$LINUXDEPLOY_OWN" \
            "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
        chmod +x "$LINUXDEPLOY_OWN"
    fi
fi

# AppDir-Struktur erstellen
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/128x128/apps"
cp "$BINARY" "$APPDIR/usr/bin/Lion-Launcher"
cp "icons/128x128.png" "$APPDIR/usr/share/icons/hicolor/128x128/apps/lion-launcher.png"

# Desktop-Datei erstellen
cat > "$APPDIR/usr/share/applications/lion-launcher.desktop" << 'DESKTOP'
[Desktop Entry]
Name=Lion Launcher
Exec=Lion-Launcher
Icon=lion-launcher
Type=Application
Categories=Game;
DESKTOP

# AppImage bauen
# APPIMAGE_EXTRACT_AND_RUN=1 → kein FUSE-Mount nötig (WSL/Fedora-kompatibel)
# NO_STRIP=1                 → überspringt strip für neuere .relr.dyn-Sektionen
# Nur --output appimage      → ruft intern das appimage-Plugin auf (kein --plugin appimage!)
APPIMAGE_OUT="./target/release/bundle/appimage"
mkdir -p "$APPIMAGE_OUT"

cd "$APPIMAGE_OUT"
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 NO_STRIP=1 \
    "$LINUXDEPLOY_OWN" \
    --appdir "Lion Launcher.AppDir" \
    --output appimage 2>&1 | grep -v "^Setting rpath\|^Calling strip\|Unable to recognise" || {
    echo -e "  ${YELLOW}⚠ AppImage-Build fehlgeschlagen – wird übersprungen${NC}"
}
cd - > /dev/null

if ls "$APPIMAGE_OUT"/*.AppImage 2>/dev/null | grep -v ".bak" | grep -q ".AppImage"; then
    echo -e "${PASS} AppImage erstellt"
else
    echo -e "  ${YELLOW}⚠ AppImage nicht erstellt (WSL/FUSE-Einschränkung) – wird übersprungen${NC}"
fi

# ---- Windows .exe (Cross-Compilation) ---------------------
echo -e "\n${YELLOW}[4/7] Windows .exe (Cross-Compilation)...${NC}"

WIN_TARGET="x86_64-pc-windows-gnu"
WIN_EXE="./target/${WIN_TARGET}/release/Lion-Launcher.exe"
WIN_ZIP="${OUT_DIR}/Lion-Launcher_0.1.0_x64-setup.zip"

if rustup target list --installed | grep -q "$WIN_TARGET" && command -v x86_64-w64-mingw32-gcc &> /dev/null; then
    echo -e "  → Kompiliere für Windows (${WIN_TARGET})..."
    cargo build --release --target "$WIN_TARGET" 2>&1 | grep -E "error|warning: unused|Compiling Lion|Finished" | grep -v "^$" | tail -5

    if [ -f "$WIN_EXE" ]; then
        # .exe in ZIP verpacken (NSIS cross-compile wird nicht von Tauri unterstützt)
        echo -e "  → Erstelle ZIP-Paket..."
        mkdir -p /tmp/lion-launcher-win
        cp "$WIN_EXE" /tmp/lion-launcher-win/
        # README für Windows hinzufügen
        cat > /tmp/lion-launcher-win/README.txt << 'WINREADME'
Lion Launcher - Windows Setup
==============================
Führe Lion-Launcher.exe direkt aus um den Launcher zu starten.
Es wird keine Installation benötigt (Portable Version).

Hinweis: Beim ersten Start kann Windows SmartScreen eine Warnung
zeigen - auf "Weitere Informationen" → "Trotzdem ausführen" klicken.
WINREADME
        (cd /tmp/lion-launcher-win && zip -r "$OLDPWD/$WIN_ZIP" .)
        rm -rf /tmp/lion-launcher-win
        echo -e "${PASS} Windows .exe erstellt: $(basename "$WIN_ZIP")"
    else
        echo -e "${FAIL} Windows-Build fehlgeschlagen"
    fi
else
    echo -e "${YELLOW}  ⚠ Windows-Cross-Toolchain nicht vorhanden – .exe wird übersprungen.${NC}"
    echo -e "  Zum Aktivieren:"
    echo -e "    rustup target add $WIN_TARGET"
    echo -e "    sudo dnf install mingw64-gcc mingw64-gcc-c++   # Fedora"
    echo -e "    sudo apt install gcc-mingw-w64-x86-64          # Ubuntu/Debian"
fi

# ---- Pakete kopieren / auflisten ---------------------------
echo -e "\n${YELLOW}[5/7] Pakete sammeln...${NC}"

BUNDLE_DIR="./target/release/bundle"
FOUND=0

while IFS= read -r -d '' f; do
    cp "$f" "$OUT_DIR/"
    echo -e "  ${PASS} ${BOLD}$(basename "$f")${NC} → $OUT_DIR/"
    FOUND=$((FOUND + 1))
done < <(find "$BUNDLE_DIR" \( -name "*.AppImage" -o -name "*.deb" -o -name "*.rpm" -o -name "*.exe" \) -not -name "*.bak" -print0 2>/dev/null)

# Windows ZIP (wurde direkt in OUT_DIR erstellt)
for f in "$OUT_DIR"/*.zip; do
    [ -f "$f" ] && FOUND=$((FOUND + 1))
done

if [ "$FOUND" -eq 0 ]; then
    echo -e "  ${YELLOW}⚠ Keine Pakete im Bundle-Verzeichnis gefunden.${NC}"
fi

# ---- Checksums erstellen -----------------------------------
echo -e "\n${YELLOW}[6/7] SHA-256 Checksums erstellen...${NC}"
if [ "$FOUND" -gt 0 ]; then
    (cd "$OUT_DIR" && sha256sum * > SHA256SUMS.txt)
    echo -e "${PASS} Checksums gespeichert in ${OUT_DIR}/SHA256SUMS.txt"
fi

# ---- Ergebnis anzeigen -------------------------------------
echo -e "\n${YELLOW}[7/7] Release-Übersicht${NC}"
echo -e "${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}${BOLD}  ✅ Release Build abgeschlossen!${NC}"
echo -e "${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo -e "  Ausgabe-Verzeichnis: ${BOLD}${OUT_DIR}/${NC}"
if [ -d "$OUT_DIR" ] && [ "$(ls -A "$OUT_DIR")" ]; then
    ls -lh "$OUT_DIR/"
fi

# ---- App direkt starten (Test-Fenster) ---------------------
BINARY_PATH="./target/release/Lion-Launcher"
if [ -f "$BINARY_PATH" ]; then
    echo -e "\n${CYAN}  🚀 Starte Lion Launcher zum Testen...${NC}\n"
    exec "$BINARY_PATH"
else
    echo -e "\n${YELLOW}  ℹ Binary nicht gefunden unter $BINARY_PATH – wird nicht gestartet.${NC}"
fi

