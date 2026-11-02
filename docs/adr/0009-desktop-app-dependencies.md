# 0009 – Desktop app dependencies: MPL-2.0 and advisories

- Status: Accepted
- Date: 2026-11-02

## Context

The Tauri desktop app (M4) brings in many transitive dependencies. Two
things came up in `cargo deny`:

1. Five crates under **MPL-2.0**, all used by Tauri itself:
   `cssparser`, `cssparser-macros`, `selectors`, `dtoa-short` (the Servo
   CSS and HTML parser) and `option-ext`. ADR 0001 requires an ADR for
   any copyleft license.
2. **Advisories:**
   - Three denial-of-service vulnerabilities (`time`, `quick-xml` via
     `plist`, and an HTML parser issue). The minimum Rust version of 1.85
     kept the resolver on old versions.
   - One "unmaintained" notice (`proc-macro-error` via Tauri's GTK3
     bindings on Linux).

## Decision

- **Allow MPL-2.0.** It is file-level copyleft: only changes to the
  MPL-licensed files themselves would have to be published. We use these
  crates unmodified, and MPL-2.0 is compatible with both MIT and AGPL-3.0
  distributions. Their notices go into `THIRD_PARTY_LICENSES` (M5).
- **Raise the minimum Rust version to 1.95** and keep the lockfile up to
  date, so security fixes are picked up. All three vulnerabilities were
  fixed by `cargo update`.
- **Vulnerabilities fail CI everywhere. "Unmaintained" notices fail only
  for our direct dependencies** (`unmaintained = "workspace"`), because
  Tauri's Linux GTK3 bindings cannot be replaced from our side.
  Dependabot and the weekly CI run keep watching.

## Consequences

- Building requires Rust 1.95 or newer.
- When Tauri moves to GTK4 bindings, the unmaintained notice disappears
  without action on our side.
