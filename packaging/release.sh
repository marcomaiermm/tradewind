#!/bin/sh
# Build release artifacts into dist/: tarball, .deb, .rpm, AppImage and SHA256SUMS.
# Requires cargo, nfpm and linuxdeploy on PATH. CI runs this on the oldest
# supported glibc (Ubuntu 22.04) so the binary runs on current distributions.
set -eu
cd "$(dirname -- "$0")/.."
version=${1:?usage: packaging/release.sh VERSION}
arch=$(uname -m)

rm -rf dist
mkdir -p dist/stage
cargo build --release --locked
DESTDIR="$PWD/dist/stage" tools/install.sh /usr

# Names without the version, so releases/latest/download/<name> always works.
tar -C dist/stage -czf "dist/tradewind-$arch-linux.tar.gz" usr
VERSION=$version nfpm package --config packaging/nfpm.yaml --packager deb --target dist/tradewind_amd64.deb
VERSION=$version nfpm package --config packaging/nfpm.yaml --packager rpm --target "dist/tradewind.$arch.rpm"

# linuxdeploy wants the icon named like the desktop entry's Icon= key.
cp data/icons/app/icon-256.png dist/tradewind.png
(
    cd dist
    LINUXDEPLOY_OUTPUT_VERSION=$version linuxdeploy --appdir AppDir \
        --executable ../target/release/tradewind \
        --desktop-file ../data/tradewind.desktop \
        --icon-file tradewind.png \
        --output appimage
)
mv dist/*.AppImage "dist/tradewind-$arch.AppImage"

rm -rf dist/stage dist/AppDir dist/tradewind.png
(cd dist && sha256sum -- * > SHA256SUMS)
ls -l dist
