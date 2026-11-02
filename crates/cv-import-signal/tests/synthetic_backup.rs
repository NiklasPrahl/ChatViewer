//! End-to-end tests against synthetic backups written by `writer`.

use std::fs;
use std::path::{Path, PathBuf};

use cv_import_signal::Error;
use cv_import_signal::key::{BackupKey, MessageBackupKey, RecoveryKey};
use cv_import_signal::layout::{self, BackupLocation};
use cv_import_signal::main_file::{FORWARD_SECRECY_MAGIC, MainReader};
use cv_import_signal::media::{self, Integrity, LocalMedia};
use cv_import_signal::metadata::LocalMetadata;
use cv_import_signal::proto::backup::{chat_item, frame};
use cv_import_signal::writer::synth::{self, SynthOptions};
use cv_import_signal::writer::write_local_backup;
use rand::SeedableRng;
use rand::rngs::StdRng;

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    key: String,
    options: SynthOptions,
}

fn fixture(options: SynthOptions) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("SignalBackups");
    let backup = synth::generate(&options);
    let key: RecoveryKey = backup.recovery_key.parse().unwrap();
    let mut rng = StdRng::seed_from_u64(99);
    write_local_backup(
        &root,
        &key,
        &backup.aci,
        &backup.info,
        backup.frames,
        &backup.media,
        &mut rng,
    )
    .unwrap();
    Fixture {
        _dir: dir,
        root,
        key: backup.recovery_key,
        options,
    }
}

fn message_key(location: &BackupLocation, recovery_key: &str) -> MessageBackupKey {
    let backup_key = BackupKey::from_recovery_key(&recovery_key.parse().unwrap());
    let metadata = LocalMetadata::read(&location.metadata_path()).unwrap();
    backup_key.message_backup_key(&metadata.backup_id(&backup_key))
}

fn local_media(location: &BackupLocation, key: &MessageBackupKey) -> Vec<LocalMedia> {
    let mut found = Vec::new();
    for frame in MainReader::open(&location.main_path(), key).unwrap() {
        if let Some(frame::Item::ChatItem(item)) = frame.unwrap().item
            && let Some(chat_item::Item::StandardMessage(m)) = item.item
        {
            found.extend(
                m.attachments
                    .iter()
                    .filter_map(|a| a.pointer.as_ref().and_then(LocalMedia::from_pointer)),
            );
        }
    }
    found
}

fn flip_byte(path: &Path, offset_from_end: usize) {
    let mut bytes = fs::read(path).unwrap();
    let i = bytes.len() - offset_from_end;
    bytes[i] ^= 0x01;
    fs::write(path, bytes).unwrap();
}

#[test]
fn decrypts_everything_with_only_the_recovery_key() {
    let f = fixture(SynthOptions::default());
    let location = layout::locate(&f.root).unwrap();
    let key = message_key(&location, &f.key);

    let reader = MainReader::open(&location.main_path(), &key).unwrap();
    assert_eq!(reader.info().version, 1);
    let mut chat_items = 0;
    let mut first_is_account = None;
    for frame in reader {
        let item = frame.unwrap().item;
        first_is_account.get_or_insert(matches!(item, Some(frame::Item::Account(_))));
        chat_items += usize::from(matches!(item, Some(frame::Item::ChatItem(_))));
    }
    assert_eq!(first_is_account, Some(true));
    assert_eq!(chat_items, f.options.messages);

    let media = local_media(&location, &key);
    assert_eq!(media.len(), f.options.media);
    let listed = media::read_files_list(&location.files_list_path()).unwrap();
    for m in &media {
        assert!(listed.contains(&m.media_name));
        let decrypted = media::decrypt_file(&location.media_path(&m.media_name), m).unwrap();
        assert_eq!(decrypted.integrity, Integrity::Verified);
        assert_eq!(decrypted.data.len() as u64, m.size);
        assert!(decrypted.data.starts_with(b"Synthetic attachment"));
    }
}

#[test]
fn wrong_key_fails_authentication() {
    let f = fixture(SynthOptions::default());
    let location = layout::locate(&f.root).unwrap();
    let other = synth::generate(&SynthOptions {
        seed: 12345,
        ..SynthOptions::default()
    });
    let key = message_key(&location, &other.recovery_key);
    assert!(matches!(
        MainReader::open(&location.main_path(), &key),
        Err(Error::AuthenticationFailed)
    ));
}

#[test]
fn tampered_or_truncated_main_is_rejected() {
    let f = fixture(SynthOptions::default());
    let location = layout::locate(&f.root).unwrap();
    let key = message_key(&location, &f.key);
    let main = location.main_path();
    let original = fs::read(&main).unwrap();

    flip_byte(&main, 100);
    assert!(matches!(
        MainReader::open(&main, &key),
        Err(Error::AuthenticationFailed)
    ));

    fs::write(&main, &original[..original.len() - 7]).unwrap();
    assert!(MainReader::open(&main, &key).is_err());

    let mut with_magic = FORWARD_SECRECY_MAGIC.to_vec();
    with_magic.extend_from_slice(&original);
    fs::write(&main, with_magic).unwrap();
    assert!(matches!(
        MainReader::open(&main, &key),
        Err(Error::ForwardSecrecyNotSupported)
    ));
}

#[test]
fn tampered_media_is_rejected() {
    let f = fixture(SynthOptions::default());
    let location = layout::locate(&f.root).unwrap();
    let key = message_key(&location, &f.key);
    let m = local_media(&location, &key).remove(0);
    let path = location.media_path(&m.media_name);
    flip_byte(&path, 40);
    assert!(matches!(
        media::decrypt_file(&path, &m),
        Err(Error::MediaAuthenticationFailed)
    ));
}

#[test]
fn backup_without_media_or_group() {
    let f = fixture(SynthOptions {
        group: false,
        media: 0,
        messages: 10,
        ..SynthOptions::default()
    });
    let location = layout::locate(&f.root).unwrap();
    let key = message_key(&location, &f.key);
    assert!(local_media(&location, &key).is_empty());
    assert!(
        media::read_files_list(&location.files_list_path())
            .unwrap()
            .is_empty()
    );
}
