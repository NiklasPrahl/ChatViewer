# 0007 – WhatsApp import heuristics

- Status: Accepted (iOS export checked; Android pending)
- Date: 2026-11-02

## Context

WhatsApp exports have no specification and change across app versions,
platforms and languages (architecture §3). The importer has to decide
several things the file does not state explicitly.

## Decision

- **Inputs:** `.zip` (iOS `_chat.txt`, Android `WhatsApp Chat with/mit
  <name>.txt`), a bare `.txt`, or an extracted folder. Media are read
  into memory on demand, never extracted to disk (ADR 0003).
- **Line headers:** iOS `[date, time] …` and Android `date, time - …`.
  The rules:
  - Separators `.`, `/` and `-` are accepted.
  - Years have 2 or 4 digits; seconds are optional.
  - AM/PM may use a narrow no-break space or the forms `a.m.`/`p.m.`.
  - Leading direction marks are ignored.
  - Arabic-Indic, Persian and Devanagari digits are read as ASCII digits.
  - Lines without a header continue the previous message.
- **Date order:** chosen from the whole file:
  1. A 4-digit first field means YMD.
  2. Otherwise a field above 12 decides between DMY and MDY.
  3. Otherwise the order that keeps the times increasing wins.
  4. As a last resort, a 12-hour clock suggests MDY and DMY is the default.

  Cases 3 and 4 lower the confidence. Case 4 also adds a warning, and the
  user can override the order (`--date-order`, later in the GUI).
- **Author vs. system line (Android):** a line is a system message if its
  text matches a known system phrase *and* the text before the first
  `": "` is not a frequent author (at least two messages). Otherwise it
  is a message. `Name:` with nothing after it is a message without text.
  Other lines without `": "` are system messages.
- **System lines (iOS):** these are written as `<chat name>: ‎<text>`.
  The author before the colon is then the group name, or the contact's
  name in a one-to-one chat. That name is used as the chat title.
- **Attachments:**
  - iOS puts a caption *before* the marker on the same line
    (`Caption ‎<Anhang: file>`). Documents carry `name.pdf • ‎12 Seiten`
    there instead, which is metadata, not a caption.
  - The explicit markers `<attached: …>` / `<Anhang: …>` and
    `… (file attached)` / `(Datei angehängt)` always mean an attachment.
  - Other `name.ext (…)` forms count only if the file is in the archive
    or has a WhatsApp media name.
  - Attachments without their file are marked `Missing`. Placeholders
    such as `<Media omitted>` or `image omitted` are marked `Omitted`.
- **Owner:** WhatsApp does not mark the owner. "You deleted this message"
  identifies the owner when it occurs for exactly one author. `--me`
  overrides this. Otherwise the owner stays `Unknown` and the report
  lists this as a limitation.
- **Lossless:** every message keeps its exact original lines as a raw
  record (ADR 0006). Anything not recognised stays text or a generic
  system event; nothing is dropped.
- **Languages:** English and German phrase tables. Further languages
  only add table rows. The detected language is part of the variant id,
  e.g. `whatsapp-ios-24h-dmy-de`.

## Verification

- 30 synthetic variants (iOS/Android × EN/DE × 12/24 h × DMY/MDY/YMD ×
  2/4-digit years, with CRLF, BOM and narrow no-break spaces mixed in)
  are generated with known ground truth and must import field by field
  correctly.
- Hand-written lines in shapes the generator does not produce (old iOS
  markers, `a.m.`, unknown system lines, emoji names, preamble lines).
- Real export (maintainer, local only, 2026-11-02, content-free checks):

  | Source | Variant | Result |
  |---|---|---|
  | iOS, one-to-one chat, ≈2.5k messages, ≈270 media | `whatsapp-ios-24h-dmy-de` | **pass**: confidence 100, no warnings, owner detected, all media files assigned, all app-generated lines classified |
  | Android | | pending |

  The first run found three gaps, now fixed and covered by tests: the
  caption before the iOS marker (36 media were missed), a message
  without text taken for a system line, and `Verpasster
  Gruppen-Videoanruf`.

## Consequences

- Phrase tables are best effort. Misclassified lines remain readable,
  and their raw text is preserved.
- A one-message author whose text looks like a system phrase may be
  classified as a system line on Android.
