//! End-to-end tests of the `chatviewer` binary on synthetic exports.

use std::path::Path;
use std::process::{Command, Output};

use cv_import_whatsapp::header::DateOrder;
use cv_import_whatsapp::synth::{self, Lang, Platform, Variant};

fn chatviewer(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_chatviewer"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn fixture(dir: &Path) -> (std::path::PathBuf, synth::SynthExport) {
    let export = synth::generate(
        Variant {
            platform: Platform::Ios,
            lang: Lang::De,
            twelve_hour: false,
            order: DateOrder::Dmy,
            four_digit_year: false,
            nnbsp: false,
            crlf: false,
            bom: false,
            group: true,
        },
        3,
        50,
    );
    let path = dir.join(&export.zip_name);
    synth::write_zip(&export, &path).unwrap();
    (path, export)
}

#[test]
fn info_reports_variant_and_hides_names_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let (zip, export) = fixture(dir.path());
    let out = chatviewer(&["info", zip.to_str().unwrap(), "--anonymous"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("whatsapp-ios-24h-dmy-de"), "{stdout}");
    assert!(stdout.contains("Chat 1"));
    assert!(!stdout.contains(&export.title));
    for name in &export.participants {
        assert!(!stdout.contains(name));
    }
}

#[test]
fn exports_all_formats_and_refuses_to_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let (zip, export) = fixture(dir.path());
    for format in ["txt", "md", "json"] {
        let out_path = dir.path().join(format!("out.{format}"));
        let args = [
            "export",
            zip.to_str().unwrap(),
            "--format",
            format,
            "-o",
            out_path.to_str().unwrap(),
        ];
        let out = chatviewer(&args);
        assert!(
            out.status.success(),
            "{format}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let written = std::fs::read_to_string(&out_path).unwrap();
        assert!(written.contains(&export.title), "{format}");

        let again = chatviewer(&args);
        assert!(!again.status.success(), "{format}: must not overwrite");
        assert!(String::from_utf8_lossy(&again.stderr).contains("--force"));
    }

    // JSON keeps everything and reads back.
    let json = std::fs::read(dir.path().join("out.json")).unwrap();
    let (data, report) = cv_export::json::read(json.as_slice()).unwrap();
    assert_eq!(data.messages.len(), export.messages.len());
    assert_eq!(report.app, "whatsapp");
}

#[test]
fn clear_errors_for_bad_input() {
    let out = chatviewer(&["info", "/nonexistent/chat.zip"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not exist"));

    let dir = tempfile::tempdir().unwrap();
    let (zip, _) = fixture(dir.path());
    let out = chatviewer(&["export", zip.to_str().unwrap(), "--format", "pdf"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown format"));
}
