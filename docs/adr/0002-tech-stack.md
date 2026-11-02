# 0002 – Tech stack: Tauri 2, Rust core, TypeScript UI

- Status: Accepted (UI framework confirmed as React before M4)
- Date: 2026-11-02

## Context

We need a cross-platform desktop GUI and a CLI that share one core. The
core does parsing, streaming over large archives, cryptography and media
decoding. The app must not use the network and must not persist data.
Comparison of options: [architecture §4](../architecture.md).

## Decision

- **Core:** Rust Cargo workspace (`cv-model`, `cv-core`, importer and
  exporter crates, `cv-cli`). Crypto from RustCrypto crates.
- **GUI:** Tauri 2 shell. Media is served from memory through a custom
  `cv://` URI protocol with Range support.
- **UI:** TypeScript with **React 19** (chosen by the maintainer at the
  start of M4), Vite, and `@tanstack/react-virtual` for the message list.
  TypeScript 5.9 and Vite 7 are used instead of the brand-new TypeScript
  7 and Vite 8 majors until those have matured.
- **CLI:** `chatviewer` binary, bundled per platform via Tauri
  `externalBin` (architecture §9).
- Importers and exporters are compile-time plugins registered in
  `cv-core`. There is no dynamic loading.

## Consequences

- Small binaries and one language for all logic. GUI and CLI behave
  identically.
- System WebViews differ (WebKitGTK, WebView2, WKWebView), so codec and
  rendering checks are needed per platform. Opus playback needs a Rust
  decoding path.
- Rust edition 2024, minimum Rust 1.95 (raised from 1.85 in M4 so dependency
  security fixes can be used), `unsafe_code = "deny"` workspace-wide.

## Alternatives considered

Electron (large bundles, JS crypto/parsing), Flutter (desktop maturity,
FFI), Qt/QML (licensing, complexity), pure Rust GUI toolkits (immature
rich text, emoji and media).
