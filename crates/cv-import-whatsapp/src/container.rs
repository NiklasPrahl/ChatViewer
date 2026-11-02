//! The export container: a `.zip`, a bare `.txt`, or an extracted folder.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use cv_core::{Error, Result};

/// Largest chat text we accept (WhatsApp caps exports at 40k messages).
const MAX_CHAT_TEXT: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
pub enum Location {
    Zip(PathBuf),
    Dir(PathBuf),
    Txt,
}

#[derive(Debug)]
pub struct Container {
    pub location: Location,
    /// File name of the chat text (e.g. `_chat.txt`).
    pub chat_file_name: String,
    /// File name of the container itself (zip or folder name).
    pub container_name: Option<String>,
    pub chat_text: String,
    /// Media file name → entry path (zip) or relative path (folder).
    pub media: BTreeMap<String, String>,
}

impl Container {
    pub fn open(path: &Path) -> Result<Self> {
        if path.is_dir() {
            return Self::open_dir(path);
        }
        match extension(path).as_deref() {
            Some("zip") => Self::open_zip(path),
            Some("txt") => {
                let chat_text = read_text(File::open(path)?, fs::metadata(path)?.len())?;
                Ok(Self {
                    location: Location::Txt,
                    chat_file_name: file_name(path),
                    container_name: None,
                    chat_text,
                    media: BTreeMap::new(),
                })
            }
            _ => Err(Error::Unsupported("expected a .zip, .txt or folder".into())),
        }
    }

    fn open_zip(path: &Path) -> Result<Self> {
        let mut zip = zip::ZipArchive::new(File::open(path)?).map_err(zip_error)?;
        let names: Vec<String> = zip
            .file_names()
            .filter_map(|n| n.ok().map(|n| n.into_owned()))
            .collect();
        let chat_entry = pick_chat_file(names.iter().map(String::as_str))
            .ok_or_else(|| Error::Unsupported("no chat text file in the archive".into()))?
            .to_owned();
        let entry = zip.by_name(&chat_entry).map_err(zip_error)?;
        let size = entry.size();
        let chat_text = read_text(entry, size)?;
        let media = names
            .iter()
            .filter(|n| **n != chat_entry && !n.ends_with('/'))
            .map(|n| (base_name(n).to_owned(), n.clone()))
            .collect();
        Ok(Self {
            location: Location::Zip(path.to_path_buf()),
            chat_file_name: base_name(&chat_entry).to_owned(),
            container_name: Some(file_name(path)),
            chat_text,
            media,
        })
    }

    fn open_dir(path: &Path) -> Result<Self> {
        let mut files = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                files.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        let chat = pick_chat_file(files.iter().map(String::as_str))
            .ok_or_else(|| Error::Unsupported("no chat text file in the folder".into()))?
            .to_owned();
        let chat_path = path.join(&chat);
        let chat_text = read_text(File::open(&chat_path)?, fs::metadata(&chat_path)?.len())?;
        let media = files
            .iter()
            .filter(|f| **f != chat)
            .map(|f| (f.clone(), f.clone()))
            .collect();
        Ok(Self {
            location: Location::Dir(path.to_path_buf()),
            chat_file_name: chat,
            container_name: Some(file_name(path)),
            chat_text,
            media,
        })
    }

    /// Reads a media file into memory. Never extracts to disk (ADR 0003).
    pub fn read_media(&self, entry: &str) -> Result<Vec<u8>> {
        match &self.location {
            Location::Zip(path) => {
                let mut zip = zip::ZipArchive::new(File::open(path)?).map_err(zip_error)?;
                let mut file = zip.by_name(entry).map_err(zip_error)?;
                let mut buf = Vec::with_capacity(file.size() as usize);
                file.read_to_end(&mut buf)?;
                Ok(buf)
            }
            Location::Dir(dir) => {
                // Entries come from our own folder listing; reject anything else.
                if entry.contains(['/', '\\']) || entry == ".." {
                    return Err(Error::NoSuchBlob);
                }
                Ok(fs::read(dir.join(entry))?)
            }
            Location::Txt => Err(Error::NoSuchBlob),
        }
    }
}

/// Chooses the chat text among file names: iOS `_chat.txt`, Android
/// `WhatsApp Chat with <name>.txt` (localized), or a single `.txt`.
pub fn pick_chat_file<'a>(names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let txt: Vec<&str> = names
        .filter(|n| n.to_ascii_lowercase().ends_with(".txt") && !n.ends_with('/'))
        .collect();
    txt.iter()
        .find(|n| base_name(n) == "_chat.txt")
        .or_else(|| txt.iter().find(|n| base_name(n).starts_with("WhatsApp")))
        .or_else(|| (txt.len() == 1).then(|| &txt[0]))
        .copied()
}

fn read_text(reader: impl Read, size: u64) -> Result<String> {
    if size > MAX_CHAT_TEXT {
        return Err(Error::Malformed("chat text is too large".into()));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    reader.take(MAX_CHAT_TEXT + 1).read_to_end(&mut bytes)?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if text.starts_with('\u{feff}') {
        text.drain(..'\u{feff}'.len_utf8());
    }
    Ok(text)
}

fn zip_error(e: zip::result::ZipError) -> Error {
    Error::Malformed(format!("ZIP archive: {e}"))
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn base_name(entry: &str) -> &str {
    entry.rsplit(['/', '\\']).next().unwrap_or(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_chat_file_by_platform_convention() {
        assert_eq!(
            pick_chat_file(["a.jpg", "_chat.txt", "notes.txt"].into_iter()),
            Some("_chat.txt")
        );
        assert_eq!(
            pick_chat_file(["WhatsApp Chat mit Bob.txt", "x.txt"].into_iter()),
            Some("WhatsApp Chat mit Bob.txt")
        );
        assert_eq!(
            pick_chat_file(["only.txt", "a.jpg"].into_iter()),
            Some("only.txt")
        );
        assert_eq!(pick_chat_file(["a.txt", "b.txt"].into_iter()), None);
    }

    #[test]
    fn strips_bom() {
        assert_eq!(read_text("\u{feff}hi".as_bytes(), 5).unwrap(), "hi");
    }
}
