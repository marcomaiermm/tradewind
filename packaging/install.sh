#!/bin/sh
# Installs or updates Tradewind with the package manager of this system:
#   curl -fsSL https://raw.githubusercontent.com/marcomaiermm/tradewind/main/packaging/install.sh | sh
set -eu

latest=https://github.com/marcomaiermm/tradewind/releases/latest/download
key=https://raw.githubusercontent.com/marcomaiermm/tradewind/main/packaging/arch/tradewind.asc
fingerprint=6A4F7C1A0DD77827FB663CBBF8A74A1AE16AF11C

say() { printf '\033[1m==> %s\033[0m\n' "$*"; }

as_root() {
    if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo "$@"; fi
}

# Package managers ask before they install anything. stdin is this script when
# it is piped into sh, so their questions go to the terminal if there is one.
ask() {
    if (: </dev/tty) 2>/dev/null; then as_root "$@" </dev/tty; else as_root "$@"; fi
}

pacman_repo() {
    say "Trusting the Tradewind signing key"
    curl -fsSL -o "$tmp/tradewind.asc" "$key"
    as_root pacman-key --add "$tmp/tradewind.asc"
    as_root pacman-key --lsign-key "$fingerprint"
    if ! grep -q '^\[tradewind\]' /etc/pacman.conf; then
        say "Adding the [tradewind] repository to /etc/pacman.conf"
        printf '\n[tradewind]\nServer = %s\n' "$latest" | as_root tee -a /etc/pacman.conf >/dev/null
    fi
    ask pacman -Syu --needed tradewind
    say "Done. Updates come with every pacman -Syu."
}

rpm_package() {
    say "Trusting the Tradewind signing key"
    curl -fsSL -o "$tmp/tradewind.asc" "$key"
    as_root rpm --import "$tmp/tradewind.asc"
    ask "$@" "$latest/tradewind.x86_64.rpm"
}

appimage() {
    say "Installing the AppImage to ~/.local/bin/tradewind"
    mkdir -p "$HOME/.local/bin"
    curl -fL --progress-bar -o "$HOME/.local/bin/tradewind.part" "$latest/tradewind-x86_64.AppImage"
    chmod +x "$HOME/.local/bin/tradewind.part"
    mv "$HOME/.local/bin/tradewind.part" "$HOME/.local/bin/tradewind"
    say "Done. Start ~/.local/bin/tradewind once and it shows up in your app menu."
    say "Backups and addon updates also need bsdtar (libarchive) and zip."
}

main() {
    if [ "$(uname -m)" != x86_64 ]; then
        echo "Tradewind only has x86_64 builds so far." >&2
        exit 1
    fi
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    ID='' ID_LIKE=''
    [ -r /etc/os-release ] && . /etc/os-release

    # Steam Deck, Fedora Atomic and openSUSE Aeon have a read-only system, the
    # AppImage lives in $HOME.
    if [ -e /run/ostree-booted ] || [ "$ID" = steamos ] || command -v transactional-update >/dev/null; then
        appimage
        return
    fi
    case " $ID $ID_LIKE " in
    *" arch "*)
        pacman_repo
        return
        ;;
    *" debian "* | *" ubuntu "*)
        say "Downloading the .deb package"
        chmod 755 "$tmp" # apt reads the file as its unprivileged _apt user
        curl -fL --progress-bar -o "$tmp/tradewind_amd64.deb" "$latest/tradewind_amd64.deb"
        ask apt-get install "$tmp/tradewind_amd64.deb"
        ;;
    *" fedora "* | *" rhel "*)
        # dnf skips the signature check for packages from a URL by default.
        rpm_package dnf install --setopt=localpkg_gpgcheck=1
        ;;
    *" suse "*)
        rpm_package zypper install
        ;;
    *)
        appimage
        return
        ;;
    esac
    say "Done. Start Tradewind from your app menu. Run this script again to update."
}

main "$@"
