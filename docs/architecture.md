# ChatViewer – Architecture

Status: **Approved 2026-11-02.** Open questions are in plan.md §4. Decisions are
recorded as ADRs in `docs/adr/`.
Research date: 2026-11-02. Sources are listed in section 10.

---

## 1. Goals and non-goals

**Goals**

- Cross-platform desktop viewer (Windows, macOS, Linux) for exported chat
  archives. It renders them like a messenger: bubbles, media, emoji, voice
  notes, reactions, quotes and system events.
- Inputs:
  - WhatsApp chat export (`.zip` or `.txt`, iOS and Android, many years
    of format variants).
  - Signal on-device ("local") backup in the new cross-platform format
    (iOS, Android, Desktop), unlocked with the 64-character recovery key.
  - The project's own archive format, "ChatViewer Archive" (CVA, see §6).
- Outputs: TXT, Markdown, JSON, and CVA ZIP (per chat, with media).
- A GUI and a CLI that share one core library.
- Light, dark and auto (follow OS) theme.
- Visible diagnostics: the detected source, format version and known
  limitations are shown in the GUI.

**Non-goals (v1)**

- Not a messenger: no sending, syncing or network access.
- No persistence: chats, media and keys are never written to disk except
  in files the user explicitly exports.
- No Signal *remote* (server-hosted) backups. That needs network access
  and server credentials.
- No legacy Signal Android `.backup` files (30-digit passphrase, pre-2025
  format). They are detected and the GUI explains the limitation (see §3.6).
- No WhatsApp encrypted databases (`msgstore.db.crypt14/15`, iOS
  `ChatStorage.sqlite`). Possible later as plugins.
- No editing of chats.

---

## 2. Research summary: Signal local backup (task A)

Everything below was checked against Signal's own source code on GitHub
(`signalapp/libsignal`, `signalapp/Signal-Android`, `signalapp/Signal-Desktop`,
`main` branches as of 2026-11-02, libsignal v0.106.1). Signal publishes **no
separate written specification**. The source code is the spec, and all of it
is AGPL-3.0-only.

### 2.1 Context

- In 2025/2026 Signal moved Android on-device backups to the cross-platform
  "backup v2" format (`backup.proto`). This is the same format used for
  Signal Secure Backups. iOS and Desktop gained on-device backups in the
  same format (press coverage of Signal 8.30, Oct 2026).
- The user unlocks a backup with the **recovery key = Account Entropy Pool
  (AEP)**: 64 characters from `[0-9a-z]`. The same key is used for
  Secure Backups. It cannot be reset.

### 2.2 Folder layout (identical on Android and Desktop)

```
SignalBackups/                        # name may differ; detected by structure
├── files/                            # shared, deduplicated media store
│   ├── 00/ … ff/                     # 256 shard dirs = first 2 hex chars of name
│   │   └── <64 hex chars>            # encrypted attachment
├── signal-backup-YYYY-MM-DD-HH-mm-ss/  # one snapshot (UTC timestamp); Android keeps 2
│   ├── main                          # encrypted message backup
│   ├── metadata                      # LocalBackup.proto Metadata (unencrypted wrapper)
│   └── files                         # list of media names used by this snapshot
└── (*.tmp snapshot dirs = incomplete, ignore)
```

The media file name is `hex(SHA-256(plaintextHash ‖ localKey))`, sharded by
its first two hex characters. Source: `MediaName.forLocalBackupFilename`.

### 2.3 Key derivation (all HKDF-SHA256)

| Step | Input key material | Salt | Info | Output |
|---|---|---|---|---|
| BackupKey | AEP as 64 ASCII bytes | none | `"20240801_SIGNAL_BACKUP_KEY"` | 32 B |
| Local metadata key | BackupKey | none | `"20241011_SIGNAL_LOCAL_BACKUP_METADATA_KEY"` | 32 B |
| BackupId | BackupKey | none | `"20241024_SIGNAL_BACKUP_ID:"` ‖ ACI service-id binary | 16 B* |
| MessageBackupKey (local) | BackupKey | none | `"20241007_SIGNAL_BACKUP_ENCRYPT_MESSAGE_BACKUP:"` ‖ BackupId | 64 B → HMAC key (32) ‖ AES key (32) |

\* BackupId length is taken from libsignal's `BackupId::LEN`. The spike
must confirm it.

Problem: the BackupId depends on the account's ACI (a UUID), which the user
does not know. **The local `metadata` file solves this:** it holds the
BackupId encrypted with AES-256-CTR (12-byte IV) under the local metadata
key. So the 64-character recovery key alone is enough to decrypt. There is
no check value on this field: a wrong key only shows up when the HMAC of
`main` fails. The UI should say "wrong recovery key or damaged backup".

Remote backups add a "forward secrecy token" (salt and a newer info string
`20250708_…`, with an `SBACKUP\x01` magic header). Local backups use
`createForLocalOrLinking` with **no token**, so they use the legacy derivation
above. The reader should still accept the magic header, as libsignal does.

### 2.4 `main` file encryption

```
[optional: "SBACKUP\x01" + varint-len + MetadataPb]   # remote/forward-secrecy only
IV (16 B) ‖ AES-256-CBC(PKCS7)( gzip( stream ) ) ‖ HMAC-SHA256 (32 B)
HMAC covers everything before it (incl. IV). Verify before trusting plaintext.
stream = varint-len ‖ BackupInfo  ‖ { varint-len ‖ Frame }*
```

- `BackupInfo`: `version`, `backupTimeMs`, `mediaRootBackupKey`,
  `currentAppVersion`, `firstAppVersion`. We show these in the GUI.
- `Frame` is a oneof: AccountData (always first), Recipient, Chat,
  ChatItem (in render order), sticker packs, calls, notification
  profiles, chat folders. Unknown or unset frames must be skipped
  (comment in `backup.proto`).
- Android caps a single frame at 25 MiB (`MAX_FRAME_LENGTH`). We use the
  same cap as a defensive limit.
- The HMAC covers the whole file, so it has to be computed in a streaming
  first pass (libsignal does the same) before or alongside decryption.

### 2.5 Media (attachment) encryption

- Each `FilePointer.LocatorInfo` has `localKey` (64 B = AES-256 key ‖
  HMAC key), `plaintextHash` (SHA-256) and `size` (plaintext length).
- File format: `IV (16) ‖ AES-256-CBC(PKCS7)(plaintext ‖ zero padding) ‖ HMAC-SHA256 (32)`
  (Android `AttachmentCipherOutputStream` + `PaddingInputStream`).
  Padding uses Signal's bucket formula: `max(541, 1.05^ceil(log_1.05(size)))`.
  Truncate to `size` after decryption and check `plaintextHash`.
- A missing `localKey` means the media was not on the device when the backup
  was made. Show a placeholder ("media not included in backup").
- `contentType`, `fileName`, `width`/`height`, `caption`, `blurHash`,
  `audioWaveform` (≤100 bytes) and `audioDurationSeconds` feed the UI
  directly.

### 2.6 Version handling

- `metadata.version`: Android refuses anything above `1`. We do the same:
  show "backup created by a newer Signal version (metadata v N); this
  viewer supports v1".
- `BackupInfo.version` and `currentAppVersion` are displayed. libsignal
  does not enforce a value, so we only warn when the version is above the
  one we tested.
- The proto schema evolves (fields like polls, pinned messages and admin
  deletes are recent). Our decoder keeps unknown fields and frames and
  reports them as "N unsupported items" instead of failing.

### 2.7 Licensing situation (input to task D)

- libsignal, Signal-Android, Signal-iOS and Signal-Desktop are all
  **AGPL-3.0-only** (Signal Messenger LLC). That includes `backup.proto`
  and `LocalBackup.proto`.
- libsignal's README says "Use outside of Signal is unsupported". There
  is no crates.io release (`libsignal-message-backup` does not exist on
  crates.io), so it can only be used as a git dependency with Signal's
  pinned toolchain and a large dependency tree.
- If our distributed binary links libsignal, the combined work must be
  distributed under AGPL-3.0. Our own MIT-licensed files stay MIT, but the
  shipped app as a whole is AGPL. Because the app has no network service,
  AGPL's network clause (§13) has no practical effect. What remains is
  GPL-style copyleft on distribution.
- **Clean-room alternative.** The crypto is standard (HKDF, AES-CBC/CTR,
  HMAC, gzip, protobuf). Reimplementing it is about 500 lines. The open
  question is the protobuf **schema**:
  - Copying `backup.proto` copies an AGPL file (including its comments).
  - Hand-writing a minimal schema that contains only field numbers, types
    and our own names is closer to reproducing a data format. In the EU,
    the CJEU's *SAS Institute v WPL* (C-406/10) holds that data file
    formats as such are not protected by software copyright, and the
    Software Directive allows interoperability work. This is still a
    legal judgement, **not settled by this document. It is not legal
    advice.**
- See `plan.md` §1 for the license recommendation.

---

## 3. Research summary: WhatsApp exports (task B)

WhatsApp publishes **no format specification**. Its help center only
describes the export flow (the page renders client-side and could not be
fetched verbatim on the research date). The findings below come from
widely used open-source parsers and their fixtures (e.g.
`Pustur/whatsapp-chat-parser`, MIT) and from documented community
observations. **Each variant must be confirmed with a test fixture before
we rely on it.** Real exports are only inspected locally and never
committed.

### 3.1 Container

| Origin | Container | Chat file name | Media names |
|---|---|---|---|
| iOS | always `.zip` | `_chat.txt` | `00000042-PHOTO-2020-06-07-15-13-20.jpg`, `-AUDIO-`, `-VIDEO-`, `-STICKER-`, `.vcf`, `.pdf` with original names |
| Android (with media) | `.zip` (or several shared files on older versions) | `WhatsApp Chat with <name>.txt` (localized, e.g. `WhatsApp Chat mit <name>.txt`) | `IMG-20210428-WA0001.jpg`, `VID-…`, `PTT-…opus`, `AUD-…`, `STK-…webp`, `DOC-…` |
| Android (no media) | `.txt` only (sometimes zipped by the share target) | as above | – |
| Older iOS (~2015–2017) | `.zip` | `_chat.txt` | `2015-08-04-PHOTO-00004762.jpg` |

Export limits reported by third parties (not confirmed on the official page):
**40,000 messages without media, 10,000 with media**, keeping the newest
messages. Media outside the window shows up as "omitted". The GUI should
warn when a chat looks truncated, i.e. the first message falls exactly on
a cap boundary or there is no "encryption notice" system message.

### 3.2 Line grammar

```
Android:  DD/MM/YYYY, HH:mm - Author: Text
          3/6/18, 1:55 p.m. - Author: Text
iOS:      [DD.MM.YY, HH:mm:ss] Author: Text
          ‎[06.03.17, 00:45:12] Author: ‎<attached: 00000042-PHOTO-….jpg>
System:   same prefix with no "Author:" part (Android: "dd/mm/yyyy, hh:mm - Text")
```

Variants to support:

- **Date order** is DMY, MDY or YMD depending on locale. It cannot be
  determined from a single line. Infer it from the whole file: any
  field > 12, monotonic ordering, then the user locale as a tie-breaker.
  The user can override it in the GUI.
- **Separators** `/`, `.`, `-`, optionally with spaces. **Years** have 2 or
  4 digits. **Seconds** are present on iOS and mostly absent on Android.
- **12-hour clock** with `AM/PM`, `am/pm`, `a.m./p.m.` and localized forms
  (`vorm./nachm.`, `午前/午後`, `ص/م`, …). Newer iOS uses **U+202F
  NARROW NO-BREAK SPACE** before AM/PM, and U+00A0 also appears.
- **Directional marks** U+200E/U+200F at the start of lines and before
  attachment markers. Strip them for matching, keep them for RTL display.
- **Non-Latin digits** (Arabic-Indic, Persian, Devanagari) and
  non-Gregorian calendars (Thai Buddhist year +543). Normalize digits and
  flag unusual year ranges.
- **Multi-line messages**: any line that does not match the header pattern
  continues the previous message. A message body can itself start with
  something that looks like a date, so the header regex must be strict.
  Use the prefix style inferred for the file.
- **No time zone** in the file. Timestamps are local wall-clock time of the
  exporting phone, stored as "naive local" in our model.
- **Encoding**: UTF-8, sometimes with BOM, CRLF or LF line endings.

### 3.3 Attachment markers

| Variant | Example |
|---|---|
| iOS current | `<attached: 00000042-PHOTO-2020-06-07-15-13-20.jpg>` (localized: `<Anhang: …>`) |
| iOS old | `2015-08-04-PHOTO-00004762.jpg <‎attached>` |
| Android | `IMG-20210428-WA0001.jpg (file attached)` (localized: `(Datei angehängt)`) |
| Omitted | `<Media omitted>` / `<Medien ausgeschlossen>`, `image omitted`, `audio omitted`, `sticker omitted`, `GIF omitted`, … |
| Caption | Android: caption on the line(s) after the marker. iOS: caption on a following line or a separate message |

Strategy: **locale-independent first**. If a token in the message matches a
file name that exists in the ZIP, it is an attachment, whatever the
surrounding words are. Localized marker tables (generated from WhatsApp's
own UI strings by community projects, re-verified by us) are only a
fallback for omitted media.

### 3.4 System messages and special content (localized)

Encryption notice, group created, added, removed, left, joined via link,
subject, icon or description changed, admin changes, number changed,
security code changed, disappearing messages on/off, missed voice/video
call, "This message was deleted", "You deleted this message",
`<This message was edited>`, view-once media, polls (`POLL:` / `OPTION: x
(n votes)` on Android), live/static location (`location: https://maps.google.com/?q=…`),
contact cards (`.vcf`), "null" bodies on some versions, and the business
account notice.

These are classified with per-locale phrase tables. When nothing matches,
the line is still shown as a generic system event.

### 3.5 What WhatsApp exports do not contain

Reactions, quote/reply links, edit history, read receipts, per-message
IDs, time zone, and the identity of "me" (inferred heuristically and
confirmable by the user). The GUI shows these as **limitations** of the
source.

### 3.6 Detection and reporting

Each parser returns a `SourceReport`:
`{app, platform (ios/android/unknown), variant id (e.g. "wa-ios-bracket-24h-v2"),
date_order, confidence, warnings[], limitations[], unknown_lines_count}`.
The GUI shows it as an info banner/panel. Low confidence or override
suggestions appear as a non-blocking warning.

---

## 4. Tech stack (task C)

### 4.1 Recommendation: Tauri 2 + Rust core + TypeScript UI

| Option | Pros | Cons |
|---|---|---|
| **Tauri 2 + Rust core** (recommended) | Small binaries (~10–20 MB). Rust is a good fit for parsing, crypto (RustCrypto crates) and streaming. A single core serves both GUI and CLI. libsignal is Rust if we ever link it. `externalBin` supports shipping a CLI. Custom URI protocols can stream media from memory without temp files. | Uses the system WebView (WebKitGTK on Linux, WebView2 on Windows, WKWebView on macOS), so rendering differences need QA. Media codec support varies by WebView (Opus/OGG on WKWebView, see §4.3). |
| Electron + TS/Node core | Uniform Chromium, broad codec support, large ecosystem. | ~100 MB+ bundles. Crypto and parsing in JS are slower. A native module is needed for good streaming. Larger attack surface. |
| Flutter + Dart (or Rust via FFI) | Consistent rendering, good list virtualization. | Desktop maturity is mixed, text/emoji rendering on Linux is weaker, and the Rust core would need FFI anyway. |
| Qt/QML + Rust/C++ | Native look, mature. | Licensing (LGPL/commercial) and build complexity. Chat UI is more work in QML. |
| Pure Rust GUI (egui, iced, Slint) | One language. | Rich text, emoji, bidi and media playback are still immature for a messenger-quality UI. |

**UI layer:** TypeScript with **Svelte 5** (small runtime, simple state)
and a virtualized list (e.g. `@tanstack/virtual`) for chats with more
than 100k messages. React is an equally valid choice if preferred. The
decision gets its own ADR.

Emoji: render with the system emoji font by default. Optionally bundle
**Twemoji** (graphics CC-BY 4.0, attribution required) for consistent
look on Linux/Windows. That is a licensing note for the ADR.

### 4.2 Data flow and memory model

```
Input (zip/folder) ──► Importer plugin ──► ChatArchive (lazy)
                                             ├─ conversations()   (index, small)
                                             ├─ messages(conv, range) (streamed pages)
                                             └─ blob(attachment_id) → Read + Seek (lazy, decrypted on demand)
GUI: Tauri commands fetch pages by index; media via `cv://blob/<id>` custom protocol
     (supports Range requests for audio/video seeking).
CLI: same ChatArchive API → exporters.
```

- **WhatsApp**: read the ZIP central directory, then a single streaming
  pass over the chat text that builds a compact index (byte offsets per
  message). Pages are parsed on demand.
- **Signal**: verify the `main` HMAC (streaming), then a decrypt + gunzip
  + frame pass. Recipients, chats and a per-chat ChatItem offset index go
  into memory, using the decompressed-stream position as the offset.
  Two options for random access:
  (a) keep decoded ChatItems for the opened chat in memory, or
  (b) re-stream.
  The spike measures this on a large synthetic backup (e.g. 1M messages).
- **No persistence**: no caches on disk and no temp files for decrypted
  media. If a WebView needs a file path for some codec, that is an
  explicit ADR exception with wipe-on-exit. Keys are held in
  `zeroize`/`secrecy` wrappers and dropped when the archive is closed.

### 4.3 Media playback

- Images (JPEG/PNG/WebP/GIF/HEIC*), video (MP4/H.264), audio (AAC/M4A,
  MP3, **Opus in OGG**: WhatsApp voice notes; Signal uses AAC/M4A).
- Opus/OGG is not supported natively by WKWebView (macOS), and WebKitGTK
  depends on GStreamer plugins. Plan: decode Opus in Rust
  (e.g. `symphonia` + `opus` decoder, license check required) to PCM/WAV
  on the fly through the custom protocol. This gets verified in the
  milestone where audio lands.
- \*HEIC: support depends on the WebView. Fall back to a Rust decoder or
  show a placeholder with "open externally" (export the single file).

### 4.4 Logging and errors

- `tracing` with a redaction layer. Message bodies, names, phone numbers,
  file names and keys are never logged. Only counts, ids (hashed), byte
  offsets, variant ids and error kinds are.
- An in-memory ring buffer feeds the **"Copy log"** button that every
  error dialog has.
- A unit test asserts that synthetic marker strings from fixtures never
  appear in logs.

---

## 5. Module layout (Cargo workspace)

```
chatviewer/
├── Cargo.toml                   # workspace
├── crates/
│   ├── cv-model/                # neutral chat model (pure data, serde)
│   ├── cv-core/                 # ChatArchive trait, importer/exporter registry,
│   │                            # input probing, blob abstraction, SourceReport,
│   │                            # redacting log layer, error types
│   ├── cv-import-whatsapp/      # WhatsApp txt/zip parser (+ locale tables)
│   ├── cv-import-signal/        # Signal local backup: kdf, decrypt, proto, mapping
│   │                            # (license per ADR, see §8)
│   ├── cv-import-cva/           # reader for our own archive format
│   ├── cv-export/               # txt, md, json, cva writers
│   └── cv-cli/                  # `chatviewer` binary (clap)
├── app/
│   ├── src-tauri/               # Tauri shell: commands, cv:// protocol, window/theme
│   └── ui/                      # Svelte/TS frontend
├── tools/
│   └── fixture-gen/             # synthetic data generator (Rust bin)
├── fixtures/                    # small committed synthetic fixtures (generated)
├── docs/ (architecture.md, plan.md, adr/, formats/cva.md)
└── .github/workflows/
```

Dependency rule: `cv-model` ← `cv-core` ← importers/exporters ← `cv-cli`,
`app`. The UI never imports parser crates. It only sees the model through
Tauri commands.

---

## 6. Neutral chat model (draft)

```rust
struct Archive { source: SourceReport, account: Option<AccountInfo>, conversations: Vec<ConversationSummary> }

struct ConversationSummary {
    id: ConvId, kind: ConvKind /* Direct | Group | NoteToSelf | Broadcast | Unknown */,
    title: String, participants: Vec<ParticipantId>, message_count: u64,
    first_ts: Option<Timestamp>, last_ts: Option<Timestamp>, avatar: Option<BlobRef>,
}

struct Participant { id: ParticipantId, display_name: String, is_self: SelfFlag /* Yes|No|Guessed */,
                     handle: Option<String> /* phone/username; never logged */, color_seed: u32 }

struct Message {
    id: MessageId,                // source id or synthetic stable id
    seq: u64,                     // render order within the conversation
    timestamp: Timestamp,         // enum { Utc(ms), NaiveLocal(datetime) }
    author: Author,               // Participant(id) | System
    content: Content,
    quote: Option<Quote>,
    reactions: Vec<Reaction>,
    edits: Vec<Edit>,             // previous versions (Signal) or flag-only (WhatsApp)
    flags: MessageFlags,          // forwarded, view_once, expiring, deleted, starred …
    delivery: Option<DeliveryInfo>,
    extra: SourceExtra,           // namespaced, opaque key/values for lossless round-trip
}

enum Content {
    Text(RichText),                              // text + ranges (bold, italic, strike, mono, spoiler, mention, link)
    Media { attachments: Vec<Attachment>, caption: Option<RichText> },
    Sticker(Attachment), Contact(ContactCard), Location(Location),
    Poll(Poll), Call(CallEvent), Payment(PaymentStub), GiftBadge(Stub),
    System(SystemEvent),                          // typed kind + actors + fallback text
    Deleted { by: Option<ParticipantId> },
    Unsupported { source_kind: String, note: String }, // visible placeholder, never silently dropped
}

struct Attachment { id: AttachmentId, kind: MediaKind /* Image|Video|Audio|Voice|Gif|Document|Sticker */,
                    mime: Option<String>, file_name: Option<String>, size: Option<u64>,
                    width: Option<u32>, height: Option<u32>, duration_ms: Option<u64>,
                    waveform: Option<Vec<u8>>, blurhash: Option<String>,
                    availability: Available(BlobRef) | Omitted | Missing | NotInBackup }

struct Quote { target: Option<MessageId>, author: Option<ParticipantId>, snippet: RichText,
               attachment_hint: Option<MediaKind>, target_not_found: bool }
struct Reaction { emoji: String, author: ParticipantId, timestamp: Option<Timestamp> }
struct SystemEvent { kind: SystemKind, actors: Vec<ParticipantId>, text: String }
```

`Timestamp` keeps the distinction between WhatsApp's naive local time and
Signal's UTC epoch. The UI shows naive times as-is, without a zone
conversion.

---

## 7. Extensibility (new messengers)

- **Compile-time plugins.** Every importer implements:

  ```rust
  trait Importer: Send + Sync {
      fn id(&self) -> &'static str;                         // "whatsapp", "signal-local", "cva"
      fn probe(&self, input: &InputSource) -> Probe;        // NotMine | Maybe(confidence) | Needs(Secret::RecoveryKey)
      fn open(&self, input: InputSource, secrets: Secrets) -> Result<Box<dyn ChatArchive>, ImportError>;
      fn capabilities(&self) -> Capabilities;               // reactions, quotes, edits, media, tz…
  }
  trait ChatArchive { fn report(&self) -> &SourceReport; fn conversations(&self) -> …;
                      fn messages(&self, conv, range) -> …; fn open_blob(&self, id) -> … }
  ```

  They are registered in a static list in `cv-core`, behind Cargo features.
  There is no dynamic library loading (no ABI or security problems).
- `probe` runs over all importers. The highest confidence wins. Ambiguous
  results go to a picker in the GUI. Importers that need a secret make the
  GUI ask for it (the Signal recovery-key dialog).
- `capabilities()` drives what the UI shows as "not available in this
  source", so a new messenger needs no UI change.
- Exporters implement `Exporter { id, extension, write(&dyn ChatArchive, conv, sink, options) }`.
- Candidates for later importers: Telegram Desktop JSON export,
  Threema backup, iMessage (`chat.db`), Instagram/Facebook JSON.

---

## 8. Own archive format "CVA" (proposal; full spec → `docs/formats/cva.md`)

A ZIP file that third-party WhatsApp tools can still read, but lossless
for ChatViewer:

```
<chat-title>.cva.zip            (extension .zip; also detectable without .cva)
├── _chat.txt                   # WhatsApp-iOS-style lines, fixed ISO-like format:
│                               #   [2024-03-01 14:05:09] Alice: Hello
│                               #   [2024-03-01 14:05:30] Bob: <attached: media/IMG_0001.jpg>
├── manifest.json               # authoritative structured data (see below)
└── media/…                     # decrypted attachments, original names, collision-safe
```

`manifest.json`:

```json
{
  "format": "chatviewer-archive", "format_version": "1.0",
  "generator": {"name": "chatviewer", "version": "0.x"},
  "source": {"app": "signal", "platform": "android", "backup_version": 1, "app_version": "…"},
  "conversation": {"id": "…", "kind": "group", "title": "…"},
  "participants": [{"id": "p1", "name": "Alice", "is_self": true}],
  "timezone": "UTC | naive",
  "messages": [ { "id": "m1", "seq": 1, "line": 3, "ts": "…", "author": "p1",
                  "content": {…}, "quote": {"target": "m0"}, "reactions": [ … ], "edits": [ … ] } ],
  "attachments": [ {"id": "a1", "path": "media/IMG_0001.jpg", "sha256": "…", "mime": "image/jpeg"} ]
}
```

- The **manifest is authoritative**. `_chat.txt` is a readable rendering
  that is kept in sync (`line` index). If the manifest is missing or
  invalid, the viewer falls back to parsing `_chat.txt` as a WhatsApp
  export, with a warning.
- For very large chats the messages can move to `messages.jsonl` (one
  object per line, streamable) referenced from the manifest. Recommended
  as the default if more than 50k messages. Open question for review.
- Versioning is semver. Readers accept the same major version and
  preserve unknown fields.
- JSON export reuses the manifest schema without media
  (`"attachments[].path": null`).

---

## 9. CLI bundling per platform

One binary: `chatviewer` (crate `cv-cli`), shipped via Tauri `bundle.externalBin`:

| Platform | Location | PATH integration |
|---|---|---|
| macOS (.app in .dmg) | `ChatViewer.app/Contents/MacOS/chatviewer` | Menu item / docs: `ln -s …/Contents/MacOS/chatviewer /usr/local/bin/chatviewer` (no silent system changes) |
| Windows (NSIS/MSI) | next to `ChatViewer.exe` in install dir | Installer checkbox "Add CLI to PATH" (per-user PATH). A separate binary is needed because the GUI uses `windows_subsystem = "windows"` (no console). |
| Linux `.deb`/`.rpm` | `/usr/bin/chatviewer` + `/usr/bin/chatviewer-gui` | automatic |
| Linux AppImage | inside the image. `ChatViewer.AppImage cli …` dispatches via argv[1] | user symlink/alias |

All builds are unsigned. The release notes must document the Gatekeeper
(`xattr -dr com.apple.quarantine`) and SmartScreen steps.

---

## 10. Sources (primary first)

Signal (source code, `main`, fetched 2026-11-02):
- libsignal key derivation: https://github.com/signalapp/libsignal/blob/main/rust/account-keys/src/backup.rs
- libsignal message backup key: https://github.com/signalapp/libsignal/blob/main/rust/message-backup/src/key.rs
- libsignal framing (HMAC/AES-CBC/gzip): https://github.com/signalapp/libsignal/blob/main/rust/message-backup/src/frame.rs
- libsignal forward-secrecy header: https://github.com/signalapp/libsignal/blob/main/rust/message-backup/src/frame/forward_secrecy.rs
- Schemas: https://github.com/signalapp/libsignal/blob/main/rust/message-backup/src/proto/backup.proto, …/LocalBackup.proto
- AEP alphabet/length: https://github.com/signalapp/libsignal/blob/main/rust/account-keys/src/lib.rs
- Android folder layout: https://github.com/signalapp/Signal-Android/blob/main/app/src/main/java/org/thoughtcrime/securesms/backup/v2/local/ArchiveFileSystem.kt
- Android export/import: …/backup/v2/local/LocalArchiver.kt, `lib/archive/.../EncryptedBackupReader.kt`, `ArchivedFilesWriter.kt`
- Media name: https://github.com/signalapp/Signal-Android/blob/main/core/models-jvm/src/main/java/org/signal/core/models/backup/MediaName.kt
- Attachment cipher/padding: `lib/libsignal-service/.../AttachmentCipherOutputStream.kt`, `PaddingInputStream.java`
- Desktop layout: https://github.com/signalapp/Signal-Desktop/blob/main/ts/services/backups/util/localBackup.node.ts
- License: https://github.com/signalapp/libsignal/blob/main/LICENSE, README "Use outside of Signal is unsupported"

Signal (secondary, rollout context):
- https://www.bleepingcomputer.com/news/security/signal-adds-encypted-local-backup-support-to-ios-desktop-apps/
- https://cyberinsider.com/signal-brings-encrypted-local-backups-to-iphone-and-desktop/
- https://aboutsignal.com/news/signal-brings-cross-platform-on-device-backups-to-android-beta/

WhatsApp (no official spec):
- WhatsApp Help Center "How to export your chat history": https://faq.whatsapp.com/1180414079177245 (body not retrievable by tooling)
- Pustur/whatsapp-chat-parser (MIT), regex + tests: https://github.com/Pustur/whatsapp-chat-parser
- whats_json (locale tables from WhatsApp strings): https://pub.dev/packages/whats_json
- Export limits (third-party): https://fone.tips/whatsapp-export-more-than-40000-messages , https://forums.androidcentral.com/threads/how-to-export-over-40-000-messages-from-whatsapp.928056

Legal:
- CJEU C-406/10 *SAS Institute v World Programming*: https://curia.europa.eu/juris/liste.jsf?num=C-406/10
- AGPL-3.0 text: https://www.gnu.org/licenses/agpl-3.0.html
