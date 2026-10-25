#!/bin/sh
# Build the Arch package from the release tarball, sign it and upload it to the
# release together with the repo database. CI runs this as root in an
# archlinux:base-devel container with the private key in SIGNING_KEY.
set -eu
cd "$(dirname -- "$0")"

printf '%s\n' "$SIGNING_KEY" | gpg --batch --import
# The release is still a draft, so fetch the tarball through the API.
gh release download "$TAG" --repo "$GITHUB_REPOSITORY" \
    --pattern tradewind-x86_64-linux.tar.gz --output "tradewind-${TAG#v}.tar.gz"
useradd --create-home builder
chown -R builder .
runuser -u builder -- makepkg --nodeps

pkg=$(ls tradewind-*.pkg.tar.zst)
gpg --batch --detach-sign "$pkg"
repo-add --sign tradewind.db.tar.zst "$pkg"

# The .db and .files names are symlinks, gh uploads what they point to.
gh release upload "$TAG" --repo "$GITHUB_REPOSITORY" --clobber \
    "$pkg" "$pkg.sig" tradewind.db tradewind.db.sig tradewind.files tradewind.files.sig
