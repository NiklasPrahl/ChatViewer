#!/bin/sh
# Cross-checks our Signal backup writer against libsignal's validator.
# Usage: tools/ci/signal-crosscheck.sh <path-to-libsignal-validator>
# Generates several synthetic backups and requires the validator to accept
# each one and to reject a tampered copy.

set -eu

validator=$1
repo_root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cargo build -q --release -p fixture-gen --manifest-path "$repo_root/Cargo.toml"
gen="$repo_root/target/release/fixture-gen"

check() { # check <name> <fixture-gen args...>
    name=$1
    shift
    out="$work/$name"
    key=$("$gen" signal --out "$out" "$@" 2>/dev/null)
    snap=$(ls -d "$out"/signal-backup-*)
    if "$validator" "$snap/main" --account-entropy "$key" --metadata "$snap/metadata" \
        >"$work/log" 2>&1; then
        echo "ok   - validator accepts $name"
    else
        echo "FAIL - validator rejects $name"
        cat "$work/log"
        exit 1
    fi
}

check default
check large --seed 7 --chats 5 --messages 20000 --media 50
check no-group-no-media --seed 3 --no-group --media 0

# Negative control: a flipped byte must be rejected.
snap=$(ls -d "$work/default"/signal-backup-*)
key=$("$gen" signal --out "$work/again" 2>/dev/null)
size=$(wc -c <"$snap/main")
printf '\377' | dd of="$snap/main" bs=1 seek=$((size / 2)) conv=notrunc 2>/dev/null
if "$validator" "$snap/main" --account-entropy "$key" --metadata "$snap/metadata" \
    >/dev/null 2>&1; then
    echo "FAIL - validator accepted a tampered backup"
    exit 1
fi
echo "ok   - validator rejects a tampered backup"
