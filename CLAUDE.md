# Project rules

## Purpose
Cross-platform desktop viewer (Windows, macOS, Linux) for exported chat archives
(WhatsApp, Signal). It renders chats like a real messenger and exports them.
It is a viewer only: never persist chats, media or keys beyond the session.

## Hard constraints
- All code, comments, docs, commits, UI strings and issues are in English.
- The repository is public. Never commit personal data: real chats, backups,
  keys, phone numbers, names, local paths. Use synthetic test data only
  (a generator script lives in tools/). Keep secret scanning and a
  pre-commit hook active.
- No telemetry, no network access at runtime.
- Builds are unsigned and produced by GitHub Actions for all three platforms.
- License: permissive open source, commercial use allowed, attribution required
  (MIT unless dependency licenses force otherwise; see docs/adr/).
- Keep personal notes in CLAUDE.local.md (gitignored).

## Architecture principles
- Core library (parsing, neutral chat model, exporters, Signal decryption)
  is separate from GUI and CLI. GUI and CLI both use the core.
- Neutral chat model (conversation, participant, message, attachment,
  reaction, quote, system event). Each messenger is a parser plugin that
  produces this model. The UI only renders the model, so new chat apps
  need no UI redesign.
- Streaming and lazy loading for large chats and media (virtualized list).
- Parsers must be tolerant of format variants across app versions and
  platforms (iOS vs Android) and report detected version and limits.
- Version or format incompatibilities must be visible in the GUI.
- Every error dialog has a "Copy log" button; logs must redact message content.

## Quality
- Document decisions as ADRs in docs/adr/.
- Unit tests for parsers with fixtures, round-trip tests for exporters,
  CI runs tests and linters on all platforms.
- Do not implement beyond the agreed milestone.
