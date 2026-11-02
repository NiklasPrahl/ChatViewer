//! Neutral chat model (docs/architecture.md §6).
//!
//! Every messenger importer produces these types; exporters and the UI
//! consume only these types. Following ADR 0006, every entity carries a
//! [`SourceExtra`] with the original record, so nothing from the source
//! is lost even where the model has no field for it yet.

use std::collections::BTreeMap;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    /// Stable within one archive.
    ConversationId
);
id_type!(
    /// Stable within one archive.
    ParticipantId
);
id_type!(
    /// Stable within one conversation.
    MessageId
);
id_type!(
    /// Importer-specific handle used to open an attachment's bytes lazily.
    BlobRef
);

/// A point in time. WhatsApp exports only know the local wall-clock time
/// of the exporting phone; Signal stores UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Timestamp {
    /// Milliseconds since the Unix epoch, UTC.
    UtcMillis(i64),
    /// Local time without time zone, shown as-is.
    NaiveLocal(NaiveDateTime),
}

/// Original data of an entity, kept for lossless round trips (ADR 0006).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceExtra {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<RawRecord>,
    /// Namespaced values the model has no field for, e.g. `"whatsapp.edited_marker"`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, serde_json::Value>,
}

impl SourceExtra {
    pub fn is_empty(&self) -> bool {
        self.raw.is_none() && self.fields.is_empty()
    }
}

/// The source-native record an entity came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "encoding", rename_all = "snake_case")]
pub enum RawRecord {
    /// Text records, e.g. the exact lines of a WhatsApp export.
    Text { format: String, text: String },
    /// Binary records, e.g. a Signal protobuf frame.
    Base64 { format: String, data: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    Direct,
    Group,
    NoteToSelf,
    Broadcast,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub id: ConversationId,
    pub kind: ConversationKind,
    pub title: String,
    pub participants: Vec<ParticipantId>,
    #[serde(default, skip_serializing_if = "SourceExtra::is_empty")]
    pub extra: SourceExtra,
}

/// Whether a participant is the archive owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfFlag {
    Yes,
    No,
    /// The source does not say (e.g. WhatsApp exports).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Participant {
    pub id: ParticipantId,
    pub display_name: String,
    pub is_self: SelfFlag,
    /// Phone number or username, if the source has one. Never logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(default, skip_serializing_if = "SourceExtra::is_empty")]
    pub extra: SourceExtra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum Author {
    Participant(ParticipantId),
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    /// Render order within the conversation, starting at 0.
    pub seq: u64,
    pub timestamp: Timestamp,
    pub author: Author,
    pub content: Content,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<Quote>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<Reaction>,
    #[serde(default, skip_serializing_if = "MessageFlags::is_empty")]
    pub flags: MessageFlags,
    #[serde(default, skip_serializing_if = "SourceExtra::is_empty")]
    pub extra: SourceExtra,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageFlags {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub edited: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub forwarded: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub view_once: bool,
}

impl MessageFlags {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Text with optional formatting ranges (byte offsets into `text`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RichText {
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<TextRange>,
}

impl RichText {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ranges: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub start: usize,
    pub len: usize,
    pub style: TextStyle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TextStyle {
    Bold,
    Italic,
    Strikethrough,
    Monospace,
    Spoiler,
    Mention(ParticipantId),
    Link(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Content {
    Text(RichText),
    Media {
        attachments: Vec<Attachment>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<RichText>,
    },
    Sticker {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        emoji: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pack_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sticker_id: Option<u32>,
        image: Attachment,
    },
    Location(Location),
    Poll(Poll),
    System(SystemEvent),
    Deleted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<ParticipantId>,
    },
    /// Visible placeholder for content this version cannot map. The raw
    /// data is kept in the message's `extra`.
    Unsupported {
        source_kind: String,
        note: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
    Audio,
    Voice,
    Gif,
    Sticker,
    Document,
    Contact,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub kind: MediaKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waveform: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
    pub availability: Availability,
}

impl Attachment {
    pub fn new(kind: MediaKind, availability: Availability) -> Self {
        Self {
            kind,
            mime: None,
            file_name: None,
            size: None,
            width: None,
            height: None,
            duration_ms: None,
            waveform: None,
            blurhash: None,
            availability,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Availability {
    Available {
        blob: BlobRef,
        integrity: Integrity,
    },
    /// The exporter chose not to include it (e.g. "<Media omitted>").
    Omitted,
    /// Referenced, but the file is not in the archive.
    Missing,
    /// Not on the device when the backup was made.
    NotInBackup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Integrity {
    /// Content matches a recorded hash.
    Verified,
    /// Authentic, but differs from recorded size or hash (ADR 0006).
    AuthenticatedOnly,
    /// The source has no integrity data (e.g. WhatsApp exports).
    Unchecked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Poll {
    pub question: String,
    pub options: Vec<PollOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollOption {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub votes: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemEvent {
    pub kind: SystemKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actors: Vec<ParticipantId>,
    /// The original, localized text.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemKind {
    EncryptionNotice,
    GroupCreated,
    MemberAdded,
    MemberRemoved,
    MemberLeft,
    MemberJoined,
    SubjectChanged,
    IconChanged,
    DescriptionChanged,
    SecurityCodeChanged,
    DisappearingMessages,
    MissedCall,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<ParticipantId>,
    pub snippet: RichText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub author: ParticipantId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<Timestamp>,
}

/// What an importer found out about its input (shown in the GUI).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SourceReport {
    /// Importer id, e.g. `"whatsapp"`.
    pub app: String,
    /// `"ios"`, `"android"`, `"desktop"` or `"unknown"`.
    pub platform: String,
    /// Detected format variant, e.g. `"whatsapp-android-24h-dmy"`.
    pub variant: String,
    /// Version information from the source, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_version: Option<String>,
    /// 0–100. Below 70 the GUI suggests checking the detected settings.
    pub confidence: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// What this source cannot contain (e.g. "no reactions").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limitations: Vec<String>,
    /// Named counters, e.g. `"unparsed_lines"`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub counters: BTreeMap<String, u64>,
}

/// A fully loaded conversation (used by exporters and tests).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationData {
    pub conversation: Conversation,
    pub participants: Vec<Participant>,
    pub messages: Vec<Message>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_compactly_and_round_trips() {
        let message = Message {
            id: MessageId::new("m1"),
            seq: 0,
            timestamp: Timestamp::NaiveLocal(
                chrono::NaiveDate::from_ymd_opt(2024, 1, 2)
                    .unwrap()
                    .and_hms_opt(3, 4, 5)
                    .unwrap(),
            ),
            author: Author::Participant(ParticipantId::new("p1")),
            content: Content::Text(RichText::plain("Hello")),
            quote: None,
            reactions: Vec::new(),
            flags: MessageFlags::default(),
            extra: SourceExtra::default(),
        };
        let json = serde_json::to_string(&message).unwrap();
        assert!(!json.contains("flags") && !json.contains("extra") && !json.contains("reactions"));
        assert_eq!(serde_json::from_str::<Message>(&json).unwrap(), message);
    }
}
