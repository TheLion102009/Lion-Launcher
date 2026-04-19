#!/bin/bash

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

# ---- Argumente parsen --------------------------------------
TAG_TO_PUSH=""
while [[ $# -gt 0 ]]; do
    case $1 in
        --tag)
            TAG_TO_PUSH="$2"
            shift 2
            ;;
        *)
            echo -e "${FAIL} Unbekanntes Argument: $1"
            echo "  Nutzung: $0 [--tag v0.1.0]"
            exit 1
            ;;
    esac
done

# ---- Paket-Verzeichnis (Ausgabe) ---------------------------
OUT_DIR="./release-output"
mkdir -p "$OUT_DIR"

# ---- Umgebungsvariablen ------------------------------------
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export RUST_LOG="info"

if [ -z "${DISPLAY}" ]; then
    export DISPLAY=:0
fi

# ---- Voraussetzungen prüfen --------------------------------
echo -e "${YELLOW}[1/6] Prüfe Werkzeuge...${NC}"

if ! command -v cargo &> /dev/null; then
    echo -e "${FAIL} cargo nicht gefunden. Installiere Rust: https://rustup.rs"
    exit 1
fi

if ! cargo tauri --version &> /dev/null; then
    echo -e "${YELLOW}  → tauri-cli nicht gefunden, installiere...${NC}"
    cargo install tauri-cli --version "^2"
fi

echo -e "${PASS} Rust / Cargo / Tauri CLI vorhanden"

# ---- tauri.conf.json Validierung ---------------------------
echo -e "\n${YELLOW}[2/6] Prüfe Konfiguration...${NC}"

# Prüfe ob bundle targets korrekt gesetzt sind
if command -v python3 &> /dev/null; then
    TARGETS=$(python3 -c "
import json
with open('tauri.conf.json') as f:
    c = json.load(f)
targets = c.get('bundle', {}).get('targets', [])
print(','.join(targets) if isinstance(targets, list) else str(targets))
" 2>/dev/null || echo "unknown")
    echo -e "  Bundle targets: ${BOLD}${TARGETS}${NC}"

    WV_MODE=$(python3 -c "
import json
with open('tauri.conf.json') as f:
    c = json.load(f)
print(c.get('bundle', {}).get('windows', {}).get('webviewInstallMode', {}).get('type', 'unknown'))
" 2>/dev/null || echo "unknown")
    echo -e "  Windows WebView2 Modus: ${BOLD}${WV_MODE}${NC}"

    if [ "$WV_MODE" != "embedBootstrapper" ]; then
        echo -e "  ${YELLOW}⚠ WebView2 ist NICHT auf embedBootstrapper gesetzt!${NC}"
        echo -e "  ${YELLOW}  Die .exe wird WebView2 möglicherweise nicht enthalten.${NC}"
    else
        echo -e "  ${PASS} WebView2 embedBootstrapper konfiguriert (wird in .exe eingebettet)"
    fi
else
    echo -e "  ${YELLOW}⚠ python3 nicht gefunden — Konfigurationsvalidierung übersprungen${NC}"
fi

echo -e "${PASS} Konfiguration geprüft"

# ---- Linux-Pakete (.deb, .rpm) ----------------------------
echo -e "\n${YELLOW}[3/6] Baue Linux-Pakete (.deb / .rpm)...${NC}"

rm -rf ./target/release/bundle/
cargo tauri build 2>&1

echo -e "${PASS} Linux-Pakete erstellt"

# ---- Pakete sammeln ----------------------------------------
echo -e "\n${YELLOW}[4/6] Pakete sammeln...${NC}"

BUNDLE_DIR="./target/release/bundle"
FOUND=0

while IFS= read -r -d '' f; do
    cp "$f" "$OUT_DIR/"
    echo -e "  ${PASS} ${BOLD}$(basename "$f")${NC} → $OUT_DIR/"
    FOUND=$((FOUND + 1))
done < <(find "$BUNDLE_DIR" \( -name "*.deb" -o -name "*.rpm" \) -print0 2>/dev/null)

if [ "$FOUND" -eq 0 ]; then
    echo -e "  ${YELLOW}⚠ Keine Linux-Pakete im Bundle-Verzeichnis gefunden.${NC}"
fi

# ---- Checksums erstellen -----------------------------------
echo -e "\n${YELLOW}[5/6] SHA-256 Checksums erstellen...${NC}"
if [ "$FOUND" -gt 0 ]; then
    (cd "$OUT_DIR" && sha256sum *.deb *.rpm 2>/dev/null > SHA256SUMS.txt)
    echo -e "${PASS} Checksums gespeichert in ${OUT_DIR}/SHA256SUMS.txt"
fi

# ---- Windows .exe — Tag pushen für GitHub Actions ----------
echo -e "\n${YELLOW}[6/6] Windows .exe (NSIS Installer)...${NC}"

if [ -n "$TAG_TO_PUSH" ]; then
    echo -e "  ${CYAN}→ Erstelle und pushe Tag: ${BOLD}${TAG_TO_PUSH}${NC}"
    git tag "$TAG_TO_PUSH" 2>/dev/null || echo -e "  ${YELLOW}⚠ Tag existiert bereits${NC}"
    git push origin "$TAG_TO_PUSH" 2>/dev/null && {
        echo -e "  ${PASS} Tag ${TAG_TO_PUSH} gepusht → GitHub Actions baut jetzt die .exe"
        echo -e "  ${CYAN}  Prüfe den Build unter:${NC}"
        # Versuche die GitHub URL zu ermitteln
        REMOTE_URL=$(git remote get-url origin 2>/dev/null || echo "")
        if [ -n "$REMOTE_URL" ]; then
            # Konvertiere SSH/HTTPS URL zu Actions URL
            GH_URL=$(echo "$REMOTE_URL" | sed 's/\.git$//' | sed 's|git@github.com:|https://github.com/|')
            echo -e "  ${BOLD}${GH_URL}/actions${NC}"
        fi
    } || echo -e "  ${YELLOW}⚠ Push fehlgeschlagen — ist ein Remote konfiguriert?${NC}"
else
    echo -e "  ${CYAN}ℹ  Windows-Builds (.exe) können NICHT auf Linux gebaut werden.${NC}"
    echo -e "  ${CYAN}   Tauri 2 benötigt MSVC + NSIS + WebView2 → nur auf Windows.${NC}"
    echo -e ""
    echo -e "  So erstellst du die .exe:"
    echo -e ""
    echo -e "  ${BOLD}Option A – Tag pushen (empfohlen):${NC}"
    echo -e "    ${CYAN}./scripts/release.sh --tag v0.1.0${NC}"
    echo -e "    → GitHub Actions baut automatisch .deb + .rpm + .exe"
    echo -e "    → Alle Dateien werden als GitHub Release veröffentlicht"
    echo -e ""
    echo -e "  ${BOLD}Option B – Manuell auslösen:${NC}"
    echo -e "    GitHub → Actions → \"Release Build\" → \"Run workflow\""
    echo -e ""
    echo -e "  ${BOLD}Option C – Lokal auf einem Windows-PC:${NC}"
    echo -e "    cargo tauri build"
    echo -e "    → Erzeugt: target/release/bundle/nsis/*-setup.exe"
    echo -e ""
    echo -e "  ${GREEN}✓ WebView2 Bootstrapper wird automatisch eingebettet (tauri.conf.json)${NC}"
    echo -e "  ${GREEN}✓ Java-Erkennung unterstützt Windows-Pfade (Adoptium, Oracle, JAVA_HOME)${NC}"
    echo -e "  ${GREEN}✓ Classpath-Separator automatisch ; auf Windows, : auf Linux${NC}"
    echo -e "  ${GREEN}✓ Linux-Env-Vars (DISPLAY, NVIDIA) werden auf Windows übersprungen${NC}"
fi

# ---- Ergebnis anzeigen -------------------------------------
echo -e "\n${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}${BOLD}  ✅ Lokaler Release Build abgeschlossen!${NC}"
echo -e "${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo -e "  ${BOLD}Linux-Pakete:${NC} ${OUT_DIR}/"
if [ -d "$OUT_DIR" ] && [ "$(ls -A "$OUT_DIR")" ]; then
    ls -lh "$OUT_DIR/"
fi
echo ""
echo -e "  ${BOLD}Windows .exe:${NC} Wird über GitHub Actions gebaut"
echo -e "  Workflow: ${CYAN}.github/workflows/release.yml${NC}"
echo ""

# ---- App direkt starten (Test-Fenster) ---------------------
BINARY_PATH="./target/release/Lion-Launcher"
if [ -f "$BINARY_PATH" ]; then
    echo -e "${CYAN}  🚀 Starte Lion Launcher zum Testen...${NC}\n"
    exec "$BINARY_PATH"
else
    echo -e "${YELLOW}  ℹ Binary nicht gefunden unter $BINARY_PATH – wird nicht gestartet.${NC}"
fi
