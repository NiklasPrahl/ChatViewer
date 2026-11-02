# 0004 – ChatViewer Archive (CVA) format

- Status: Proposed (full specification in M3: `docs/formats/cva.md`)
- Date: 2026-11-02

## Context

Signal chats need to be exported per chat as a ZIP with media, and the
viewer must be able to reopen it like a WhatsApp export. Plain WhatsApp
text cannot hold reactions, quotes, edits or exact timestamps.

## Decision (draft)

A ZIP containing:

- `_chat.txt`: WhatsApp-iOS-style lines in a fixed, locale-independent
  format (`[YYYY-MM-DD HH:mm:ss] Author: text`,
  `<attached: media/…>`). It is readable by humans and by third-party
  WhatsApp tools.
- `manifest.json`: **authoritative** structured data: format id and
  semver version, generator, source report, participants, messages
  (neutral model incl. reactions, quotes and edits), and the attachment
  index with SHA-256.
- `media/`: attachments with original names, made collision-safe.

The viewer prefers `manifest.json`. If it is missing or invalid, the
viewer parses `_chat.txt` as a WhatsApp export and shows a warning.
Readers accept the same major version and preserve unknown fields. The
JSON export reuses the manifest schema.

## Open questions

- Should messages move to a streamable `messages.jsonl` above a size
  threshold (proposed: more than 50k messages)?
- Time zone representation for sources with naive local time.

## Consequences

- A lossless round-trip (source → CVA → model) becomes a testable
  invariant.
- `_chat.txt` and the manifest must be kept consistent (line index).
