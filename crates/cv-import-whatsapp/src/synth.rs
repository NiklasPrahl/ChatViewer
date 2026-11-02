//! Synthetic WhatsApp exports in many format variants, with the ground
//! truth needed to check the parser. Fictional names, random words, tiny
//! fake media files; never real data.

use std::io::Write;
use std::path::Path;

use chrono::{Duration, NaiveDate, NaiveDateTime, Timelike};
use cv_model::{MediaKind, SystemKind};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::header::DateOrder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Ios,
    Android,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

#[derive(Debug, Clone, Copy)]
pub struct Variant {
    pub platform: Platform,
    pub lang: Lang,
    pub twelve_hour: bool,
    pub order: DateOrder,
    pub four_digit_year: bool,
    /// Narrow no-break space before AM/PM (newer iOS/Android).
    pub nnbsp: bool,
    pub crlf: bool,
    pub bom: bool,
    pub group: bool,
}

impl Variant {
    /// All supported combinations (German exports use a 24-hour clock).
    pub fn matrix() -> Vec<Variant> {
        let mut out = Vec::new();
        let mut i = 0;
        for platform in [Platform::Ios, Platform::Android] {
            for lang in [Lang::En, Lang::De] {
                for twelve_hour in [false, true] {
                    if lang == Lang::De && twelve_hour {
                        continue;
                    }
                    for order in [DateOrder::Dmy, DateOrder::Mdy, DateOrder::Ymd] {
                        for four_digit_year in [false, true] {
                            if order == DateOrder::Ymd && !four_digit_year {
                                continue;
                            }
                            i += 1;
                            out.push(Variant {
                                platform,
                                lang,
                                twelve_hour,
                                order,
                                four_digit_year,
                                nnbsp: i % 2 == 0,
                                crlf: i % 3 == 0,
                                bom: i % 4 == 0,
                                group: i % 2 == 1,
                            });
                        }
                    }
                }
            }
        }
        out
    }
}

/// What a synthetic message is, independent of its rendering.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Text(String),
    Media {
        file: String,
        kind: MediaKind,
        caption: Option<String>,
        /// Whether the file is in the archive.
        included: bool,
    },
    Omitted(MediaKind),
    Deleted {
        own: bool,
    },
    Location(f64, f64),
    Poll {
        question: String,
        options: Vec<(String, u32)>,
    },
    System(SystemKind),
}

#[derive(Debug, Clone)]
pub struct SynthMessage {
    /// `None` for system messages.
    pub author: Option<&'static str>,
    pub time: NaiveDateTime,
    pub kind: Kind,
    pub edited: bool,
    /// The rendered system text (for system messages).
    pub system_text: Option<String>,
}

pub struct SynthExport {
    pub variant: Variant,
    pub title: String,
    pub self_name: &'static str,
    pub participants: Vec<&'static str>,
    pub messages: Vec<SynthMessage>,
    pub chat_file_name: String,
    pub zip_name: String,
    pub text: String,
    pub media: Vec<(String, Vec<u8>)>,
}

const WORDS: &[&str] = &[
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "tempor",
    "magna",
    "aliqua",
    "enim",
    "minim",
    "veniam",
    "quis",
    "nostrud",
    "ullamco",
    "laboris",
    "nisi",
    "commodo",
    "consequat",
];

pub fn generate(variant: Variant, seed: u64, count: usize) -> SynthExport {
    let mut rng = StdRng::seed_from_u64(seed);
    let participants: Vec<&'static str> = if variant.group {
        vec!["Alice Example", "Bob Sample", "Carol Test"]
    } else {
        vec!["Alice Example", "Bob Sample"]
    };
    let title = if variant.group {
        "Synth Team"
    } else {
        "Bob Sample"
    }
    .to_owned();
    let mut time = NaiveDate::from_ymd_opt(2023, 1, 5)
        .unwrap()
        .and_hms_opt(9, 15, 0)
        .unwrap();
    let mut messages = Vec::new();
    let mut media = Vec::new();
    let mut media_n = 0u32;

    let push_system = |time, kind, messages: &mut Vec<SynthMessage>| {
        messages.push(SynthMessage {
            author: None,
            time,
            kind: Kind::System(kind),
            edited: false,
            system_text: Some(system_text(variant.lang, kind)),
        });
    };
    push_system(time, SystemKind::EncryptionNotice, &mut messages);
    if variant.group {
        time += Duration::minutes(1);
        push_system(time, SystemKind::GroupCreated, &mut messages);
    }

    while messages.len() < count {
        time += Duration::minutes(rng.random_range(1..60 * 30));
        if !variant.twelve_hour && variant.platform == Platform::Android {
            time = time.with_second(0).unwrap();
        }
        let time = if variant.platform == Platform::Android {
            time.with_second(0).unwrap()
        } else {
            time
        };
        let author = participants[rng.random_range(0..participants.len())];
        let roll = rng.random_range(0..100);
        let kind = match roll {
            0..=49 => Kind::Text(sentence(&mut rng, roll % 7 == 0)),
            50..=64 => {
                media_n += 1;
                let (file, kind) = media_name(variant.platform, &mut rng, media_n, time);
                let included = roll != 64;
                if included {
                    media.push((file.clone(), fake_media(kind, media_n)));
                }
                let caption = (roll % 3 == 0 && kind != MediaKind::Sticker)
                    .then(|| sentence(&mut rng, false));
                Kind::Media {
                    file,
                    kind,
                    caption,
                    included,
                }
            }
            65..=69 => Kind::Omitted(match variant.platform {
                Platform::Android => MediaKind::Other,
                Platform::Ios => {
                    [MediaKind::Image, MediaKind::Video, MediaKind::Audio][rng.random_range(0..3)]
                }
            }),
            70..=74 => Kind::Deleted {
                own: author == participants[0],
            },
            75..=77 => Kind::Location(52.5 + f64::from(roll) / 100.0, 13.4),
            78..=80 if variant.platform == Platform::Android => Kind::Poll {
                question: "Lunch today?".into(),
                options: vec![("Pizza".into(), 2), ("Salad".into(), 1), ("Soup".into(), 0)],
            },
            81..=83 if variant.group => {
                push_system(time, SystemKind::MemberAdded, &mut messages);
                continue;
            }
            _ => Kind::Text(sentence(&mut rng, false)),
        };
        let edited = matches!(kind, Kind::Text(_)) && roll % 11 == 0;
        messages.push(SynthMessage {
            author: Some(author),
            time,
            kind,
            edited,
            system_text: None,
        });
    }

    let chat_file_name = match (variant.platform, variant.lang) {
        (Platform::Ios, _) => "_chat.txt".to_owned(),
        (Platform::Android, Lang::En) => format!("WhatsApp Chat with {title}.txt"),
        (Platform::Android, Lang::De) => format!("WhatsApp Chat mit {title}.txt"),
    };
    let mut export = SynthExport {
        variant,
        zip_name: format!("WhatsApp Chat - {title}.zip"),
        title,
        self_name: participants[0],
        participants,
        messages,
        chat_file_name,
        text: String::new(),
        media,
    };
    export.text = render(&export);
    export
}

fn words(rng: &mut StdRng, n: usize) -> String {
    let w: Vec<&str> = (0..n)
        .map(|_| WORDS[rng.random_range(0..WORDS.len())])
        .collect();
    let mut s = w.join(" ");
    s[..1].make_ascii_uppercase();
    s
}

fn sentence(rng: &mut StdRng, multi_line: bool) -> String {
    let n = rng.random_range(2..10);
    let first = words(rng, n);
    if multi_line {
        // Continuation lines that look a bit like headers must stay text.
        let (a, b) = (words(rng, 3), words(rng, 2));
        format!("{first}\n12.03.23 was a good day\nNote: {a}\n\n- {b}")
    } else {
        first
    }
}

fn media_name(
    platform: Platform,
    rng: &mut StdRng,
    n: u32,
    time: NaiveDateTime,
) -> (String, MediaKind) {
    let kinds = [
        (MediaKind::Image, "IMG", "PHOTO", "jpg"),
        (MediaKind::Video, "VID", "VIDEO", "mp4"),
        (MediaKind::Voice, "PTT", "AUDIO", "opus"),
        (MediaKind::Sticker, "STK", "STICKER", "webp"),
        (MediaKind::Document, "", "", "pdf"),
    ];
    let (kind, android, ios, ext) = kinds[rng.random_range(0..kinds.len())];
    let name = match (platform, kind) {
        (_, MediaKind::Document) => format!("Synthetic report {n}.{ext}"),
        (Platform::Android, _) => format!("{android}-{}-WA{n:04}.{ext}", time.format("%Y%m%d")),
        (Platform::Ios, _) => format!("{n:08}-{ios}-{}.{ext}", time.format("%Y-%m-%d-%H-%M-%S")),
    };
    let name = match (platform, kind) {
        (Platform::Ios, MediaKind::Document) => format!("{n:08}-{name}"),
        _ => name,
    };
    (name, kind)
}

fn fake_media(kind: MediaKind, n: u32) -> Vec<u8> {
    format!("synthetic {kind:?} file {n}\n").into_bytes()
}

pub fn system_text(lang: Lang, kind: SystemKind) -> String {
    use SystemKind::*;
    match (lang, kind) {
        (Lang::En, EncryptionNotice) => "Messages and calls are end-to-end encrypted. No one outside of this chat, not even WhatsApp, can read or listen to them.".into(),
        (Lang::De, EncryptionNotice) => "Nachrichten und Anrufe sind Ende-zu-Ende-verschlüsselt. Niemand außerhalb dieses Chats kann sie lesen oder anhören.".into(),
        (Lang::En, GroupCreated) => "Alice Example created group \"Synth Team\"".into(),
        (Lang::De, GroupCreated) => "Alice Example hat die Gruppe „Synth Team“ erstellt".into(),
        (Lang::En, MemberAdded) => "Alice Example added Dave Demo".into(),
        (Lang::De, MemberAdded) => "Alice Example hat Dave Demo hinzugefügt".into(),
        (_, other) => panic!("no synthetic text for {other:?}"),
    }
}

fn format_time(v: &Variant, t: NaiveDateTime) -> String {
    let (y, m, d) = (
        t.format("%Y").to_string(),
        t.format("%m").to_string(),
        t.format("%d").to_string(),
    );
    let y = if v.four_digit_year {
        y
    } else {
        y[2..].to_owned()
    };
    let date = match v.order {
        DateOrder::Dmy => format!("{d}.{m}.{y}"),
        DateOrder::Mdy => format!(
            "{}/{}/{y}",
            m.trim_start_matches('0'),
            d.trim_start_matches('0')
        ),
        DateOrder::Ymd => format!("{y}-{m}-{d}"),
    };
    let seconds = v.platform == Platform::Ios;
    let time = if v.twelve_hour {
        let space = if v.nnbsp { "\u{202f}" } else { " " };
        let fmt = if seconds { "%-I:%M:%S" } else { "%-I:%M" };
        format!("{}{space}{}", t.format(fmt), t.format("%p"))
    } else if seconds {
        t.format("%H:%M:%S").to_string()
    } else {
        t.format("%H:%M").to_string()
    };
    format!("{date}, {time}")
}

fn render(e: &SynthExport) -> String {
    let v = &e.variant;
    let ios = v.platform == Platform::Ios;
    let lrm = if ios { "\u{200e}" } else { "" };
    let mut lines: Vec<String> = Vec::new();
    for m in &e.messages {
        let when = format_time(v, m.time);
        let prefix = |author: &str| {
            if ios {
                format!("[{when}] {author}: ")
            } else {
                format!("{when} - {author}: ")
            }
        };
        let Some(author) = m.author else {
            let text = m.system_text.clone().unwrap_or_default();
            lines.push(if ios {
                format!("\u{200e}[{when}] {}: \u{200e}{text}", e.title)
            } else {
                format!("{when} - {text}")
            });
            continue;
        };
        let mut body = match &m.kind {
            Kind::Text(t) => t.clone(),
            Kind::Media { file, caption, .. } => {
                let marker = match (v.platform, v.lang) {
                    (Platform::Ios, Lang::En) => format!("\u{200e}<attached: {file}>"),
                    (Platform::Ios, Lang::De) => format!("\u{200e}<Anhang: {file}>"),
                    (Platform::Android, Lang::En) => format!("{file} (file attached)"),
                    (Platform::Android, Lang::De) => format!("{file} (Datei angehängt)"),
                };
                // Real iOS exports put the caption (or a document's
                // "name • N pages" info) before the marker; Android after it.
                let info = match (v.platform, file.split_once('-')) {
                    (Platform::Ios, Some((_, name))) if name.ends_with(".pdf") => {
                        Some(format!("{name} • \u{200e}3 pages "))
                    }
                    _ => None,
                };
                match (v.platform, caption, info) {
                    (Platform::Ios, Some(c), _) => format!("{c} {marker}"),
                    (Platform::Ios, None, Some(info)) => format!("{info}{marker}"),
                    (Platform::Android, Some(c), _) => format!("{marker}\n{c}"),
                    (_, None, _) => marker,
                }
            }
            Kind::Omitted(kind) => match (v.platform, v.lang) {
                (Platform::Android, Lang::En) => "<Media omitted>".into(),
                (Platform::Android, Lang::De) => "<Medien ausgeschlossen>".into(),
                (Platform::Ios, lang) => {
                    let (en, de) = match kind {
                        MediaKind::Image => ("image omitted", "Bild weggelassen"),
                        MediaKind::Video => ("video omitted", "Video weggelassen"),
                        _ => ("audio omitted", "Audio weggelassen"),
                    };
                    format!("\u{200e}{}", if lang == Lang::En { en } else { de })
                }
            },
            Kind::Deleted { own } => {
                let text = match (v.lang, own) {
                    (Lang::En, false) => "This message was deleted",
                    (Lang::En, true) => "You deleted this message",
                    (Lang::De, false) => "Diese Nachricht wurde gelöscht",
                    (Lang::De, true) => "Du hast diese Nachricht gelöscht",
                };
                if ios {
                    format!("\u{200e}{text}.")
                } else {
                    text.to_owned()
                }
            }
            Kind::Location(lat, lon) => {
                let label = match (v.platform, v.lang) {
                    (Platform::Android, _) => "location",
                    (Platform::Ios, Lang::En) => "Location",
                    (Platform::Ios, Lang::De) => "Standort",
                };
                format!("{lrm}{label}: https://maps.google.com/?q={lat},{lon}")
            }
            Kind::Poll { question, options } => {
                let (head, unit) = match v.lang {
                    Lang::En => ("POLL:", ["vote", "votes"]),
                    Lang::De => ("UMFRAGE:", ["Stimme", "Stimmen"]),
                };
                let opts: Vec<String> = options
                    .iter()
                    .map(|(o, n)| format!("OPTION: {o} ({n} {})", unit[usize::from(*n != 1)]))
                    .collect();
                format!("{head}\n{question}\n{}", opts.join("\n"))
            }
            Kind::System(_) => unreachable!("system messages have no author"),
        };
        if m.edited {
            let marker = match v.lang {
                Lang::En => "<This message was edited>",
                Lang::De => "<Diese Nachricht wurde bearbeitet>",
            };
            body = format!("{body} {lrm}{marker}");
        }
        let line_start = if body.starts_with('\u{200e}') {
            "\u{200e}"
        } else {
            ""
        };
        lines.push(format!("{line_start}{}{body}", prefix(author)));
    }
    let newline = if v.crlf { "\r\n" } else { "\n" };
    let mut text = lines.join("\n").replace('\n', newline);
    text.push_str(newline);
    if v.bom {
        text.insert(0, '\u{feff}');
    }
    text
}

/// Writes the export as a ZIP like WhatsApp's "Export chat".
pub fn write_zip(export: &SynthExport, path: &Path) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file(export.chat_file_name.as_str(), options)?;
    zip.write_all(export.text.as_bytes())?;
    for (name, data) in &export.media {
        zip.start_file(name.as_str(), options)?;
        zip.write_all(data)?;
    }
    zip.finish()?;
    Ok(())
}
