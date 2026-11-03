ChatViewer is an offline viewer for exported WhatsApp chats and Signal
on-device backups. It shows chats like a messenger and exports them,
including one self-contained, decrypted ZIP per chat.

## Downloads

| System | File |
|---|---|
| macOS (Apple silicon) | `ChatViewer_<version>_aarch64.dmg` |
| Windows | `ChatViewer_<version>_x64-setup.exe` or `.msi` |
| Linux | `.deb`, `.rpm` or `.AppImage` |

Each file has a `.sha256` checksum next to it.

## Unsigned builds

These builds are not code-signed, so your system will warn you:

- **macOS:** after moving ChatViewer to Applications, run
  `xattr -dr com.apple.quarantine /Applications/ChatViewer.app` once, or
  right-click the app and choose *Open*.
- **Windows:** in the SmartScreen dialog choose *More info → Run anyway*.
- **Linux AppImage:** `chmod +x ChatViewer_*.AppImage`.

## Command line

The `chatviewer` CLI is included:

- **macOS:** `/Applications/ChatViewer.app/Contents/MacOS/chatviewer`. Put
  it on your PATH with
  `sudo ln -s /Applications/ChatViewer.app/Contents/MacOS/chatviewer /usr/local/bin/chatviewer`.
- **Windows:** `chatviewer.exe` in the installation folder.
- **Linux (.deb/.rpm):** `/usr/bin/chatviewer`.
- **AppImage:** `./ChatViewer_*.AppImage cli <arguments>`.

## Licenses

ChatViewer's own code is MIT-licensed. These builds include the Signal
importer, which uses Signal's AGPL-3.0 backup format definitions, so the
applications are distributed under the GNU AGPL v3.0. License texts and
third-party notices are included with the app (About dialog).
