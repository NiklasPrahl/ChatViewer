#!/bin/sh
# Builds the chatviewer CLI in release mode and places it where Tauri's
# externalBin expects it: app/src-tauri/binaries/chatviewer-<target-triple>.
set -eu
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
triple=${1:-$(rustc -vV | sed -n 's/^host: //p')}
cargo build --release --locked -p cv-cli --target "$triple" --manifest-path "$repo_root/Cargo.toml"
ext=""
case "$triple" in *windows*) ext=".exe" ;; esac
mkdir -p "$repo_root/app/src-tauri/binaries"
cp "$repo_root/target/$triple/release/chatviewer$ext" \
   "$repo_root/app/src-tauri/binaries/chatviewer-$triple$ext"
echo "sidecar: app/src-tauri/binaries/chatviewer-$triple$ext"
