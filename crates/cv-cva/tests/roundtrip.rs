//! Source → CVA → model must be lossless (ADR 0004, 0006).

use std::collections::HashMap;
use std::io::Read;

use cv_core::{ChatArchive, ImportOptions, Importer, InputSource, Probe};
use cv_cva::{CvaArchive, CvaImporter};
use cv_import_whatsapp::WhatsAppImporter;
use cv_import_whatsapp::synth::{self, Variant};
use cv_model::{Availability, BlobRef, Content, ConversationData};

/// Replaces CVA media paths with the source blob references.
fn normalize(mut data: ConversationData, cva: &CvaArchive) -> ConversationData {
    let back: HashMap<&str, &str> = cva
        .manifest()
        .media
        .iter()
        .map(|m| (m.path.as_str(), m.source_blob.as_str()))
        .collect();
    for m in &mut data.messages {
        let attachments: Vec<&mut cv_model::Attachment> = match &mut m.content {
            Content::Media { attachments, .. } => attachments.iter_mut().collect(),
            Content::Sticker { image, .. } => vec![image],
            _ => vec![],
        };
        for a in attachments {
            if let Availability::Available { blob, .. } = &mut a.availability {
                *blob = BlobRef::new(back[blob.as_str()]);
            }
        }
    }
    data
}

fn read_all(archive: &dyn ChatArchive, blob: &BlobRef) -> Vec<u8> {
    let mut out = Vec::new();
    archive
        .open_blob(blob)
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    out
}

#[test]
fn whatsapp_to_cva_is_lossless_for_every_variant() {
    let dir = tempfile::tempdir().unwrap();
    for (i, variant) in Variant::matrix().into_iter().enumerate().step_by(3) {
        let export = synth::generate(variant, 100 + i as u64, 80);
        let source_path = dir.path().join(format!("{i}-source.zip"));
        synth::write_zip(&export, &source_path).unwrap();
        let source = WhatsAppImporter
            .open(&InputSource::new(&source_path), &ImportOptions::default())
            .unwrap();
        let id = source.conversation_ids().remove(0);
        let original = source.load(&id).unwrap();

        let cva_path = dir.path().join(format!("{i}.cva.zip"));
        let summary =
            cv_cva::write(&*source, &id, std::fs::File::create(&cva_path).unwrap()).unwrap();
        assert_eq!(summary.media_failed, 0);
        assert_eq!(summary.media_written as usize, export.media.len());

        let input = InputSource::new(&cva_path);
        assert_eq!(CvaImporter.probe(&input), Probe::Maybe(100));
        // CVA must win over the WhatsApp importer for its own files.
        assert!(WhatsAppImporter.probe(&input) < CvaImporter.probe(&input));
        let cva = CvaArchive::open(&cva_path).unwrap();
        assert_eq!(cva.report(), source.report(), "variant {i}: report");
        let reread = cva.load(&id).unwrap();

        // Media bytes survive.
        for entry in &cva.manifest().media {
            assert_eq!(
                read_all(&cva, &BlobRef::new(entry.path.clone())),
                read_all(&*source, &BlobRef::new(entry.source_blob.clone())),
                "variant {i}: media"
            );
        }
        assert_eq!(normalize(reread, &cva), original, "variant {i}: data");
    }
}

#[test]
fn cva_chat_text_is_readable_without_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let export = synth::generate(Variant::matrix()[5], 7, 60);
    let source_path = dir.path().join("source.zip");
    synth::write_zip(&export, &source_path).unwrap();
    let source = WhatsAppImporter
        .open(&InputSource::new(&source_path), &ImportOptions::default())
        .unwrap();
    let id = source.conversation_ids().remove(0);
    let original = source.load(&id).unwrap();
    let cva_path = dir.path().join("chat.zip");
    cv_cva::write(&*source, &id, std::fs::File::create(&cva_path).unwrap()).unwrap();

    // Read the same file as a plain WhatsApp export (fallback path).
    let fallback = WhatsAppImporter
        .open(&InputSource::new(&cva_path), &ImportOptions::default())
        .unwrap();
    let data = fallback.load(&fallback.conversation_ids()[0]).unwrap();
    assert_eq!(data.messages.len(), original.messages.len());
    let available = |d: &ConversationData| {
        d.messages
            .iter()
            .filter(|m| match &m.content {
                Content::Media { attachments, .. } => attachments
                    .iter()
                    .any(|a| matches!(a.availability, Availability::Available { .. })),
                Content::Sticker { image, .. } => {
                    matches!(image.availability, Availability::Available { .. })
                }
                _ => false,
            })
            .count()
    };
    assert_eq!(available(&data), available(&original));
    for (a, b) in data.messages.iter().zip(&original.messages) {
        if let (Content::Text(x), Content::Text(y)) = (&a.content, &b.content) {
            assert_eq!(x.text, y.text);
        }
    }
}

#[test]
fn rejects_newer_major_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file(cv_cva::MANIFEST, zip::write::SimpleFileOptions::default())
        .unwrap();
    std::io::Write::write_all(
        &mut zip,
        br#"{"format":"chatviewer-archive","format_version":"2.0","generator":{"name":"x","version":"9"},
           "source":{"app":"x","platform":"x","variant":"x","confidence":1},
           "conversation":{"id":"c","kind":"direct","title":"t","participants":[]},
           "participants":[],"message_count":0,"media":[]}"#,
    )
    .unwrap();
    zip.finish().unwrap();
    let err = CvaArchive::open(&path).err().expect("must fail");
    assert!(err.to_string().contains("2.0"));
}
