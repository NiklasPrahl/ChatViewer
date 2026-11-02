//! Deterministic synthetic backup content: fictional people, random words,
//! small text attachments. Frames are produced lazily, so very large
//! backups (millions of messages) can be generated in bounded memory.

use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};

use super::MediaBlob;
use crate::proto::backup::{
    AccountData, BackupInfo, Chat, ChatItem, Contact, FilePointer, Frame, Group, MessageAttachment,
    Quote, Reaction, Recipient, SendStatus, StandardMessage, Text, account_data, chat_item,
    contact, file_pointer, frame, group, quote, recipient, send_status,
};

use crate::proto::backup::account_data::auto_download_settings::AutoDownloadOption;

const FIRST_NAMES: &[&str] = &[
    "Alice", "Bob", "Carol", "Dave", "Erin", "Frank", "Grace", "Heidi",
];
const WORDS: &[&str] = &[
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
    "enim",
];
const EMOJI: &[&str] = &["👍", "❤️", "😂", "😮", "🙏"];
const SELF_ID: u64 = 1;
/// 2024-01-01T00:00:00Z
const START_MS: u64 = 1_704_067_200_000;
const STEP_MS: u64 = 37_000;

#[derive(Debug, Clone)]
pub struct SynthOptions {
    pub seed: u64,
    /// One-to-one chats, one contact each.
    pub direct_chats: usize,
    /// Adds one group chat with all contacts.
    pub group: bool,
    /// Total chat items across all chats.
    pub messages: usize,
    /// Number of messages that carry an attachment with a local copy.
    pub media: usize,
}

impl Default for SynthOptions {
    fn default() -> Self {
        Self {
            seed: 1,
            direct_chats: 2,
            group: true,
            messages: 50,
            media: 3,
        }
    }
}

/// Everything needed to write a synthetic backup.
pub struct SyntheticBackup {
    pub recovery_key: String,
    pub aci: [u8; 16],
    pub info: BackupInfo,
    pub frames: FrameGenerator,
    pub media: Vec<MediaBlob>,
}

pub fn generate(options: &SynthOptions) -> SyntheticBackup {
    let mut rng = StdRng::seed_from_u64(options.seed);
    let recovery_key = super::generate_recovery_key(&mut rng);
    let aci = random::<16>(&mut rng);
    let contacts: Vec<SynthContact> = (0..options.direct_chats.max(1))
        .map(|i| SynthContact {
            recipient_id: SELF_ID + 1 + i as u64,
            aci: random::<16>(&mut rng),
            profile_key: random::<32>(&mut rng),
            first: FIRST_NAMES[i % FIRST_NAMES.len()].to_string(),
            last: format!("Example{}", i + 1),
        })
        .collect();

    // Chats: one per contact, plus the optional group.
    let mut chats: Vec<SynthChat> = contacts
        .iter()
        .enumerate()
        .map(|(i, c)| SynthChat {
            chat_id: i as u64 + 1,
            recipient_id: c.recipient_id,
            members: vec![c.recipient_id],
            history: Vec::new(),
        })
        .collect();
    let group_recipient_id = SELF_ID + 1 + contacts.len() as u64;
    if options.group {
        chats.push(SynthChat {
            chat_id: chats.len() as u64 + 1,
            recipient_id: group_recipient_id,
            members: contacts.iter().map(|c| c.recipient_id).collect(),
            history: Vec::new(),
        });
    }

    // Messages that carry attachments, spread across the history.
    let media_count = options.media.min(options.messages);
    let media_every = options
        .messages
        .checked_div(media_count)
        .map_or(usize::MAX, |n| n.max(1));
    let media: Vec<MediaBlob> = (0..media_count)
        .map(|i| MediaBlob {
            local_key: random::<64>(&mut rng),
            plaintext: format!("Synthetic attachment {i}\n")
                .repeat(rng.random_range(10..200))
                .into_bytes(),
        })
        .collect();

    let info = BackupInfo {
        version: 1,
        backup_time_ms: START_MS + (options.messages as u64 + 10) * STEP_MS,
        media_root_backup_key: random::<32>(&mut rng).to_vec(),
        current_app_version: "chatviewer-fixture-gen 0".into(),
        first_app_version: "chatviewer-fixture-gen 0".into(),
        debug_info: Vec::new(),
    };

    let header = header_frames(
        &mut rng,
        &aci,
        &contacts,
        &chats,
        options.group,
        group_recipient_id,
    );
    let media_refs = media
        .iter()
        .map(|m| MediaRef {
            local_key: m.local_key,
            plaintext_hash: m.plaintext_hash(),
            size: m.plaintext.len() as u32,
        })
        .collect();

    SyntheticBackup {
        recovery_key,
        aci,
        info,
        frames: FrameGenerator {
            rng,
            header: header.into_iter(),
            chats,
            total: options.messages,
            produced: 0,
            media: media_refs,
            media_every,
        },
        media,
    }
}

struct SynthContact {
    recipient_id: u64,
    aci: [u8; 16],
    profile_key: [u8; 32],
    first: String,
    last: String,
}

struct SynthChat {
    chat_id: u64,
    recipient_id: u64,
    members: Vec<u64>,
    /// Recent messages (author, sent time, body) for quotes.
    history: Vec<(u64, u64, String)>,
}

struct MediaRef {
    local_key: [u8; 64],
    plaintext_hash: [u8; 32],
    size: u32,
}

fn random<const N: usize>(rng: &mut impl RngCore) -> [u8; N] {
    let mut out = [0u8; N];
    rng.fill_bytes(&mut out);
    out
}

fn header_frames(
    rng: &mut StdRng,
    self_aci: &[u8; 16],
    contacts: &[SynthContact],
    chats: &[SynthChat],
    with_group: bool,
    group_recipient_id: u64,
) -> Vec<Frame> {
    let mut frames = vec![Frame {
        item: Some(frame::Item::Account(AccountData {
            profile_key: random::<32>(rng).to_vec(),
            given_name: "Synthetic".into(),
            family_name: "Owner".into(),
            account_settings: Some(account_data::AccountSettings {
                read_receipts: true,
                typing_indicators: true,
                link_previews: false,
                phone_number_sharing_mode: account_data::PhoneNumberSharingMode::Nobody as i32,
                default_sent_media_quality: account_data::SentMediaQuality::Standard as i32,
                auto_download_settings: Some(account_data::AutoDownloadSettings {
                    images: AutoDownloadOption::Never as i32,
                    audio: AutoDownloadOption::Never as i32,
                    video: AutoDownloadOption::Never as i32,
                    documents: AutoDownloadOption::Never as i32,
                }),
                app_theme: account_data::AppTheme::System as i32,
                calls_use_less_data_setting: account_data::CallsUseLessDataSetting::Never as i32,
                unread_badge_type: account_data::account_settings::UnreadBadgeType::UnreadMessages
                    as i32,
                ..Default::default()
            }),
            ..Default::default()
        })),
    }];
    frames.push(Frame {
        item: Some(frame::Item::Recipient(Recipient {
            id: SELF_ID,
            destination: Some(recipient::Destination::Self_(Default::default())),
        })),
    });
    for c in contacts {
        frames.push(Frame {
            item: Some(frame::Item::Recipient(Recipient {
                id: c.recipient_id,
                destination: Some(recipient::Destination::Contact(Contact {
                    aci: Some(c.aci.to_vec()),
                    registration: Some(contact::Registration::Registered(Default::default())),
                    profile_key: Some(c.profile_key.to_vec()),
                    profile_sharing: true,
                    profile_given_name: Some(c.first.clone()),
                    profile_family_name: Some(c.last.clone()),
                    ..Default::default()
                })),
            })),
        });
    }
    if with_group {
        let mut members = vec![group::Member {
            user_id: self_aci.to_vec(),
            role: group::member::Role::Administrator as i32,
            ..Default::default()
        }];
        members.extend(contacts.iter().map(|c| group::Member {
            user_id: c.aci.to_vec(),
            role: group::member::Role::Default as i32,
            ..Default::default()
        }));
        frames.push(Frame {
            item: Some(frame::Item::Recipient(Recipient {
                id: group_recipient_id,
                destination: Some(recipient::Destination::Group(Group {
                    master_key: random::<32>(rng).to_vec(),
                    whitelisted: true,
                    snapshot: Some(group::GroupSnapshot {
                        title: Some(group::GroupAttributeBlob {
                            content: Some(group::group_attribute_blob::Content::Title(
                                "Synthetic Group".into(),
                            )),
                        }),
                        access_control: Some(group::AccessControl {
                            attributes: group::access_control::AccessRequired::Member as i32,
                            members: group::access_control::AccessRequired::Member as i32,
                            add_from_invite_link:
                                group::access_control::AccessRequired::Unsatisfiable as i32,
                            member_label: group::access_control::AccessRequired::Member as i32,
                        }),
                        version: 1,
                        members,
                        ..Default::default()
                    }),
                    ..Default::default()
                })),
            })),
        });
    }
    for chat in chats {
        frames.push(Frame {
            item: Some(frame::Item::Chat(Chat {
                id: chat.chat_id,
                recipient_id: chat.recipient_id,
                ..Default::default()
            })),
        });
    }
    frames
}

/// Lazily yields header frames, then chat items in global time order.
pub struct FrameGenerator {
    rng: StdRng,
    header: std::vec::IntoIter<Frame>,
    chats: Vec<SynthChat>,
    total: usize,
    produced: usize,
    media: Vec<MediaRef>,
    media_every: usize,
}

impl FrameGenerator {
    fn sentence(&mut self) -> String {
        let n = self.rng.random_range(3..14);
        let words: Vec<&str> = (0..n)
            .map(|_| WORDS[self.rng.random_range(0..WORDS.len())])
            .collect();
        let mut s = words.join(" ");
        if let Some(first) = s.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        s.push('.');
        s
    }

    fn next_item(&mut self) -> ChatItem {
        let index = self.produced;
        let date_sent = START_MS + index as u64 * STEP_MS;
        let chat_index = self.rng.random_range(0..self.chats.len());
        let outgoing = self.rng.random_bool(0.4);
        let body = self.sentence();

        let chat = &self.chats[chat_index];
        let author = if outgoing {
            SELF_ID
        } else {
            chat.members[self.rng.random_range(0..chat.members.len())]
        };

        let quote = (index % 7 == 6)
            .then(|| chat.history.last().cloned())
            .flatten()
            .map(|(q_author, q_sent, q_body)| Quote {
                target_sent_timestamp: Some(q_sent),
                author_id: q_author,
                text: Some(Text {
                    body: q_body,
                    body_ranges: Vec::new(),
                }),
                r#type: quote::Type::Normal as i32,
                attachments: Vec::new(),
            });

        let reactions = if index % 5 == 4 {
            let reactor = if outgoing { chat.members[0] } else { SELF_ID };
            vec![Reaction {
                emoji: EMOJI[self.rng.random_range(0..EMOJI.len())].into(),
                author_id: reactor,
                sent_timestamp: date_sent + 5_000,
                sort_order: 1,
            }]
        } else {
            Vec::new()
        };

        let attachments = if index.is_multiple_of(self.media_every)
            && index / self.media_every < self.media.len()
        {
            let m = &self.media[index / self.media_every];
            let n = index / self.media_every;
            let mut transit_key = [0u8; 64];
            self.rng.fill_bytes(&mut transit_key);
            vec![MessageAttachment {
                pointer: Some(FilePointer {
                    content_type: Some("text/plain".into()),
                    file_name: Some(format!("synthetic-{n}.txt")),
                    locator_info: Some(file_pointer::LocatorInfo {
                        key: transit_key.to_vec(),
                        integrity_check: Some(
                            file_pointer::locator_info::IntegrityCheck::PlaintextHash(
                                m.plaintext_hash.to_vec(),
                            ),
                        ),
                        size: m.size,
                        local_key: Some(m.local_key.to_vec()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                was_downloaded: true,
                client_uuid: Some(random::<16>(&mut self.rng).to_vec()),
                ..Default::default()
            }]
        } else {
            Vec::new()
        };

        let directional = if outgoing {
            chat_item::DirectionalDetails::Outgoing(chat_item::OutgoingMessageDetails {
                send_status: chat
                    .members
                    .iter()
                    .map(|&id| SendStatus {
                        recipient_id: id,
                        timestamp: date_sent + 2_000,
                        delivery_status: Some(send_status::DeliveryStatus::Read(
                            send_status::Read {
                                sealed_sender: true,
                            },
                        )),
                    })
                    .collect(),
                date_received: date_sent,
            })
        } else {
            chat_item::DirectionalDetails::Incoming(chat_item::IncomingMessageDetails {
                date_received: date_sent + 1_000,
                date_server_sent: Some(date_sent + 500),
                read: true,
                sealed_sender: true,
            })
        };

        let item = ChatItem {
            chat_id: chat.chat_id,
            author_id: author,
            date_sent,
            directional_details: Some(directional),
            item: Some(chat_item::Item::StandardMessage(StandardMessage {
                quote,
                text: Some(Text {
                    body: body.clone(),
                    body_ranges: Vec::new(),
                }),
                attachments,
                reactions,
                ..Default::default()
            })),
            ..Default::default()
        };

        let history = &mut self.chats[chat_index].history;
        history.push((author, date_sent, body));
        if history.len() > 8 {
            history.remove(0);
        }
        self.produced += 1;
        item
    }
}

impl Iterator for FrameGenerator {
    type Item = Frame;

    fn next(&mut self) -> Option<Frame> {
        if let Some(frame) = self.header.next() {
            return Some(frame);
        }
        (self.produced < self.total).then(|| Frame {
            item: Some(frame::Item::ChatItem(self.next_item())),
        })
    }
}
