# 0006 – Lossless import: keep every piece of source data

- Status: Accepted
- Date: 2026-11-02

## Context

Users open archives to see and keep their chats. Data that an importer
drops is lost for good once the archive is exported to CVA. Signal
backups in particular carry much more than text: reactions, quotes, edit
history, stickers with emoji, link previews, voice-note waveforms, polls,
group changes and chat styles. The first real backup (ADR 0005) also
showed media that differs from its recorded hash (stickers).

## Decision

1. **Map everything we can to the neutral model.** Importers map every
   source field that has a meaning in the model (architecture §6), not
   only what the UI shows today.
2. **Keep the rest, never drop it.** Every message, participant and
   conversation carries the source-native record it came from
   (`SourceExtra`). For Signal, that is the frame's protobuf bytes. CVA
   stores it (base64), so a later version of ChatViewer can map fields
   that today's version ignores. Unknown frames and items become visible
   `Unsupported` placeholders with their raw data, never silent gaps.
3. **Account for every file.** Each importer can enumerate all media
   references (Signal: `walk::for_each_media`, covering attachments,
   voice messages, GIFs, long texts, link previews, quote thumbnails,
   contact avatars, stickers, view-once media, story replies, wallpapers
   and edit history). The import report shows referenced vs. available
   files.
4. **Detect schema drift.** prost drops protobuf fields it does not
   know. The Signal importer compares each frame's size with its
   re-encoded size. A difference is reported in the source report as
   "data from a newer Signal version not understood", and the raw frame
   is kept per rule 2. The check can also fire on valid but
   non-canonical encodings, so it is a hint, not an error.
5. **Degrade per item, with the best available fallback**, and show it:

   | Situation | Rendering |
   |---|---|
   | Media verified | normal |
   | Media authentic but differs from recorded size/hash (`AuthenticatedOnly`) | normal; noted in the source report (for stickers no per-item badge, since this is known Desktop behaviour) |
   | Sticker without usable image (not in backup, HMAC fails, undecodable) | the sticker's emoji, large, with a small "Sticker" label; without emoji a generic sticker placeholder |
   | Other media not in backup | placeholder with type, file name, size, dimensions, duration, caption and blurhash preview where available |
   | Quote target missing | the quote's own stored text and author |

6. **Exports state their losses.** TXT and Markdown are lossy by nature
   and say so in their header. JSON and CVA are lossless (rule 2). The
   round-trip test source → CVA → model must preserve the raw records
   byte for byte.

## Consequences

- CVA files get larger because they include raw records. This is
  acceptable; media dominates the size anyway.
- The model needs a `Sticker { emoji, pack_id, sticker_id, image }`
  content type and `SourceExtra` on all entities (architecture §6).
- `walk.rs` must be extended whenever Signal's schema adds a
  `FilePointer`. The spike's coverage check ("listed but not referenced")
  catches misses on real backups.

## Alternatives considered

Mapping only what the UI renders (simpler, but loses data permanently on
export), and a full generic protobuf-to-JSON dump instead of a model
(lossless, but the UI would have to understand every source format, which
contradicts the plugin architecture).
