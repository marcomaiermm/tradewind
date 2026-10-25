#!/bin/sh
# Sign the Arch package of a release and upload the signature together with the
# signed repo database. CI runs this in an archlinux:base container with the
# private key in SIGNING_KEY.
set -eu

printf '%s\n' "$SIGNING_KEY" | gpg --batch --import
# The release is still a draft, so fetch the package through the API.
gh release download "$TAG" --repo "$GITHUB_REPOSITORY" --pattern '*.pkg.tar.zst'
pkg=$(ls -- *.pkg.tar.zst)
gpg --batch --detach-sign "$pkg"
repo-add --sign tradewind.db.tar.zst "$pkg"

# The .db and .files names are symlinks, gh uploads what they point to.
gh release upload "$TAG" --repo "$GITHUB_REPOSITORY" --clobber \
    "$pkg.sig" tradewind.db tradewind.db.sig tradewind.files tradewind.files.sig
