#!/bin/sh
# Self-test for .githooks/pre-commit and .gitleaks.toml.
# Plants fake secrets in a throwaway repository and checks that the hook
# blocks them. All "secrets" are generated at runtime, so none are stored
# in this repository.

set -eu

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

failures=0

rand() { # rand <alphabet> <length>
    LC_ALL=C tr -dc "$1" </dev/urandom | head -c "$2"
}

new_repo() {
    rm -rf "$work/repo"
    mkdir -p "$work/repo/.githooks"
    cp "$repo_root/.gitleaks.toml" "$work/repo/"
    cp "$repo_root/.githooks/pre-commit" "$work/repo/.githooks/"
    git -C "$work/repo" init -q
    git -C "$work/repo" config core.hooksPath .githooks
    git -C "$work/repo" config user.name selftest
    git -C "$work/repo" config user.email selftest@example.invalid
}

# expect <block|allow> <description> <path> <content-command>
expect() {
    want=$1 desc=$2 path=$3
    new_repo
    mkdir -p "$(dirname "$work/repo/$path")"
    eval "$4" >"$work/repo/$path"
    git -C "$work/repo" add -f -- "$path"
    if (cd "$work/repo" && sh .githooks/pre-commit) >"$work/log" 2>&1; then
        got=allow
    else
        got=block
    fi
    if [ "$got" = "$want" ]; then
        echo "ok   - $desc ($got)"
    else
        echo "FAIL - $desc: expected $want, got $got"
        sed 's/^/       /' "$work/log"
        failures=$((failures + 1))
    fi
}

key=$(rand 'a-z0-9' 64)
grouped=$(printf '%s' "$(rand 'a-z0-9' 64)" | sed 's/..../& /g; s/ $//')
phone="+49 151 $(rand '0-9' 8)"
hexdigest=$(rand '0-9a-f' 64)

expect block "recovery key" notes.txt "printf 'key = %s\n' '$key'"
expect block "grouped recovery key" notes.txt "printf 'key: %s\n' '$grouped'"
expect block "phone number" notes.txt "printf 'call %s\n' '$phone'"
expect block "WhatsApp chat file name" "_chat.txt" "printf 'hello\n'"
expect block "zip outside fixtures" "export.zip" "printf 'PK'"
expect block "WhatsApp media name in source" src/a.rs "printf '// IMG-20200101-WA%s.jpg\n' '$(rand 0-9 4)'"
expect block "home path" notes.txt "printf 'see /Users/%s/Desktop\n' '$(rand a-z 8)'"
expect block "file over 1 MiB" big.bin "head -c 1100000 /dev/zero"
expect allow "SHA-256 hex digest" notes.txt "printf 'sha256 %s\n' '$hexdigest'"
expect allow "fictional US phone number" notes.txt "printf 'call +1 555 0142\n'"
expect allow "fictional UK phone number" notes.txt "printf 'call +44 7700 900123\n'"
expect allow "zip inside fixtures" fixtures/wa/sample.zip "printf 'PK'"
expect allow "plain source file" src/a.rs "printf 'fn main() {}\n'"

if [ "$failures" -gt 0 ]; then
    echo "$failures hook self-test case(s) failed"
    exit 1
fi
echo "hook self-test passed"
