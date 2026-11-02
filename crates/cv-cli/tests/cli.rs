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

fn chatviewer_with_stdin(args: &[&str], stdin: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_chatviewer"))
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("binary runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn signal_backup_to_cva_and_back() {
    use cv_import_signal::writer::synth::{self as signal_synth, SynthOptions};
    use rand::SeedableRng;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("SignalBackups");
    let b = signal_synth::generate(&SynthOptions::default());
    let key: cv_import_signal::key::RecoveryKey = b.recovery_key.parse().unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    cv_import_signal::writer::write_local_backup(
        &root, &key, &b.aci, &b.info, b.frames, &b.media, &mut rng,
    )
    .unwrap();
    let root_s = root.to_str().unwrap();

    // Without a key on a non-terminal stdin: clear error.
    let out = chatviewer_with_stdin(&["info", root_s], "");
    assert!(!out.status.success());

    let out = chatviewer_with_stdin(&["info", root_s, "--recovery-key-stdin"], &b.recovery_key);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("source:       signal"), "{stdout}");
    assert!(!stdout.contains(&b.recovery_key));
    let chat = stdout
        .lines()
        .find(|l| l.contains("Group"))
        .and_then(|l| l.split_whitespace().next())
        .expect("group chat listed")
        .to_owned();

    let zip = dir.path().join("group.zip");
    let zip_s = zip.to_str().unwrap();
    let args = [
        "export",
        root_s,
        "--recovery-key-stdin",
        "--chat",
        &chat,
        "--format",
        "cva",
        "-o",
        zip_s,
    ];
    let out = chatviewer_with_stdin(&args, &b.recovery_key);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("media files"));

    // The archive reopens without a key, as a ChatViewer archive.
    let out = chatviewer(&["info", zip_s]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("source:       signal"), "{stdout}");
    assert!(stdout.contains("Synthetic Group"));
}

#[test]
fn exports_every_signal_chat_to_its_own_reopenable_zip() {
    use cv_core::ChatArchive;
    use cv_import_signal::writer::synth::{self as signal_synth, SynthOptions};
    use rand::SeedableRng;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("SignalBackups");
    let options = SynthOptions {
        messages: 90,
        media: 6,
        ..SynthOptions::default()
    };
    let b = signal_synth::generate(&options);
    let key: cv_import_signal::key::RecoveryKey = b.recovery_key.parse().unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(2);
    cv_import_signal::writer::write_local_backup(
        &root, &key, &b.aci, &b.info, b.frames, &b.media, &mut rng,
    )
    .unwrap();
    let out = dir.path().join("exports");
    std::fs::create_dir(&out).unwrap();
    let args = [
        "export",
        root.to_str().unwrap(),
        "--recovery-key-stdin",
        "--all",
        "--format",
        "cva",
        "--out-dir",
        out.to_str().unwrap(),
    ];
    let run = chatviewer_with_stdin(&args, &b.recovery_key);
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let mut files: Vec<_> = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert_eq!(files.len(), 3, "one ZIP per chat: {files:?}");
    let mut total = 0;
    let mut media = 0;
    for f in &files {
        // Each archive opens on its own: no key, no backup needed.
        let cva = cv_cva::CvaArchive::open(f).unwrap();
        let data = cva.load(&cva.conversation_ids()[0]).unwrap();
        total += data.messages.len();
        media += cva.manifest().media.len();
    }
    assert_eq!(total, options.messages);
    assert_eq!(media, options.media);

    // A second run never overwrites: new files get " (2)".
    let again = chatviewer_with_stdin(&args, &b.recovery_key);
    assert!(again.status.success());
    let names: Vec<String> = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 6);
    assert_eq!(names.iter().filter(|n| n.ends_with(" (2).zip")).count(), 3);
}
