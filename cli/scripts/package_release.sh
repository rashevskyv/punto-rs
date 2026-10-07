#!/usr/bin/env bash
# Собирает архивы релиза в cli/dist: статический Linux x86_64 (musl) и
# Windows x86_64 (mingw), с контрольными суммами.
# Нужны: musl-tools, mingw-w64, rustup-цели x86_64-unknown-linux-musl и
# x86_64-pc-windows-gnu, zip.
set -euo pipefail

cli_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$cli_dir"
rm -rf dist
mkdir -p dist

linux_target=x86_64-unknown-linux-musl
cargo build --release --locked --target "$linux_target"
binary="target/$linux_target/release/punto-rs"
"$binary" --version
"$binary" --check-config -c config/punto-rs.conf
if readelf --program-headers "$binary" | grep --quiet 'Requesting program interpreter'; then
    echo "release binary is dynamically linked" >&2
    exit 1
fi
install -Dm755 "$binary" dist/punto-rs/punto-rs
install -Dm644 config/punto-rs.conf dist/punto-rs/config/punto-rs.conf
install -Dm644 systemd/punto-rs.service dist/punto-rs/systemd/punto-rs.service
install -Dm644 ../README.md dist/punto-rs/README.md
install -Dm644 ../LICENSE dist/punto-rs/LICENSE
install -Dm755 scripts/install.sh dist/punto-rs/scripts/install.sh
cp -R LICENSES dist/punto-rs/LICENSES
cp -R docs dist/punto-rs/docs
tar --create --gzip --file dist/punto-rs-linux-x86_64.tar.gz --directory dist punto-rs
rm -rf dist/punto-rs

windows_target=x86_64-pc-windows-gnu
cargo build --release --locked --target "$windows_target"
mkdir -p dist/punto-rs-windows
install -m644 "target/$windows_target/release/punto-rs.exe" dist/punto-rs-windows/punto-rs.exe
install -m644 config/punto-rs.conf dist/punto-rs-windows/config.conf.example
install -m644 ../README.md ../LICENSE dist/punto-rs-windows/
cp -R LICENSES dist/punto-rs-windows/LICENSES
(cd dist/punto-rs-windows && zip -qr ../punto-rs-windows-x86_64.zip .)
rm -rf dist/punto-rs-windows

cd dist
for asset in punto-rs-linux-x86_64.tar.gz punto-rs-windows-x86_64.zip; do
    sha256sum "$asset" > "$asset.sha256"
done
ls -l
