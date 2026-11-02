//! Maps Signal backup frames to the neutral model (ADR 0006: map what has
//! a meaning in the model, keep the raw frame for everything else).

use std::collections::HashMap;

use base64::Engine;
use cv_model::{
    Attachment, Author, Availability, BlobRef, Content, Integrity, MediaKind, Message,
    MessageFlags, MessageId, ParticipantId, Poll, PollOption, Quote, RawRecord, Reaction, RichText,
    SourceExtra, SystemEvent, SystemKind, TextRange, TextStyle, Timestamp,
};
use serde_json::json;

use crate::layout::BackupLocation;
use crate::media::LocalMedia;
use crate::proto::backup::{
    ChatItem, ChatUpdateMessage, FilePointer, Reaction as ProtoReaction, Text, body_range,
    chat_item, chat_update_message, direct_story_reply_message, group_call,
    group_change_chat_update, individual_call, message_attachment, simple_chat_update,
};

pub const BLOB_PREFIX: &str = "signal:";

/// What mapping needs to know about recipients.
pub struct Context<'a> {
    pub names: &'a HashMap<u64, String>,
    pub aci_to_recipient: &'a HashMap<[u8; 16], u64>,
    pub location: &'a BackupLocation,
}

impl Context<'_> {
    pub fn participant(id: u64) -> ParticipantId {
        ParticipantId::new(format!("r{id}"))
    }

    fn name(&self, id: u64) -> String {
        self.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "Unknown".into())
    }

    fn by_aci(&self, aci: &[u8]) -> Option<u64> {
        let aci: [u8; 16] = aci.try_into().ok()?;
        self.aci_to_recipient.get(&aci).copied()
    }

    fn name_by_aci(&self, aci: Option<&[u8]>) -> String {
        aci.and_then(|a| self.by_aci(a))
            .map_or_else(|| "Someone".into(), |id| self.name(id))
    }
}

/// Stable message id: Signal identifies messages by author and sent time
/// (quotes refer to them the same way).
pub fn message_id(author_id: u64, date_sent: u64) -> MessageId {
    MessageId::new(format!("{author_id}-{date_sent}"))
}

pub fn raw_record(format: &str, bytes: &[u8]) -> RawRecord {
    RawRecord::Base64 {
        format: format.into(),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

/// Maps one chat item. `known` holds the ids of earlier messages in the
/// conversation, to resolve quote targets.
pub fn chat_item(
    item: &ChatItem,
    raw_frame: &[u8],
    seq: u64,
    ctx: &Context<'_>,
    known: &HashMap<(u64, u64), MessageId>,
) -> Message {
    use chat_item::Item;

    let mut extra = SourceExtra {
        raw: Some(raw_record("signal-backup-frame", raw_frame)),
        fields: Default::default(),
    };
    let mut flags = MessageFlags {
        edited: !item.revisions.is_empty(),
        ..MessageFlags::default()
    };
    if !item.revisions.is_empty() {
        extra
            .fields
            .insert("signal.revisions".into(), item.revisions.len().into());
    }
    if let Some(ms) = item.expires_in_ms {
        extra
            .fields
            .insert("signal.expires_in_ms".into(), ms.into());
    }
    if item.sms {
        extra.fields.insert("signal.sms".into(), true.into());
    }
    let direction = match &item.directional_details {
        Some(chat_item::DirectionalDetails::Incoming(_)) => "incoming",
        Some(chat_item::DirectionalDetails::Outgoing(_)) => "outgoing",
        Some(chat_item::DirectionalDetails::Directionless(_)) | None => "directionless",
    };
    extra
        .fields
        .insert("signal.direction".into(), direction.into());

    let author_participant = Context::participant(item.author_id);
    let mut author = Author::Participant(author_participant.clone());
    let mut quote = None;
    let mut reactions: &[ProtoReaction] = &[];

    let content = match &item.item {
        Some(Item::StandardMessage(m)) => {
            reactions = &m.reactions;
            quote = m.quote.as_ref().map(|q| {
                let target = q
                    .target_sent_timestamp
                    .and_then(|ts| known.get(&(q.author_id, ts)).cloned());
                if q.target_sent_timestamp.is_some() && target.is_none() {
                    extra
                        .fields
                        .insert("signal.quote_target_missing".into(), true.into());
                }
                Quote {
                    target,
                    author: Some(Context::participant(q.author_id)),
                    snippet: q
                        .text
                        .as_ref()
                        .map(|t| rich_text(t, ctx))
                        .unwrap_or_default(),
                }
            });
            if let Some(long_text) = &m.long_text {
                let a = attachment(long_text, None, ctx);
                if let Availability::Available { blob, .. } = &a.availability {
                    extra
                        .fields
                        .insert("signal.long_text_blob".into(), blob.as_str().into());
                }
            }
            if !m.link_preview.is_empty() {
                let previews: Vec<_> = m
                    .link_preview
                    .iter()
                    .map(|p| {
                        let image = p.image.as_ref().map(|i| attachment(i, None, ctx));
                        json!({
                            "url": p.url,
                            "title": p.title,
                            "description": p.description,
                            "date": p.date,
                            "image": image,
                        })
                    })
                    .collect();
                extra
                    .fields
                    .insert("signal.link_previews".into(), previews.into());
            }
            let text = m.text.as_ref().map(|t| rich_text(t, ctx));
            if m.attachments.is_empty() {
                Content::Text(text.unwrap_or_default())
            } else {
                Content::Media {
                    attachments: m
                        .attachments
                        .iter()
                        .filter_map(|a| {
                            let flag = message_attachment::Flag::try_from(a.flag).ok();
                            a.pointer.as_ref().map(|p| attachment(p, flag, ctx))
                        })
                        .collect(),
                    caption: text.filter(|t| !t.text.is_empty()),
                }
            }
        }
        Some(Item::ContactMessage(m)) => {
            reactions = &m.reactions;
            let contact = m.contact.as_ref();
            let mut lines = Vec::new();
            if let Some(name) = contact.and_then(|c| c.name.as_ref()) {
                let full = [&name.given_name, &name.middle_name, &name.family_name]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                lines.push(full);
            }
            if let Some(c) = contact {
                lines.extend(c.number.iter().map(|n| n.value.clone()));
                lines.extend(c.email.iter().map(|e| e.value.clone()));
                if !c.organization.is_empty() {
                    lines.push(c.organization.clone());
                }
            }
            let mut card = match contact.and_then(|c| c.avatar.as_ref()) {
                Some(avatar) => attachment(avatar, None, ctx),
                None => Attachment::new(MediaKind::Contact, Availability::NotInBackup),
            };
            card.kind = MediaKind::Contact;
            Content::Media {
                attachments: vec![card],
                caption: Some(RichText::plain(lines.join("\n"))),
            }
        }
        Some(Item::StickerMessage(m)) => {
            reactions = &m.reactions;
            let sticker = m.sticker.as_ref();
            let mut image = match sticker.and_then(|s| s.data.as_ref()) {
                Some(data) => attachment(data, None, ctx),
                None => Attachment::new(MediaKind::Sticker, Availability::NotInBackup),
            };
            image.kind = MediaKind::Sticker;
            Content::Sticker {
                emoji: sticker.and_then(|s| s.emoji.clone()),
                pack_id: sticker.map(|s| hex::encode(&s.pack_id)),
                sticker_id: sticker.map(|s| s.sticker_id),
                image,
            }
        }
        Some(Item::RemoteDeletedMessage(_)) => Content::Deleted {
            by: Some(author_participant.clone()),
        },
        Some(Item::AdminDeletedMessage(m)) => Content::Deleted {
            by: Some(Context::participant(m.admin_id)),
        },
        Some(Item::UpdateMessage(update)) => {
            author = Author::System;
            Content::System(system_event(update, item.author_id, ctx))
        }
        Some(Item::PaymentNotification(_)) => Content::Unsupported {
            source_kind: "payment".into(),
            note: "Signal payment notification (not shown)".into(),
        },
        Some(Item::GiftBadge(_)) => Content::Unsupported {
            source_kind: "gift_badge".into(),
            note: "Signal gift badge".into(),
        },
        Some(Item::ViewOnceMessage(m)) => {
            reactions = &m.reactions;
            flags.view_once = true;
            Content::Media {
                attachments: m
                    .attachment
                    .iter()
                    .filter_map(|a| a.pointer.as_ref().map(|p| attachment(p, None, ctx)))
                    .collect(),
                caption: None,
            }
        }
        Some(Item::DirectStoryReplyMessage(m)) => {
            reactions = &m.reactions;
            extra
                .fields
                .insert("signal.story_reply".into(), true.into());
            match &m.reply {
                Some(direct_story_reply_message::Reply::TextReply(r)) => Content::Text(
                    r.text
                        .as_ref()
                        .map(|t| rich_text(t, ctx))
                        .unwrap_or_default(),
                ),
                Some(direct_story_reply_message::Reply::Emoji(e)) => {
                    Content::Text(RichText::plain(e.clone()))
                }
                None => Content::Text(RichText::default()),
            }
        }
        Some(Item::Poll(p)) => {
            reactions = &p.reactions;
            extra
                .fields
                .insert("signal.poll_allow_multiple".into(), p.allow_multiple.into());
            extra
                .fields
                .insert("signal.poll_ended".into(), p.has_ended.into());
            Content::Poll(Poll {
                question: p.question.clone(),
                options: p
                    .options
                    .iter()
                    .map(|o| PollOption {
                        text: o.option.clone(),
                        votes: Some(o.votes.len() as u32),
                    })
                    .collect(),
            })
        }
        None => Content::Unsupported {
            source_kind: "unknown".into(),
            note: "message type from a newer Signal version".into(),
        },
    };

    Message {
        id: message_id(item.author_id, item.date_sent),
        seq,
        timestamp: Timestamp::UtcMillis(item.date_sent as i64),
        author,
        content,
        quote,
        reactions: reactions
            .iter()
            .map(|r| Reaction {
                emoji: r.emoji.clone(),
                author: Context::participant(r.author_id),
                timestamp: Some(Timestamp::UtcMillis(r.sent_timestamp as i64)),
            })
            .collect(),
        flags,
        extra,
    }
}

/// Maps a `FilePointer`. Media are decrypted only when opened.
pub fn attachment(
    p: &FilePointer,
    flag: Option<message_attachment::Flag>,
    ctx: &Context<'_>,
) -> Attachment {
    use message_attachment::Flag;

    let mime = p.content_type.clone().filter(|m| !m.is_empty());
    let kind = match (flag, mime.as_deref()) {
        (Some(Flag::VoiceMessage), _) => MediaKind::Voice,
        (Some(Flag::Gif), _) | (_, Some("image/gif")) => MediaKind::Gif,
        (_, Some(m)) if m.starts_with("image/") => MediaKind::Image,
        (_, Some(m)) if m.starts_with("video/") => MediaKind::Video,
        (_, Some(m)) if m.starts_with("audio/") => MediaKind::Audio,
        _ => MediaKind::Document,
    };
    let availability = match LocalMedia::from_pointer(p) {
        Some(local) if ctx.location.media_path(&local.media_name).is_file() => {
            Availability::Available {
                blob: BlobRef::new(format!("{BLOB_PREFIX}{}", local.media_name)),
                // Checked when opened (HMAC, then hash).
                integrity: Integrity::Unchecked,
            }
        }
        Some(_) => Availability::Missing,
        None => Availability::NotInBackup,
    };
    let mut a = Attachment::new(kind, availability);
    a.mime = mime;
    a.file_name = p.file_name.clone().filter(|n| !n.is_empty());
    a.size = p.locator_info.as_ref().map(|l| u64::from(l.size));
    a.width = p.width;
    a.height = p.height;
    a.duration_ms = p.audio_duration_seconds.map(|s| (s * 1000.0) as u64);
    a.waveform = p.audio_waveform.clone();
    a.blurhash = p.blur_hash.clone();
    a
}

/// Signal stores formatting as UTF-16 offsets; the model uses byte offsets.
fn rich_text(t: &Text, ctx: &Context<'_>) -> RichText {
    let mut utf16_to_byte = Vec::with_capacity(t.body.len() + 1);
    for (byte, c) in t.body.char_indices() {
        for _ in 0..c.len_utf16() {
            utf16_to_byte.push(byte);
        }
    }
    utf16_to_byte.push(t.body.len());
    let at = |i: u32| utf16_to_byte.get(i as usize).copied();

    let ranges = t
        .body_ranges
        .iter()
        .filter_map(|r| {
            let start = at(r.start)?;
            let end = at(r.start + r.length)?;
            let style = match r.associated_value.as_ref()? {
                body_range::AssociatedValue::MentionAci(aci) => {
                    TextStyle::Mention(Context::participant(ctx.by_aci(aci)?))
                }
                body_range::AssociatedValue::Style(s) => {
                    match body_range::Style::try_from(*s).ok()? {
                        body_range::Style::Bold => TextStyle::Bold,
                        body_range::Style::Italic => TextStyle::Italic,
                        body_range::Style::Spoiler => TextStyle::Spoiler,
                        body_range::Style::Strikethrough => TextStyle::Strikethrough,
                        body_range::Style::Monospace => TextStyle::Monospace,
                        body_range::Style::None => return None,
                    }
                }
            };
            Some(TextRange {
                start,
                len: end - start,
                style,
            })
        })
        .collect();
    RichText {
        text: t.body.clone(),
        ranges,
    }
}

fn system_event(update: &ChatUpdateMessage, author_id: u64, ctx: &Context<'_>) -> SystemEvent {
    use chat_update_message::Update;

    let author = ctx.name(author_id);
    let actors = vec![Context::participant(author_id)];
    let event = |kind, text: String| SystemEvent {
        kind,
        actors: actors.clone(),
        text,
    };
    match &update.update {
        Some(Update::SimpleUpdate(s)) => {
            use simple_chat_update::Type;
            let (kind, text) = match Type::try_from(s.r#type).unwrap_or(Type::Unknown) {
                Type::JoinedSignal => (SystemKind::Other, format!("{author} joined Signal")),
                Type::IdentityUpdate => (
                    SystemKind::SecurityCodeChanged,
                    format!("Your safety number with {author} has changed"),
                ),
                Type::IdentityVerified => (
                    SystemKind::SecurityCodeChanged,
                    format!("You marked your safety number with {author} as verified"),
                ),
                Type::IdentityDefault => (
                    SystemKind::SecurityCodeChanged,
                    format!("You marked your safety number with {author} as unverified"),
                ),
                Type::ChangeNumber => (
                    SystemKind::Other,
                    format!("{author} changed their phone number"),
                ),
                Type::EndSession => (SystemKind::Other, "Secure session reset".into()),
                Type::ChatSessionRefresh => (SystemKind::Other, "Chat session refreshed".into()),
                Type::BadDecrypt => (SystemKind::Other, "A message could not be delivered".into()),
                Type::Blocked => (SystemKind::Other, format!("You blocked {author}")),
                Type::Unblocked => (SystemKind::Other, format!("You unblocked {author}")),
                Type::MessageRequestAccepted => {
                    (SystemKind::Other, "You accepted the message request".into())
                }
                Type::ReportedSpam => (SystemKind::Other, "Reported as spam".into()),
                other => (SystemKind::Other, humanize(other.as_str_name())),
            };
            event(kind, text)
        }
        Some(Update::GroupChange(change)) => {
            let mut kind = SystemKind::Other;
            let mut lines = Vec::new();
            for u in change.updates.iter().filter_map(|u| u.update.as_ref()) {
                let (k, line) = group_update(u, ctx);
                if kind == SystemKind::Other {
                    kind = k;
                }
                lines.push(line);
            }
            event(kind, lines.join("\n"))
        }
        Some(Update::ExpirationTimerChange(t)) => event(
            SystemKind::DisappearingMessages,
            disappearing_text(&author, t.expires_in_ms),
        ),
        Some(Update::ProfileChange(p)) => event(
            SystemKind::Other,
            format!(
                "{} changed their profile name to {}",
                p.previous_name, p.new_name
            ),
        ),
        Some(Update::IndividualCall(c)) => {
            use individual_call::{Direction, State, Type};
            let video = Type::try_from(c.r#type) == Ok(Type::VideoCall);
            let medium = if video { "video" } else { "voice" };
            let state = State::try_from(c.state).unwrap_or(State::UnknownState);
            if matches!(state, State::Missed | State::MissedNotificationProfile) {
                event(SystemKind::MissedCall, format!("Missed {medium} call"))
            } else {
                let dir = match Direction::try_from(c.direction) {
                    Ok(Direction::Outgoing) => "Outgoing",
                    _ => "Incoming",
                };
                let declined = if state == State::NotAccepted {
                    " (not answered)"
                } else {
                    ""
                };
                event(SystemKind::Other, format!("{dir} {medium} call{declined}"))
            }
        }
        Some(Update::GroupCall(c)) => {
            use group_call::State;
            match State::try_from(c.state) {
                Ok(State::Missed | State::MissedNotificationProfile) => {
                    event(SystemKind::MissedCall, "Missed group call".into())
                }
                _ => event(SystemKind::Other, "Group call".into()),
            }
        }
        Some(other) => event(SystemKind::Other, humanize(&variant_name(other))),
        None => event(SystemKind::Other, "Chat update".into()),
    }
}

fn group_update(
    u: &group_change_chat_update::update::Update,
    ctx: &Context<'_>,
) -> (SystemKind, String) {
    use group_change_chat_update::update::Update as G;
    let who = |aci: &Option<Vec<u8>>| ctx.name_by_aci(aci.as_deref());
    match u {
        G::GroupCreationUpdate(g) => (
            SystemKind::GroupCreated,
            format!("{} created the group", who(&g.updater_aci)),
        ),
        G::GroupNameUpdate(g) => (
            SystemKind::SubjectChanged,
            match &g.new_group_name {
                Some(name) => format!("{} changed the group name to “{name}”", who(&g.updater_aci)),
                None => format!("{} removed the group name", who(&g.updater_aci)),
            },
        ),
        G::GroupAvatarUpdate(g) => (
            SystemKind::IconChanged,
            format!("{} changed the group avatar", who(&g.updater_aci)),
        ),
        G::GroupDescriptionUpdate(g) => (
            SystemKind::DescriptionChanged,
            format!("{} changed the group description", who(&g.updater_aci)),
        ),
        G::GroupMemberAddedUpdate(g) => (
            SystemKind::MemberAdded,
            format!(
                "{} added {}",
                who(&g.updater_aci),
                ctx.name_by_aci(Some(&g.new_member_aci))
            ),
        ),
        G::GroupMemberJoinedUpdate(g) => (
            SystemKind::MemberJoined,
            format!(
                "{} joined the group",
                ctx.name_by_aci(Some(&g.new_member_aci))
            ),
        ),
        G::GroupMemberJoinedByLinkUpdate(g) => (
            SystemKind::MemberJoined,
            format!(
                "{} joined the group via the group link",
                ctx.name_by_aci(Some(&g.new_member_aci))
            ),
        ),
        G::GroupMemberLeftUpdate(g) => (
            SystemKind::MemberLeft,
            format!("{} left the group", ctx.name_by_aci(Some(&g.aci))),
        ),
        G::GroupMemberRemovedUpdate(g) => (
            SystemKind::MemberRemoved,
            format!(
                "{} removed {}",
                who(&g.remover_aci),
                ctx.name_by_aci(Some(&g.removed_aci))
            ),
        ),
        G::GroupExpirationTimerUpdate(g) => (
            SystemKind::DisappearingMessages,
            disappearing_text(&who(&g.updater_aci), g.expires_in_ms),
        ),
        other => (SystemKind::Other, humanize(&variant_name(other))),
    }
}

fn disappearing_text(who: &str, expires_in_ms: u64) -> String {
    if expires_in_ms == 0 {
        format!("{who} turned off disappearing messages")
    } else {
        format!(
            "{who} set disappearing messages to {}",
            duration(expires_in_ms)
        )
    }
}

fn duration(ms: u64) -> String {
    let s = ms / 1000;
    let (n, unit) = match s {
        s if s % (7 * 86_400) == 0 && s > 0 => (s / (7 * 86_400), "week"),
        s if s % 86_400 == 0 && s > 0 => (s / 86_400, "day"),
        s if s % 3600 == 0 && s > 0 => (s / 3600, "hour"),
        s if s % 60 == 0 && s > 0 => (s / 60, "minute"),
        s => (s, "second"),
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// `GroupInviteLinkResetUpdate(…)` → `GroupInviteLinkResetUpdate`.
fn variant_name(value: &impl std::fmt::Debug) -> String {
    let debug = format!("{value:?}");
    debug.split(['(', ' ', '{']).next().unwrap_or("").to_owned()
}

/// `GroupInviteLinkResetUpdate` / `PAYMENTS_ACTIVATED` → "Group invite link reset update".
fn humanize(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c == '_' {
            out.push(' ');
        } else if c.is_uppercase() && i > 0 && !name.contains('_') {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else if i == 0 {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanizes_names() {
        assert_eq!(
            humanize("GroupInviteLinkResetUpdate"),
            "Group invite link reset update"
        );
        assert_eq!(humanize("PAYMENTS_ACTIVATED"), "Payments activated");
    }

    #[test]
    fn formats_durations() {
        assert_eq!(duration(86_400_000), "1 day");
        assert_eq!(duration(4 * 7 * 86_400_000), "4 weeks");
        assert_eq!(duration(30_000), "30 seconds");
    }

    #[test]
    fn converts_utf16_ranges_to_bytes() {
        use crate::proto::backup::BodyRange;
        let location = BackupLocation {
            root: "r".into(),
            snapshot: "r/s".into(),
            snapshots: vec![],
        };
        let names = HashMap::new();
        let acis = HashMap::new();
        let ctx = Context {
            names: &names,
            aci_to_recipient: &acis,
            location: &location,
        };
        // "😀 bold": the emoji is 2 UTF-16 units and 4 bytes.
        let text = Text {
            body: "😀 bold".into(),
            body_ranges: vec![BodyRange {
                start: 3,
                length: 4,
                associated_value: Some(body_range::AssociatedValue::Style(
                    body_range::Style::Bold as i32,
                )),
            }],
        };
        let rich = rich_text(&text, &ctx);
        assert_eq!(
            &rich.text[rich.ranges[0].start..][..rich.ranges[0].len],
            "bold"
        );
    }
}
