#!/bin/bash
# ============================================================
#  Lion Launcher – DEV-MODUS  (RustRover / Schnelltest)
#  Startet die App live im Entwicklungsmodus.
#  Funktioniert sowohl im Flatpak-Terminal (RustRover)
#  als auch in einem normalen Host-Terminal.
# ============================================================

# ---- Farben ------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${BOLD}${CYAN}"
echo "  🦁  Lion Launcher — Dev Mode"
echo "======================================${NC}"

# ---- Flatpak-Erkennung & Host-Sprung -----------------------
if [ -f "/.flatpak-info" ]; then
    echo -e "\n${YELLOW}  ℹ RustRover läuft als Flatpak – versuche Host-Zugriff...${NC}"

    if flatpak-spawn --host echo "ok" &>/dev/null 2>&1; then
        echo -e "${GREEN}  ✓ Host-Zugriff möglich – starte auf dem Host...${NC}\n"
        exec flatpak-spawn --host bash "$(realpath "$0")" "$@"
    else
        echo -e "${RED}${BOLD}"
        echo "  ⛔ Host-Zugriff verweigert!"
        echo ""
        echo "  Einmalige Einrichtung notwendig:"
        echo "  ─────────────────────────────────────────────────────"
        echo "  1. Öffne ein echtes Terminal (NICHT RustRover)"
        echo "  2. Führe aus:"
        echo ""
        echo -e "     ${CYAN}bash $(realpath "$(dirname "$0")")/setup-host.sh${NC}${RED}${BOLD}"
        echo ""
        echo "  3. Starte RustRover neu"
        echo "  4. Führe dieses Skript erneut aus"
        echo "  ─────────────────────────────────────────────────────"
        echo -e "${NC}"
        echo -e "${YELLOW}  Alternativ: Skript direkt im Host-Terminal ausführen:${NC}"
        echo -e "  ${BOLD}bash $(realpath "$0")${NC}"
        echo ""
        exit 1
    fi
fi

set -e

# ---- Voraussetzungen prüfen --------------------------------
echo -e "\n${YELLOW}[1/3] Prüfe Werkzeuge...${NC}"

if ! command -v cargo &>/dev/null; then
    [ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env" || {
        echo -e "${RED}✗ cargo nicht gefunden. Installiere Rust: https://rustup.rs${NC}"
        exit 1
    }
fi

if ! cargo tauri --version &>/dev/null; then
    echo -e "${YELLOW}  → tauri-cli nicht gefunden, installiere...${NC}"
    cargo install tauri-cli --version "^2"
fi

if ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    echo -e "${RED}✗ webkit2gtk-4.1 fehlt!${NC}"
    echo -e "  Führe einmalig aus: ${BOLD}bash $(realpath "$(dirname "$0")")/setup-host.sh${NC}"
    exit 1
fi

echo -e "${GREEN}✓ Alle Werkzeuge vorhanden${NC}"

# ---- Umgebungsvariablen (Linux) ----------------------------
echo -e "\n${YELLOW}[2/3] Setze Umgebungsvariablen...${NC}"

export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export RUST_LOG="debug"
export RUST_BACKTRACE=1

[ -z "${DISPLAY}" ] && export DISPLAY=:0 && echo -e "  → DISPLAY auf :0 gesetzt"

echo -e "${GREEN}✓ Umgebung bereit${NC}"

# ---- Dev-Server starten ------------------------------------
echo -e "\n${YELLOW}[3/3] Starte Lion Launcher im Dev-Modus...${NC}"
echo -e "${CYAN}  Fenster öffnet sich automatisch.${NC}"
echo -e "${CYAN}  Strg+C zum Beenden.${NC}\n"

cd "$(dirname "$(realpath "$0")")/.."
cargo tauri dev

