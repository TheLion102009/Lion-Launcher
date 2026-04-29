#!/bin/bash
# ============================================================
#  Lion Launcher – DEV-MODUS  (RustRover / Schnelltest)
#  Startet die App live im Entwicklungsmodus.
#  Funktioniert auf Arch Linux, Debian/Ubuntu, Fedora und
#  im Flatpak-Terminal (RustRover) via host-spawn.
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

# ---- Distro-Erkennung --------------------------------------
if [ -f /etc/os-release ]; then
    . /etc/os-release
    DISTRO_ID="${ID:-unknown}"
    DISTRO_ID_LIKE="${ID_LIKE:-}"
else
    DISTRO_ID="unknown"
    DISTRO_ID_LIKE=""
fi

is_arch() {
    case "$DISTRO_ID" in
        arch|manjaro|endeavouros|garuda|artix|cachyos) return 0 ;;
    esac
    echo "$DISTRO_ID_LIKE" | grep -q arch && return 0
    return 1
}

is_debian() {
    case "$DISTRO_ID" in
        ubuntu|debian|linuxmint|pop|zorin|elementary) return 0 ;;
    esac
    echo "$DISTRO_ID_LIKE" | grep -q debian && return 0
    return 1
}

is_fedora() {
    case "$DISTRO_ID" in
        fedora|rhel|centos|rocky|alma|nobara) return 0 ;;
    esac
    echo "$DISTRO_ID_LIKE" | grep -q "fedora\|rhel" && return 0
    return 1
}

show_install_hint() {
    echo -e "${YELLOW}  Installiere die fehlenden Abhängigkeiten:${NC}"
    if is_arch; then
        echo -e "  ${BOLD}sudo pacman -S --needed webkit2gtk-4.1 base-devel gtk3 libappindicator-gtk3 librsvg pkgconf${NC}"
        # Prüfe ob yay oder paru vorhanden ist für AUR-Pakete
        if command -v yay &>/dev/null; then
            echo -e "  ${CYAN}(yay erkannt – AUR-Pakete werden falls nötig über yay installiert)${NC}"
        elif command -v paru &>/dev/null; then
            echo -e "  ${CYAN}(paru erkannt – AUR-Pakete werden falls nötig über paru installiert)${NC}"
        fi
    elif is_debian; then
        echo -e "  ${BOLD}sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev libsoup-3.0-dev${NC}"
    elif is_fedora; then
        echo -e "  ${BOLD}sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel openssl-devel libsoup3-devel${NC}"
    else
        echo -e "  ${BOLD}bash $(realpath "$(dirname "$0")")/setup-host.sh${NC}"
    fi
}

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
        exit 1
    fi
fi

set -e

# ---- Voraussetzungen prüfen --------------------------------
echo -e "\n${YELLOW}[1/3] Prüfe Werkzeuge...${NC}"

# Arch Linux: pkgconf statt pkg-config
if is_arch && ! command -v pkg-config &>/dev/null; then
    if command -v pkgconf &>/dev/null; then
        # pkgconf ist das Arch-native Tool, alias setzen wenn nötig
        export PKG_CONFIG="pkgconf"
        echo -e "${CYAN}  → pkgconf (Arch-native) wird als pkg-config verwendet${NC}"
    fi
fi

# Cargo / Rust
if ! command -v cargo &>/dev/null; then
    [ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env" || {
        echo -e "${RED}✗ cargo nicht gefunden. Installiere Rust: https://rustup.rs${NC}"
        if is_arch; then
            echo -e "  ${CYAN}Auf Arch auch möglich: sudo pacman -S rust${NC}"
        fi
        exit 1
    }
fi

# Tauri CLI
if ! cargo tauri --version &>/dev/null; then
    echo -e "${YELLOW}  → tauri-cli nicht gefunden, installiere...${NC}"
    cargo install tauri-cli --version "^2"
fi

# webkit2gtk-4.1
if ! ${PKG_CONFIG:-pkg-config} --exists webkit2gtk-4.1 2>/dev/null; then
    echo -e "${RED}✗ webkit2gtk-4.1 fehlt!${NC}"
    show_install_hint
    echo ""
    if is_arch; then
        echo -e "  ${YELLOW}Alternativ setup-host.sh ausführen:${NC}"
        echo -e "  ${BOLD}bash $(realpath "$(dirname "$0")")/setup-host.sh${NC}"
    fi
    exit 1
fi

echo -e "${GREEN}✓ Alle Werkzeuge vorhanden${NC}"
if is_arch; then
    echo -e "${CYAN}  → Arch Linux erkannt (${DISTRO_ID})${NC}"
elif is_debian; then
    echo -e "${CYAN}  → Debian/Ubuntu erkannt (${DISTRO_ID})${NC}"
elif is_fedora; then
    echo -e "${CYAN}  → Fedora/RHEL erkannt (${DISTRO_ID})${NC}"
fi

# ---- Umgebungsvariablen (Linux) ----------------------------
echo -e "\n${YELLOW}[2/3] Setze Umgebungsvariablen...${NC}"

export WEBKIT_DISABLE_DMABUF_RENDERER=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export RUST_LOG="debug"
export RUST_BACKTRACE=1

# Wayland-Fallback auf X11 (Arch + GNOME/KDE/Sway unter Wayland häufig nötig)
if [ -n "${WAYLAND_DISPLAY}" ] && [ -z "${DISPLAY}" ]; then
    export GDK_BACKEND=wayland
    echo -e "  → Wayland erkannt (GDK_BACKEND=wayland)"
elif [ -z "${DISPLAY}" ]; then
    export DISPLAY=:0
    export GDK_BACKEND=x11
    echo -e "  → DISPLAY auf :0 gesetzt, GDK_BACKEND=x11"
fi

# Arch: NVIDIA + Wayland Workaround
if is_arch && lspci 2>/dev/null | grep -qi nvidia; then
    export __GL_GSYNC_ALLOWED=0
    export __GL_VRR_ALLOWED=0
    echo -e "  → NVIDIA GPU erkannt, Sync-Flags deaktiviert"
fi

echo -e "${GREEN}✓ Umgebung bereit${NC}"

# ---- Dev-Server starten ------------------------------------
echo -e "\n${YELLOW}[3/3] Starte Lion Launcher im Dev-Modus...${NC}"
echo -e "${CYAN}  Fenster öffnet sich automatisch.${NC}"
echo -e "${CYAN}  Strg+C zum Beenden.${NC}\n"

cd "$(dirname "$(realpath "$0")")/.."
cargo tauri dev

