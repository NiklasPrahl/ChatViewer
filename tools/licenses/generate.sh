#!/bin/sh
# Writes target/licenses/THIRD_PARTY_LICENSES.txt (Rust crates + npm
# packages) for release bundles. Needs cargo-about and installed npm deps.
set -eu
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
out="$repo_root/target/licenses/THIRD_PARTY_LICENSES.txt"
mkdir -p "$(dirname "$out")"
cargo about generate --fail \
    --config "$repo_root/about.toml" \
    --manifest-path "$repo_root/app/src-tauri/Cargo.toml" \
    "$repo_root/tools/licenses/about.hbs" > "$out"
node "$repo_root/tools/licenses/npm-licenses.mjs" >> "$out"
echo "licenses: $out ($(wc -l < "$out") lines)"
