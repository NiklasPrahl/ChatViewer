# ChatViewer Archive (CVA) – format version 1.0

A CVA file holds **one chat** with its media as a ZIP. ChatViewer reopens
it losslessly, and WhatsApp-oriented tools can still read its text.
Decision record: [ADR 0004](../adr/0004-cva-archive-format.md).

## Layout

```
<name>.zip
├── manifest.json      metadata (required)
├── messages.jsonl     one message per line (required)
├── _chat.txt          human-readable rendering (required, informative)
└── media/             attachments (optional)
    └── 000012-3-photo.jpg
```

A reader recognises CVA by `manifest.json` with `"format":
"chatviewer-archive"`. If the manifest is missing or invalid, the file
can still be opened as a WhatsApp export through `_chat.txt`. In that
case only the text survives, without reactions, quotes or raw records.

## manifest.json

```json
{
  "format": "chatviewer-archive",
  "format_version": "1.0",
  "generator": { "name": "chatviewer", "version": "0.0.0" },
  "source": { "app": "signal", "platform": "desktop", "variant": "…", "confidence": 100, … },
  "conversation": { "id": "c12", "kind": "group", "title": "…", "participants": ["r3", "r7"], … },
  "participants": [ { "id": "r3", "display_name": "…", "is_self": "yes", … } ],
  "message_count": 2527,
  "media": [
    { "path": "media/000012-3-photo.jpg", "source_blob": "signal:9f…", "size": 48211, "sha256": "…" }
  ]
}
```

- `source`: the importer's report for the original input (app, platform,
  detected variant, warnings, limitations, counters).
- `conversation`, `participants`: the neutral model (architecture §6),
  serialized as JSON. Each carries `extra.raw`, the original source
  record (ADR 0006).
- `media`: every file in `media/`, with the blob reference it had in the
  source archive, its size and SHA-256.

## messages.jsonl

One JSON object per line, in render order, using the same model and
serialization as the JSON export (`cv_model::Message`). Media references
in `content` point to `media/…` paths. The lines are UTF-8 and separated
by `\n`.

Each message's `extra.raw` keeps the source record:

- WhatsApp: `{"encoding": "text", "format": "whatsapp-txt", "text": <original lines>}`
- Signal: `{"encoding": "base64", "format": "signal-backup-frame", "data": <protobuf Frame>}`

## _chat.txt

The text uses the iOS WhatsApp line format with a fixed date format:

```
[2024-03-01 14:05:09] Alice: Hello
[2024-03-01 14:05:30] Bob: Look at this ‎<attached: 000013-4-photo.jpg>
[2024-03-01 14:06:00] Team: ‎Alice added Carol
```

- Times are the source's local time (WhatsApp) or UTC (Signal).
- System lines use the chat title as author and start with U+200E, as
  in iOS exports.
- The file is informative: readers must use `messages.jsonl` when it is
  present.

## Versioning

- `format_version` is `major.minor`. Readers accept any minor version of
  their major version and reject other majors with a clear message.
- Minor versions only add optional fields.
- Media file names are sanitized: no path separators or control
  characters, at most 80 characters, prefixed with the message sequence
  number and a counter so they are unique.

## Privacy

A CVA file contains the full chat and its media, decrypted. It is only
written when the user explicitly exports, to a location they choose.
Existing files are not overwritten without confirmation (`--force` in
the CLI).
