//! Hand-written lines in shapes the synthetic generator does not produce
//! (independent of `synth`, so generator and parser cannot share a bug).

use cv_core::{ImportOptions, Importer, InputSource};
use cv_import_whatsapp::WhatsAppImporter;
use cv_model::{Author, Availability, Content, ConversationData, MediaKind, SystemKind};

fn parse(file_name: &str, text: &str) -> (ConversationData, cv_model::SourceReport) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(file_name);
    std::fs::write(&path, text).unwrap();
    let archive = WhatsAppImporter
        .open(&InputSource::new(&path), &ImportOptions::default())
        .unwrap();
    let data = archive.load(&archive.conversation_ids()[0]).unwrap();
    (data, archive.report().clone())
}

#[test]
fn old_ios_markers_and_emoji_names() {
    let text = "\
[04.08.15, 10:01:02] Zoë 🌻 Example: Hi
[04.08.15, 10:01:30] Zoë 🌻 Example: 2015-08-04-PHOTO-00004762.jpg <\u{200e}attached>
[14.08.15, 22:15:00] Max Sample: Bye: see you
";
    let (data, report) = parse("_chat.txt", text);
    assert_eq!(report.platform, "ios");
    assert_eq!(data.participants[0].display_name, "Zoë 🌻 Example");
    match &data.messages[1].content {
        Content::Media { attachments, .. } => {
            assert_eq!(attachments[0].kind, MediaKind::Image);
            assert_eq!(attachments[0].availability, Availability::Missing);
        }
        other => panic!("expected media, got {other:?}"),
    }
    match &data.messages[2].content {
        Content::Text(t) => assert_eq!(t.text, "Bye: see you"),
        other => panic!("expected text, got {other:?}"),
    }
}

#[test]
fn android_dotted_am_pm_and_unknown_system_lines() {
    let text = "\
Exported chat preamble that is not a message
1/5/24, 9:15 a.m. - Messages and calls are end-to-end encrypted. Tap to learn more.
1/5/24, 9:16 a.m. - Ana Example: Hola
1/13/24, 1:05 p.m. - Ben Sample: Hey
1/13/24, 1:06 p.m. - Something happened that we do not know
1/13/24, 1:07 p.m. - Ana Example: I added sugar
1/13/24, 1:08 p.m. - Ben Sample: Ok
";
    let (data, report) = parse("WhatsApp Chat with Ben Sample.txt", text);
    assert_eq!(report.platform, "android");
    assert!(report.variant.contains("-12h-mdy-"), "{}", report.variant);
    assert_eq!(report.counters.get("unrecognised_lines"), Some(&1));
    assert_eq!(data.conversation.title, "Ben Sample");
    assert_eq!(data.messages.len(), 6);

    let time = |i: usize| match data.messages[i].timestamp {
        cv_model::Timestamp::NaiveLocal(t) => t.format("%Y-%m-%d %H:%M").to_string(),
        other => panic!("{other:?}"),
    };
    assert_eq!(time(0), "2024-01-05 09:15");
    assert_eq!(time(2), "2024-01-13 13:05");

    assert!(matches!(&data.messages[0].content,
        Content::System(e) if e.kind == SystemKind::EncryptionNotice));
    assert!(matches!(data.messages[3].author, Author::System));
    assert!(matches!(&data.messages[3].content,
        Content::System(e) if e.kind == SystemKind::Other));
    // A frequent author whose text resembles a system phrase stays a message.
    assert!(matches!(&data.messages[4].content, Content::Text(t) if t.text == "I added sugar"));
}

/// Shapes seen in a real iOS export (2026-11), with synthetic content.
#[test]
fn ios_caption_before_marker_empty_text_and_group_calls() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("WhatsApp Chat - Ben Sample");
    std::fs::create_dir(&folder).unwrap();
    for f in [
        "00000001-PHOTO-2024-02-01-10-00-00.jpg",
        "00000002-Report_final.pdf",
    ] {
        std::fs::write(folder.join(f), b"synthetic").unwrap();
    }
    let text = "\
[01.02.24, 09:59:00] Ana Example: Hallo
[01.02.24, 10:00:00] Ana Example: Look at this 😂 \u{200e}<Anhang: 00000001-PHOTO-2024-02-01-10-00-00.jpg>
[01.02.24, 10:01:00] Ben Sample: Report_final.pdf • \u{200e}12 Seiten \u{200e}<Anhang: 00000002-Report_final.pdf>
[01.02.24, 10:02:00] Ben Sample:
[01.02.24, 10:03:00] Ana Example: \u{200e}Verpasster Gruppen-Videoanruf
[13.02.24, 10:04:00] Ben Sample: Ok
";
    std::fs::write(folder.join("_chat.txt"), text).unwrap();
    let archive = WhatsAppImporter
        .open(&InputSource::new(&folder), &ImportOptions::default())
        .unwrap();
    let data = archive.load(&archive.conversation_ids()[0]).unwrap();
    assert_eq!(data.participants.len(), 2);

    match &data.messages[1].content {
        Content::Media {
            attachments,
            caption,
        } => {
            assert!(matches!(
                attachments[0].availability,
                Availability::Available { .. }
            ));
            assert_eq!(
                caption.as_ref().map(|c| c.text.as_str()),
                Some("Look at this 😂")
            );
        }
        other => panic!("expected media, got {other:?}"),
    }
    match &data.messages[2].content {
        Content::Media {
            attachments,
            caption,
        } => {
            assert_eq!(attachments[0].kind, MediaKind::Document);
            assert!(matches!(
                attachments[0].availability,
                Availability::Available { .. }
            ));
            assert_eq!(caption, &None, "document info is not a caption");
        }
        other => panic!("expected document, got {other:?}"),
    }
    assert!(matches!(&data.messages[3].content, Content::Text(t) if t.text.is_empty()));
    assert!(!matches!(data.messages[3].author, Author::System));
    assert!(matches!(&data.messages[4].content,
        Content::System(e) if e.kind == SystemKind::MissedCall && e.actors.len() == 1));
}

#[test]
fn ambiguous_dates_are_reported() {
    let text = "\
01/02/24, 10:00 - Ana Example: one
01/02/24, 11:00 - Ben Sample: two
";
    let (_, report) = parse("WhatsApp Chat with Ben Sample.txt", text);
    assert!(
        report.warnings.iter().any(|w| w.contains("date order")),
        "{:?}",
        report.warnings
    );
    assert!(report.confidence < 70);
}
