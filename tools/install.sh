#!/bin/sh
# Install Tradewind into a prefix (default ~/.local).
# Packagers: DESTDIR=/staging tools/install.sh /usr
set -eu
source_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
install_prefix=${1:-"$HOME/.local"}
destdir=${DESTDIR:-}
if [ ! -x "$source_root/target/release/tradewind" ]; then
    printf '%s\n' 'Run cargo build --release --locked first.' >&2
    exit 1
fi
case "$install_prefix" in
    /*) ;;
    *) printf '%s\n' 'The install prefix must be an absolute path.' >&2; exit 1 ;;
esac
case "$install_prefix" in
    *[!a-zA-Z0-9/_.-]*) printf '%s\n' 'The install prefix may only contain letters, digits, /, _, . and -.' >&2; exit 1 ;;
esac
root="$destdir$install_prefix"
install -Dm755 "$source_root/target/release/tradewind" "$root/bin/tradewind"
install -Dm644 "$source_root/LICENSE-MIT" "$root/share/licenses/tradewind/LICENSE-MIT"
install -Dm644 "$source_root/LICENSE-APACHE" "$root/share/licenses/tradewind/LICENSE-APACHE"
install -Dm644 "$source_root/data/icons/LICENSE.lucide" "$root/share/licenses/tradewind/Lucide"
install -Dm644 "$source_root/data/fonts/OFL.txt" "$root/share/licenses/tradewind/OpenSans-OFL"
install -Dm644 "$source_root/vendor/iced_tiny_skia/LICENSE" "$root/share/licenses/tradewind/Iced"
for icon_size in 16 32 48 128 256; do
    install -Dm644 "$source_root/data/icons/app/icon-$icon_size.png" \
        "$root/share/icons/hicolor/${icon_size}x${icon_size}/apps/tradewind.png"
done
install -Dm644 "$source_root/data/icons/app/tradewind.svg" "$root/share/icons/hicolor/scalable/apps/tradewind.svg"
desktop_entry=$(mktemp)
trap 'rm -f "$desktop_entry"' EXIT HUP INT TERM
# Absolute artwork also works when the desktop has not indexed the user icon theme.
sed -e "s|^Exec=.*|Exec=\"$install_prefix/bin/tradewind\"|" \
    -e "s|^Icon=.*|Icon=$install_prefix/share/icons/hicolor/256x256/apps/tradewind.png|" \
    "$source_root/data/tradewind.desktop" > "$desktop_entry"
install -Dm644 "$desktop_entry" "$root/share/applications/tradewind.desktop"
# Package managers refresh desktop and icon caches through their own hooks.
if [ -z "$destdir" ]; then
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$root/share/applications" || true
    fi
    # Do not require an icon-theme index in a user prefix. KDE also watches these paths.
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache --force --ignore-theme-index "$root/share/icons/hicolor" || true
    fi
fi
printf 'Installed: %s/bin/tradewind\n' "$root"
