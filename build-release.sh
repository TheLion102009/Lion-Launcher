#!/bin/bash
# Lion Launcher - Release Build Script
# Erstellt: AppImage, .deb, .rpm (Linux) und .exe (Windows via Cross-Compilation)

set -e

# Füge ~/.local/bin zum PATH hinzu (für linuxdeploy)
export PATH="$HOME/.local/bin:$PATH"

echo "🦁 Lion Launcher - Release Build"
echo "================================="
echo ""

# Farben
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Version aus Cargo.toml lesen
VERSION=$(grep -m1 'version' Cargo.toml | cut -d'"' -f2)
echo -e "Version: ${GREEN}$VERSION${NC}"
echo ""

# Funktion für Fortschritt
print_step() {
    echo -e "${BLUE}▶${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

# Prüfe Abhängigkeiten
check_dependencies() {
    print_step "Prüfe Abhängigkeiten..."

    local missing=()

    # Tauri CLI
    if ! command -v cargo-tauri &> /dev/null; then
        missing+=("cargo-tauri")
    fi

    # Für .deb Builds
    if ! command -v dpkg &> /dev/null; then
        print_warning "dpkg nicht gefunden - .deb Build möglicherweise nicht verfügbar"
    fi

    # Für .rpm Builds
    if ! command -v rpmbuild &> /dev/null; then
        print_warning "rpmbuild nicht gefunden - .rpm Build möglicherweise nicht verfügbar"
    fi

    # Für Windows Cross-Compilation
    if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
        print_warning "mingw-w64 nicht gefunden - Windows Cross-Compilation nicht verfügbar"
        print_warning "Installiere mit: sudo apt install gcc-mingw-w64-x86-64"
    fi

    if [ ${#missing[@]} -gt 0 ]; then
        print_warning "Fehlende Abhängigkeiten: ${missing[*]}"

        for dep in "${missing[@]}"; do
            case $dep in
                cargo-tauri)
                    print_step "Installiere Tauri CLI..."
                    cargo install tauri-cli --version "^2.0.0"
                    ;;
            esac
        done
    fi

    print_success "Abhängigkeiten geprüft"
}

# Build für Linux
build_linux() {
    local bundles=$1

    print_step "Starte Linux Build ($bundles)..."
    echo ""

    # Check if AppImage is requested
    if echo "$bundles" | grep -q "appimage"; then
        print_step "Prüfe linuxdeploy für AppImage..."

        if ! command -v linuxdeploy &> /dev/null; then
            print_warning "linuxdeploy nicht gefunden. Installiere..."

            mkdir -p "$HOME/.local/bin"
            cd "$HOME/.local/bin"

            # Download linuxdeploy
            if [ ! -f linuxdeploy-x86_64.AppImage ]; then
                wget -q --show-progress https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage || {
                    print_error "Download von linuxdeploy fehlgeschlagen"
                    cd - > /dev/null
                    return 1
                }
                chmod +x linuxdeploy-x86_64.AppImage
                ln -sf linuxdeploy-x86_64.AppImage linuxdeploy
            fi

            # Download AppImage plugin
            if [ ! -f linuxdeploy-plugin-appimage-x86_64.AppImage ]; then
                wget -q --show-progress https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage || {
                    print_error "Download von linuxdeploy-plugin-appimage fehlgeschlagen"
                    cd - > /dev/null
                    return 1
                }
                chmod +x linuxdeploy-plugin-appimage-x86_64.AppImage
            fi

            # Add to PATH
            export PATH="$HOME/.local/bin:$PATH"

            cd - > /dev/null

            if command -v linuxdeploy &> /dev/null; then
                print_success "linuxdeploy installiert"
            else
                print_error "linuxdeploy konnte nicht installiert werden"
                print_warning "Baue stattdessen ohne AppImage..."
                bundles="${bundles//appimage/}"
                if [ -z "$bundles" ]; then
                    bundles="deb"
                fi
            fi
        else
            print_success "linuxdeploy gefunden"
        fi
    fi

    cargo tauri build --bundles "$bundles"
    local exit_code=$?

    if [ $exit_code -eq 0 ]; then
        print_success "Linux Build abgeschlossen"
        echo ""
        print_step "Ausgabe-Dateien:"

        if [ -d "target/release/bundle" ]; then
            find target/release/bundle -type f \( -name "*.AppImage" -o -name "*.deb" -o -name "*.rpm" \) -exec ls -lh {} \;
        fi
    else
        print_error "Build fehlgeschlagen (Exit Code: $exit_code)"

        # Fallback: Build nur die Binary
        print_warning "Versuche einfachen Binary-Build als Fallback..."
        cargo build --release

        if [ $? -eq 0 ]; then
            print_success "Binary erstellt: target/release/Lion-Launcher"
            print_warning "Keine Bundles erstellt. Nutze die Binary direkt."
        fi

        return $exit_code
    fi
}

# Build für Windows (Cross-Compilation)
build_windows() {
    print_step "Starte Windows Cross-Compilation..."

    # Prüfe ob Windows Target installiert ist
    if ! rustup target list --installed | grep -q "x86_64-pc-windows-gnu"; then
        print_step "Installiere Windows Target..."
        rustup target add x86_64-pc-windows-gnu
    fi

    # Prüfe ob mingw installiert ist
    if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
        print_error "mingw-w64 nicht installiert!"
        print_warning "Installiere mit: sudo apt install gcc-mingw-w64-x86-64"
        return 1
    fi

    # Erstelle .cargo/config.toml für Cross-Compilation falls nicht vorhanden
    mkdir -p .cargo
    cat > .cargo/config.toml << 'EOF'
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
ar = "x86_64-w64-mingw32-ar"

[target.x86_64-pc-windows-msvc]
linker = "lld-link"
EOF

    # Build mit NSIS Installer
    cargo tauri build --target x86_64-pc-windows-gnu --bundles nsis

    print_success "Windows Build abgeschlossen"
}

# Hauptmenü
show_menu() {
    echo "Wähle Build-Typ:"
    echo ""
    echo "  Linux:"
    echo "    1) AppImage"
    echo "    2) .deb (Debian/Ubuntu)"
    echo "    3) .rpm (Fedora/RHEL)"
    echo "    4) Alle Linux-Formate (AppImage + .deb + .rpm)"
    echo ""
    echo "  Windows:"
    echo "    5) .exe (NSIS Installer) - Cross-Compilation"
    echo ""
    echo "  Alle Plattformen:"
    echo "    6) Alle Linux-Formate + Windows .exe"
    echo ""
    echo "    0) Abbrechen"
    echo ""
    read -p "Auswahl (0-6): " choice

    case $choice in
        1) build_linux "appimage" ;;
        2) build_linux "deb" ;;
        3) build_linux "rpm" ;;
        4) build_linux "appimage,deb,rpm" ;;
        5) build_windows ;;
        6)
            build_linux "appimage,deb,rpm"
            build_windows
            ;;
        0)
            echo "Abgebrochen."
            exit 0
            ;;
        *)
            print_error "Ungültige Auswahl!"
            exit 1
            ;;
    esac
}

# Zeige Build-Ergebnisse
show_results() {
    echo ""
    echo "================================="
    print_success "Build abgeschlossen!"
    echo ""
    echo "📦 Ausgabe-Dateien:"
    echo ""

    BUNDLE_DIR="target/release/bundle"

    # Linux Bundles
    if [ -d "$BUNDLE_DIR" ]; then
        # AppImage
        if ls "$BUNDLE_DIR/appimage/"*.AppImage 1> /dev/null 2>&1; then
            echo -e "  ${GREEN}AppImage:${NC}"
            for f in "$BUNDLE_DIR/appimage/"*.AppImage; do
                size=$(ls -lh "$f" | awk '{print $5}')
                echo "    📦 $(basename "$f") ($size)"
            done
            echo ""
        fi

        # .deb
        if ls "$BUNDLE_DIR/deb/"*.deb 1> /dev/null 2>&1; then
            echo -e "  ${GREEN}Debian Package (.deb):${NC}"
            for f in "$BUNDLE_DIR/deb/"*.deb; do
                size=$(ls -lh "$f" | awk '{print $5}')
                echo "    📦 $(basename "$f") ($size)"
            done
            echo ""
        fi

        # .rpm
        if ls "$BUNDLE_DIR/rpm/"*.rpm 1> /dev/null 2>&1; then
            echo -e "  ${GREEN}RPM Package (.rpm):${NC}"
            for f in "$BUNDLE_DIR/rpm/"*.rpm; do
                size=$(ls -lh "$f" | awk '{print $5}')
                echo "    📦 $(basename "$f") ($size)"
            done
            echo ""
        fi
    fi

    # Windows Bundles
    WIN_BUNDLE_DIR="target/x86_64-pc-windows-gnu/release/bundle"
    if [ -d "$WIN_BUNDLE_DIR" ]; then
        # NSIS Installer (.exe)
        if ls "$WIN_BUNDLE_DIR/nsis/"*.exe 1> /dev/null 2>&1; then
            echo -e "  ${GREEN}Windows Installer (.exe):${NC}"
            for f in "$WIN_BUNDLE_DIR/nsis/"*.exe; do
                size=$(ls -lh "$f" | awk '{print $5}')
                echo "    📦 $(basename "$f") ($size)"
            done
            echo ""
        fi

        # MSI
        if ls "$WIN_BUNDLE_DIR/msi/"*.msi 1> /dev/null 2>&1; then
            echo -e "  ${GREEN}Windows MSI:${NC}"
            for f in "$WIN_BUNDLE_DIR/msi/"*.msi; do
                size=$(ls -lh "$f" | awk '{print $5}')
                echo "    📦 $(basename "$f") ($size)"
            done
            echo ""
        fi
    fi

    # Kopiere alle Bundles in einen gemeinsamen Ordner
    RELEASE_DIR="releases/v$VERSION"
    mkdir -p "$RELEASE_DIR"

    print_step "Kopiere Bundles nach $RELEASE_DIR..."

    # Kopiere Linux Bundles
    [ -d "$BUNDLE_DIR/appimage" ] && cp -f "$BUNDLE_DIR/appimage/"*.AppImage "$RELEASE_DIR/" 2>/dev/null || true
    [ -d "$BUNDLE_DIR/deb" ] && cp -f "$BUNDLE_DIR/deb/"*.deb "$RELEASE_DIR/" 2>/dev/null || true
    [ -d "$BUNDLE_DIR/rpm" ] && cp -f "$BUNDLE_DIR/rpm/"*.rpm "$RELEASE_DIR/" 2>/dev/null || true

    # Kopiere Windows Bundles
    [ -d "$WIN_BUNDLE_DIR/nsis" ] && cp -f "$WIN_BUNDLE_DIR/nsis/"*.exe "$RELEASE_DIR/" 2>/dev/null || true
    [ -d "$WIN_BUNDLE_DIR/msi" ] && cp -f "$WIN_BUNDLE_DIR/msi/"*.msi "$RELEASE_DIR/" 2>/dev/null || true

    # Erstelle Checksums
    print_step "Erstelle Checksums..."
    cd "$RELEASE_DIR"
    sha256sum * > checksums-sha256.txt 2>/dev/null || true
    cd - > /dev/null

    echo ""
    echo -e "📁 Alle Release-Dateien in: ${GREEN}$RELEASE_DIR${NC}"
    echo ""
    ls -lh "$RELEASE_DIR" 2>/dev/null || true
    echo ""
    echo "Fertig! 🎉"
}

# Schnellbau ohne Menü
quick_build() {
    local target=$1

    case $target in
        linux)
            build_linux "appimage,deb,rpm"
            ;;
        windows)
            build_windows
            ;;
        all)
            build_linux "appimage,deb,rpm"
            build_windows
            ;;
        appimage)
            build_linux "appimage"
            ;;
        deb)
            build_linux "deb"
            ;;
        rpm)
            build_linux "rpm"
            ;;
        *)
            print_error "Unbekanntes Target: $target"
            echo "Verfügbare Targets: linux, windows, all, appimage, deb, rpm"
            exit 1
            ;;
    esac

    show_results
}

# Hilfe anzeigen
show_help() {
    echo "Usage: $0 [OPTION]"
    echo ""
    echo "Optionen:"
    echo "  (keine)      Interaktives Menü anzeigen"
    echo "  --linux      Alle Linux-Formate bauen (AppImage, .deb, .rpm)"
    echo "  --windows    Windows .exe bauen (Cross-Compilation)"
    echo "  --all        Alle Formate bauen (Linux + Windows)"
    echo "  --appimage   Nur AppImage bauen"
    echo "  --deb        Nur .deb bauen"
    echo "  --rpm        Nur .rpm bauen"
    echo "  --help       Diese Hilfe anzeigen"
    echo ""
    echo "Beispiele:"
    echo "  $0                  # Interaktives Menü"
    echo "  $0 --linux          # Alle Linux-Formate"
    echo "  $0 --all            # Alles bauen"
    echo ""
}

# Hauptprogramm
main() {
    # Prüfe Abhängigkeiten
    check_dependencies

    # Verarbeite Kommandozeilen-Argumente
    case "${1:-}" in
        --help|-h)
            show_help
            exit 0
            ;;
        --linux)
            quick_build "linux"
            ;;
        --windows)
            quick_build "windows"
            ;;
        --all)
            quick_build "all"
            ;;
        --appimage)
            quick_build "appimage"
            ;;
        --deb)
            quick_build "deb"
            ;;
        --rpm)
            quick_build "rpm"
            ;;
        "")
            show_menu
            show_results
            ;;
        *)
            print_error "Unbekannte Option: $1"
            show_help
            exit 1
            ;;
    esac
}

# Start
main "$@"
