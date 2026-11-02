//! Localized phrases of WhatsApp exports (English and German so far).
//!
//! Matching is best effort: anything not recognised stays ordinary text,
//! and the original line is kept in the message's raw record (ADR 0006).
//! Attachments are detected primarily by file names present in the
//! archive, which works in every language.

use std::sync::LazyLock;

use cv_model::{MediaKind, SystemKind};
use regex::Regex;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("valid regex")
}

/// System message phrases: (kind, language, pattern).
static SYSTEM: LazyLock<Vec<(SystemKind, &'static str, Regex)>> = LazyLock::new(|| {
    use SystemKind::*;
    [
        (
            EncryptionNotice,
            "en",
            r"^Messages and calls are end-to-end encrypted",
        ),
        (
            EncryptionNotice,
            "de",
            r"^Nachrichten und Anrufe sind Ende-zu-Ende-verschlüsselt",
        ),
        (GroupCreated, "en", r#"created (?:group "|this group)"#),
        (
            GroupCreated,
            "de",
            r"(?:hat|hast) (?:die Gruppe .+|diese Gruppe) erstellt",
        ),
        (
            MemberJoined,
            "en",
            r"joined using (?:this group's invite link|a group link)",
        ),
        (
            MemberJoined,
            "de",
            r"(?:ist|bist) .*(?:Einladungslink|Gruppenlink).* beigetreten",
        ),
        (MemberLeft, "en", r"^\S.* left$"),
        (MemberLeft, "de", r"(?:hat|hast) die Gruppe verlassen$"),
        (MemberAdded, "en", r"^\S.* added \S"),
        (MemberAdded, "de", r"(?:hat|hast) .+ hinzugefügt$"),
        (MemberRemoved, "en", r"^\S.* removed \S"),
        (MemberRemoved, "de", r"(?:hat|hast) .+ entfernt$"),
        (
            SubjectChanged,
            "en",
            r#"changed (?:the subject|the group name|this group's name)"#,
        ),
        (
            SubjectChanged,
            "de",
            r"(?:hat|hast) den (?:Betreff|Gruppennamen)",
        ),
        (
            IconChanged,
            "en",
            r"changed (?:this group's icon|the group icon)",
        ),
        (IconChanged, "de", r"(?:hat|hast) das Gruppenbild geändert"),
        (DescriptionChanged, "en", r"changed the group description"),
        (
            DescriptionChanged,
            "de",
            r"(?:hat|hast) die Gruppenbeschreibung geändert",
        ),
        (SecurityCodeChanged, "en", r"security code .*changed"),
        (SecurityCodeChanged, "de", r"Sicherheitsnummer .*geändert"),
        (DisappearingMessages, "en", r"(?i)disappearing messages"),
        (
            DisappearingMessages,
            "de",
            r"(?i)selbstlöschende nachrichten|verschwindende nachrichten",
        ),
        (MissedCall, "en", r"^Missed (?:group )?(?:voice|video) call"),
        (
            MissedCall,
            "de",
            r"^Verpasster (?:Gruppen-?)?(?:[Ss]prach|[Vv]ideo)anruf",
        ),
    ]
    .into_iter()
    .map(|(kind, lang, p)| (kind, lang, re(p)))
    .collect()
});

/// Classifies a system message text.
pub fn system_kind(text: &str) -> Option<(SystemKind, &'static str)> {
    SYSTEM
        .iter()
        .find(|(_, _, re)| re.is_match(text))
        .map(|(kind, lang, _)| (*kind, *lang))
}

/// Deleted-message placeholders: (language, deleted by the owner).
const DELETED: &[(&str, &str, bool)] = &[
    ("This message was deleted", "en", false),
    ("You deleted this message", "en", true),
    ("Diese Nachricht wurde gelöscht", "de", false),
    ("Du hast diese Nachricht gelöscht", "de", true),
];

/// Returns (language, deleted by owner) if the text is a deletion placeholder.
pub fn deleted(text: &str) -> Option<(&'static str, bool)> {
    let text = text.trim_end_matches('.');
    DELETED
        .iter()
        .find(|(phrase, _, _)| *phrase == text)
        .map(|(_, lang, own)| (*lang, *own))
}

static EDITED: LazyLock<Regex> = LazyLock::new(|| {
    re(r"\s*\u{200e}?<(?:This message was edited|Diese Nachricht wurde bearbeitet)\.?>$")
});

/// Removes a trailing "message edited" marker. Returns the remaining text.
pub fn strip_edited(text: &str) -> Option<&str> {
    EDITED.find(text).map(|m| &text[..m.start()])
}

/// Omitted-media placeholders: (pattern, kind, language).
static OMITTED: LazyLock<Vec<(Regex, MediaKind, &'static str)>> = LazyLock::new(|| {
    use MediaKind::*;
    [
        (r"^<Media omitted>$", Other, "en"),
        (r"^<Medien ausgeschlossen>$", Other, "de"),
        (r"^image omitted$", Image, "en"),
        (r"^video omitted$", Video, "en"),
        (r"^audio omitted$", Audio, "en"),
        (r"^sticker omitted$", Sticker, "en"),
        (r"^GIF omitted$", Gif, "en"),
        (r"document omitted$", Document, "en"),
        (r"^Contact card omitted$", Contact, "en"),
        (r"^Bild weggelassen$", Image, "de"),
        (r"^Video weggelassen$", Video, "de"),
        (r"^Audio weggelassen$", Audio, "de"),
        (r"^Sticker weggelassen$", Sticker, "de"),
        (r"^GIF weggelassen$", Gif, "de"),
        (r"Dokument weggelassen$", Document, "de"),
        (r"^Kontaktkarte weggelassen$", Contact, "de"),
    ]
    .into_iter()
    .map(|(p, kind, lang)| (re(p), kind, lang))
    .collect()
});

pub fn omitted(text: &str) -> Option<(MediaKind, &'static str)> {
    OMITTED
        .iter()
        .find(|(re, _, _)| re.is_match(text))
        .map(|(_, kind, lang)| (*kind, *lang))
}

/// iOS: `<attached: 00000012-PHOTO-….jpg>`, anywhere in the message
/// (a caption comes before it on the same line). Group 1 is the label.
pub static IOS_ATTACHMENT: LazyLock<Regex> =
    LazyLock::new(|| re(r"\u{200e}?<([^<>:\n]{1,40}):\s?([^<>\n]+?)>"));

/// iOS marker labels meaning "attached" in the supported languages.
const ATTACHED_LABELS: &[&str] = &["attached", "Anhang"];

pub fn is_attached_label(label: &str) -> bool {
    ATTACHED_LABELS.contains(&label)
}

static DOCUMENT_INFO: LazyLock<Regex> = LazyLock::new(|| re(r"^[^\n]+? • \u{200e}?\d+ \S+$"));

/// iOS shows `name.pdf • ‎12 pages` before a document; that is metadata,
/// not a caption.
pub fn is_document_info(text: &str) -> bool {
    DOCUMENT_INFO.is_match(text)
}
/// Old iOS: `2015-08-04-PHOTO-00004762.jpg <attached>`.
pub static OLD_IOS_ATTACHMENT: LazyLock<Regex> =
    LazyLock::new(|| re(r"^(\S+\.[A-Za-z0-9]{1,8}) <\u{200e}?[^<>\n]{1,30}>"));
/// Android: `IMG-20231231-WA0001.jpg (file attached)` in any language.
pub static ANDROID_ATTACHMENT: LazyLock<Regex> =
    LazyLock::new(|| re(r"^([^\n]+?\.[A-Za-z0-9]{1,8}) \([^()\n]{1,40}\)$"));

/// Android's "(file attached)" in the supported languages.
const ATTACHED_PHRASES: &[&str] = &["(file attached)", "(Datei angehängt)"];

pub fn is_attached_phrase(line: &str) -> bool {
    ATTACHED_PHRASES
        .iter()
        .any(|p| line.trim_end().ends_with(p))
}

static LOCATION: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"^(?:location|Location|Standort): (https?://\S*?(?:[?&]q=|@)(-?\d+(?:\.\d+)?),\s?(-?\d+(?:\.\d+)?)\S*)$",
    )
});

/// Returns (url, latitude, longitude) for a shared location.
pub fn location(text: &str) -> Option<(String, f64, f64)> {
    let caps = LOCATION.captures(text)?;
    Some((
        caps[1].to_owned(),
        caps[2].parse().ok()?,
        caps[3].parse().ok()?,
    ))
}

static POLL_OPTION: LazyLock<Regex> =
    LazyLock::new(|| re(r"^OPTION: (.*) \((\d+) (?:votes?|Stimmen?)\)$"));

/// Poll options as (text, votes).
pub type PollOptions = Vec<(String, Option<u32>)>;

/// Parses an Android poll: `POLL:` / `UMFRAGE:`, question, `OPTION:` lines.
pub fn poll(text: &str) -> Option<(String, PollOptions)> {
    let mut lines = text.lines();
    let first = lines.next()?.trim();
    if first != "POLL:" && first != "UMFRAGE:" {
        return None;
    }
    let question = lines.next()?.to_owned();
    let mut options = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let caps = POLL_OPTION.captures(line)?;
        options.push((caps[1].to_owned(), caps[2].parse().ok()));
    }
    (!options.is_empty()).then_some((question, options))
}

/// Media kind from an exported file name (iOS and Android conventions).
pub fn media_kind(file_name: &str) -> MediaKind {
    let upper = file_name.to_ascii_uppercase();
    let ext = upper.rsplit('.').next().unwrap_or("");
    if upper.starts_with("STK-") || upper.contains("-STICKER-") {
        MediaKind::Sticker
    } else if upper.starts_with("PTT-") || (upper.contains("-AUDIO-") && ext == "OPUS") {
        MediaKind::Voice
    } else if upper.contains("-GIF-") {
        MediaKind::Gif
    } else if upper.starts_with("IMG-") || upper.contains("-PHOTO-") {
        MediaKind::Image
    } else if upper.starts_with("VID-") || upper.contains("-VIDEO-") {
        MediaKind::Video
    } else if upper.starts_with("AUD-") || upper.contains("-AUDIO-") {
        MediaKind::Audio
    } else {
        match ext {
            "JPG" | "JPEG" | "PNG" | "HEIC" | "WEBP" => MediaKind::Image,
            "MP4" | "MOV" | "3GP" => MediaKind::Video,
            "OPUS" | "OGG" => MediaKind::Voice,
            "M4A" | "AAC" | "MP3" | "WAV" => MediaKind::Audio,
            "VCF" => MediaKind::Contact,
            "GIF" => MediaKind::Gif,
            _ => MediaKind::Document,
        }
    }
}

/// MIME type guessed from the extension.
pub fn mime_type(file_name: &str) -> Option<&'static str> {
    let ext = file_name.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "3gp" => "video/3gpp",
        "opus" => "audio/ogg",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "mp3" => "audio/mpeg",
        "pdf" => "application/pdf",
        "vcf" => "text/vcard",
        "txt" => "text/plain",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_phrases_in_both_languages() {
        assert_eq!(
            system_kind("Messages and calls are end-to-end encrypted. No one outside…")
                .unwrap()
                .0,
            SystemKind::EncryptionNotice
        );
        assert_eq!(
            system_kind("Alice hat die Gruppe verlassen").unwrap(),
            (SystemKind::MemberLeft, "de")
        );
        assert_eq!(deleted("You deleted this message."), Some(("en", true)));
        assert_eq!(
            strip_edited("Hallo <Diese Nachricht wurde bearbeitet>"),
            Some("Hallo")
        );
        assert_eq!(strip_edited("Hallo"), None);
        assert_eq!(omitted("Bild weggelassen"), Some((MediaKind::Image, "de")));
    }

    #[test]
    fn parses_location_and_poll() {
        let (_, lat, lon) = location("location: https://maps.google.com/?q=52.52,13.405").unwrap();
        assert_eq!((lat, lon), (52.52, 13.405));
        let (q, options) =
            poll("POLL:\nLunch?\nOPTION: Pizza (2 votes)\nOPTION: Salad (0 votes)").unwrap();
        assert_eq!(q, "Lunch?");
        assert_eq!(
            options,
            [("Pizza".into(), Some(2)), ("Salad".into(), Some(0))]
        );
    }

    #[test]
    fn classifies_media_names() {
        assert_eq!(media_kind("PTT-20240101-WA0001.opus"), MediaKind::Voice);
        assert_eq!(
            media_kind("00000012-AUDIO-2024-01-01-10-00-00.opus"),
            MediaKind::Voice
        );
        assert_eq!(media_kind("STK-20240101-WA0001.webp"), MediaKind::Sticker);
        assert_eq!(
            media_kind("00000013-PHOTO-2024-01-01-10-00-00.jpg"),
            MediaKind::Image
        );
        assert_eq!(media_kind("Report.pdf"), MediaKind::Document);
    }
}
