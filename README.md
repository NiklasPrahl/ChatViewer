# ChatViewer

> **Status: pre-alpha.** Repository foundation only (milestone M0).
> There is no application code yet. See [docs/plan.md](docs/plan.md).

A cross-platform desktop viewer (Windows, macOS, Linux) for exported chat
archives. It renders chats like a messenger and exports them to TXT,
Markdown, JSON and a re-openable archive format.

Planned inputs:

- WhatsApp chat exports (`.zip` / `.txt`, iOS and Android)
- Signal on-device backups (new cross-platform format, unlocked with the
  64-character recovery key)

ChatViewer is a **viewer only**. It has no network access, no telemetry,
and never stores chats, media or keys beyond the running session.

## Documentation

- [Architecture](docs/architecture.md)
- [Plan and milestones](docs/plan.md)
- [Architecture Decision Records](docs/adr/)

## Contributing

1. Activate the repository hooks once per clone:

   ```bash
   git config core.hooksPath .githooks
   ```

2. Install [gitleaks](https://github.com/gitleaks/gitleaks) (v8.30 or
   newer). The pre-commit hook refuses to run without it.
3. **Never commit real chats, backups, recovery keys, phone numbers, names
   or local paths.** Use synthetic data only. A generator will live in
   `tools/`.

All code, comments, docs, commit messages and issues are in English.

## License

The repository is licensed under the [MIT License](LICENSE), unless a
crate states otherwise. A planned exception is the Signal importer,
which is intended to be AGPL-3.0-only. Release binaries that include it
are then distributed under AGPL-3.0 as a whole. See
[ADR 0001](docs/adr/0001-license.md).
