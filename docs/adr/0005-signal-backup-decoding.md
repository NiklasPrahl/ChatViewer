# 0005 – Signal local backup decoding (M1 spike results)

- Status: Accepted (Desktop backup checked; Android and iOS pending)
- Date: 2026-11-02

## Context

M1 had to prove that a Signal on-device backup in the new cross-platform
format can be opened with only the 64-character recovery key, before any
GUI work. It also had to settle the license question (ADR 0001) and the
memory strategy for large backups. The format summary is in
[architecture §2](../architecture.md).

## Decision

- `crates/cv-import-signal` (AGPL-3.0-only) implements the format
  **without linking libsignal**. It uses RustCrypto (`hkdf`, `hmac`,
  `sha2`, `aes`, `cbc`, `ctr`), `flate2` and `prost`. Signal's
  `backup.proto` and `LocalBackup.proto` are vendored unmodified from
  libsignal `v0.106.1` and compiled with `protox`, so no `protoc` is
  needed. This confirms **ADR 0001, option B**.
- Reading pipeline, verified end to end:
  1. Recovery key → BackupKey → local metadata key.
  2. Decrypt the BackupId from `metadata` (AES-256-CTR, 12-byte nonce,
     32-bit counter).
  3. Derive the MessageBackupKey (legacy info string, no salt).
  4. Verify the HMAC over the whole `main` file in a first pass.
  5. Stream AES-256-CBC → gzip → varint-delimited `BackupInfo` + frames.
- Media: `files/<2 hex>/<hex(SHA-256(plaintextHash ‖ localKey))>`.
  The HMAC under the attachment's `localKey` is **authoritative**: a file
  that fails it is rejected. After decryption, the content is checked
  against `plaintextHash` (at the recorded size, or the unpadded full
  plaintext). A mismatch is not an error but `Integrity::AuthenticatedOnly`,
  shown as a warning. Reason: Signal Desktop copies its at-rest files into
  the backup and backs up local sticker copies whose bytes differ slightly
  from the recorded size/hash. Desktop itself skips this check for local
  files (`decryptAttachmentV2`, `type: 'local'`).
- Rejections (each covered by a test):
  - `metadata.version > 1`
  - a forward-secrecy header (`SBACKUP\x01`, server backups only)
  - a HMAC mismatch, reported as "wrong recovery key or damaged backup"
  - frames larger than 25 MiB
  - media whose HMAC fails
- Recovery key input accepts upper case, spaces, dashes and line breaks.
  Like libsignal's validator, it reads `#` as `o` and `=` as `0`.
- **Memory strategy for M3:** gzip streams are not seekable. A full pass
  over 1M messages takes about 1 s with constant memory, so re-streaming
  is cheap. M3 will do one pass on open to build the chat list and
  per-chat counts, and decode the opened chat's items into memory with a
  second pass. A full in-memory model of all chats is not needed.

## Verification

| Check | Result |
|---|---|
| KDF vs. libsignal test vector (`message_backup_key_legacy`) | pass |
| BackupId length | 16 bytes (confirmed in source and by the validator) |
| libsignal `validator` (v0.106.1, `--account-entropy` + `--metadata`) on our synthetic backups: default; 5 chats + group, 20,000 messages, 50 media; no group, no media | accepted (CI job `signal-crosscheck`) |
| Validator on a tampered backup | rejected |
| Unit + integration tests (`cargo test --all-features`) | 27 pass |
| 1,000,000 messages, 21 chats, 200 media (34 MiB `main`), release build, Apple Silicon | HMAC pass 0.09 s; decrypt + decode 1.06 s; 200/200 media verified; total 1.16 s; peak RSS 4.5 MB |

Notes for CI: libsignal's validator needs the features `cli,test-util`
and a `protoc` binary to build.

## Real-backup check (maintainer, local only)

To run it against your own backups, without committing anything:

```bash
cargo run --release -p cv-import-signal --features cli --bin signal-spike -- /path/to/SignalBackups
```

The key is requested with a hidden prompt. The tool prints only counts,
versions, timings and verification results, never content. Record only
the following below: platform (Android/iOS/Desktop), app version,
`metadata`/`BackupInfo` versions, pass/fail, and any error message.

| Source | App version | Metadata v. | Backup v. | Result |
|---|---|---|---|---|
| Desktop (macOS) | 8.33.0 | 1 | 1 | **pass**: main file fully decoded (≈135k chat items, ≈1.5k chats); no unknown frames, no unknown fields. All ≈3.5k distinct local media files authentic, none failed; ≈93 % verified by hash, the rest are stickers (see below). |
| Android | | | | pending |
| iOS | | | | pending |

Findings from the Desktop backup (2026-11-02, content-free diagnostics):

- Every non-sticker media file verified by hash. That covers attachments,
  voice messages, GIFs, link preview images, quote thumbnails and
  contact avatars, including those in edit history.
- About 7 % of the distinct media files are **stickers** whose bytes
  differ from the recorded size/hash (mostly `image/webp`, nearly all
  < 1 % shorter). They pass the HMAC and are treated as
  `AuthenticatedOnly` (ADR 0006). Signal Desktop backs up its local
  sticker-pack copy and does not check these files either.
- Media are referenced from many places (`walk.rs`). After traversing
  all of them, all but 7 entries of the snapshot's `files` list (0.2 %)
  are accounted for. The 7 remain unexplained (possibly stale entries).
  M3 must report such files instead of dropping them.
- No frame re-encodes to a different size, so this schema version
  understands everything this Desktop version writes.
- Sticker messages nearly always carry an emoji (≈99 %). About half of
  them have no local image, so the emoji fallback (ADR 0006) matters.
- `BackupInfo.firstAppVersion` is empty in Desktop backups.

## Consequences

- Signal decryption is feasible without libsignal. The format is pinned to
  a libsignal tag, and schema updates are a file copy plus tests.
- Synthetic backups are verified against Signal's own validator in CI, so
  generator drift is caught early.
- If real backups differ from synthetic ones (e.g. new frame types), the
  reader already tolerates unknown fields and frames. The spike counts
  them as "unknown".

## Alternatives considered

Linking libsignal (ADR 0001 option A) was not needed. The validator is
used only as an external test oracle in CI and is never shipped.
