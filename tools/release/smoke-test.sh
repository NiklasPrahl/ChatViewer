#!/bin/sh
# Smoke test of release bundles on CI runners: the bundled CLI runs, opens
# synthetic WhatsApp and Signal archives, and writes a CVA export that
# reopens. Usage: tools/release/smoke-test.sh <target-triple>
set -eu

target=$1
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
bundle="${BUNDLE_DIR:-$repo_root/target/$target/release/bundle}"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

ext=""
case "$target" in *windows*) ext=".exe" ;; esac

# Locate the CLI as shipped in the bundle where we can.
case "$target" in
  *apple-darwin)
    app=$(find "$bundle/macos" -maxdepth 1 -name '*.app' | head -1)
    cli="$app/Contents/MacOS/chatviewer"
    test -f "$app/Contents/Resources/THIRD_PARTY_LICENSES.txt"
    test -f "$app/Contents/Resources/LICENSES/AGPL-3.0-only.txt"
    ls "$bundle/dmg/"*.dmg >/dev/null
    ;;
  *linux*)
    deb=$(ls "$bundle/deb/"*.deb | head -1)
    sudo dpkg -i "$deb"
    cli=/usr/bin/chatviewer
    ls "$bundle/rpm/"*.rpm "$bundle/appimage/"*.AppImage >/dev/null
    ;;
  *windows*)
    # Installers are checked for presence; the CLI is the sidecar built
    # for the bundle.
    ls "$bundle/msi/"*.msi "$bundle/nsis/"*-setup.exe >/dev/null
    cli="$repo_root/app/src-tauri/binaries/chatviewer-$target$ext"
    ;;
esac

echo "CLI: $cli"
"$cli" --version

# Synthetic inputs only.
cargo build --release --locked -p fixture-gen --target "$target" --manifest-path "$repo_root/Cargo.toml"
gen="$repo_root/target/$target/release/fixture-gen$ext"
"$gen" whatsapp --out "$work/wa" --messages 40 >/dev/null
wa=$(find "$work/wa/00" -name '*.zip' | head -1)
"$cli" info "$wa" --anonymous

key=$("$gen" signal --out "$work/SignalBackups" 2>/dev/null)
printf '%s\n' "$key" | "$cli" info "$work/SignalBackups" --recovery-key-stdin --anonymous
mkdir "$work/out"
printf '%s\n' "$key" | "$cli" export "$work/SignalBackups" --recovery-key-stdin --all \
  --format cva --out-dir "$work/out"
count=$(find "$work/out" -name '*.zip' | wc -l | tr -d ' ')
test "$count" -eq 3
first=$(find "$work/out" -name '*.zip' | head -1)
"$cli" info "$first" --anonymous
echo "smoke test passed"
