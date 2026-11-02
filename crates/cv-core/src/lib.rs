//! Interfaces between importers, exporters and front ends (GUI, CLI).
//!
//! Importers are compile-time plugins (docs/architecture.md §7): each
//! implements [`Importer`], and front ends pick the best match with
//! [`probe_all`].

use std::io::Read;
use std::path::{Path, PathBuf};

use cv_model::{BlobRef, ConversationData, ConversationId, SourceReport};

pub mod logging;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported input: {0}")]
    Unsupported(String),
    #[error("malformed input: {0}")]
    Malformed(String),
    #[error("no such conversation: {0}")]
    NoSuchConversation(String),
    #[error("no such attachment")]
    NoSuchBlob,
    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What the user selected: a file or a folder.
#[derive(Debug, Clone)]
pub struct InputSource {
    pub path: PathBuf,
}

impl InputSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Lowercase file extension, if any.
    pub fn extension(&self) -> Option<String> {
        self.path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
    }
}

/// How well an importer recognises an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Probe {
    NotMine,
    /// Confidence 1–100.
    Maybe(u8),
}

/// User choices that override detection (e.g. date order).
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Importer-specific overrides as `key=value` pairs, e.g. `date_order=mdy`.
    pub overrides: Vec<(String, String)>,
    /// Display name of the archive owner, if the source does not say.
    pub self_name: Option<String>,
}

impl ImportOptions {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.overrides
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

pub trait Importer: Send + Sync {
    /// Stable id, e.g. `"whatsapp"`.
    fn id(&self) -> &'static str;
    fn probe(&self, input: &InputSource) -> Probe;
    fn open(&self, input: &InputSource, options: &ImportOptions) -> Result<Box<dyn ChatArchive>>;
}

/// An opened archive. Implementations load lazily where they can.
pub trait ChatArchive {
    fn report(&self) -> &SourceReport;
    fn conversation_ids(&self) -> Vec<ConversationId>;
    fn load(&self, id: &ConversationId) -> Result<ConversationData>;
    /// Opens an attachment's bytes. Never writes to disk (ADR 0003).
    fn open_blob(&self, blob: &BlobRef) -> Result<Box<dyn Read + '_>>;
}

/// Picks the importer with the highest confidence, if any.
pub fn probe_all<'a>(
    importers: &'a [Box<dyn Importer>],
    input: &InputSource,
) -> Option<&'a dyn Importer> {
    importers
        .iter()
        .map(|i| (i.probe(input), i))
        .filter(|(p, _)| *p != Probe::NotMine)
        .max_by_key(|(p, _)| *p)
        .map(|(_, i)| i.as_ref())
}

pub trait Exporter: Send + Sync {
    /// Stable id, e.g. `"markdown"`.
    fn id(&self) -> &'static str;
    fn extension(&self) -> &'static str;
    /// Whether the format keeps all data (ADR 0006).
    fn lossless(&self) -> bool;
    fn export(
        &self,
        data: &ConversationData,
        report: &SourceReport,
        out: &mut dyn std::io::Write,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(&'static str, Probe);

    impl Importer for Fixed {
        fn id(&self) -> &'static str {
            self.0
        }
        fn probe(&self, _: &InputSource) -> Probe {
            self.1
        }
        fn open(&self, _: &InputSource, _: &ImportOptions) -> Result<Box<dyn ChatArchive>> {
            Err(Error::Other("test".into()))
        }
    }

    #[test]
    fn probe_picks_highest_confidence() {
        let importers: Vec<Box<dyn Importer>> = vec![
            Box::new(Fixed("a", Probe::Maybe(20))),
            Box::new(Fixed("b", Probe::Maybe(90))),
            Box::new(Fixed("c", Probe::NotMine)),
        ];
        let input = InputSource::new("x.zip");
        assert_eq!(probe_all(&importers, &input).unwrap().id(), "b");
        assert!(probe_all(&importers[2..], &input).is_none());
    }

    #[test]
    fn options_use_last_override() {
        let options = ImportOptions {
            overrides: vec![("k".into(), "1".into()), ("k".into(), "2".into())],
            self_name: None,
        };
        assert_eq!(options.get("k"), Some("2"));
        assert_eq!(options.get("x"), None);
    }
}
