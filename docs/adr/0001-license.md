# 0001 – License: MIT repository, AGPL-3.0 Signal importer

- Status: Accepted (confirmed by the M1 spike, see ADR 0005)
- Date: 2026-11-02

## Context

The project goal is a permissive license that allows commercial use and
requires attribution (MIT preferred). Signal publishes its backup format
only as AGPL-3.0-only source code (libsignal, the apps, `backup.proto`,
`LocalBackup.proto`). There is no separately licensed specification.
libsignal is not published on crates.io and states that use outside
Signal is unsupported. Details: [architecture §2.7](../architecture.md).

## Decision

- The repository is licensed under **MIT** by default. Every crate states
  its SPDX license in `Cargo.toml`.
- The Signal importer crate (`cv-import-signal`) is **AGPL-3.0-only**. It
  vendors Signal's `.proto` files unmodified, with attribution. Its crypto
  is implemented with RustCrypto crates, without linking libsignal.
- Signal support sits behind a default-on Cargo feature `signal`.
  Official release binaries include it and are therefore distributed under
  **AGPL-3.0** as a whole. Building without the feature gives an MIT-only
  binary.
- `cargo-deny` enforces the license allow-list. AGPL is allowed only for
  `cv-import-signal`.
- Any other copyleft or attribution-only dependency (e.g. Twemoji
  CC-BY 4.0, codecs) needs its own ADR.

## Consequences

- Our own code stays reusable under MIT, including commercially.
- Official binaries carry AGPL obligations: offer the corresponding source
  (the tagged repository) and keep license notices. AGPL §13 (network use)
  has no practical effect because the app has no network service.
- README, About dialog and `THIRD_PARTY_LICENSES` must explain the split.
- Revisit after M1: an MIT-only implementation with a self-written schema
  (option C in [plan §1](../plan.md)) needs a legal opinion first. This
  ADR is not legal advice.

## Alternatives considered

- **Link libsignal (option A):** least crypto work, but a heavy git
  dependency, use outside Signal is unsupported, and the binary is AGPL
  anyway.
- **Own minimal schema, MIT everywhere (option C):** fits the goal fully,
  but carries legal uncertainty. Deferred.
- **AGPL sidecar process (option D):** the "aggregate" argument is
  debatable when the two always ship together, and it adds IPC complexity.
