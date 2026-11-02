//! Signal backup → model → CVA → model, on synthetic backups.

use std::collections::HashMap;
use std::io::Read;

use cv_core::{ChatArchive, ImportOptions, Importer, InputSource, Probe, Secret};
use cv_cva::CvaArchive;
use cv_import_signal::SignalImporter;
use cv_import_signal::writer::synth::{self, SynthOptions};
use cv_import_signal::writer::write_local_backup;
use cv_model::{
    Author, Availability, BlobRef, Content, ConversationData, ConversationKind, SelfFlag,
};
use rand::SeedableRng;
use rand::rngs::StdRng;

fn backup(dir: &std::path::Path, options: SynthOptions) -> (std::path::PathBuf, String) {
    let root = dir.join("SignalBackups");
    let b = synth::generate(&options);
    let key = b.recovery_key.parse().unwrap();
    let mut rng = StdRng::seed_from_u64(5);
    write_local_backup(&root, &key, &b.aci, &b.info, b.frames, &b.media, &mut rng).unwrap();
    (root, b.recovery_key)
}

fn open(root: &std::path::Path, key: &str) -> Box<dyn ChatArchive> {
    let options = ImportOptions {
        secret: Some(Secret::new(key.to_owned())),
        ..ImportOptions::default()
    };
    SignalImporter
        .open(&InputSource::new(root), &options)
        .unwrap()
}

#[test]
fn maps_synthetic_backup_to_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let options = SynthOptions {
        messages: 120,
        media: 6,
        ..SynthOptions::default()
    };
    let (root, key) = backup(dir.path(), options.clone());
    assert_eq!(
        SignalImporter.probe(&InputSource::new(&root)),
        Probe::Maybe(95)
    );
    assert!(SignalImporter.secret_prompt().is_some());

    let archive = open(&root, &key);
    assert_eq!(archive.report().app, "signal");
    let ids = archive.conversation_ids();
    assert_eq!(ids.len(), 3, "two direct chats and one group");

    let mut total = 0;
    let mut media = 0;
    let mut reactions = 0;
    let mut resolved_quotes = 0;
    for id in &ids {
        let data = archive.load(id).unwrap();
        total += data.messages.len();
        let me = data
            .participants
            .iter()
            .find(|p| p.is_self == SelfFlag::Yes)
            .unwrap();
        assert_eq!(me.display_name, "Synthetic Owner");
        if data.conversation.kind == ConversationKind::Group {
            assert_eq!(data.conversation.title, "Synthetic Group");
            assert!(data.participants.len() >= 3);
        }
        for m in &data.messages {
            assert!(m.extra.raw.is_some(), "raw frame kept");
            assert!(matches!(m.author, Author::Participant(_)));
            reactions += m.reactions.len();
            if m.quote.as_ref().is_some_and(|q| q.target.is_some()) {
                resolved_quotes += 1;
            }
            if let Content::Media { attachments, .. } = &m.content {
                for a in attachments {
                    let Availability::Available { blob, .. } = &a.availability else {
                        panic!("synthetic media must be available");
                    };
                    let mut bytes = Vec::new();
                    archive
                        .open_blob(blob)
                        .unwrap()
                        .read_to_end(&mut bytes)
                        .unwrap();
                    assert!(bytes.starts_with(b"Synthetic attachment"));
                    media += 1;
                }
            }
        }
    }
    assert_eq!(total, options.messages);
    assert_eq!(media, options.media);
    assert!(reactions > 0);
    assert!(resolved_quotes > 0, "quotes point to earlier messages");
}

#[test]
fn wrong_key_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let (root, _) = backup(dir.path(), SynthOptions::default());
    let other = synth::generate(&SynthOptions {
        seed: 999,
        ..SynthOptions::default()
    });
    let options = ImportOptions {
        secret: Some(Secret::new(other.recovery_key)),
        ..ImportOptions::default()
    };
    let err = SignalImporter
        .open(&InputSource::new(&root), &options)
        .err()
        .expect("must fail");
    assert!(err.to_string().contains("wrong recovery key"), "{err}");
    let missing = SignalImporter
        .open(&InputSource::new(&root), &ImportOptions::default())
        .err()
        .expect("must fail");
    assert!(missing.to_string().contains("recovery key"));
}

#[test]
fn every_chat_round_trips_through_cva() {
    let dir = tempfile::tempdir().unwrap();
    let (root, key) = backup(
        dir.path(),
        SynthOptions {
            messages: 90,
            media: 5,
            ..SynthOptions::default()
        },
    );
    let archive = open(&root, &key);
    for (i, id) in archive.conversation_ids().iter().enumerate() {
        let original = archive.load(id).unwrap();
        let path = dir.path().join(format!("chat{i}.zip"));
        let summary = cv_cva::write(&*archive, id, std::fs::File::create(&path).unwrap()).unwrap();
        assert_eq!(summary.media_failed, 0);

        let cva = CvaArchive::open(&path).unwrap();
        assert_eq!(cva.report(), archive.report());
        let back: HashMap<String, String> = cva
            .manifest()
            .media
            .iter()
            .map(|m| (m.path.clone(), m.source_blob.clone()))
            .collect();
        for entry in &cva.manifest().media {
            let mut a = Vec::new();
            let mut b = Vec::new();
            cva.open_blob(&BlobRef::new(entry.path.clone()))
                .unwrap()
                .read_to_end(&mut a)
                .unwrap();
            archive
                .open_blob(&BlobRef::new(entry.source_blob.clone()))
                .unwrap()
                .read_to_end(&mut b)
                .unwrap();
            assert_eq!(a, b, "media bytes");
        }
        let mut reread: ConversationData = cva.load(id).unwrap();
        for m in &mut reread.messages {
            if let Content::Media { attachments, .. } = &mut m.content {
                for a in attachments {
                    if let Availability::Available { blob, .. } = &mut a.availability {
                        *blob = BlobRef::new(back[blob.as_str()].clone());
                    }
                }
            }
        }
        assert_eq!(reread, original, "chat {i}");
    }
}
