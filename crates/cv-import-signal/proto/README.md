# Vendored Signal schemas

These files are copied unmodified from
[signalapp/libsignal](https://github.com/signalapp/libsignal), tag `v0.106.1`:

| File | Upstream path |
|---|---|
| `backup.proto` | `rust/message-backup/src/proto/backup.proto` |
| `LocalBackup.proto` | `rust/message-backup/src/proto/LocalBackup.proto` |

Copyright Signal Messenger, LLC. Licensed under the GNU Affero General
Public License v3.0 only (AGPL-3.0-only). This is why the
`cv-import-signal` crate is AGPL-3.0-only (see `docs/adr/0001-license.md`).

To update, copy the files from a newer libsignal tag, update the tag above,
and run the tests.
