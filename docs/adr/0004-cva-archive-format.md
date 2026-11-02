# 0004 – ChatViewer Archive (CVA) format

- Status: Accepted (format version 1.0, implemented in M3)
- Date: 2026-11-02
- Specification: [docs/formats/cva.md](../formats/cva.md)

## Context

Signal chats need to be exported per chat as a ZIP with media, and the
viewer must be able to reopen it like a WhatsApp export. Plain WhatsApp
text cannot hold reactions, quotes, edits or exact timestamps.

## Decision

A ZIP containing:

- `manifest.json`: format id and semver version, generator, the source
  report, the conversation and its participants, and a media index
  (path, source blob, size, SHA-256).
- `messages.jsonl`: **authoritative**. It holds one message per line in
  the neutral model, including each message's raw source record
  (ADR 0006). This answers the draft's open question: JSON Lines is used
  always, not only above a size threshold. It streams, keeps the manifest
  small, and leaves a single code path.
- `_chat.txt`: iOS-WhatsApp-style lines (`[YYYY-MM-DD HH:MM:SS] Author:
  text`, attachments as `‎<attached: file>`). It is informative and
  keeps the archive readable without ChatViewer.
- `media/`: decrypted attachments, stored without recompression, with
  sanitized, unique names.

Rules:

- Time zones: timestamps keep the source's kind (WhatsApp local time,
  Signal UTC). `_chat.txt` shows them as stored.
- Reading: the CVA importer recognises the manifest and wins over the
  WhatsApp importer. Without a manifest, the file opens as a WhatsApp
  export through `_chat.txt`. Readers accept the same major version.
- Media that cannot be read from the source while exporting are marked
  `Missing` and counted in the export summary. The export does not fail.

## Consequences

- Source → CVA → model is lossless and tested for WhatsApp (all format
  variants) and Signal (synthetic backups), including media bytes.
- Fields unknown to an older reader are dropped when it rereads a newer
  minor version. The raw source records still preserve the original
  data.
- A CVA file is a decrypted copy of a chat. It is only written on an
  explicit user export (ADR 0003).
