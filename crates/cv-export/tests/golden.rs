//! Golden outputs for every exporter and a lossless JSON round trip,
//! on one synthetic conversation that covers every content type.

use chrono::NaiveDate;
use cv_core::Exporter;
use cv_export::{json, markdown::MarkdownExporter, text::TextExporter};
use cv_model::*;

fn at(day: u32, h: u32, m: u32) -> Timestamp {
    Timestamp::NaiveLocal(
        NaiveDate::from_ymd_opt(2024, 3, day)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap(),
    )
}

fn sample() -> (ConversationData, SourceReport) {
    let alice = ParticipantId::new("p1");
    let bob = ParticipantId::new("p2");
    let msg = |seq: u64, ts, author: Author, content| Message {
        id: MessageId::new(format!("m{seq}")),
        seq,
        timestamp: ts,
        author,
        content,
        quote: None,
        reactions: Vec::new(),
        flags: MessageFlags::default(),
        extra: SourceExtra {
            raw: Some(RawRecord::Text {
                format: "test".into(),
                text: format!("raw line {seq}"),
            }),
            fields: Default::default(),
        },
    };
    let a = || Author::Participant(alice.clone());
    let b = || Author::Participant(bob.clone());
    let image = |availability| {
        let mut att = Attachment::new(MediaKind::Image, availability);
        att.file_name = Some("IMG-0001.jpg".into());
        att.mime = Some("image/jpeg".into());
        att
    };

    let mut messages = vec![
        msg(
            0,
            at(1, 9, 0),
            Author::System,
            Content::System(SystemEvent {
                kind: SystemKind::EncryptionNotice,
                actors: vec![],
                text: "Messages are end-to-end encrypted.".into(),
            }),
        ),
        msg(
            1,
            at(1, 9, 5),
            a(),
            Content::Text(RichText::plain("Hi *Bob*!\nSecond line <ok>")),
        ),
        msg(
            2,
            at(1, 9, 6),
            b(),
            Content::Media {
                attachments: vec![image(Availability::Available {
                    blob: BlobRef::new("IMG-0001.jpg"),
                    integrity: Integrity::Unchecked,
                })],
                caption: Some(RichText::plain("Look")),
            },
        ),
        msg(
            3,
            at(1, 9, 7),
            a(),
            Content::Media {
                attachments: vec![Attachment::new(MediaKind::Video, Availability::Omitted)],
                caption: None,
            },
        ),
        msg(
            4,
            at(2, 18, 30),
            b(),
            Content::Sticker {
                emoji: Some("😀".into()),
                pack_id: None,
                sticker_id: None,
                image: Attachment::new(MediaKind::Sticker, Availability::NotInBackup),
            },
        ),
        msg(
            5,
            at(2, 18, 31),
            a(),
            Content::Location(Location {
                latitude: 52.52,
                longitude: 13.405,
                name: Some("Synthetic Square".into()),
                url: Some("https://maps.example/?q=52.52,13.405".into()),
            }),
        ),
        msg(
            6,
            at(2, 18, 32),
            b(),
            Content::Poll(Poll {
                question: "Lunch?".into(),
                options: vec![
                    PollOption {
                        text: "Pizza".into(),
                        votes: Some(2),
                    },
                    PollOption {
                        text: "Salad".into(),
                        votes: None,
                    },
                ],
            }),
        ),
        msg(7, at(2, 18, 33), a(), Content::Deleted { by: None }),
        msg(
            8,
            at(2, 18, 34),
            b(),
            Content::Text(RichText::plain("Sure")),
        ),
        msg(
            9,
            at(2, 18, 35),
            a(),
            Content::Unsupported {
                source_kind: "payment".into(),
                note: "not mapped yet".into(),
            },
        ),
    ];
    messages[1].flags.edited = true;
    messages[8].quote = Some(Quote {
        target: Some(MessageId::new("m1")),
        author: Some(alice.clone()),
        snippet: RichText::plain("Hi *Bob*!"),
    });
    messages[8].reactions = vec![Reaction {
        emoji: "👍".into(),
        author: alice.clone(),
        timestamp: Some(Timestamp::UtcMillis(1_709_400_000_000)),
    }];

    let participant = |id: &ParticipantId, name: &str, is_self| Participant {
        id: id.clone(),
        display_name: name.into(),
        is_self,
        handle: None,
        extra: SourceExtra::default(),
    };
    let data = ConversationData {
        conversation: Conversation {
            id: ConversationId::new("c1"),
            kind: ConversationKind::Direct,
            title: "Bob Sample".into(),
            participants: vec![alice.clone(), bob.clone()],
            extra: SourceExtra::default(),
        },
        participants: vec![
            participant(&alice, "Alice Example", SelfFlag::Yes),
            participant(&bob, "Bob Sample", SelfFlag::No),
        ],
        messages,
    };
    let report = SourceReport {
        app: "test".into(),
        platform: "android".into(),
        variant: "test-variant".into(),
        confidence: 100,
        ..Default::default()
    };
    (data, report)
}

fn export(exporter: &dyn Exporter) -> String {
    let (data, report) = sample();
    let mut out = Vec::new();
    exporter.export(&data, &report, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn text_golden() {
    insta::assert_snapshot!(export(&TextExporter));
}

#[test]
fn markdown_golden() {
    insta::assert_snapshot!(export(&MarkdownExporter));
}

#[test]
fn json_round_trip_is_lossless() {
    let out = export(&json::JsonExporter);
    let (data, report) = json::read(out.as_bytes()).unwrap();
    assert_eq!((data, report), sample());
    // Keep the format stable; version string is replaced to avoid churn.
    let stable = out.replace(env!("CARGO_PKG_VERSION"), "<version>");
    insta::assert_snapshot!(stable);
}

#[test]
fn json_rejects_other_versions() {
    let out = export(&json::JsonExporter).replace("\"1.0\"", "\"2.0\"");
    assert!(json::read(out.as_bytes()).is_err());
}
