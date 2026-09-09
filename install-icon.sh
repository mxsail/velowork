#!/bin/bash
# Install Velowork icon and desktop entry

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ASSETS_DIR="$SCRIPT_DIR/assets"
TARGET_ICON_DIR="$HOME/.local/share/icons/hicolor"
DESKTOP_DIR="$HOME/.local/share/applications"
CREATE_DESKTOP_SHORTCUT=false

# Parse arguments
POSITIONAL_SET=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        -d|--dest|--icon-dir)
            TARGET_ICON_DIR="$2"
            shift 2
            ;;
        -s|--source|--assets-dir)
            ASSETS_DIR="$2"
            shift 2
            ;;
        --desktop-dir)
            DESKTOP_DIR="$2"
            shift 2
            ;;
        --shortcut|--create-desktop-shortcut)
            CREATE_DESKTOP_SHORTCUT=true
            shift 1
            ;;
        -h|--help)
            echo "Usage: $0 [TARGET_ICON_DIR] [options]"
            echo ""
            echo "Options:"
            echo "  -d, --icon-dir DIR              Specify target icon base directory (default: $HOME/.local/share/icons/hicolor)"
            echo "  -s, --assets-dir DIR            Specify source assets directory (default: <script_dir>/assets)"
            echo "  --desktop-dir DIR               Specify target desktop entry directory (default: $HOME/.local/share/applications)"
            echo "  --shortcut                      Also create shortcut icon on Desktop ($HOME/Desktop)"
            echo "  -h, --help                      Show this help message"
            exit 0
            ;;
        *)
            if [ -z "$POSITIONAL_SET" ]; then
                TARGET_ICON_DIR="$1"
                POSITIONAL_SET=1
                shift
            else
                echo "Error: Unknown argument '$1'" >&2
                exit 1
            fi
            ;;
    esac
done

echo "Installing Velowork icons..."
echo "  Source assets: $ASSETS_DIR"
echo "  Target icon dir: $TARGET_ICON_DIR"

# Install PNG icons at various sizes
for size in 16 24 32 48 64 128 256 512 1024; do
    SIZE_DIR="$TARGET_ICON_DIR/${size}x${size}/apps"
    if [ -f "$ASSETS_DIR/app-icon-${size}.png" ]; then
        mkdir -p "$SIZE_DIR"
        cp "$ASSETS_DIR/app-icon-${size}.png" "$SIZE_DIR/velowork.png"
        echo "  Installed ${size}x${size} PNG icon -> $SIZE_DIR/velowork.png"
    fi
done

# Install SVG scalable icon if present
SCALABLE_DIR="$TARGET_ICON_DIR/scalable/apps"
if [ -f "$ASSETS_DIR/velowork_icon.svg" ]; then
    mkdir -p "$SCALABLE_DIR"
    cp "$ASSETS_DIR/velowork_icon.svg" "$SCALABLE_DIR/velowork.svg"
    echo "  Installed SVG scalable icon -> $SCALABLE_DIR/velowork.svg"
elif [ -f "$ASSETS_DIR/app-icon-simple.svg" ]; then
    mkdir -p "$SCALABLE_DIR"
    cp "$ASSETS_DIR/app-icon-simple.svg" "$SCALABLE_DIR/velowork.svg"
    echo "  Installed SVG scalable icon -> $SCALABLE_DIR/velowork.svg"
fi

# Install desktop entry
DESKTOP_SOURCE="$SCRIPT_DIR/velowork.desktop"
if [ -f "$DESKTOP_SOURCE" ]; then
    mkdir -p "$DESKTOP_DIR"
    cp "$DESKTOP_SOURCE" "$DESKTOP_DIR/velowork.desktop"
    echo "Installed desktop entry -> $DESKTOP_DIR/velowork.desktop"
fi

# Optional: Create desktop shortcut on ~/Desktop
if [ "$CREATE_DESKTOP_SHORTCUT" = true ] && [ -f "$DESKTOP_SOURCE" ]; then
    DESKTOP_FOLDER="${XDG_DESKTOP_DIR:-$HOME/Desktop}"
    if [ -d "$DESKTOP_FOLDER" ]; then
        cp "$DESKTOP_SOURCE" "$DESKTOP_FOLDER/velowork.desktop"
        chmod +x "$DESKTOP_FOLDER/velowork.desktop"
        echo "Installed desktop shortcut -> $DESKTOP_FOLDER/velowork.desktop"
    fi
fi

# Update icon cache
if command -v gtk-update-icon-cache &> /dev/null; then
    gtk-update-icon-cache -f -t "$TARGET_ICON_DIR" 2>/dev/null || true
    echo "Updated icon cache at $TARGET_ICON_DIR"
fi

# Update desktop database
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi

echo ""
echo "Installation complete!"
echo ""
echo "To use the icon:"
echo "  1. Run: cargo run (or run the built binary)"
echo "  2. The icon should appear in your task panel and application launcher"
echo ""
