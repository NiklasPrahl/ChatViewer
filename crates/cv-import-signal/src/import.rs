//! The Signal importer: an index pass on open, per-chat decoding on load,
//! media decrypted on access (ADR 0005).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Cursor, Read};

use cv_core::{
    ChatArchive, ChatSummary, Error, ImportOptions, Importer, InputSource, Probe, Result,
};
use cv_model::{
    BlobRef, Conversation, ConversationData, ConversationId, ConversationKind, Participant,
    SelfFlag, SourceExtra, SourceReport, Timestamp,
};
use prost::Message as _;

use crate::key::{BackupKey, MessageBackupKey, RecoveryKey};
use crate::layout::{self, BackupLocation};
use crate::main_file::MainReader;
use crate::mapping::{self, BLOB_PREFIX, Context};
use crate::media::{self, Integrity as MediaIntegrity, LocalMedia};
use crate::metadata::LocalMetadata;
use crate::proto::backup::{Frame, Recipient, contact, frame, group, recipient};
use crate::walk;

pub struct SignalImporter;

impl Importer for SignalImporter {
    fn id(&self) -> &'static str {
        "signal-local"
    }

    fn probe(&self, input: &InputSource) -> Probe {
        if input.path().is_dir() && layout::locate(input.path()).is_ok() {
            Probe::Maybe(95)
        } else {
            Probe::NotMine
        }
    }

    fn secret_prompt(&self) -> Option<&'static str> {
        Some("Signal recovery key (64 characters)")
    }

    fn open(&self, input: &InputSource, options: &ImportOptions) -> Result<Box<dyn ChatArchive>> {
        let secret = options
            .secret
            .as_ref()
            .ok_or_else(|| Error::Other("the Signal recovery key is required".into()))?;
        let key: RecoveryKey = secret.expose().parse().map_err(to_core)?;
        Ok(Box::new(SignalArchive::open(input, &key)?))
    }
}

fn to_core(e: crate::Error) -> Error {
    match e {
        crate::Error::Io(e) => Error::Io(e),
        crate::Error::NotABackup(_) => Error::Unsupported(e.to_string()),
        crate::Error::UnsupportedMetadataVersion { .. }
        | crate::Error::ForwardSecrecyNotSupported => Error::Unsupported(e.to_string()),
        other => Error::Malformed(other.to_string()),
    }
}

struct ChatIndex {
    id: u64,
    recipient_id: u64,
    raw: Vec<u8>,
    messages: u64,
    authors: BTreeSet<u64>,
    first: Option<u64>,
    last: Option<u64>,
}

struct RecipientInfo {
    kind: ConversationKind,
    name: String,
    handle: Option<String>,
    is_self: bool,
    group_members: Vec<[u8; 16]>,
    raw: Vec<u8>,
}

pub struct SignalArchive {
    location: BackupLocation,
    key: MessageBackupKey,
    report: SourceReport,
    recipients: HashMap<u64, RecipientInfo>,
    names: HashMap<u64, String>,
    aci_to_recipient: HashMap<[u8; 16], u64>,
    chats: Vec<ChatIndex>,
    media: HashMap<String, LocalMedia>,
}

impl SignalArchive {
    pub fn open(input: &InputSource, recovery_key: &RecoveryKey) -> Result<Self> {
        let location = layout::locate(input.path()).map_err(to_core)?;
        let backup_key = BackupKey::from_recovery_key(recovery_key);
        let metadata = LocalMetadata::read(&location.metadata_path()).map_err(to_core)?;
        let key = backup_key.message_backup_key(&metadata.backup_id(&backup_key));
        let mut reader = MainReader::open(&location.main_path(), &key).map_err(to_core)?;
        let info = reader.info().clone();

        let mut recipients = HashMap::new();
        let mut aci_to_recipient = HashMap::new();
        let mut chats: Vec<ChatIndex> = Vec::new();
        let mut chat_pos: HashMap<u64, usize> = HashMap::new();
        let mut media = HashMap::new();
        let mut counters: BTreeMap<String, u64> = BTreeMap::new();
        let mut self_name = None;
        let mut bump = |k: &str| *counters.entry(k.to_owned()).or_default() += 1;

        while let Some(frame) = reader.next_frame().map_err(to_core)? {
            let raw = reader.last_frame_bytes();
            if frame.encoded_len() != raw.len() {
                bump("frames_with_unknown_fields");
            }
            walk::for_each_media(&frame, &mut |r| {
                if let Some(local) = LocalMedia::from_pointer(r.pointer) {
                    media.insert(local.media_name.clone(), local);
                }
            });
            match &frame.item {
                Some(frame::Item::Account(a)) => {
                    let name = join_name(&a.given_name, &a.family_name);
                    self_name = (!name.is_empty()).then_some(name);
                }
                Some(frame::Item::Recipient(r)) => {
                    bump("recipients");
                    let info = recipient_info(r, raw.to_vec());
                    if let Some(recipient::Destination::Contact(c)) = &r.destination {
                        if let Some(aci) =
                            c.aci.as_deref().and_then(|a| <[u8; 16]>::try_from(a).ok())
                        {
                            aci_to_recipient.insert(aci, r.id);
                        }
                    }
                    recipients.insert(r.id, info);
                }
                Some(frame::Item::Chat(c)) => {
                    chat_pos.insert(c.id, chats.len());
                    chats.push(ChatIndex {
                        id: c.id,
                        recipient_id: c.recipient_id,
                        raw: raw.to_vec(),
                        messages: 0,
                        authors: BTreeSet::new(),
                        first: None,
                        last: None,
                    });
                }
                Some(frame::Item::ChatItem(item)) => {
                    bump("chat_items");
                    if let Some(&i) = chat_pos.get(&item.chat_id) {
                        let chat = &mut chats[i];
                        chat.messages += 1;
                        chat.authors.insert(item.author_id);
                        chat.first =
                            Some(chat.first.map_or(item.date_sent, |f| f.min(item.date_sent)));
                        chat.last =
                            Some(chat.last.map_or(item.date_sent, |l| l.max(item.date_sent)));
                    }
                }
                Some(_) => bump("other_frames"),
                None => bump("unknown_frames"),
            }
        }
        drop(reader);

        // The owner: name from AccountData, id from the Self recipient.
        let self_id = recipients
            .iter()
            .find(|(_, r)| r.is_self)
            .map(|(id, _)| *id);
        if let (Some(id), Some(name)) = (self_id, self_name) {
            if let Some(r) = recipients.get_mut(&id) {
                r.name = name;
            }
        }
        if let Some(id) = self_id {
            aci_to_recipient.retain(|_, r| *r != id);
        }
        let names = recipients
            .iter()
            .map(|(id, r)| (*id, r.name.clone()))
            .collect();
        counters.insert("chats".into(), chats.len() as u64);
        counters.insert("media_files".into(), media.len() as u64);

        let mut warnings = Vec::new();
        if counters
            .get("frames_with_unknown_fields")
            .copied()
            .unwrap_or(0)
            > 0
        {
            warnings.push(
                "this backup contains data from a newer Signal version that is not understood; \
                 it is kept but not shown"
                    .into(),
            );
        }
        let platform = info
            .current_app_version
            .split_whitespace()
            .next()
            .map_or("unknown", |p| match p.to_ascii_lowercase().as_str() {
                "desktop" => "desktop",
                "android" => "android",
                "ios" => "ios",
                _ => "unknown",
            });
        let report = SourceReport {
            app: "signal".into(),
            platform: platform.into(),
            variant: format!(
                "signal-local-backup-v{}-metadata-v{}",
                info.version, metadata.version
            ),
            source_version: Some(format!(
                "{} (backup format {}, metadata {})",
                info.current_app_version, info.version, metadata.version
            )),
            confidence: 100,
            warnings,
            limitations: vec![
                "media that were not on the device when the backup was made are not included"
                    .into(),
                "stories, call history and settings are not shown".into(),
            ],
            counters,
        };
        log::info!(
            "signal: {} chats, {} chat items, {} media files",
            chats.len(),
            report.counters.get("chat_items").copied().unwrap_or(0),
            media.len()
        );

        Ok(Self {
            location,
            key,
            report,
            recipients,
            names,
            aci_to_recipient,
            chats,
            media,
        })
    }

    /// Decodes the given chats in one pass over the backup.
    fn load_chats(&self, ids: &[u64]) -> Result<Vec<ConversationData>> {
        let ctx = Context {
            names: &self.names,
            aci_to_recipient: &self.aci_to_recipient,
            location: &self.location,
        };
        let slot: HashMap<u64, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let mut out: Vec<ConversationData> = ids
            .iter()
            .map(|id| {
                let chat = self.chats.iter().find(|c| c.id == *id).expect("known chat");
                let (conversation, participants) = self.conversation(chat);
                ConversationData {
                    conversation,
                    participants,
                    messages: Vec::with_capacity(chat.messages as usize),
                }
            })
            .collect();
        let mut known: Vec<HashMap<(u64, u64), cv_model::MessageId>> =
            vec![HashMap::new(); ids.len()];

        let mut reader =
            MainReader::open(&self.location.main_path(), &self.key).map_err(to_core)?;
        while let Some(frame) = reader.next_frame().map_err(to_core)? {
            let Frame {
                item: Some(frame::Item::ChatItem(item)),
            } = &frame
            else {
                continue;
            };
            let Some(&i) = slot.get(&item.chat_id) else {
                continue;
            };
            let messages = &mut out[i].messages;
            let message = mapping::chat_item(
                item,
                reader.last_frame_bytes(),
                messages.len() as u64,
                &ctx,
                &known[i],
            );
            known[i].insert((item.author_id, item.date_sent), message.id.clone());
            messages.push(message);
        }
        Ok(out)
    }

    fn conversation(&self, chat: &ChatIndex) -> (Conversation, Vec<Participant>) {
        let recipient = self.recipients.get(&chat.recipient_id);
        let kind = recipient.map_or(ConversationKind::Unknown, |r| r.kind);
        let title = recipient.map_or_else(|| "Unknown chat".into(), |r| r.name.clone());

        let mut ids: BTreeSet<u64> = chat.authors.clone();
        if kind == ConversationKind::Direct || kind == ConversationKind::NoteToSelf {
            ids.insert(chat.recipient_id);
        }
        if let Some(r) = recipient {
            ids.extend(
                r.group_members
                    .iter()
                    .filter_map(|aci| self.aci_to_recipient.get(aci)),
            );
        }
        if let Some((self_id, _)) = self.recipients.iter().find(|(_, r)| r.is_self) {
            ids.insert(*self_id);
        }
        let participants: Vec<Participant> = ids
            .iter()
            .map(|id| {
                let r = self.recipients.get(id);
                Participant {
                    id: Context::participant(*id),
                    display_name: r.map_or_else(|| "Unknown".into(), |r| r.name.clone()),
                    is_self: if r.is_some_and(|r| r.is_self) {
                        SelfFlag::Yes
                    } else {
                        SelfFlag::No
                    },
                    handle: r.and_then(|r| r.handle.clone()),
                    extra: SourceExtra {
                        raw: r.map(|r| mapping::raw_record("signal-backup-frame", &r.raw)),
                        fields: Default::default(),
                    },
                }
            })
            .collect();
        let conversation = Conversation {
            id: chat_id(chat.id),
            kind,
            title,
            participants: participants.iter().map(|p| p.id.clone()).collect(),
            extra: SourceExtra {
                raw: Some(mapping::raw_record("signal-backup-frame", &chat.raw)),
                fields: [(
                    "signal.recipient_raw".to_owned(),
                    recipient.map_or(serde_json::Value::Null, |r| {
                        serde_json::to_value(mapping::raw_record("signal-backup-frame", &r.raw))
                            .unwrap_or_default()
                    }),
                )]
                .into(),
            },
        };
        (conversation, participants)
    }
}

fn chat_id(id: u64) -> ConversationId {
    ConversationId::new(format!("c{id}"))
}

fn join_name(a: &str, b: &str) -> String {
    [a, b]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

fn recipient_info(r: &Recipient, raw: Vec<u8>) -> RecipientInfo {
    use recipient::Destination;
    let mut info = RecipientInfo {
        kind: ConversationKind::Unknown,
        name: String::new(),
        handle: None,
        is_self: false,
        group_members: Vec::new(),
        raw,
    };
    match &r.destination {
        Some(Destination::Contact(c)) => {
            info.kind = ConversationKind::Direct;
            info.name = contact_name(c);
            info.handle = c
                .e164
                .map(|n| format!("+{n}"))
                .or_else(|| c.username.clone());
        }
        Some(Destination::Group(g)) => {
            info.kind = ConversationKind::Group;
            let snapshot = g.snapshot.as_ref();
            info.name = snapshot
                .and_then(|s| s.title.as_ref())
                .and_then(|t| match &t.content {
                    Some(group::group_attribute_blob::Content::Title(t)) => Some(t.clone()),
                    _ => None,
                })
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "Unnamed group".into());
            info.group_members = snapshot
                .map(|s| {
                    s.members
                        .iter()
                        .filter_map(|m| <[u8; 16]>::try_from(m.user_id.as_slice()).ok())
                        .collect()
                })
                .unwrap_or_default();
        }
        Some(Destination::Self_(_)) => {
            info.kind = ConversationKind::NoteToSelf;
            info.name = "Note to Self".into();
            info.is_self = true;
        }
        Some(Destination::ReleaseNotes(_)) => {
            info.kind = ConversationKind::Broadcast;
            info.name = "Signal".into();
        }
        Some(Destination::DistributionList(d)) => {
            info.kind = ConversationKind::Broadcast;
            info.name = match &d.item {
                Some(crate::proto::backup::distribution_list_item::Item::DistributionList(l)) => {
                    l.name.clone()
                }
                _ => "Story list".into(),
            };
        }
        Some(Destination::CallLink(c)) => {
            info.name = if c.name.is_empty() {
                "Call link".into()
            } else {
                c.name.clone()
            };
        }
        None => info.name = "Unknown".into(),
    }
    info
}

/// Nickname, then address-book name, then profile name, then username or
/// phone number.
fn contact_name(c: &crate::proto::backup::Contact) -> String {
    let nick = c
        .nickname
        .as_ref()
        .map(|n: &contact::Name| join_name(&n.given, &n.family))
        .unwrap_or_default();
    let system = join_name(&c.system_given_name, &c.system_family_name);
    let profile = join_name(
        c.profile_given_name.as_deref().unwrap_or(""),
        c.profile_family_name.as_deref().unwrap_or(""),
    );
    [nick, system, c.system_nickname.clone(), profile]
        .into_iter()
        .find(|n| !n.is_empty())
        .or_else(|| c.username.clone())
        .or_else(|| c.e164.map(|n| format!("+{n}")))
        .unwrap_or_else(|| "Unknown contact".into())
}

impl ChatArchive for SignalArchive {
    fn report(&self) -> &SourceReport {
        &self.report
    }

    fn conversation_ids(&self) -> Vec<ConversationId> {
        self.chats
            .iter()
            .filter(|c| c.messages > 0)
            .map(|c| chat_id(c.id))
            .collect()
    }

    fn summary(&self, id: &ConversationId) -> Result<ChatSummary> {
        let chat = self
            .chats
            .iter()
            .find(|c| chat_id(c.id) == *id)
            .ok_or_else(|| Error::NoSuchConversation(id.to_string()))?;
        let (conversation, participants) = self.conversation(chat);
        let ts = |ms: Option<u64>| ms.map(|ms| Timestamp::UtcMillis(ms as i64));
        Ok(ChatSummary {
            id: id.clone(),
            title: conversation.title,
            kind: conversation.kind,
            participants: participants.len(),
            messages: chat.messages,
            first: ts(chat.first),
            last: ts(chat.last),
        })
    }

    fn load(&self, id: &ConversationId) -> Result<ConversationData> {
        let chat = self
            .chats
            .iter()
            .find(|c| chat_id(c.id) == *id)
            .ok_or_else(|| Error::NoSuchConversation(id.to_string()))?;
        Ok(self.load_chats(&[chat.id])?.remove(0))
    }

    fn load_all(&self) -> Result<Vec<ConversationData>> {
        let ids: Vec<u64> = self
            .chats
            .iter()
            .filter(|c| c.messages > 0)
            .map(|c| c.id)
            .collect();
        self.load_chats(&ids)
    }

    fn open_blob(&self, blob: &BlobRef) -> Result<Box<dyn Read + '_>> {
        let name = blob
            .as_str()
            .strip_prefix(BLOB_PREFIX)
            .ok_or(Error::NoSuchBlob)?;
        let local = self.media.get(name).ok_or(Error::NoSuchBlob)?;
        let decrypted =
            media::decrypt_file(&self.location.media_path(name), local).map_err(to_core)?;
        if decrypted.integrity == MediaIntegrity::AuthenticatedOnly {
            log::debug!("signal: media authentic but differs from recorded hash");
        }
        Ok(Box::new(Cursor::new(decrypted.data)))
    }
}
