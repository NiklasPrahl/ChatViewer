//! Interfaces between importers, exporters and front ends (GUI, CLI).
//!
//! Importers are compile-time plugins (docs/architecture.md §7): each
//! implements [`Importer`], and front ends pick the best match with
//! [`probe_all`].

use std::io::Read;
use std::path::{Path, PathBuf};

use cv_model::{
    BlobRef, ConversationData, ConversationId, ConversationKind, SourceReport, Timestamp,
};

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

/// A secret such as a backup recovery key. Zeroized on drop, never
/// printed (ADR 0003).
#[derive(Clone)]
pub struct Secret(zeroize::Zeroizing<String>);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(zeroize::Zeroizing::new(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// User choices that override detection (e.g. date order).
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// Importer-specific overrides as `key=value` pairs, e.g. `date_order=mdy`.
    pub overrides: Vec<(String, String)>,
    /// Display name of the archive owner, if the source does not say.
    pub self_name: Option<String>,
    /// Secret the importer asked for via [`Importer::secret_prompt`].
    pub secret: Option<Secret>,
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
    /// What to ask the user for before [`Importer::open`], if anything
    /// (e.g. `"recovery key"`). Front ends pass it in [`ImportOptions::secret`].
    fn secret_prompt(&self) -> Option<&'static str> {
        None
    }
    fn open(&self, input: &InputSource, options: &ImportOptions) -> Result<Box<dyn ChatArchive>>;
}

/// Overview of one chat, cheap to compute (for chat lists).
#[derive(Debug, Clone, PartialEq)]
pub struct ChatSummary {
    pub id: ConversationId,
    pub title: String,
    pub kind: ConversationKind,
    pub participants: usize,
    pub messages: u64,
    pub first: Option<Timestamp>,
    pub last: Option<Timestamp>,
}

/// An opened archive. Implementations load lazily where they can.
pub trait ChatArchive: Send {
    fn report(&self) -> &SourceReport;
    fn conversation_ids(&self) -> Vec<ConversationId>;
    fn load(&self, id: &ConversationId) -> Result<ConversationData>;
    /// Chat overview. The default loads the chat; archives with an index
    /// should override it.
    fn summary(&self, id: &ConversationId) -> Result<ChatSummary> {
        let data = self.load(id)?;
        Ok(ChatSummary {
            id: id.clone(),
            title: data.conversation.title,
            kind: data.conversation.kind,
            participants: data.participants.len(),
            messages: data.messages.len() as u64,
            first: data.messages.first().map(|m| m.timestamp),
            last: data.messages.last().map(|m| m.timestamp),
        })
    }
    /// All chats. The default loads them one by one; archives that can
    /// decode everything in one pass should override it.
    fn load_all(&self) -> Result<Vec<ConversationData>> {
        self.conversation_ids()
            .iter()
            .map(|id| self.load(id))
            .collect()
    }
    /// Several chats, in the given order. Archives that can decode many
    /// chats in one pass should override it.
    fn load_many(&self, ids: &[ConversationId]) -> Result<Vec<ConversationData>> {
        ids.iter().map(|id| self.load(id)).collect()
    }
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

/// A file name for a chat title that is safe on every platform.
pub fn safe_file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            c if c.is_control() => '_',
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .take(80)
        .collect();
    let cleaned = cleaned.trim_matches(['.', ' ']);
    if cleaned.is_empty() {
        "chat".into()
    } else {
        cleaned.to_owned()
    }
}

/// `dir/<stem>.<ext>`, or `dir/<stem> (2).<ext>` and so on if taken.
/// Never returns an existing path.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let candidate = dir.join(format!("{stem}.{ext}"));
    if !candidate.exists() {
        return candidate;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("some free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_safe_and_unique() {
        assert_eq!(safe_file_stem("A/B: c?"), "A_B_ c_");
        assert_eq!(safe_file_stem(" .. "), "chat");
        let dir = std::env::temp_dir().join(format!("cv-core-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = unique_path(&dir, "Team", "zip");
        std::fs::write(&first, b"x").unwrap();
        let second = unique_path(&dir, "Team", "zip");
        assert!(second.ends_with("Team (2).zip"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
            secret: Some(Secret::new("hidden".into())),
        };
        assert!(!format!("{options:?}").contains("hidden"));
        assert_eq!(options.get("k"), Some("2"));
        assert_eq!(options.get("x"), None);
    }
}
