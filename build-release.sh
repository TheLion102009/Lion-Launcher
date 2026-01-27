#!/bin/bash
# Lion Launcher - Release Build Script

set -e

echo "🦁 Lion Launcher - Release Build"
echo "================================="
echo ""

# Farben
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

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

# 1. Prüfe ob Tauri CLI installiert ist
print_step "Prüfe Tauri CLI..."
if ! command -v cargo-tauri &> /dev/null; then
    print_warning "Tauri CLI nicht gefunden. Installiere..."
    cargo install tauri-cli --version "^2.0.0"
fi
print_success "Tauri CLI bereit"

# 2. Build-Typ auswählen
echo ""
echo "Wähle Build-Typ:"
echo "  1) AppImage (Linux)"
echo "  2) .deb (Debian/Ubuntu)"
echo "  3) Beide (AppImage + .deb)"
echo "  4) Alle unterstützten Formate"
echo ""
read -p "Auswahl (1-4): " choice

case $choice in
    1)
        BUNDLES="appimage"
        ;;
    2)
        BUNDLES="deb"
        ;;
    3)
        BUNDLES="appimage,deb"
        ;;
    4)
        BUNDLES="all"
        ;;
    *)
        echo "Ungültige Auswahl. Verwende AppImage als Standard."
        BUNDLES="appimage"
        ;;
esac

# 3. Build starten
echo ""
print_step "Starte Release Build ($BUNDLES)..."
echo ""

if [ "$BUNDLES" == "all" ]; then
    cargo tauri build
else
    cargo tauri build --bundles "$BUNDLES"
fi

# 4. Zeige Ergebnisse
echo ""
echo "================================="
print_success "Build abgeschlossen!"
echo ""
echo "📦 Ausgabe-Dateien:"
echo ""

# Finde und zeige alle Bundle-Dateien
BUNDLE_DIR="target/release/bundle"
if [ -d "$BUNDLE_DIR" ]; then
    # AppImage
    if ls "$BUNDLE_DIR/appimage/"*.AppImage 1> /dev/null 2>&1; then
        echo "  AppImage:"
        ls -lh "$BUNDLE_DIR/appimage/"*.AppImage | awk '{print "    " $9 " (" $5 ")"}'
    fi

    # .deb
    if ls "$BUNDLE_DIR/deb/"*.deb 1> /dev/null 2>&1; then
        echo "  Debian Package:"
        ls -lh "$BUNDLE_DIR/deb/"*.deb | awk '{print "    " $9 " (" $5 ")"}'
    fi

    # .rpm
    if ls "$BUNDLE_DIR/rpm/"*.rpm 1> /dev/null 2>&1; then
        echo "  RPM Package:"
        ls -lh "$BUNDLE_DIR/rpm/"*.rpm | awk '{print "    " $9 " (" $5 ")"}'
    fi
fi

echo ""
echo "Fertig! 🎉"
