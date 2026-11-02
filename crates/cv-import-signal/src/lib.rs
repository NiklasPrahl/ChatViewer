//! Reader for Signal on-device ("local") backups in the cross-platform
//! backup format.
//!
//! This is the M1 feasibility spike (see `docs/plan.md`). It decrypts and
//! decodes a backup using only the 64-character recovery key. Mapping to
//! the neutral chat model comes in M3.
//!
//! Format reference: `docs/architecture.md` §2 and ADR 0005.

pub mod crypto;
pub mod error;
pub mod import;
pub mod key;
pub mod layout;
pub mod main_file;
mod mapping;
pub mod media;
pub mod metadata;
pub mod walk;
#[cfg(feature = "writer")]
pub mod writer;

pub use error::{Error, Result};
pub use import::{SignalArchive, SignalImporter};

/// Code generated from Signal's vendored protobuf schemas.
#[allow(clippy::all, clippy::pedantic)]
pub mod proto {
    /// `signal.backup` (`backup.proto`).
    pub mod backup {
        include!(concat!(env!("OUT_DIR"), "/signal.backup.rs"));
    }
    /// `signal.backup.local` (`LocalBackup.proto`).
    pub mod local {
        include!(concat!(env!("OUT_DIR"), "/signal.backup.local.rs"));
    }
}
