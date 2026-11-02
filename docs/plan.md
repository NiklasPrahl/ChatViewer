# ChatViewer – Plan

Status: **Approved 2026-11-02.** Companion to `docs/architecture.md`.
Each milestone stops for review before the next one starts.

---

## 1. License recommendation (task D)

### Facts (see architecture §2.7)

- Everything Signal publishes about the backup format is **AGPL-3.0-only**:
  libsignal, the apps, and `backup.proto`. There is no separate permissive
  specification.
- The crypto layer is standard and small. The protobuf schema is the
  sticking point.
- The project goal (CLAUDE.md) is a permissive license with commercial use
  and attribution, i.e. MIT, "unless dependency licenses force otherwise".

### Options

| | Option | Repo/app license | Effort | Legal risk | Fits goal |
|---|---|---|---|---|---|
| A | Link libsignal (`libsignal-message-backup`) as a git dependency | App binaries **AGPL-3.0**. Our own crates can stay MIT-licensed files. | low crypto effort, high build/dependency effort (not on crates.io, "unsupported outside Signal") | low | partly. Commercial use is allowed under AGPL, but it is copyleft. |
| B | Clean-room crypto (RustCrypto) **plus vendored `backup.proto`** in a separate `cv-import-signal` crate licensed AGPL-3.0 | MIT for everything except that crate. Distributed app = AGPL-3.0 as a whole. A Cargo feature `signal` lets anyone build an **MIT-only** binary without Signal support. | medium | low | good compromise |
| C | Clean-room crypto **plus our own hand-written minimal schema** (field numbers and types only, own names, no copied text) | MIT everywhere | medium–high (schema maintenance as Signal evolves) | **medium**, depends on whether a data format is protectable (EU: likely not, per *SAS v WPL*). Needs legal review. | full |
| D | Signal support as a separate AGPL sidecar executable, talking to the MIT app over stdin/stdout JSON | MIT app + AGPL helper shipped side by side ("aggregate") | medium–high | low–medium (the FSF's "arm's length" test is debatable when the two always ship together) | mostly |

### Recommendation

**Option B now, Option C evaluated later:**

1. Repository default license: **MIT** (`LICENSE`). Every crate declares
   its SPDX license.
2. `crates/cv-import-signal` is **AGPL-3.0-only**, vendors `backup.proto`
   and `LocalBackup.proto` unmodified with attribution, and is behind the
   default-on Cargo feature `signal`.
3. Official release binaries include Signal support and are therefore
   distributed under **AGPL-3.0**. The README, About dialog and
   `THIRD_PARTY_LICENSES` explain this clearly. Corresponding source is
   the tagged repo. Because there is no network service, AGPL §13 has no
   practical effect.
4. Users or companies who need a permissive binary build with
   `--no-default-features --features whatsapp,cva` and get MIT-only
   output.
5. After the spike, decide in an ADR whether Option C (MIT everywhere) is
   worth a legal opinion. **This document is not legal advice.**

Also needed in ADRs: Twemoji (CC-BY 4.0) if bundled, Opus/symphonia
codec licenses, and Tauri (MIT/Apache-2.0). `cargo-deny` enforces an
allow-list in CI.

---

## 2. Milestones (task E)

Rule: each milestone ends with a demo, green CI and updated docs/ADRs,
then **stop for approval**.

### M0 – Repository foundation (no app code)

- `git init`, MIT `LICENSE`, `README` (draft), `.gitignore` (blocks
  `*.zip`, `*.txt` outside `fixtures/`, `SignalBackups/`, `*.backup`,
  `CLAUDE.local.md`).
- Pre-commit hook: **gitleaks** with custom rules (64-char `[a-z0-9]`
  recovery keys, phone numbers, real-looking WhatsApp file names outside
  `fixtures/`), `cargo fmt`, and a file-size limit.
- GitHub: secret scanning + push protection, Dependabot, a CI skeleton
  (Linux/macOS/Windows matrix: fmt, clippy, test, cargo-deny).
- ADRs 0001–0004: license (§1), tech stack, no-persistence policy,
  CVA format draft.
- **Exit:** CI green on an empty workspace. The hooks reject a planted
  fake key in a test commit.

### M1 – Signal decryption feasibility spike (CLI only, throwaway-quality allowed)

Goal: prove we can open a real new-format local backup with only the
recovery key, before any GUI work.

- `tools/fixture-gen`: write a **synthetic** Signal local backup (fake
  AEP, fake ACI, 3 chats, about 50 messages, 3 media files, reactions,
  quotes) using our own writer, following the layout in architecture §2.2.
- `cv-import-signal` spike: AEP → BackupKey → metadata key → decrypt
  BackupId → MessageBackupKey → HMAC verify → AES-CBC → gunzip →
  BackupInfo + frames → counts per frame type; decrypt one attachment and
  verify its `plaintextHash`.
- **Cross-validation:** in a CI-only job (not shipped), run libsignal's
  `validator` binary against our synthetic backup. Check our KDF against
  libsignal's published test constants.
- **Manual check by the maintainer (local only, never committed):** run
  the spike against one real Android, one iOS and one Desktop backup.
  Record only pass/fail, version numbers and counts in the ADR.
- Measure: decrypt throughput and memory on a synthetic 1M-message backup.
  Choose the in-memory index vs re-streaming strategy.
- **Exit:** ADR "Signal backup decoding" (verified layout, versions seen,
  any deviations from this draft) and final license decision (A/B/C).

### M2 – Core model + WhatsApp importer + CLI

- `cv-model`, `cv-core` (registry, probe, SourceReport, redacting logs).
- `cv-import-whatsapp`: container detection, header-style inference, date
  order inference, multi-line handling, attachment matching by ZIP
  entries, omitted media, system-message tables (EN, DE first; table
  format allows more locales), limitation reporting.
- `cv-export`: TXT, Markdown, JSON.
- `cv-cli`: `chatviewer info <input>`,
  `chatviewer export <input> --chat <id> --format txt|md|json|cva -o <out>`.
- **Exit:** the variant matrix (§3.2) passes, plus golden outputs.

### M3 – Signal importer (full) + CVA format

- Map all Signal frame types to the model (`Unsupported` placeholders for
  the rest, with counts in the report), media decryption via lazy blobs,
  and version/limitation reporting.
- `docs/formats/cva.md` spec v1.0, CVA writer (`cv-export`) and reader
  (`cv-import-cva`).
- CLI: `chatviewer export <signal-dir> --recovery-key-stdin --chat <id> --format cva`.
  The key is never accepted as a plain argument, because it would end up
  in shell history.
- **Exit:** Signal → CVA → model round-trip is lossless for all
  supported features. WhatsApp → CVA → model round-trip passes too.

### M4 – GUI MVP (Tauri)

- Open dialog + drag & drop, recovery-key dialog (masked, paste,
  formatted in groups of 4, key zeroized after use), and a picker when
  the format is ambiguous.
- Chat list, virtualized message list, bubbles (in/out, grouping, day
  separators), system events, images and video, quotes, reactions, edited
  and deleted states.
- **Source info panel:** app, platform, variant/version, limitations and
  warnings. Version-incompatibility banner.
- Light/dark/auto theme. Error dialog with **Copy log**.
- Export dialog (TXT/MD/JSON/CVA).
- **Exit:** opens all fixtures. 100k-message synthetic chat scrolls
  smoothly (target: 60 fps, <300 MB RAM).

### M5 – Media polish, packaging, release

- Voice notes with waveform (Signal waveform, computed for WhatsApp),
  Opus decoding path, stickers (WebP, animated), documents, contact cards,
  locations (static, no map tiles because there is no network), polls.
- Packaging: macOS `.dmg`, Windows NSIS (+ "add CLI to PATH"), Linux
  `.deb`/`.rpm`/AppImage, all with the CLI bundled per architecture §9.
  Release workflow on tags, unsigned, with checksums.
- **Exit:** a release-candidate artifact for all three OSes, install and
  smoke tests pass on CI runners.

Possible later work (not planned): search, more locales, more importers
(Telegram, Threema, iMessage), print/PDF export.

---

## 3. Test strategy (synthetic data only)

### 3.1 Fixture generator (`tools/fixture-gen`)

- Deterministic (seeded) Rust binary. Generates names from a fictional
  word list, phone numbers from reserved fictional ranges (e.g. `+1 555
  01xx`, UK Ofcom drama ranges), and media as tiny generated
  JPEG/PNG/WebP/OGG/M4A files.
- **WhatsApp matrix**, generated from a variant table:
  `{ios, android, ios-legacy} × {en, de, fr, es, ar, fa, ja, th…} ×
  {12h, 24h} × {DMY, MDY, YMD} × {2/4-digit year} × {with/without seconds}
  × {LRM marks, U+202F, CRLF, BOM}`. Each variant is a `.zip` or `.txt`
  containing multi-line messages, every attachment type, omitted media,
  captions, edited/deleted messages, polls, locations and system events.
- **Signal:** synthetic backup folders (writer from M1). Variants: with
  and without media, missing `localKey`, `metadata.version = 2` (must be
  rejected), wrong key, truncated `main`, flipped HMAC byte, a forward-
  secrecy header, two snapshots plus a `.tmp` dir, renamed root folder.
- Small fixtures are committed under `fixtures/` with a `GENERATED` marker.
  Large ones (1M messages) are generated in CI on demand.

### 3.2 Test types

| Layer | Tests |
|---|---|
| WhatsApp parser | unit tests per variant fixture (expected model as JSON snapshot via `insta`), date-order inference tests, property tests (`proptest`): random messages → render in variant X → parse → equal model |
| Signal crypto | KDF test vectors (our own plus libsignal's published constants), negative tests (wrong key, bad HMAC, truncation, oversize frame), attachment padding/hash checks |
| Signal mapping | frame → model snapshot tests, unknown-frame tolerance |
| Exporters | golden files for TXT/MD/JSON; **round-trip**: model → CVA → model equality, model → JSON → model equality |
| Robustness | `cargo-fuzz` targets for the WhatsApp line parser, ZIP probing, Signal frame decoder and CVA reader (nightly CI job, short runs on PRs) |
| Privacy | log-redaction test (fixture marker strings must not appear in captured logs). A "no writes" test runs the CLI `info` in a sandboxed temp HOME and asserts that no files were created. |
| GUI | component tests (Vitest), Playwright/WebDriver smoke test via `tauri-driver` on Linux/Windows (opens fixtures, checks the banner and the bubble count) |
| Performance | criterion benchmarks: parse 100k WhatsApp lines, decrypt 1M Signal frames. Tracked but non-blocking. |

### 3.3 CI

- Matrix: ubuntu-latest, macos-latest (arm64), windows-latest.
- Jobs: fmt, clippy `-D warnings`, test, cargo-deny (licenses + advisories),
  gitleaks, frontend lint/test, and libsignal-validator cross-check
  (Linux only, M1+).
- Release job (M5): `tauri build` per OS, artifacts attached to a draft
  GitHub release.

---

## 4. Open questions for you

1. **License:** do you accept Option B (MIT repo, AGPL Signal module, AGPL
   release binaries)? Or should we pursue Option C (MIT everywhere) with
   a legal review?
2. **Frontend framework:** Svelte (recommended) or React?
3. **CVA:** should the messages always live in `manifest.json`, or move to
   `messages.jsonl` above a size threshold (recommended)?
4. **Minimum OS versions:** proposed macOS 12+, Windows 10+, and Linux
   with WebKitGTK 4.1 (Ubuntu 22.04+).
5. **Locales:** which system-message languages besides EN/DE matter most
   to you for M2?
6. **Real-data checks in M1:** can you run the spike locally against your
   own Signal backups (Android/iOS/Desktop), so that only anonymized
   pass/fail results are recorded?
