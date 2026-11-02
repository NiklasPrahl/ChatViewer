//! Exporters for the neutral model.
//!
//! TXT and Markdown are for reading and are lossy by nature; they say so
//! in their header. JSON is lossless, including each message's raw source
//! record (ADR 0006), and can be read back with [`json::read`].

use std::collections::HashMap;

use cv_core::Exporter;
use cv_model::{
    Author, Availability, Content, ConversationData, MediaKind, Message, ParticipantId, Timestamp,
};

pub mod json;
pub mod markdown;
pub mod text;

/// All exporters, for front ends to list.
pub fn exporters() -> Vec<Box<dyn Exporter>> {
    vec![
        Box::new(text::TextExporter),
        Box::new(markdown::MarkdownExporter),
        Box::new(json::JsonExporter),
    ]
}

/// Finds an exporter by id or file extension.
pub fn by_name(name: &str) -> Option<Box<dyn Exporter>> {
    let name = name.to_ascii_lowercase();
    exporters()
        .into_iter()
        .find(|e| e.id() == name || e.extension() == name)
}

/// Display names by participant id.
pub(crate) fn names(data: &ConversationData) -> HashMap<&ParticipantId, &str> {
    data.participants
        .iter()
        .map(|p| (&p.id, p.display_name.as_str()))
        .collect()
}

pub(crate) fn author_name<'a>(m: &Message, names: &HashMap<&ParticipantId, &'a str>) -> &'a str {
    match &m.author {
        Author::Participant(id) => names.get(id).copied().unwrap_or("Unknown"),
        Author::System => "System",
    }
}

pub(crate) fn format_time(t: &Timestamp, fmt: &str) -> String {
    match t {
        Timestamp::NaiveLocal(t) => t.format(fmt).to_string(),
        Timestamp::UtcMillis(ms) => chrono::DateTime::from_timestamp_millis(*ms)
            .map(|t| format!("{} UTC", t.naive_utc().format(fmt)))
            .unwrap_or_else(|| "invalid time".into()),
    }
}

fn kind_label(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Image => "image",
        MediaKind::Video => "video",
        MediaKind::Audio => "audio",
        MediaKind::Voice => "voice message",
        MediaKind::Gif => "GIF",
        MediaKind::Sticker => "sticker",
        MediaKind::Document => "document",
        MediaKind::Contact => "contact card",
        MediaKind::Other => "media",
    }
}

/// One-line descriptions of a message's non-text parts, shared by the
/// text-based exporters. Returns (main text, notes such as attachments).
pub(crate) fn describe(content: &Content) -> (String, Vec<String>) {
    let attachment_note = |a: &cv_model::Attachment| {
        let kind = kind_label(a.kind);
        let name = a.file_name.as_deref().unwrap_or("");
        match &a.availability {
            Availability::Available { .. } => format!("<{kind}: {name}>"),
            Availability::Omitted => format!("<{kind} omitted>"),
            Availability::Missing => format!("<{kind} missing: {name}>"),
            Availability::NotInBackup => format!("<{kind} not in backup: {name}>"),
        }
    };
    match content {
        Content::Text(t) => (t.text.clone(), Vec::new()),
        Content::Media {
            attachments,
            caption,
        } => (
            caption.as_ref().map(|c| c.text.clone()).unwrap_or_default(),
            attachments.iter().map(attachment_note).collect(),
        ),
        Content::Sticker { emoji, image, .. } => {
            let note = match (&image.availability, emoji) {
                (Availability::Available { .. }, _) | (_, None) => attachment_note(image),
                (_, Some(e)) => format!("<sticker {e}>"),
            };
            (String::new(), vec![note])
        }
        Content::Location(l) => {
            let mut note = format!("<location: {}, {}>", l.latitude, l.longitude);
            if let Some(name) = &l.name {
                note.push_str(&format!(" {name}"));
            }
            (l.url.clone().unwrap_or_default(), vec![note])
        }
        Content::Poll(p) => {
            let mut lines = vec![format!("Poll: {}", p.question)];
            for o in &p.options {
                match o.votes {
                    Some(n) => lines.push(format!("- {} ({n} votes)", o.text)),
                    None => lines.push(format!("- {}", o.text)),
                }
            }
            (lines.join("\n"), Vec::new())
        }
        Content::System(e) => (e.text.clone(), Vec::new()),
        Content::Deleted { .. } => (String::new(), vec!["<message deleted>".into()]),
        Content::Unsupported { source_kind, .. } => {
            (String::new(), vec![format!("<unsupported: {source_kind}>")])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_exporters_by_id_or_extension() {
        assert_eq!(by_name("md").unwrap().id(), "markdown");
        assert_eq!(by_name("JSON").unwrap().id(), "json");
        assert_eq!(by_name("txt").unwrap().id(), "text");
        assert!(by_name("pdf").is_none());
    }
}
