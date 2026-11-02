# 0003 – No persistence of chats, media or keys

- Status: Accepted
- Date: 2026-11-02

## Context

ChatViewer handles highly personal data and Signal recovery keys. The
project rules state that it is a viewer only and never persists chats,
media or keys beyond the session.

## Decision

- Opened archives are read in place (ZIP, folder). Decrypted content lives
  only in process memory.
- No caches, thumbnails, indexes or temp files on disk. The only files we
  write are explicit user exports, at a location the user chooses.
- Recovery keys are held in `secrecy`/`zeroize` types and dropped when the
  archive is closed. They are never logged and never stored. The CLI reads
  them from stdin or a TTY prompt, never from command-line arguments.
- App settings may be persisted (theme, window size, recent-file list is
  **off** by default), but they never contain chat content or keys.
- Logs are kept in memory (ring buffer for "Copy log") and are redacted:
  no message content, names, phone numbers, file names or keys.
- Any exception (e.g. a codec that requires a file path) needs its own ADR
  that defines wipe-on-exit behavior.

## Consequences

- Very large chats must be handled with streaming and in-memory indexes.
  Reopening an archive repeats the parsing work.
- Tests must verify that no files are written (sandboxed HOME/TEMP) and
  that logs are redacted.

## Alternatives considered

An on-disk cache for faster reopening was rejected for privacy reasons.
