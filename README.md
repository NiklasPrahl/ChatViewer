# ChatViewer

> **Status: pre-alpha.** Milestone M4: desktop app (Tauri + React) on
> top of WhatsApp and Signal import, the ChatViewer Archive format and the
> `chatviewer` command line. See [docs/plan.md](docs/plan.md).

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

## Desktop app (development build)

Requires Rust 1.95+ and Node.js 22. On Linux, also the WebKitGTK
development packages listed in `.github/workflows/ci.yml`.

```bash
cd app && npm install && npm run tauri dev
```

`npm run dev:mock` shows the interface in a browser with synthetic data
(no Rust backend), useful for UI work.

## Command line (development build)

```bash
cargo run -p cv-cli -- info path/to/WhatsApp-export.zip --anonymous
cargo run -p cv-cli -- export path/to/WhatsApp-export.zip --format md -o chat.md
cargo run -p cv-cli -- info path/to/SignalBackups --stats
cargo run -p cv-cli -- export path/to/SignalBackups --chat c12 --format cva -o chat.zip
cargo run -p cv-cli -- export path/to/SignalBackups --all --format cva --out-dir exports/
```

- `info` shows the detected source, format variant, warnings and chats.
  With `--stats` it adds aggregated counts, never content.
- `export` writes txt or md (both lossy), json (lossless), or cva: a ZIP
  with all media that ChatViewer can reopen
  ([format](docs/formats/cva.md)).
- `--all` or repeated `--chat` with `--out-dir` writes **one file per
  chat**, e.g. one decrypted, self-contained ZIP per Signal chat. The app
  does the same via "Export chats…".
- Signal backups ask for the 64-character recovery key with a hidden
  prompt, or read it from stdin with `--recovery-key-stdin`.
- Existing files are never overwritten without `--force`.

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
