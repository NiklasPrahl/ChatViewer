//! Generates every supported export variant, imports it and compares the
//! result with the generator's ground truth.

use std::io::Read;
use std::path::Path;

use cv_core::{ChatArchive, ImportOptions, Importer, InputSource, Probe};
use cv_import_whatsapp::WhatsAppImporter;
use cv_import_whatsapp::header::DateOrder;
use cv_import_whatsapp::synth::{self, Kind, Lang, Platform, SynthExport, Variant};
use cv_model::{
    Author, Availability, Content, ConversationData, ConversationKind, SelfFlag, Timestamp,
};

fn import(path: &Path, options: &ImportOptions) -> Box<dyn ChatArchive> {
    WhatsAppImporter
        .open(&InputSource::new(path), options)
        .expect("import succeeds")
}

fn check(export: &SynthExport, data: &ConversationData, label: &str) {
    assert_eq!(
        data.messages.len(),
        export.messages.len(),
        "{label}: message count"
    );
    let name = |author: &Author| match author {
        Author::Participant(id) => data
            .participants
            .iter()
            .find(|p| &p.id == id)
            .map(|p| p.display_name.clone()),
        Author::System => None,
    };
    for (i, (expected, got)) in export.messages.iter().zip(&data.messages).enumerate() {
        let at = format!("{label}: message {i}");
        assert_eq!(
            got.timestamp,
            Timestamp::NaiveLocal(expected.time),
            "{at}: time"
        );
        assert_eq!(
            name(&got.author).as_deref(),
            expected.author,
            "{at}: author"
        );
        assert_eq!(got.flags.edited, expected.edited, "{at}: edited");
        match (&expected.kind, &got.content) {
            (Kind::Text(t), Content::Text(rt)) => assert_eq!(&rt.text, t, "{at}: text"),
            (
                Kind::Media {
                    file,
                    kind,
                    caption,
                    included,
                },
                content,
            ) => {
                let (attachment, got_caption) = match content {
                    Content::Media {
                        attachments,
                        caption,
                    } => (&attachments[0], caption.as_ref().map(|c| c.text.clone())),
                    Content::Sticker { image, .. } => (image, None),
                    other => panic!("{at}: expected media, got {other:?}"),
                };
                assert_eq!(attachment.file_name.as_ref(), Some(file), "{at}: file");
                assert_eq!(attachment.kind, *kind, "{at}: kind");
                assert_eq!(&got_caption, caption, "{at}: caption");
                let available = matches!(attachment.availability, Availability::Available { .. });
                assert_eq!(available, *included, "{at}: availability");
            }
            (Kind::Omitted(kind), Content::Media { attachments, .. }) => {
                assert_eq!(attachments[0].availability, Availability::Omitted, "{at}");
                assert_eq!(attachments[0].kind, *kind, "{at}: omitted kind");
            }
            (Kind::Deleted { .. }, Content::Deleted { .. }) => {}
            (Kind::Location(lat, lon), Content::Location(l)) => {
                assert_eq!((l.latitude, l.longitude), (*lat, *lon), "{at}: location");
            }
            (Kind::Poll { question, options }, Content::Poll(p)) => {
                assert_eq!(&p.question, question, "{at}: poll question");
                let got: Vec<_> = p
                    .options
                    .iter()
                    .map(|o| (o.text.clone(), o.votes))
                    .collect();
                let want: Vec<_> = options.iter().map(|(t, n)| (t.clone(), Some(*n))).collect();
                assert_eq!(got, want, "{at}: poll options");
            }
            (Kind::System(kind), Content::System(e)) => assert_eq!(e.kind, *kind, "{at}: system"),
            (want, got) => panic!("{at}: expected {want:?}, got {got:?}"),
        }
    }

    assert_eq!(data.conversation.title, export.title, "{label}: title");
    let kind = if export.variant.group {
        ConversationKind::Group
    } else {
        ConversationKind::Direct
    };
    assert_eq!(data.conversation.kind, kind, "{label}: conversation kind");
    let mut names: Vec<&str> = data
        .participants
        .iter()
        .map(|p| p.display_name.as_str())
        .collect();
    names.sort_unstable();
    let mut want = export.participants.clone();
    want.sort_unstable();
    assert_eq!(names, want, "{label}: participants");

    let own_delete = export
        .messages
        .iter()
        .any(|m| matches!(m.kind, Kind::Deleted { own: true }));
    if own_delete {
        let me = data
            .participants
            .iter()
            .find(|p| p.display_name == export.self_name)
            .unwrap();
        assert_eq!(me.is_self, SelfFlag::Yes, "{label}: self detection");
    }
}

#[test]
fn every_variant_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let variants = Variant::matrix();
    assert!(variants.len() >= 20);
    for (i, variant) in variants.into_iter().enumerate() {
        let export = synth::generate(variant, i as u64 + 1, 120);
        let label = format!("variant {i} {variant:?}");
        let variant_dir = dir.path().join(i.to_string());
        std::fs::create_dir(&variant_dir).unwrap();
        let path = variant_dir.join(&export.zip_name);
        synth::write_zip(&export, &path).unwrap();

        let input = InputSource::new(&path);
        assert!(
            WhatsAppImporter.probe(&input) >= Probe::Maybe(90),
            "{label}: probe"
        );
        let archive = import(&path, &ImportOptions::default());
        let report = archive.report();
        let platform = match variant.platform {
            Platform::Ios => "ios",
            Platform::Android => "android",
        };
        assert_eq!(report.platform, platform, "{label}: platform");
        assert!(
            report.variant.contains(variant.order.label()),
            "{label}: {}",
            report.variant
        );
        let lang = match variant.lang {
            Lang::En => "-en",
            Lang::De => "-de",
        };
        assert!(
            report.variant.ends_with(lang),
            "{label}: {}",
            report.variant
        );
        assert!(report.warnings.is_empty(), "{label}: {:?}", report.warnings);
        assert!(report.confidence >= 80, "{label}: confidence");

        let id = &archive.conversation_ids()[0];
        let data = archive.load(id).unwrap();
        check(&export, &data, &label);

        // Every message keeps its original lines (ADR 0006).
        assert!(
            data.messages.iter().all(|m| m.extra.raw.is_some()),
            "{label}: raw"
        );

        // Available media can be opened, in memory.
        let (file, bytes) = &export.media[0];
        let blob = data
            .messages
            .iter()
            .find_map(|m| {
                match &m.content {
                    Content::Media { attachments, .. } => Some(&attachments[0]),
                    Content::Sticker { image, .. } => Some(image),
                    _ => None,
                }
                .filter(|a| a.file_name.as_ref() == Some(file))
                .and_then(|a| match &a.availability {
                    Availability::Available { blob, .. } => Some(blob.clone()),
                    _ => None,
                })
            })
            .expect("first media file is referenced");
        let mut got = Vec::new();
        archive
            .open_blob(&blob)
            .unwrap()
            .read_to_end(&mut got)
            .unwrap();
        assert_eq!(&got, bytes, "{label}: media bytes");
    }
}

fn android_variant() -> Variant {
    Variant {
        platform: Platform::Android,
        lang: Lang::En,
        twelve_hour: false,
        order: DateOrder::Dmy,
        four_digit_year: false,
        nnbsp: false,
        crlf: false,
        bom: false,
        group: false,
    }
}

#[test]
fn bare_txt_and_extracted_folder() {
    let dir = tempfile::tempdir().unwrap();
    let export = synth::generate(android_variant(), 7, 60);

    // Bare .txt: same messages, every attachment reported missing.
    let txt = dir.path().join(&export.chat_file_name);
    std::fs::write(&txt, &export.text).unwrap();
    let archive = import(&txt, &ImportOptions::default());
    let data = archive.load(&archive.conversation_ids()[0]).unwrap();
    assert_eq!(data.messages.len(), export.messages.len());
    assert!(
        archive
            .report()
            .limitations
            .iter()
            .any(|l| l.contains("no media"))
    );

    // Extracted folder: media available again.
    let folder = dir.path().join("extracted");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join(&export.chat_file_name), &export.text).unwrap();
    for (name, bytes) in &export.media {
        std::fs::write(folder.join(name), bytes).unwrap();
    }
    let archive = import(&folder, &ImportOptions::default());
    let data = archive.load(&archive.conversation_ids()[0]).unwrap();
    check(&export, &data, "folder");
}

#[test]
fn date_order_override_wins() {
    let dir = tempfile::tempdir().unwrap();
    let export = synth::generate(android_variant(), 9, 40);
    let path = dir.path().join(&export.zip_name);
    synth::write_zip(&export, &path).unwrap();
    let options = ImportOptions {
        overrides: vec![("date_order".into(), "mdy".into())],
        self_name: Some("Bob Sample".into()),
    };
    let archive = import(&path, &options);
    assert!(archive.report().variant.contains("-mdy-"));
    let data = archive.load(&archive.conversation_ids()[0]).unwrap();
    let bob = data
        .participants
        .iter()
        .find(|p| p.display_name == "Bob Sample")
        .unwrap();
    assert_eq!(bob.is_self, SelfFlag::Yes);
}

#[test]
fn rejects_unrelated_input_and_logs_no_content() {
    cv_core::logging::init(None);
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("notes.txt");
    std::fs::write(&other, "shopping list\nmilk\n").unwrap();
    assert_eq!(
        WhatsAppImporter.probe(&InputSource::new(&other)),
        Probe::NotMine
    );
    assert!(
        WhatsAppImporter
            .open(&InputSource::new(&other), &ImportOptions::default())
            .is_err()
    );

    let export = synth::generate(android_variant(), 11, 40);
    let path = dir.path().join(&export.zip_name);
    synth::write_zip(&export, &path).unwrap();
    import(&path, &ImportOptions::default());
    let log = cv_core::logging::snapshot().join("\n");
    assert!(log.contains("whatsapp:"), "the importer logs something");
    for name in export.participants {
        assert!(!log.contains(name), "log must not contain names");
    }
    for m in &export.messages {
        if let Kind::Text(t) = &m.kind {
            assert!(
                !log.contains(t.as_str()),
                "log must not contain message text"
            );
        }
    }
}
