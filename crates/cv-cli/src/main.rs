//! `chatviewer`: inspect and export chat archives from the command line.
//!
//! Uses the same core as the GUI. Reads archives in place and writes only
//! the export file the user asks for (ADR 0003).

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufWriter, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use cv_core::{ChatArchive, ImportOptions, Importer, InputSource, Secret, probe_all};
use cv_model::Timestamp;

#[derive(Parser)]
#[command(
    name = "chatviewer",
    version,
    about = "Inspect and export chat archives"
)]
struct Cli {
    /// Print log messages to stderr (never includes message content).
    #[arg(short, long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show what was detected: source, format variant, warnings, chats.
    Info {
        #[command(flatten)]
        input: InputArgs,
        /// Hide chat titles and names (for sharing the output).
        #[arg(long)]
        anonymous: bool,
        /// Also count message types, media and quotes over all chats
        /// (aggregated numbers only, no content).
        #[arg(long)]
        stats: bool,
    },
    /// Export one chat to txt, md, json or cva (ZIP with media).
    Export {
        #[command(flatten)]
        input: InputArgs,
        /// Output format: txt, md, json or cva.
        #[arg(short, long)]
        format: String,
        /// Output file; `-` or omitted for stdout (not for cva).
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Overwrite an existing output file.
        #[arg(long)]
        force: bool,
        /// Chat id from `info` (needed when an archive has several chats).
        #[arg(long)]
        chat: Option<String>,
    },
}

#[derive(Args)]
struct InputArgs {
    /// WhatsApp export (.zip, .txt or folder), Signal backup folder, or
    /// ChatViewer archive (.zip).
    input: PathBuf,
    /// Read a required secret (Signal recovery key) from stdin instead of
    /// a hidden prompt. Never pass keys as arguments.
    #[arg(long)]
    recovery_key_stdin: bool,
    /// Date order of the source, if detection guesses wrong: dmy, mdy or ymd.
    #[arg(long)]
    date_order: Option<String>,
    /// Your display name in the chat, if the source does not mark it.
    #[arg(long)]
    me: Option<String>,
}

fn importers() -> Vec<Box<dyn Importer>> {
    vec![
        Box::new(cv_cva::CvaImporter),
        Box::new(cv_import_whatsapp::WhatsAppImporter),
        #[cfg(feature = "signal")]
        Box::new(cv_import_signal::SignalImporter),
    ]
}

fn read_secret(prompt: &str, from_stdin: bool) -> Result<Secret, Box<dyn std::error::Error>> {
    if from_stdin {
        let mut line = String::new();
        io::stdin().lock().read_line(&mut line)?;
        let secret = Secret::new(line.trim().to_owned());
        line.clear();
        return Ok(secret);
    }
    if !io::stdin().is_terminal() {
        return Err(format!("this input needs a {prompt}; pass --recovery-key-stdin").into());
    }
    Ok(Secret::new(rpassword::prompt_password(format!(
        "{prompt} (input hidden): "
    ))?))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    cv_core::logging::init(cli.verbose.then_some(log::Level::Debug));
    match run(cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn open(args: &InputArgs) -> Result<Box<dyn ChatArchive>, Box<dyn std::error::Error>> {
    if !args.input.exists() {
        return Err(format!("{} does not exist", args.input.display()).into());
    }
    let input = InputSource::new(&args.input);
    let importers = importers();
    let importer = probe_all(&importers, &input).ok_or("no importer recognises this input")?;
    log::info!("cli: opening input with importer {}", importer.id());
    let mut options = ImportOptions {
        self_name: args.me.clone(),
        ..ImportOptions::default()
    };
    if let Some(order) = &args.date_order {
        options.overrides.push(("date_order".into(), order.clone()));
    }
    if let Some(prompt) = importer.secret_prompt() {
        options.secret = Some(read_secret(prompt, args.recovery_key_stdin)?);
    }
    Ok(importer.open(&input, &options)?)
}

fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::Info {
            input,
            anonymous,
            stats,
        } => {
            let archive = open(&input)?;
            info(&*archive, anonymous)?;
            if stats {
                print_stats(&*archive)?;
            }
            Ok(())
        }
        Command::Export {
            input,
            format,
            output,
            force,
            chat,
        } => {
            let cva = matches!(format.to_ascii_lowercase().as_str(), "cva" | "zip");
            let exporter = cv_export::by_name(&format);
            if exporter.is_none() && !cva {
                return Err(format!("unknown format {format:?} (use txt, md, json or cva)").into());
            }
            if cva && output.as_deref().is_none_or(|p| p == Path::new("-")) {
                return Err("cva export needs an output file (-o chat.zip)".into());
            }
            let archive = open(&input)?;
            let ids = archive.conversation_ids();
            let id = match chat {
                Some(chat) => ids
                    .into_iter()
                    .find(|id| id.as_str() == chat)
                    .ok_or_else(|| format!("no chat with id {chat:?}; see `chatviewer info`"))?,
                None if ids.len() == 1 => ids.into_iter().next().expect("one id"),
                None => return Err("this archive has several chats; choose one with --chat".into()),
            };
            if cva {
                let path = output.as_deref().expect("checked above");
                let summary = cv_cva::write(&*archive, &id, create(path, force)?)?;
                eprintln!(
                    "wrote {} messages and {} media files{}",
                    summary.messages,
                    summary.media_written,
                    match summary.media_failed {
                        0 => String::new(),
                        n => format!("; {n} media files could not be read and are marked missing"),
                    }
                );
                return Ok(());
            }
            let exporter = exporter.expect("checked above");
            let data = archive.load(&id)?;
            let mut out: Box<dyn Write> = match output.as_deref() {
                None => Box::new(io::stdout().lock()),
                Some(p) if p == Path::new("-") => Box::new(io::stdout().lock()),
                Some(path) => Box::new(BufWriter::new(create(path, force)?)),
            };
            exporter.export(&data, archive.report(), &mut out)?;
            out.flush()?;
            if !exporter.lossless() {
                eprintln!(
                    "note: {} export is lossy; use json to keep everything",
                    exporter.extension()
                );
            }
            Ok(())
        }
    }
}

/// Creates the output file; never overwrites without `--force`.
fn create(path: &Path, force: bool) -> Result<File, String> {
    let mut file = OpenOptions::new();
    file.write(true);
    if force {
        file.create(true).truncate(true);
    } else {
        file.create_new(true);
    }
    file.open(path).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => {
            format!("{} exists; use --force to overwrite", path.display())
        }
        _ => e.to_string(),
    })
}

fn info(archive: &dyn ChatArchive, anonymous: bool) -> Result<(), Box<dyn std::error::Error>> {
    let r = archive.report();
    println!("source:       {} ({})", r.app, r.platform);
    println!("variant:      {}", r.variant);
    if let Some(v) = &r.source_version {
        println!("version:      {v}");
    }
    println!("confidence:   {}", r.confidence);
    print_list("warnings:", &r.warnings);
    print_list("limitations:", &r.limitations);
    if !r.counters.is_empty() {
        println!("counters:");
        for (k, v) in &r.counters {
            println!("  {k:<24} {v}");
        }
    }
    println!("chats:");
    for (i, id) in archive.conversation_ids().iter().enumerate() {
        let s = archive.summary(id)?;
        let title = if anonymous {
            format!("Chat {}", i + 1)
        } else {
            s.title.clone()
        };
        let range = match (s.first, s.last) {
            (Some(first), Some(last)) => format!("{} to {}", day(&first), day(&last)),
            _ => String::new(),
        };
        println!(
            "  {id}  {title:?}  {:?}  {} participants  {} messages  {range}",
            s.kind, s.participants, s.messages,
        );
    }
    Ok(())
}

fn print_list(label: &str, items: &[String]) {
    if items.is_empty() {
        println!("{label:<13} none");
    } else {
        println!("{label}");
        for item in items {
            println!("  - {item}");
        }
    }
}

fn day(t: &Timestamp) -> String {
    match t {
        Timestamp::NaiveLocal(t) => t.format("%Y-%m-%d").to_string(),
        Timestamp::UtcMillis(ms) => chrono::DateTime::from_timestamp_millis(*ms)
            .map(|t| t.format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
    }
}

/// Content-free statistics over all chats.
fn print_stats(archive: &dyn ChatArchive) -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::BTreeMap;

    use cv_model::{Availability, Content};

    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut add = |k: String| *counts.entry(k).or_default() += 1;
    for data in archive.load_all()? {
        for m in &data.messages {
            let attachments: Vec<&cv_model::Attachment> = match &m.content {
                Content::Text(_) => {
                    add("content text".into());
                    vec![]
                }
                Content::Media { attachments, .. } => {
                    add("content media".into());
                    attachments.iter().collect()
                }
                Content::Sticker { image, emoji, .. } => {
                    add("content sticker".into());
                    if emoji.is_some() {
                        add("sticker with emoji".into());
                    }
                    vec![image]
                }
                Content::Location(_) => {
                    add("content location".into());
                    vec![]
                }
                Content::Poll(_) => {
                    add("content poll".into());
                    vec![]
                }
                Content::System(e) => {
                    add(format!("system {:?}", e.kind));
                    vec![]
                }
                Content::Deleted { .. } => {
                    add("content deleted".into());
                    vec![]
                }
                Content::Unsupported { source_kind, .. } => {
                    add(format!("unsupported {source_kind}"));
                    vec![]
                }
            };
            for a in attachments {
                let status = match &a.availability {
                    Availability::Available { .. } => "available",
                    Availability::Omitted => "omitted",
                    Availability::Missing => "missing",
                    Availability::NotInBackup => "not in backup",
                };
                add(format!("media {:?} {status}", a.kind));
            }
            if let Some(q) = &m.quote {
                add(if q.target.is_some() {
                    "quote resolved".into()
                } else {
                    "quote unresolved".into()
                });
            }
            for _ in &m.reactions {
                add("reaction".into());
            }
            if m.flags.edited {
                add("edited".into());
            }
            if m.flags.view_once {
                add("view once".into());
            }
        }
    }
    println!("statistics:");
    for (k, v) in &counts {
        println!("  {k:<36} {v}");
    }
    Ok(())
}
