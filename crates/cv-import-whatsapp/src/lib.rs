//! Importer for WhatsApp chat exports ("Export chat" on iOS and Android).
//!
//! Supported inputs: the exported `.zip`, a bare `.txt`, or an extracted
//! folder. Format variants are described in docs/architecture.md §3.

use std::io::{Cursor, Read};

use cv_core::{ChatArchive, Error, ImportOptions, Importer, InputSource, Probe, Result};
use cv_model::{BlobRef, ConversationData, ConversationId, SourceReport};

mod container;
pub mod header;
pub mod locale;
mod parse;
#[cfg(feature = "synth")]
pub mod synth;

use container::Container;

pub struct WhatsAppImporter;

impl Importer for WhatsAppImporter {
    fn id(&self) -> &'static str {
        "whatsapp"
    }

    fn probe(&self, input: &InputSource) -> Probe {
        let path = input.path();
        if path.is_dir() {
            return match Container::open(path) {
                Ok(c) if looks_like_chat(&c.chat_text) => Probe::Maybe(90),
                _ => Probe::NotMine,
            };
        }
        match input.extension().as_deref() {
            Some("zip") => {
                let Ok(file) = std::fs::File::open(path) else {
                    return Probe::NotMine;
                };
                let Ok(zip) = zip::ZipArchive::new(file) else {
                    return Probe::NotMine;
                };
                let names: Vec<String> = zip
                    .file_names()
                    .filter_map(|n| n.ok().map(|n| n.into_owned()))
                    .collect();
                match container::pick_chat_file(names.iter().map(String::as_str)) {
                    Some(name) if name.ends_with("_chat.txt") || name.contains("WhatsApp") => {
                        Probe::Maybe(95)
                    }
                    Some(_) => Probe::Maybe(40),
                    None => Probe::NotMine,
                }
            }
            Some("txt") => {
                let mut head = String::new();
                let _ =
                    std::fs::File::open(path).map(|f| f.take(16 * 1024).read_to_string(&mut head));
                if looks_like_chat(&head) {
                    Probe::Maybe(85)
                } else {
                    Probe::NotMine
                }
            }
            _ => Probe::NotMine,
        }
    }

    fn open(&self, input: &InputSource, options: &ImportOptions) -> Result<Box<dyn ChatArchive>> {
        let container = Container::open(input.path())?;
        let (data, report) = parse::parse(&container, options);
        if data.messages.is_empty() {
            return Err(Error::Unsupported(
                "no WhatsApp messages found in this file".into(),
            ));
        }
        Ok(Box::new(WhatsAppArchive {
            container,
            data,
            report,
        }))
    }
}

/// At least one of the first lines is a message header.
fn looks_like_chat(text: &str) -> bool {
    text.lines()
        .take(20)
        .any(|l| header::parse_header(l.trim_start_matches('\u{feff}')).is_some())
}

pub struct WhatsAppArchive {
    container: Container,
    data: ConversationData,
    report: SourceReport,
}

impl ChatArchive for WhatsAppArchive {
    fn report(&self) -> &SourceReport {
        &self.report
    }

    fn conversation_ids(&self) -> Vec<ConversationId> {
        vec![self.data.conversation.id.clone()]
    }

    fn load(&self, id: &ConversationId) -> Result<ConversationData> {
        if *id != self.data.conversation.id {
            return Err(Error::NoSuchConversation(id.to_string()));
        }
        Ok(self.data.clone())
    }

    fn open_blob(&self, blob: &BlobRef) -> Result<Box<dyn Read + '_>> {
        if !self.container.media.values().any(|e| e == blob.as_str()) {
            return Err(Error::NoSuchBlob);
        }
        Ok(Box::new(Cursor::new(
            self.container.read_media(blob.as_str())?,
        )))
    }
}
