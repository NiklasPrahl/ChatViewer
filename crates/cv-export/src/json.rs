//! Lossless JSON: the neutral model plus the source report.

use std::io::{Read, Write};

use cv_core::{Error, Exporter, Result};
use cv_model::{Conversation, ConversationData, Message, Participant, SourceReport};
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "chatviewer-json";
pub const FORMAT_VERSION: &str = "1.0";

#[derive(Debug, Serialize, Deserialize)]
pub struct Document {
    pub format: String,
    pub format_version: String,
    pub generator: Generator,
    pub source: SourceReport,
    pub conversation: Conversation,
    pub participants: Vec<Participant>,
    pub messages: Vec<Message>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Generator {
    pub name: String,
    pub version: String,
}

pub struct JsonExporter;

impl Exporter for JsonExporter {
    fn id(&self) -> &'static str {
        "json"
    }

    fn extension(&self) -> &'static str {
        "json"
    }

    fn lossless(&self) -> bool {
        true
    }

    fn export(
        &self,
        data: &ConversationData,
        report: &SourceReport,
        out: &mut dyn Write,
    ) -> Result<()> {
        let document = Document {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION.into(),
            generator: Generator {
                name: "chatviewer".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            source: report.clone(),
            conversation: data.conversation.clone(),
            participants: data.participants.clone(),
            messages: data.messages.clone(),
        };
        serde_json::to_writer_pretty(&mut *out, &document)
            .map_err(|e| Error::Other(format!("JSON export failed: {e}")))?;
        writeln!(out)?;
        Ok(())
    }
}

/// Reads a document written by [`JsonExporter`]. Accepts the same major
/// format version.
pub fn read(input: impl Read) -> Result<(ConversationData, SourceReport)> {
    let document: Document = serde_json::from_reader(input)
        .map_err(|e| Error::Malformed(format!("not a ChatViewer JSON export: {e}")))?;
    let major = document.format_version.split('.').next();
    if document.format != FORMAT || major != FORMAT_VERSION.split('.').next() {
        return Err(Error::Unsupported(format!(
            "{} version {}",
            document.format, document.format_version
        )));
    }
    Ok((
        ConversationData {
            conversation: document.conversation,
            participants: document.participants,
            messages: document.messages,
        },
        document.source,
    ))
}
