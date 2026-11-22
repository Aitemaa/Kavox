#!/usr/bin/env bash
# Builds Kavox from source and installs it for the current user (no root needed).
#
#   ./scripts/install.sh              build and install into ~/.local
#   ./scripts/install.sh --deps       first install build dependencies (pacman, apt or dnf; uses sudo)
#   ./scripts/install.sh --uninstall  remove Kavox (your sounds and settings are kept)
#
# Set PREFIX to install somewhere other than ~/.local.
set -euo pipefail

APP_ID="io.github.aitemaa.kavox"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
APPS_DIR="$PREFIX/share/applications"
ICONS_DIR="$PREFIX/share/icons/hicolor"
DESKTOP_FILE="$APPS_DIR/$APP_ID.desktop"
AUTOSTART_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/Kavox.desktop"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MIN_RUST="1.88"

say() { printf '\033[1;35m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# True if version $1 >= $2 (dotted numbers).
version_ge() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]; }

uninstall() {
    if pgrep -x kavox >/dev/null 2>&1; then
        say "Quitting the running Kavox"
        pkill -TERM -x kavox || true
        sleep 1
    fi
    rm -f "$BIN_DIR/kavox" "$BIN_DIR/kavoxctl" "$DESKTOP_FILE" "$AUTOSTART_FILE"
    for size in 32x32 128x128 256x256; do
        rm -f "$ICONS_DIR/$size/apps/kavox.png"
    done
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$APPS_DIR" 2>/dev/null || true
    say "Kavox removed."
    echo "Your sounds and settings are still in:"
    echo "  ${XDG_CONFIG_HOME:-$HOME/.config}/$APP_ID"
    echo "  ${XDG_DATA_HOME:-$HOME/.local/share}/$APP_ID"
    echo "Delete those folders to remove everything."
}

install_deps() {
    if command -v pacman >/dev/null; then
        say "Installing build dependencies with pacman"
        sudo pacman -S --needed base-devel rustup libpulse libxkbcommon
        rustup toolchain list | grep -q stable || rustup default stable
    elif command -v apt-get >/dev/null; then
        say "Installing build dependencies with apt"
        sudo apt-get update
        sudo apt-get install -y build-essential pkg-config libpulse-dev libxkbcommon-dev libgl1-mesa-dev
    elif command -v dnf >/dev/null; then
        say "Installing build dependencies with dnf"
        sudo dnf install -y gcc make pkgconf-pkg-config pulseaudio-libs-devel libxkbcommon-devel mesa-libGL-devel
    else
        die "--deps supports pacman, apt and dnf. Install these yourself: Rust $MIN_RUST+, pkg-config, and the development packages for libpulse and libxkbcommon."
    fi
    if ! command -v cargo >/dev/null; then
        warn "Rust isn't installed. Get it from https://rustup.rs (distro Rust packages are often too old), then run this script again."
    fi
}

check_tools() {
    command -v cargo >/dev/null || die "Rust isn't installed. Get it from https://rustup.rs, or run this script with --deps."
    local rust
    rust="$(rustc --version | awk '{print $2}')"
    version_ge "$rust" "$MIN_RUST" || die "Rust $rust is too old; Kavox needs $MIN_RUST or newer. Run: rustup update stable"

    command -v pkg-config >/dev/null || die "pkg-config isn't installed. Run this script with --deps."
    local missing=()
    for lib in libpulse libpulse-simple; do
        pkg-config --exists "$lib" || missing+=("$lib")
    done
    [ ${#missing[@]} -eq 0 ] || die "Missing development libraries: ${missing[*]}. Run this script with --deps."
}

build() {
    cd "$ROOT"
    say "Building Kavox and kavoxctl (the first build takes a minute)"
    cargo build --release -p kavox -p kavoxctl
}

install_files() {
    say "Installing into $PREFIX"
    if pgrep -x kavox >/dev/null 2>&1; then
        warn "Kavox is running; quit it and start it again to use the new version."
    fi
    install -Dm755 "$ROOT/target/release/kavox" "$BIN_DIR/kavox"
    install -Dm755 "$ROOT/target/release/kavoxctl" "$BIN_DIR/kavoxctl"
    install -Dm644 "$ROOT/assets/icons/32x32.png" "$ICONS_DIR/32x32/apps/kavox.png"
    install -Dm644 "$ROOT/assets/icons/128x128.png" "$ICONS_DIR/128x128/apps/kavox.png"
    install -Dm644 "$ROOT/assets/icons/128x128@2x.png" "$ICONS_DIR/256x256/apps/kavox.png"
    mkdir -p "$APPS_DIR"
    cat >"$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=Kavox
GenericName=Soundboard
Comment=Free soundboard with a virtual microphone
Exec=$BIN_DIR/kavox
Icon=kavox
Terminal=false
Categories=AudioVideo;Audio;
Keywords=soundboard;voice;microphone;discord;
StartupWMClass=kavox
EOF
    command -v update-desktop-database >/dev/null && update-desktop-database -q "$APPS_DIR" 2>/dev/null || true
    command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf "$ICONS_DIR" 2>/dev/null || true
}

main() {
    case "${1:-}" in
        --uninstall) uninstall; return ;;
        --deps) install_deps ;;
        "") ;;
        -h|--help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; return ;;
        *) die "unknown option: $1 (try --help)" ;;
    esac
    check_tools
    build
    install_files

    say "Kavox is installed."
    echo "  Start it from your app launcher, or run: kavox"
    echo "  Control it from key bindings with: kavoxctl play <sound name>"
    case ":$PATH:" in
        *":$BIN_DIR:"*) ;;
        *) warn "$BIN_DIR isn't on your PATH; add it to use the kavox and kavoxctl commands." ;;
    esac
}

main "$@"
