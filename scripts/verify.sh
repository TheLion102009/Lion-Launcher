#!/bin/bash
# ============================================================
#  Lion Launcher – RELEASE-CHECK
#  Führt alle Prüfungen (clippy, tests, release-compile)
#  wie beim echten Release durch – aber OHNE Installer-Pakete.
#  Am Ende wird die fertige Binary direkt gestartet (Fenster).
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
echo "  🦁  Lion Launcher — Release-Check"
echo "============================================${NC}"
echo -e "  Alle Prüfungen, kein Paket-Output\n"

# ---- Voraussetzungen prüfen --------------------------------
echo -e "${YELLOW}[1/7] Prüfe Werkzeuge...${NC}"

for tool in cargo rustfmt; do
    if ! command -v "$tool" &> /dev/null; then
        echo -e "${FAIL} $tool nicht gefunden."
        exit 1
    fi
done

if ! cargo tauri --version &> /dev/null; then
    echo -e "${YELLOW}  → tauri-cli nicht gefunden, installiere...${NC}"
    cargo install tauri-cli --version "^2"
fi

echo -e "${PASS} Alle Werkzeuge vorhanden"

# ---- Umgebungsvariablen ------------------------------------
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export RUST_LOG="info"
export RUST_BACKTRACE=1

if [ -z "${DISPLAY}" ]; then
    export DISPLAY=:0
fi

# ---- Format-Prüfung ----------------------------------------
echo -e "\n${YELLOW}[2/7] Formatierung prüfen (rustfmt)...${NC}"
if cargo fmt --all -- --check; then
    echo -e "${PASS} Code-Formatierung korrekt"
else
    echo -e "${FAIL} Formatierungsfehler gefunden. Führe 'cargo fmt --all' aus."
    exit 1
fi

# ---- Clippy (Lint) -----------------------------------------
echo -e "\n${YELLOW}[3/7] Statische Analyse (clippy)...${NC}"
if cargo clippy --all-targets --all-features -- -D warnings; then
    echo -e "${PASS} Keine Clippy-Warnungen"
else
    echo -e "${FAIL} Clippy-Fehler gefunden."
    exit 1
fi

# ---- Tests -------------------------------------------------
echo -e "\n${YELLOW}[4/7] Tests ausführen...${NC}"
if cargo test --all 2>&1; then
    echo -e "${PASS} Alle Tests bestanden"
else
    echo -e "${FAIL} Tests fehlgeschlagen."
    exit 1
fi

# ---- Release-Compile (ohne Bundle) -------------------------
echo -e "\n${YELLOW}[5/7] Release-Kompilierung (kein Bundle)...${NC}"
if cargo build --release 2>&1; then
    echo -e "${PASS} Release-Binary erfolgreich erstellt"
else
    echo -e "${FAIL} Kompilierung fehlgeschlagen."
    exit 1
fi

# ---- Binary-Größe anzeigen ---------------------------------
echo -e "\n${YELLOW}[6/7] Build-Info...${NC}"
BINARY="./target/release/Lion-Launcher"
if [ -f "$BINARY" ]; then
    SIZE=$(du -sh "$BINARY" | cut -f1)
    echo -e "  ${PASS} Binary: ${BOLD}$BINARY${NC} (${SIZE})"
fi

# ---- Ergebnis ----------------------------------------------
echo -e "\n${YELLOW}[7/7] Release-Check abgeschlossen${NC}"
echo -e "\n${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}${BOLD}  ✅ Alle Prüfungen bestanden!${NC}"
echo -e "${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "\n${CYAN}  Starte Lion Launcher (Release-Binary)...${NC}\n"

# ---- App direkt starten ------------------------------------
exec "$BINARY"

