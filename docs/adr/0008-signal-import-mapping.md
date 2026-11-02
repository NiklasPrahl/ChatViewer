# 0008 – Signal import: mapping to the neutral model

- Status: Accepted
- Date: 2026-11-02

## Context

M1 proved that a Signal local backup can be decrypted (ADR 0005). M3
maps it to the neutral model, as losslessly as possible (ADR 0006).

## Decision

- **Passes:** opening runs one index pass (HMAC check, recipients, chats,
  per-chat counts and date range, media list). `load` decodes the
  requested chats in one further pass; `load_all` decodes every chat in a
  single pass. Media are decrypted only when opened, in memory.
- **The key** is passed as a `Secret` (zeroized, never printed). It is
  kept in memory only while the archive is open.
- **Ids:**
  - Participants are `r<recipient id>`.
  - Chats are `c<chat id>`.
  - Messages are `<author id>-<sent timestamp>`, which is how Signal
    quotes refer to them, so quote targets resolve.
- **Names:** a nickname comes first, then the address-book name, the
  profile name, the username, and finally the phone number. The owner's
  name comes from `AccountData`.
- **Content:**
  - Text keeps its formatting ranges and mentions. Signal's UTF-16
    offsets are converted to byte offsets.
  - Attachments keep type, MIME, name, size, dimensions, duration,
    waveform and blurhash. Their availability is `Available`, `Missing`
    (key present, file absent) or `NotInBackup`.
  - Stickers keep their emoji, pack and sticker id.
  - Contact cards become a contact attachment with the name, numbers and
    e-mail addresses as caption.
  - Remote and admin deletions become `Deleted`.
  - View-once messages set a flag. Story replies become text.
  - Polls keep options and voter counts.
  - Payments and gift badges become `Unsupported` placeholders.
- **System events:** calls, safety-number changes, disappearing-message
  timers and group changes become `SystemEvent`s with English text. The
  common group changes are typed (created, name, avatar, description,
  member added, joined, left, removed). The rest are `Other`, with a
  readable name derived from the update type, so new update types never
  break the import.
- **Extra fields** (namespaced `signal.*`): link previews (with image
  attachments), long-text blob, number of revisions, expiry, SMS flag,
  direction, poll settings, unresolved quote target. Each message, chat
  and participant also keeps its raw protobuf frame.
- **Not mapped to chats:** account settings, sticker packs, chat folders,
  notification profiles and call history. They stay out of per-chat
  exports, and the report says so.

## Verification

- Synthetic backups: mapping tests and a lossless round trip through CVA,
  including media bytes. They are cross-checked against libsignal's
  validator in CI.
- Real Desktop 8.33.0 backup (maintainer, local, content-free
  `chatviewer info --stats`, 2026-11-02):
  - Every chat item is mapped (≈135k); none is `Unsupported`.
  - No local media file is missing.
  - ≈99 % of quotes resolve to their target. The rest point to messages
    that are no longer in the backup.
  - ≈4 % of system events remain `Other` (calls, invite links, admin
    changes).

## Consequences

- Front ends can show every message, including placeholders, without
  knowing Signal.
- Opening a chat costs one pass over the backup (≈0.25 s for a 9 MB
  backup). This is acceptable for M4. Caching decoded chats in memory is
  possible later.
