//! M1 feasibility spike: opens a Signal local backup with the recovery key
//! and prints a content-free summary (counts, versions, timings).
//!
//! The recovery key is read from a hidden prompt, or from stdin with
//! `--recovery-key-stdin`. It is never accepted as an argument.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::Parser;
use cv_import_signal::key::{BackupKey, RecoveryKey};
use cv_import_signal::layout;
use cv_import_signal::main_file::MainReader;
use cv_import_signal::media::{self, Integrity, LocalMedia};
use cv_import_signal::metadata::LocalMetadata;
use cv_import_signal::proto::backup::{Frame, chat_item, frame, recipient};
use cv_import_signal::walk::{self, MediaRef};
use prost::Message;
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(about = "Decrypt a Signal local backup and print a content-free summary")]
struct Args {
    /// Backup folder (SignalBackups), its parent, or one snapshot folder.
    path: PathBuf,
    /// Read the recovery key from stdin instead of a hidden prompt.
    #[arg(long)]
    recovery_key_stdin: bool,
    /// Number of attachments to decrypt and verify (0 = none, -1 = all).
    #[arg(long, default_value_t = 3, allow_negative_numbers = true)]
    verify_media: i64,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn read_key(from_stdin: bool) -> Result<RecoveryKey, Box<dyn std::error::Error>> {
    let input = if from_stdin || !io::stdin().is_terminal() {
        let mut line = Zeroizing::new(String::new());
        io::stdin().lock().read_line(&mut line)?;
        line
    } else {
        Zeroizing::new(rpassword::prompt_password("Recovery key (input hidden): ")?)
    };
    Ok(input.parse()?)
}

#[derive(Default)]
struct Stats {
    frames: BTreeMap<&'static str, u64>,
    recipients: BTreeMap<&'static str, u64>,
    items: BTreeMap<&'static str, u64>,
    reactions: u64,
    quotes: u64,
    /// Media references per (role, in edit history): (total, with local copy).
    media_refs: BTreeMap<(&'static str, bool), (u64, u64)>,
    /// Distinct local media with role and content type, for diagnostics.
    local_media: BTreeMap<String, (LocalMedia, &'static str, String)>,
    sticker_messages: u64,
    stickers_with_emoji: u64,
    /// Frames whose bytes contain fields this schema does not know.
    frames_with_unknown_fields: u64,
}

impl Stats {
    fn add(&mut self, frame: &Frame, raw_len: usize) {
        if frame.encoded_len() != raw_len {
            self.frames_with_unknown_fields += 1;
        }
        walk::for_each_media(frame, &mut |r| self.media(r));
        let kind = match &frame.item {
            Some(frame::Item::Account(_)) => "account",
            Some(frame::Item::Recipient(r)) => {
                let kind = match &r.destination {
                    Some(recipient::Destination::Contact(_)) => "contact",
                    Some(recipient::Destination::Group(_)) => "group",
                    Some(recipient::Destination::DistributionList(_)) => "distribution list",
                    Some(recipient::Destination::Self_(_)) => "self",
                    Some(recipient::Destination::ReleaseNotes(_)) => "release notes",
                    Some(recipient::Destination::CallLink(_)) => "call link",
                    None => "unknown",
                };
                *self.recipients.entry(kind).or_default() += 1;
                "recipient"
            }
            Some(frame::Item::Chat(_)) => "chat",
            Some(frame::Item::ChatItem(item)) => {
                self.add_item(item);
                "chat item"
            }
            Some(frame::Item::InstalledStickerPack(_) | frame::Item::HiddenStickerPack(_)) => {
                "sticker pack"
            }
            Some(frame::Item::AdHocCall(_)) => "ad-hoc call",
            Some(frame::Item::NotificationProfile(_)) => "notification profile",
            Some(frame::Item::ChatFolder(_)) => "chat folder",
            Some(frame::Item::FavoriteStickerList(_)) => "favorite stickers",
            None => "unknown (skipped)",
        };
        *self.frames.entry(kind).or_default() += 1;
    }

    fn add_item(&mut self, item: &cv_import_signal::proto::backup::ChatItem) {
        use chat_item::Item;
        let kind = match &item.item {
            Some(Item::StandardMessage(m)) => {
                self.reactions += m.reactions.len() as u64;
                self.quotes += u64::from(m.quote.is_some());
                "standard"
            }
            Some(Item::ContactMessage(_)) => "contact",
            Some(Item::StickerMessage(m)) => {
                self.sticker_messages += 1;
                let emoji = m.sticker.as_ref().and_then(|s| s.emoji.as_deref());
                self.stickers_with_emoji += u64::from(emoji.is_some_and(|e| !e.is_empty()));
                "sticker"
            }
            Some(Item::RemoteDeletedMessage(_)) => "remote deleted",
            Some(Item::UpdateMessage(_)) => "update",
            Some(Item::PaymentNotification(_)) => "payment",
            Some(Item::GiftBadge(_)) => "gift badge",
            Some(Item::ViewOnceMessage(_)) => "view once",
            Some(Item::DirectStoryReplyMessage(_)) => "story reply",
            Some(Item::Poll(_)) => "poll",
            Some(Item::AdminDeletedMessage(_)) => "admin deleted",
            None => "unknown",
        };
        *self.items.entry(kind).or_default() += 1;
    }

    fn media(&mut self, r: MediaRef<'_>) {
        let local = LocalMedia::from_pointer(r.pointer);
        let entry = self
            .media_refs
            .entry((r.role.label(), r.in_revision))
            .or_default();
        entry.0 += 1;
        if let Some(local) = local {
            entry.1 += 1;
            let content_type = r.pointer.content_type.clone().unwrap_or_default();
            self.local_media.entry(local.media_name.clone()).or_insert((
                local,
                r.role.label(),
                content_type,
            ));
        }
    }
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let location = layout::locate(&args.path)?;
    println!("snapshot:            {}", location.snapshot_name());
    println!("snapshots found:     {}", location.snapshots.len());

    let key = read_key(args.recovery_key_stdin)?;
    let started = Instant::now();
    let backup_key = BackupKey::from_recovery_key(&key);
    drop(key);

    let metadata = LocalMetadata::read(&location.metadata_path())?;
    println!("metadata version:    {}", metadata.version);
    let message_key = backup_key.message_backup_key(&metadata.backup_id(&backup_key));

    let main_len = std::fs::metadata(location.main_path())?.len();
    let t = Instant::now();
    let mut reader = MainReader::open(&location.main_path(), &message_key)?;
    let hmac_time = t.elapsed();
    let info = reader.info().clone();
    println!("backup version:      {}", info.version);
    println!("backup time (ms):    {}", info.backup_time_ms);
    println!("created by app:      {}", info.current_app_version);
    println!("first app version:   {}", info.first_app_version);

    let t = Instant::now();
    let mut stats = Stats::default();
    while let Some(frame) = reader.next_frame()? {
        let raw_len = reader.last_frame_len();
        stats.add(&frame, raw_len);
    }
    let decode_time = t.elapsed();

    println!("frames:");
    for (kind, n) in &stats.frames {
        println!("  {kind:<20} {n}");
    }
    println!("recipients:");
    for (kind, n) in &stats.recipients {
        println!("  {kind:<20} {n}");
    }
    println!("chat items:");
    for (kind, n) in &stats.items {
        println!("  {kind:<20} {n}");
    }
    println!("reactions:           {}", stats.reactions);
    println!("quotes:              {}", stats.quotes);
    println!(
        "stickers:            {} messages, {} with emoji",
        stats.sticker_messages, stats.stickers_with_emoji
    );
    println!(
        "unknown fields:      {} frames contain data this schema does not know",
        stats.frames_with_unknown_fields
    );
    println!("media references (role [in edit history]: total / with local copy):");
    for ((role, in_revision), (total, local)) in &stats.media_refs {
        let rev = if *in_revision { " [edit history]" } else { "" };
        println!("  {:<34} {total} / {local}", format!("{role}{rev}"));
    }

    let listed: BTreeSet<String> = media::read_files_list(&location.files_list_path())
        .unwrap_or_default()
        .into_iter()
        .collect();
    let referenced: BTreeSet<&String> = stats.local_media.keys().collect();
    let on_disk = referenced
        .iter()
        .filter(|name| location.media_path(name).is_file())
        .count();
    println!("media coverage:");
    println!("  distinct local files referenced: {}", referenced.len());
    println!("  in snapshot list:                {}", listed.len());
    println!(
        "  listed but not referenced:       {}",
        listed.iter().filter(|n| !referenced.contains(n)).count()
    );
    println!(
        "  referenced but not listed:       {}",
        referenced.iter().filter(|n| !listed.contains(**n)).count()
    );
    println!("  referenced and on disk:          {on_disk}");

    let limit = match args.verify_media {
        n if n < 0 => usize::MAX,
        n => n as usize,
    };
    let (mut verified, mut authentic_only, mut failed) = (0u64, 0u64, 0u64);
    let mut outcomes: BTreeMap<(String, &'static str, String), u64> = BTreeMap::new();
    let t = Instant::now();
    for (m, kind, content_type) in stats.local_media.values().take(limit) {
        let outcome = match media::decrypt_file(&location.media_path(&m.media_name), m) {
            Ok(d) if d.integrity == Integrity::Verified => {
                verified += 1;
                "verified".to_string()
            }
            Ok(_) => {
                authentic_only += 1;
                "authentic, differs from recorded size/hash".to_string()
            }
            Err(e) => {
                failed += 1;
                e.to_string()
            }
        };
        *outcomes
            .entry((outcome, kind, content_type.clone()))
            .or_default() += 1;
    }
    println!(
        "  checked:           {verified} verified, {authentic_only} authentic only, {failed} failed ({:.2?})",
        t.elapsed()
    );
    println!("media outcomes (outcome / role / content type):");
    for ((outcome, kind, content_type), n) in &outcomes {
        println!("  {n:>6}  {outcome} / {kind} / {content_type}");
    }

    let mib = main_len as f64 / (1024.0 * 1024.0);
    println!("main file:           {mib:.1} MiB");
    println!("hmac pass:           {hmac_time:.2?}");
    println!(
        "decrypt+decode:      {decode_time:.2?} ({:.1} MiB/s)",
        mib / decode_time.as_secs_f64().max(1e-9)
    );
    println!("total:               {:.2?}", started.elapsed());
    if failed > 0 {
        return Err("some attachments failed verification".into());
    }
    Ok(())
}
