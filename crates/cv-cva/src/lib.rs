//! ChatViewer Archive (CVA): one chat as a ZIP with media, re-openable
//! by ChatViewer and readable by WhatsApp tools. Spec: docs/formats/cva.md.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Cursor, Read, Seek, Write};
use std::path::Path;

use cv_core::{ChatArchive, Error, ImportOptions, Importer, InputSource, Probe, Result};
use cv_model::{
    Author, Availability, BlobRef, Content, Conversation, ConversationData, ConversationId,
    Message, Participant, SourceReport, Timestamp,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

pub const FORMAT: &str = "chatviewer-archive";
pub const FORMAT_VERSION: &str = "1.0";
pub const MANIFEST: &str = "manifest.json";
pub const MESSAGES: &str = "messages.jsonl";
pub const CHAT_TEXT: &str = "_chat.txt";
pub const MEDIA_DIR: &str = "media/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: String,
    pub generator: Generator,
    pub source: SourceReport,
    pub conversation: Conversation,
    pub participants: Vec<Participant>,
    pub message_count: u64,
    pub media: Vec<MediaEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Generator {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaEntry {
    /// Path inside the ZIP, e.g. `media/000012-photo.jpg`.
    pub path: String,
    /// The blob reference in the source archive.
    pub source_blob: String,
    pub size: u64,
    pub sha256: String,
}

/// What happened while writing.
#[derive(Debug, Default)]
pub struct WriteSummary {
    pub messages: u64,
    pub media_written: u64,
    /// Media that could not be read from the source (marked `Missing`).
    pub media_failed: u64,
}

/// Writes one conversation of `archive` as CVA.
pub fn write(
    archive: &dyn ChatArchive,
    id: &ConversationId,
    out: impl Write + Seek,
) -> Result<WriteSummary> {
    write_data(archive, archive.load(id)?, out)
}

/// Writes an already loaded conversation as CVA (for batch exports that
/// load many chats in one pass). Media are read from `archive`.
pub fn write_data(
    archive: &dyn ChatArchive,
    mut data: ConversationData,
    out: impl Write + Seek,
) -> Result<WriteSummary> {
    let mut zip = zip::ZipWriter::new(out);
    let deflate = SimpleFileOptions::default();
    // Media are usually compressed already.
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut summary = WriteSummary::default();
    let mut entries = Vec::new();
    let mut written: HashMap<String, String> = HashMap::new();

    for message in &mut data.messages {
        let seq = message.seq;
        for attachment in attachments_mut(&mut message.content) {
            let Availability::Available { blob, integrity } = &attachment.availability else {
                continue;
            };
            if let Some(path) = written.get(blob.as_str()) {
                attachment.availability = Availability::Available {
                    blob: BlobRef::new(path.clone()),
                    integrity: *integrity,
                };
                continue;
            }
            let mut bytes = Vec::new();
            let read = archive
                .open_blob(blob)
                .and_then(|mut r| r.read_to_end(&mut bytes).map_err(Error::from));
            if read.is_err() {
                log::warn!("cva: a media file could not be read and is marked missing");
                summary.media_failed += 1;
                attachment.availability = Availability::Missing;
                continue;
            }
            let name = safe_name(attachment.file_name.as_deref(), attachment.mime.as_deref());
            let path = format!("{MEDIA_DIR}{seq:06}-{}-{name}", entries.len());
            zip.start_file(path.as_str(), stored).map_err(zip_error)?;
            zip.write_all(&bytes)?;
            entries.push(MediaEntry {
                path: path.clone(),
                source_blob: blob.as_str().to_owned(),
                size: bytes.len() as u64,
                sha256: hex::encode(Sha256::digest(&bytes)),
            });
            written.insert(blob.as_str().to_owned(), path.clone());
            attachment.availability = Availability::Available {
                blob: BlobRef::new(path),
                integrity: *integrity,
            };
            summary.media_written += 1;
        }
    }

    zip.start_file(MESSAGES, deflate).map_err(zip_error)?;
    for message in &data.messages {
        serde_json::to_writer(&mut zip, message).map_err(json_error)?;
        zip.write_all(b"\n")?;
    }
    summary.messages = data.messages.len() as u64;

    zip.start_file(CHAT_TEXT, deflate).map_err(zip_error)?;
    write_chat_text(&data, &mut zip)?;

    let manifest = Manifest {
        format: FORMAT.into(),
        format_version: FORMAT_VERSION.into(),
        generator: Generator {
            name: "chatviewer".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        },
        source: archive.report().clone(),
        conversation: data.conversation,
        participants: data.participants,
        message_count: summary.messages,
        media: entries,
    };
    zip.start_file(MANIFEST, deflate).map_err(zip_error)?;
    serde_json::to_writer_pretty(&mut zip, &manifest).map_err(json_error)?;
    zip.finish().map_err(zip_error)?;
    Ok(summary)
}

fn attachments_mut(content: &mut Content) -> Vec<&mut cv_model::Attachment> {
    match content {
        Content::Media { attachments, .. } => attachments.iter_mut().collect(),
        Content::Sticker { image, .. } => vec![image],
        _ => Vec::new(),
    }
}

/// A file name that is safe inside a ZIP on every platform.
fn safe_name(file_name: Option<&str>, mime: Option<&str>) -> String {
    let base = file_name
        .map(|n| n.rsplit(['/', '\\']).next().unwrap_or(n))
        .unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            c if c.is_control() => '_',
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .take(80)
        .collect();
    let cleaned = cleaned.trim_matches(['.', ' ']).to_owned();
    if !cleaned.is_empty() {
        return cleaned;
    }
    let ext = match mime.unwrap_or("") {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "audio/aac" => "aac",
        "audio/mp4" => "m4a",
        "audio/mpeg" => "mp3",
        "audio/ogg" => "ogg",
        "application/pdf" => "pdf",
        _ => "bin",
    };
    format!("file.{ext}")
}

/// `_chat.txt` in the iOS WhatsApp line format, so the archive stays
/// readable without the manifest (and by WhatsApp tools).
fn write_chat_text(data: &ConversationData, out: &mut impl Write) -> Result<()> {
    let names: HashMap<_, _> = data
        .participants
        .iter()
        .map(|p| (&p.id, p.display_name.as_str()))
        .collect();
    for m in &data.messages {
        let time = match m.timestamp {
            Timestamp::NaiveLocal(t) => t.format("%Y-%m-%d %H:%M:%S").to_string(),
            Timestamp::UtcMillis(ms) => chrono::DateTime::from_timestamp_millis(ms)
                .map(|t| t.naive_utc().format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default(),
        };
        let author = match &m.author {
            Author::Participant(id) => names.get(id).copied().unwrap_or("Unknown"),
            Author::System => data.conversation.title.as_str(),
        };
        writeln!(out, "[{time}] {author}: {}", chat_text_body(m))?;
    }
    Ok(())
}

fn chat_text_body(m: &Message) -> String {
    let marker = |a: &cv_model::Attachment| match &a.availability {
        Availability::Available { blob, .. } => {
            let name = blob.as_str().rsplit('/').next().unwrap_or("");
            format!("\u{200e}<attached: {name}>")
        }
        _ => "\u{200e}<attachment not included>".to_owned(),
    };
    match &m.content {
        Content::Text(t) => t.text.clone(),
        Content::Media {
            attachments,
            caption,
        } => {
            let mut parts: Vec<String> = caption.iter().map(|c| c.text.clone()).collect();
            parts.extend(attachments.iter().map(marker));
            parts.join(" ")
        }
        Content::Sticker { image, emoji, .. } => match (&image.availability, emoji) {
            (Availability::Available { .. }, _) | (_, None) => marker(image),
            (_, Some(e)) => e.clone(),
        },
        Content::Location(l) => format!(
            "\u{200e}Location: https://maps.google.com/?q={},{}",
            l.latitude, l.longitude
        ),
        Content::Poll(p) => {
            let mut lines = vec!["POLL:".to_owned(), p.question.clone()];
            for o in &p.options {
                lines.push(format!(
                    "OPTION: {} ({} votes)",
                    o.text,
                    o.votes.unwrap_or(0)
                ));
            }
            lines.join("\n")
        }
        Content::System(e) => format!("\u{200e}{}", e.text),
        Content::Deleted { .. } => "\u{200e}This message was deleted.".into(),
        Content::Unsupported { note, .. } => format!("\u{200e}<{note}>"),
    }
}

fn zip_error(e: zip::result::ZipError) -> Error {
    Error::Malformed(format!("ZIP archive: {e}"))
}

fn json_error(e: serde_json::Error) -> Error {
    Error::Malformed(format!("JSON: {e}"))
}

/// Opens CVA files.
pub struct CvaImporter;

impl Importer for CvaImporter {
    fn id(&self) -> &'static str {
        "cva"
    }

    fn probe(&self, input: &InputSource) -> Probe {
        if input.extension().as_deref() != Some("zip") {
            return Probe::NotMine;
        }
        match read_manifest(input.path()) {
            Ok(m) if m.format == FORMAT => Probe::Maybe(100),
            _ => Probe::NotMine,
        }
    }

    fn open(&self, input: &InputSource, _: &ImportOptions) -> Result<Box<dyn ChatArchive>> {
        Ok(Box::new(CvaArchive::open(input.path())?))
    }
}

fn read_manifest(path: &Path) -> Result<Manifest> {
    let mut zip = zip::ZipArchive::new(File::open(path)?).map_err(zip_error)?;
    let entry = zip.by_name(MANIFEST).map_err(zip_error)?;
    serde_json::from_reader(io::BufReader::new(entry)).map_err(json_error)
}

pub struct CvaArchive {
    path: std::path::PathBuf,
    manifest: Manifest,
    messages: Vec<Message>,
}

impl CvaArchive {
    pub fn open(path: &Path) -> Result<Self> {
        let manifest = read_manifest(path)?;
        let major = manifest.format_version.split('.').next();
        if manifest.format != FORMAT || major != FORMAT_VERSION.split('.').next() {
            return Err(Error::Unsupported(format!(
                "{} version {} (this viewer reads {FORMAT_VERSION})",
                manifest.format, manifest.format_version
            )));
        }
        let mut zip = zip::ZipArchive::new(File::open(path)?).map_err(zip_error)?;
        let entry = zip.by_name(MESSAGES).map_err(zip_error)?;
        let mut messages = Vec::with_capacity(manifest.message_count as usize);
        for line in io::BufRead::lines(io::BufReader::new(entry)) {
            let line = line?;
            if !line.is_empty() {
                messages.push(serde_json::from_str(&line).map_err(json_error)?);
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            manifest,
            messages,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
}

impl ChatArchive for CvaArchive {
    fn report(&self) -> &SourceReport {
        &self.manifest.source
    }

    fn conversation_ids(&self) -> Vec<ConversationId> {
        vec![self.manifest.conversation.id.clone()]
    }

    fn load(&self, id: &ConversationId) -> Result<ConversationData> {
        if *id != self.manifest.conversation.id {
            return Err(Error::NoSuchConversation(id.to_string()));
        }
        Ok(ConversationData {
            conversation: self.manifest.conversation.clone(),
            participants: self.manifest.participants.clone(),
            messages: self.messages.clone(),
        })
    }

    fn open_blob(&self, blob: &BlobRef) -> Result<Box<dyn Read + '_>> {
        if !self.manifest.media.iter().any(|m| m.path == blob.as_str()) {
            return Err(Error::NoSuchBlob);
        }
        let mut zip = zip::ZipArchive::new(File::open(&self.path)?).map_err(zip_error)?;
        let mut entry = zip.by_name(blob.as_str()).map_err(zip_error)?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        Ok(Box::new(Cursor::new(bytes)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_made_safe() {
        assert_eq!(safe_name(Some("../a:b?.jpg"), None), "a_b_.jpg");
        assert_eq!(safe_name(None, Some("image/png")), "file.png");
        assert_eq!(safe_name(Some(" . "), Some("video/mp4")), "file.mp4");
    }
}
