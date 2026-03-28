#!/bin/bash
# ============================================================
#  EINMALIGE EINRICHTUNG – muss EINMAL in einem echten
#  Host-Terminal (NICHT in RustRover) ausgeführt werden!
#
#  Was dieses Skript tut:
#    1. Setzt die Flatpak-Permission für RustRover, damit
#       der integrierte Terminal Befehle auf dem Host starten kann.
#    2. Prüft ob alle Tauri-Abhängigkeiten vorhanden sind.
#    3. Installiert fehlende Pakete automatisch.
# ============================================================

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${BOLD}${CYAN}"
echo "  🦁  Lion Launcher — Einmalige Host-Einrichtung"
echo "=====================================================${NC}"

# ---- Sind wir wirklich NICHT im Flatpak? -------------------
if [ -f "/.flatpak-info" ]; then
    echo -e "${RED}${BOLD}"
    echo "  ⛔ Dieses Skript wurde IM Flatpak-Terminal gestartet!"
    echo ""
    echo "  Öffne ein echtes Terminal (z.B. Konsole, GNOME Terminal,"
    echo "  xterm) und führe das Skript dort aus:${NC}"
    echo ""
    echo -e "  ${BOLD}bash $(realpath "$0")${NC}"
    echo ""
    exit 1
fi

# ---- Schritt 1: Flatpak-Permission -------------------------
echo -e "\n${YELLOW}[1/3] Setze Flatpak-Permission für RustRover...${NC}"

FLATPAK_APP="com.jetbrains.RustRover"

if flatpak list --app 2>/dev/null | grep -q "$FLATPAK_APP"; then
    flatpak override --user --talk-name=org.freedesktop.Flatpak "$FLATPAK_APP"
    echo -e "${GREEN}✓ Permission gesetzt für $FLATPAK_APP${NC}"
else
    echo -e "${YELLOW}  ⚠ $FLATPAK_APP nicht als Flatpak gefunden.${NC}"
    echo -e "  Falls du RustRover anders installiert hast, ist dieser Schritt nicht nötig."
fi

# ---- Schritt 2: System-Abhängigkeiten prüfen & installieren ----
echo -e "\n${YELLOW}[2/3] Prüfe System-Abhängigkeiten (Tauri/WebKit)...${NC}"

# Distro erkennen
if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO_ID="$ID"
else
    DISTRO_ID="unknown"
fi

install_deps() {
    case "$DISTRO_ID" in
        fedora|rhel|centos|rocky|alma)
            echo -e "  → Erkannt: ${BOLD}Fedora/RHEL${NC}"
            sudo dnf install -y \
                webkit2gtk4.1-devel \
                openssl-devel \
                curl \
                wget \
                file \
                libappindicator-gtk3-devel \
                librsvg2-devel \
                gtk3-devel \
                pango-devel \
                gdk-pixbuf2-devel \
                libsoup3-devel \
                glib2-devel
            ;;
        ubuntu|debian|linuxmint|pop)
            echo -e "  → Erkannt: ${BOLD}Ubuntu/Debian${NC}"
            sudo apt update
            sudo apt install -y \
                libwebkit2gtk-4.1-dev \
                libssl-dev \
                curl \
                wget \
                file \
                libgtk-3-dev \
                libayatana-appindicator3-dev \
                librsvg2-dev \
                patchelf \
                libsoup-3.0-dev
            ;;
        arch|manjaro|endeavouros)
            echo -e "  → Erkannt: ${BOLD}Arch Linux${NC}"
            sudo pacman -S --needed \
                webkit2gtk-4.1 \
                base-devel \
                curl \
                wget \
                file \
                openssl \
                appmenu-gtk-module \
                gtk3 \
                libappindicator-gtk3 \
                librsvg \
                libvips
            ;;
        opensuse*|sles)
            echo -e "  → Erkannt: ${BOLD}openSUSE${NC}"
            sudo zypper install -y \
                webkit2gtk3-soup2-devel \
                libopenssl-devel \
                curl \
                wget \
                file \
                gtk3-devel \
                libappindicator3-1 \
                librsvg-devel
            ;;
        *)
            echo -e "${YELLOW}  ⚠ Unbekannte Distribution: $DISTRO_ID${NC}"
            echo -e "  Installiere manuell: webkit2gtk-4.1-dev, libssl-dev, libgtk-3-dev"
            ;;
    esac
}

if ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    echo -e "  → webkit2gtk-4.1 fehlt, installiere Abhängigkeiten..."
    install_deps
else
    echo -e "${GREEN}  ✓ webkit2gtk-4.1 bereits vorhanden${NC}"
fi

# ---- Schritt 3: Rust/Tauri-CLI prüfen ----------------------
echo -e "\n${YELLOW}[3/3] Prüfe Rust & Tauri CLI...${NC}"

if ! command -v cargo &>/dev/null; then
    echo -e "  → Rust nicht gefunden, installiere..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

if ! cargo tauri --version &>/dev/null; then
    echo -e "  → tauri-cli nicht gefunden, installiere..."
    cargo install tauri-cli --version "^2"
fi

echo -e "${GREEN}✓ Rust & Tauri CLI vorhanden${NC}"

# ---- Fertig ------------------------------------------------
echo -e "\n${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}${BOLD}  ✅ Einrichtung abgeschlossen!${NC}"
echo -e "${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo -e "  ${BOLD}Nächste Schritte:${NC}"
echo -e "  1. RustRover neu starten (damit die Permission aktiv wird)"
echo -e "  2. Im RustRover-Terminal das Skript ausführen:"
echo -e "     ${CYAN}./scripts/dev.sh${NC}"
echo ""

