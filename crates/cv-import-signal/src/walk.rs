//! Finds every media reference (`FilePointer`) in a frame, so an importer
//! can account for every file in a backup and nothing is dropped silently.
//!
//! Keep in sync with `backup.proto`: when a schema update adds a
//! `FilePointer` field, add it here (see the coverage check in
//! `signal-spike`).

use crate::proto::backup::{
    AccountData, Chat, ChatItem, ChatStyle, FilePointer, Frame, MessageAttachment, Quote,
    chat_item, chat_style, direct_story_reply_message, frame, message_attachment,
};

/// Where a media reference sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MediaRole {
    Attachment,
    VoiceMessage,
    Gif,
    LongText,
    LinkPreviewImage,
    QuoteThumbnail,
    ContactAvatar,
    Sticker,
    ViewOnce,
    StoryReplyLongText,
    Wallpaper,
}

impl MediaRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Attachment => "attachment",
            Self::VoiceMessage => "voice message",
            Self::Gif => "gif",
            Self::LongText => "long text",
            Self::LinkPreviewImage => "link preview image",
            Self::QuoteThumbnail => "quote thumbnail",
            Self::ContactAvatar => "contact avatar",
            Self::Sticker => "sticker",
            Self::ViewOnce => "view once",
            Self::StoryReplyLongText => "story reply long text",
            Self::Wallpaper => "wallpaper",
        }
    }
}

/// One media reference found in a frame.
#[derive(Debug, Clone, Copy)]
pub struct MediaRef<'a> {
    pub role: MediaRole,
    /// The reference belongs to an earlier version of an edited message.
    pub in_revision: bool,
    pub pointer: &'a FilePointer,
}

/// Calls `f` for every media reference in `frame`.
pub fn for_each_media<'a>(frame: &'a Frame, f: &mut impl FnMut(MediaRef<'a>)) {
    match &frame.item {
        Some(frame::Item::Account(account)) => account_data(account, f),
        Some(frame::Item::Chat(chat)) => chat_media(chat, f),
        Some(frame::Item::ChatItem(item)) => chat_item_media(item, false, f),
        _ => {}
    }
}

fn account_data<'a>(account: &'a AccountData, f: &mut impl FnMut(MediaRef<'a>)) {
    if let Some(style) = account
        .account_settings
        .as_ref()
        .and_then(|s| s.default_chat_style.as_ref())
    {
        chat_style_media(style, f);
    }
}

fn chat_media<'a>(chat: &'a Chat, f: &mut impl FnMut(MediaRef<'a>)) {
    if let Some(style) = &chat.style {
        chat_style_media(style, f);
    }
}

fn chat_style_media<'a>(style: &'a ChatStyle, f: &mut impl FnMut(MediaRef<'a>)) {
    if let Some(chat_style::Wallpaper::WallpaperPhoto(pointer)) = &style.wallpaper {
        emit(f, MediaRole::Wallpaper, false, Some(pointer));
    }
}

fn chat_item_media<'a>(item: &'a ChatItem, in_revision: bool, f: &mut impl FnMut(MediaRef<'a>)) {
    use chat_item::Item;

    for revision in &item.revisions {
        chat_item_media(revision, true, f);
    }
    match &item.item {
        Some(Item::StandardMessage(m)) => {
            for attachment in &m.attachments {
                message_attachment_media(attachment, None, in_revision, f);
            }
            emit(f, MediaRole::LongText, in_revision, m.long_text.as_ref());
            for preview in &m.link_preview {
                emit(
                    f,
                    MediaRole::LinkPreviewImage,
                    in_revision,
                    preview.image.as_ref(),
                );
            }
            if let Some(quote) = &m.quote {
                quote_media(quote, in_revision, f);
            }
        }
        Some(Item::ContactMessage(m)) => {
            let avatar = m.contact.as_ref().and_then(|c| c.avatar.as_ref());
            emit(f, MediaRole::ContactAvatar, in_revision, avatar);
        }
        Some(Item::StickerMessage(m)) => {
            let data = m.sticker.as_ref().and_then(|s| s.data.as_ref());
            emit(f, MediaRole::Sticker, in_revision, data);
        }
        Some(Item::ViewOnceMessage(m)) => {
            if let Some(attachment) = &m.attachment {
                message_attachment_media(attachment, Some(MediaRole::ViewOnce), in_revision, f);
            }
        }
        Some(Item::DirectStoryReplyMessage(m)) => {
            if let Some(direct_story_reply_message::Reply::TextReply(reply)) = &m.reply {
                emit(
                    f,
                    MediaRole::StoryReplyLongText,
                    in_revision,
                    reply.long_text.as_ref(),
                );
            }
        }
        _ => {}
    }
}

fn quote_media<'a>(quote: &'a Quote, in_revision: bool, f: &mut impl FnMut(MediaRef<'a>)) {
    for attachment in &quote.attachments {
        if let Some(thumbnail) = &attachment.thumbnail {
            message_attachment_media(thumbnail, Some(MediaRole::QuoteThumbnail), in_revision, f);
        }
    }
}

fn message_attachment_media<'a>(
    attachment: &'a MessageAttachment,
    role: Option<MediaRole>,
    in_revision: bool,
    f: &mut impl FnMut(MediaRef<'a>),
) {
    use message_attachment::Flag;

    let role = role.unwrap_or(match Flag::try_from(attachment.flag) {
        Ok(Flag::VoiceMessage) => MediaRole::VoiceMessage,
        Ok(Flag::Gif) => MediaRole::Gif,
        _ => MediaRole::Attachment,
    });
    emit(f, role, in_revision, attachment.pointer.as_ref());
}

fn emit<'a>(
    f: &mut impl FnMut(MediaRef<'a>),
    role: MediaRole,
    in_revision: bool,
    pointer: Option<&'a FilePointer>,
) {
    if let Some(pointer) = pointer {
        f(MediaRef {
            role,
            in_revision,
            pointer,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::backup::{
        ContactAttachment, ContactMessage, LinkPreview, StandardMessage, Sticker, StickerMessage,
        quote,
    };

    fn pointer() -> Option<FilePointer> {
        Some(FilePointer::default())
    }

    fn attachment(flag: message_attachment::Flag) -> MessageAttachment {
        MessageAttachment {
            pointer: pointer(),
            flag: flag as i32,
            ..Default::default()
        }
    }

    fn roles(frame: &Frame) -> Vec<(MediaRole, bool)> {
        let mut found = Vec::new();
        for_each_media(frame, &mut |r| found.push((r.role, r.in_revision)));
        found.sort();
        found
    }

    fn chat_item(item: chat_item::Item, revisions: Vec<ChatItem>) -> Frame {
        Frame {
            item: Some(frame::Item::ChatItem(ChatItem {
                item: Some(item),
                revisions,
                ..Default::default()
            })),
        }
    }

    #[test]
    fn finds_media_in_every_part_of_a_standard_message_and_its_revisions() {
        use message_attachment::Flag;

        let message = StandardMessage {
            attachments: vec![
                attachment(Flag::None),
                attachment(Flag::VoiceMessage),
                attachment(Flag::Gif),
            ],
            long_text: pointer(),
            link_preview: vec![LinkPreview {
                image: pointer(),
                ..Default::default()
            }],
            quote: Some(Quote {
                attachments: vec![quote::QuotedAttachment {
                    thumbnail: Some(attachment(Flag::None)),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let revision = ChatItem {
            item: Some(chat_item::Item::StandardMessage(StandardMessage {
                attachments: vec![attachment(Flag::None)],
                ..Default::default()
            })),
            ..Default::default()
        };
        let frame = chat_item(chat_item::Item::StandardMessage(message), vec![revision]);
        assert_eq!(
            roles(&frame),
            [
                (MediaRole::Attachment, false),
                (MediaRole::Attachment, true),
                (MediaRole::VoiceMessage, false),
                (MediaRole::Gif, false),
                (MediaRole::LongText, false),
                (MediaRole::LinkPreviewImage, false),
                (MediaRole::QuoteThumbnail, false),
            ]
        );
    }

    #[test]
    fn finds_sticker_contact_and_wallpaper_media() {
        let sticker = chat_item(
            chat_item::Item::StickerMessage(StickerMessage {
                sticker: Some(Sticker {
                    data: pointer(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            vec![],
        );
        assert_eq!(roles(&sticker), [(MediaRole::Sticker, false)]);

        let contact = chat_item(
            chat_item::Item::ContactMessage(ContactMessage {
                contact: Some(ContactAttachment {
                    avatar: pointer(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            vec![],
        );
        assert_eq!(roles(&contact), [(MediaRole::ContactAvatar, false)]);

        let chat = Frame {
            item: Some(frame::Item::Chat(Chat {
                style: Some(ChatStyle {
                    wallpaper: Some(chat_style::Wallpaper::WallpaperPhoto(FilePointer::default())),
                    ..Default::default()
                }),
                ..Default::default()
            })),
        };
        assert_eq!(roles(&chat), [(MediaRole::Wallpaper, false)]);
    }
}
