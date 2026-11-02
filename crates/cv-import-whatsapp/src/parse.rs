//! Turns the chat text of an export into the neutral model.

use std::collections::{BTreeMap, HashMap};

use chrono::NaiveDateTime;
use cv_core::ImportOptions;
use cv_model::{
    Attachment, Author, Availability, BlobRef, Content, Conversation, ConversationData,
    ConversationId, ConversationKind, Integrity, Location, MediaKind, Message, MessageFlags,
    MessageId, Participant, ParticipantId, Poll, PollOption, RawRecord, RichText, SelfFlag,
    SourceExtra, SourceReport, SystemEvent, SystemKind, Timestamp,
};

use crate::container::Container;
use crate::header::{self, DateOrder, Header, Style};
use crate::locale;

/// WhatsApp's documented export caps (with / without media).
const EXPORT_LIMITS: [usize; 2] = [10_000, 40_000];

struct RawMessage<'a> {
    header: Header,
    lines: Vec<&'a str>,
}

/// A message after the author/system split.
struct Split {
    author: Option<String>,
    body: String,
    system: bool,
}

pub fn parse(container: &Container, options: &ImportOptions) -> (ConversationData, SourceReport) {
    let mut counters: BTreeMap<String, u64> = BTreeMap::new();
    let mut languages: HashMap<&'static str, u32> = HashMap::new();
    let mut warnings = Vec::new();

    // 1. Group lines into messages.
    let mut raw: Vec<RawMessage<'_>> = Vec::new();
    let mut preamble = 0u64;
    for line in container.chat_text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(header) = header::parse_header(line) {
            raw.push(RawMessage {
                header,
                lines: vec![line],
            });
        } else if let Some(current) = raw.last_mut() {
            current.lines.push(line);
            bump(&mut counters, "continuation_lines", 1);
        } else if !line.trim().is_empty() {
            preamble += 1;
        }
    }
    // A trailing newline produces an empty continuation line.
    if let Some(last) = raw.last_mut() {
        while last.lines.len() > 1 && last.lines.last().is_some_and(|l| l.is_empty()) {
            last.lines.pop();
        }
    }
    if preamble > 0 {
        bump(&mut counters, "unrecognised_lines", preamble);
        warnings.push(format!(
            "{preamble} lines before the first message were not recognised"
        ));
    }

    // 2. Style and date order.
    let bracket = raw
        .iter()
        .filter(|m| m.header.style == Style::Bracket)
        .count();
    let style = if bracket * 2 >= raw.len() && !raw.is_empty() {
        Style::Bracket
    } else {
        Style::Dash
    };
    if bracket != 0 && bracket != raw.len() {
        warnings.push("the file mixes iOS and Android line formats".into());
    }
    let twelve_hour = raw.iter().any(|m| m.header.pm.is_some());
    let headers: Vec<&Header> = raw.iter().map(|m| &m.header).collect();
    let (order, order_confidence) = match options.get("date_order").and_then(DateOrder::parse) {
        Some(order) => (order, 100),
        None => {
            let guess = header::infer_date_order(&headers, twelve_hour);
            if guess.ambiguous && raw.len() > 1 {
                warnings.push(format!(
                    "the date order could not be determined from the data; assumed {}",
                    guess.order.label().to_uppercase()
                ));
            }
            (guess.order, guess.confidence)
        }
    };

    // 3. Split author and text. Frequent prefixes are real authors.
    let mut prefix_counts: HashMap<&str, u32> = HashMap::new();
    for m in &raw {
        if let Some((prefix, _)) = author_candidate(&m.header.rest) {
            *prefix_counts.entry(prefix).or_default() += 1;
        }
    }
    let splits: Vec<Split> = raw
        .iter()
        .map(|m| split(m, style, &prefix_counts))
        .collect();

    // 4. Participants: authors of non-system messages, in order of appearance.
    let mut participants: Vec<Participant> = Vec::new();
    let mut ids: HashMap<String, ParticipantId> = HashMap::new();
    for s in splits.iter().filter(|s| !s.system) {
        let name = s.author.clone().unwrap_or_default();
        ids.entry(name.clone()).or_insert_with(|| {
            let id = ParticipantId::new(format!("p{}", participants.len() + 1));
            participants.push(Participant {
                id: id.clone(),
                display_name: name,
                is_self: SelfFlag::Unknown,
                handle: None,
                extra: SourceExtra::default(),
            });
            id
        });
    }
    // iOS writes the group (or contact) name as author of system lines.
    let system_authors: Vec<&str> = splits
        .iter()
        .filter(|s| s.system)
        .filter_map(|s| s.author.as_deref())
        .collect();
    let group_name = system_authors
        .iter()
        .find(|a| !ids.contains_key(**a))
        .map(|a| (*a).to_owned());

    // 5. Messages.
    let mut messages = Vec::with_capacity(raw.len());
    let mut last_time: Option<NaiveDateTime> = None;
    let mut own_names: Vec<String> = Vec::new();
    for (seq, (m, s)) in raw.iter().zip(&splits).enumerate() {
        let time = match header::to_datetime(&m.header, order) {
            Some(t) => t,
            None => {
                bump(&mut counters, "invalid_dates", 1);
                last_time.unwrap_or_default()
            }
        };
        last_time = Some(time);
        let author_id = s.author.as_ref().and_then(|a| ids.get(a)).cloned();

        let mut extra = SourceExtra {
            raw: Some(RawRecord::Text {
                format: "whatsapp-txt".into(),
                text: m.lines.join("\n"),
            }),
            fields: BTreeMap::new(),
        };
        let mut flags = MessageFlags::default();

        let (author, content) = if s.system {
            bump(&mut counters, "system_messages", 1);
            let text = header::strip_marks(&s.body).to_owned();
            let kind = locale::system_kind(&text).map_or(SystemKind::Other, |(kind, lang)| {
                *languages.entry(lang).or_default() += 1;
                kind
            });
            if let Some(a) = &s.author {
                extra
                    .fields
                    .insert("whatsapp.system_author".into(), a.clone().into());
            }
            let actors = author_id.clone().into_iter().collect();
            (
                Author::System,
                Content::System(SystemEvent { kind, actors, text }),
            )
        } else {
            let mut body = header::strip_marks(&s.body);
            if let Some(stripped) = locale::strip_edited(body) {
                flags.edited = true;
                body = stripped;
                bump(&mut counters, "edited", 1);
            }
            let content = content(body, container, &mut languages, &mut counters);
            if let Content::Deleted { .. } = content
                && locale::deleted(body).is_some_and(|(_, own)| own)
            {
                own_names.extend(s.author.clone());
            }
            (
                Author::Participant(
                    author_id
                        .clone()
                        .unwrap_or_else(|| ParticipantId::new("p0")),
                ),
                content,
            )
        };

        messages.push(Message {
            id: MessageId::new(format!("m{seq}")),
            seq: seq as u64,
            timestamp: Timestamp::NaiveLocal(time),
            author,
            content,
            quote: None,
            reactions: Vec::new(),
            flags,
            extra,
        });
    }

    // 6. Who is the archive owner?
    let self_name = options.self_name.clone().or_else(|| {
        own_names.sort();
        own_names.dedup();
        (own_names.len() == 1).then(|| own_names[0].clone())
    });
    if let Some(name) = &self_name {
        for p in &mut participants {
            p.is_self = if &p.display_name == name {
                SelfFlag::Yes
            } else {
                SelfFlag::No
            };
        }
    }

    // 7. Conversation.
    let kind = if participants.len() > 2
        || group_name.is_some()
        || messages.iter().any(|m| {
            matches!(&m.content, Content::System(e) if matches!(e.kind,
                SystemKind::GroupCreated | SystemKind::MemberAdded | SystemKind::MemberLeft
                | SystemKind::MemberRemoved | SystemKind::SubjectChanged))
        }) {
        ConversationKind::Group
    } else {
        ConversationKind::Direct
    };
    // iOS names the contact (or group) as author of system lines.
    let title = group_name
        .or_else(|| title_from_file_name(&container.chat_file_name))
        .or_else(|| {
            container
                .container_name
                .as_deref()
                .and_then(title_from_file_name)
        })
        .or_else(|| system_authors.first().map(|a| (*a).to_owned()))
        .unwrap_or_else(|| "WhatsApp chat".into());
    for (name, lang) in [(" with ", "en"), (" mit ", "de")] {
        if container.chat_file_name.contains(name) {
            *languages.entry(lang).or_default() += 1;
        }
    }

    // 8. Report.
    bump(&mut counters, "messages", messages.len() as u64);
    if EXPORT_LIMITS.contains(&messages.len()) {
        warnings.push(format!(
            "the chat has exactly {} messages, WhatsApp's export limit; older messages may be missing",
            messages.len()
        ));
    }
    let platform = match style {
        Style::Bracket => "ios",
        Style::Dash => "android",
    };
    let language = languages
        .iter()
        .max_by_key(|(lang, n)| (**n, std::cmp::Reverse(**lang)))
        .map_or("unknown", |(lang, _)| *lang);
    let clock = if twelve_hour { "12h" } else { "24h" };
    let mut limitations = vec![
        "reactions are not included in WhatsApp exports".to_owned(),
        "replies are not linked to the quoted message".to_owned(),
        "edit history is not included (only an 'edited' marker)".to_owned(),
        "times are local to the exporting phone, without time zone".to_owned(),
    ];
    if self_name.is_none() {
        limitations.push("the export does not say which participant is you".into());
    }
    if container.media.is_empty() {
        limitations.push("this export contains no media files".into());
    }
    let parse_rate = if raw.is_empty() { 0 } else { 100 };
    let report = SourceReport {
        app: "whatsapp".into(),
        platform: platform.into(),
        variant: format!("whatsapp-{platform}-{clock}-{}-{language}", order.label()),
        source_version: None,
        confidence: order_confidence.min(parse_rate),
        warnings,
        limitations,
        counters,
    };
    log::debug!(
        "whatsapp: {} messages, variant {}, {} participants",
        messages.len(),
        report.variant,
        participants.len()
    );

    let conversation = Conversation {
        id: ConversationId::new("whatsapp"),
        kind,
        title,
        participants: participants.iter().map(|p| p.id.clone()).collect(),
        extra: SourceExtra::default(),
    };
    (
        ConversationData {
            conversation,
            participants,
            messages,
        },
        report,
    )
}

/// `Name: text`, or `Name:` for a message without text.
fn author_candidate(rest: &str) -> Option<(&str, &str)> {
    rest.split_once(": ")
        .or_else(|| rest.strip_suffix(':').map(|author| (author, "")))
}

fn split(m: &RawMessage<'_>, style: Style, prefix_counts: &HashMap<&str, u32>) -> Split {
    let rest = &m.header.rest;
    let continuation = m.lines[1..].join("\n");
    let with_continuation = |body: &str| {
        if m.lines.len() > 1 {
            format!("{body}\n{continuation}")
        } else {
            body.to_owned()
        }
    };
    let candidate = author_candidate(rest);
    match style {
        Style::Dash => {
            let frequent = candidate.is_some_and(|(a, _)| prefix_counts.get(a).copied() >= Some(2));
            match candidate {
                Some((author, body)) if frequent || locale::system_kind(rest).is_none() => Split {
                    author: Some(author.to_owned()),
                    body: with_continuation(body),
                    system: false,
                },
                _ => Split {
                    author: None,
                    body: with_continuation(rest),
                    system: true,
                },
            }
        }
        Style::Bracket => match candidate {
            Some((author, body)) => {
                // iOS marks system texts with a leading U+200E.
                let marked = body.starts_with('\u{200e}');
                let system = marked && locale::system_kind(header::strip_marks(body)).is_some();
                Split {
                    author: Some(author.to_owned()),
                    body: with_continuation(body),
                    system,
                }
            }
            None => Split {
                author: None,
                body: with_continuation(rest),
                system: true,
            },
        },
    }
}

fn content(
    body: &str,
    container: &Container,
    languages: &mut HashMap<&'static str, u32>,
    counters: &mut BTreeMap<String, u64>,
) -> Content {
    let mut lang = |l: &'static str| *languages.entry(l).or_default() += 1;

    if let Some((l, _)) = locale::deleted(body) {
        lang(l);
        bump(counters, "deleted", 1);
        return Content::Deleted { by: None };
    }
    if let Some((question, options)) = locale::poll(body) {
        bump(counters, "polls", 1);
        return Content::Poll(Poll {
            question,
            options: options
                .into_iter()
                .map(|(text, votes)| PollOption { text, votes })
                .collect(),
        });
    }
    if let Some((url, latitude, longitude)) = locale::location(body) {
        bump(counters, "locations", 1);
        return Content::Location(Location {
            latitude,
            longitude,
            name: None,
            url: Some(url),
        });
    }

    // Attachments, recognised by markers whose file name is in the archive
    // or looks like an exported media name.
    let first_line = body.lines().next().unwrap_or("");
    // `explicit`: the marker itself says "attached", so the file name
    // need not be in the archive or look like an exported media name.
    let span = |c: &regex::Captures<'_>| {
        let m = c.get(0).expect("match");
        (m.start(), m.end())
    };
    let found = locale::IOS_ATTACHMENT
        .captures(body)
        .map(|c| (span(&c), c[2].to_owned(), locale::is_attached_label(&c[1])))
        .or_else(|| {
            locale::OLD_IOS_ATTACHMENT
                .captures(first_line)
                .map(|c| (span(&c), c[1].to_owned(), false))
        })
        .or_else(|| {
            locale::ANDROID_ATTACHMENT.captures(first_line).map(|c| {
                let explicit = locale::is_attached_phrase(first_line);
                (span(&c), c[1].to_owned(), explicit)
            })
        });
    if let Some(((start, end), file, explicit)) = found {
        let file = file.trim();
        let file = header::strip_marks(file).to_owned();
        let available = container.media.get(&file);
        let looks_like_media = explicit || available.is_some() || is_export_media_name(&file);
        if looks_like_media {
            let availability = match available {
                Some(entry) => {
                    bump(counters, "attachments_available", 1);
                    Availability::Available {
                        blob: BlobRef::new(entry.clone()),
                        integrity: Integrity::Unchecked,
                    }
                }
                None => {
                    bump(counters, "attachments_missing", 1);
                    Availability::Missing
                }
            };
            let kind = locale::media_kind(&file);
            let mut attachment = Attachment::new(kind, availability);
            attachment.mime = locale::mime_type(&file).map(str::to_owned);
            attachment.file_name = Some(file);
            // iOS puts the caption before the marker, Android after it.
            let caption = format!("{}{}", &body[..start], &body[end..]);
            let caption = caption.trim_matches(|c: char| c.is_whitespace() || c == '\u{200e}');
            let caption = (!caption.is_empty() && !locale::is_document_info(caption))
                .then(|| RichText::plain(caption));
            return media_content(attachment, caption);
        }
    }
    if let Some((kind, l)) = locale::omitted(first_line.trim()) {
        lang(l);
        bump(counters, "attachments_omitted", 1);
        let caption = body
            .split_once('\n')
            .map(|(_, rest)| rest.trim())
            .filter(|c| !c.is_empty());
        return media_content(
            Attachment::new(kind, Availability::Omitted),
            caption.map(RichText::plain),
        );
    }
    Content::Text(RichText::plain(body))
}

fn bump(counters: &mut BTreeMap<String, u64>, key: &str, n: u64) {
    *counters.entry(key.to_owned()).or_default() += n;
}

fn media_content(attachment: Attachment, caption: Option<RichText>) -> Content {
    if attachment.kind == MediaKind::Sticker {
        Content::Sticker {
            emoji: None,
            pack_id: None,
            sticker_id: None,
            image: attachment,
        }
    } else {
        Content::Media {
            attachments: vec![attachment],
            caption,
        }
    }
}

/// File names WhatsApp generates for exported media, used when the file
/// itself is not in the archive.
fn is_export_media_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["IMG-", "VID-", "AUD-", "PTT-", "STK-", "DOC-"]
        .iter()
        .any(|p| upper.starts_with(p))
        || ["-PHOTO-", "-VIDEO-", "-AUDIO-", "-STICKER-", "-GIF-"]
            .iter()
            .any(|p| upper.contains(p))
        || upper.ends_with(".VCF")
}

/// `WhatsApp Chat with Bob.txt`, `WhatsApp Chat mit Bob.txt`,
/// `WhatsApp Chat - Bob.zip` → `Bob`.
fn title_from_file_name(name: &str) -> Option<String> {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let rest = stem.strip_prefix("WhatsApp Chat")?;
    let rest = rest
        .strip_prefix(" - ")
        .or_else(|| rest.strip_prefix(" with "))
        .or_else(|| rest.strip_prefix(" mit "))?;
    (!rest.is_empty()).then(|| rest.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_from_file_names() {
        assert_eq!(
            title_from_file_name("WhatsApp Chat with Bob.txt").as_deref(),
            Some("Bob")
        );
        assert_eq!(
            title_from_file_name("WhatsApp Chat mit Bob.txt").as_deref(),
            Some("Bob")
        );
        assert_eq!(
            title_from_file_name("WhatsApp Chat - Team.zip").as_deref(),
            Some("Team")
        );
        assert_eq!(title_from_file_name("_chat.txt"), None);
    }
}
