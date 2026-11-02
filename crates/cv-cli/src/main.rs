//! `chatviewer`: inspect and export chat archives from the command line.
//!
//! Uses the same core as the GUI. Reads archives in place and writes only
//! the export file the user asks for (ADR 0003).

use std::fs::OpenOptions;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use cv_core::{ChatArchive, ImportOptions, Importer, InputSource, probe_all};
use cv_model::{ConversationData, Timestamp};

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
    },
    /// Export one chat to txt, md or json.
    Export {
        #[command(flatten)]
        input: InputArgs,
        /// Output format: txt, md or json.
        #[arg(short, long)]
        format: String,
        /// Output file; `-` or omitted for stdout.
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
    /// Chat archive: WhatsApp .zip/.txt or extracted folder.
    input: PathBuf,
    /// Date order of the source, if detection guesses wrong: dmy, mdy or ymd.
    #[arg(long)]
    date_order: Option<String>,
    /// Your display name in the chat, if the source does not mark it.
    #[arg(long)]
    me: Option<String>,
}

fn importers() -> Vec<Box<dyn Importer>> {
    vec![Box::new(cv_import_whatsapp::WhatsAppImporter)]
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
    Ok(importer.open(&input, &options)?)
}

fn run(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::Info { input, anonymous } => info(&*open(&input)?, anonymous),
        Command::Export {
            input,
            format,
            output,
            force,
            chat,
        } => {
            let exporter = cv_export::by_name(&format)
                .ok_or_else(|| format!("unknown format {format:?} (use txt, md or json)"))?;
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
            let data = archive.load(&id)?;
            let mut out: Box<dyn Write> = match output.as_deref() {
                None => Box::new(io::stdout().lock()),
                Some(p) if p == Path::new("-") => Box::new(io::stdout().lock()),
                Some(path) => {
                    let mut file = OpenOptions::new();
                    file.write(true);
                    if force {
                        file.create(true).truncate(true);
                    } else {
                        file.create_new(true);
                    }
                    let file = file.open(path).map_err(|e| match e.kind() {
                        io::ErrorKind::AlreadyExists => {
                            format!("{} exists; use --force to overwrite", path.display())
                        }
                        _ => e.to_string(),
                    })?;
                    Box::new(BufWriter::new(file))
                }
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
        let data = archive.load(id)?;
        let title = if anonymous {
            format!("Chat {}", i + 1)
        } else {
            data.conversation.title.clone()
        };
        let range = date_range(&data);
        println!(
            "  {id}  {title:?}  {:?}  {} participants  {} messages  {range}",
            data.conversation.kind,
            data.participants.len(),
            data.messages.len(),
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

fn date_range(data: &ConversationData) -> String {
    let day = |t: &Timestamp| match t {
        Timestamp::NaiveLocal(t) => t.format("%Y-%m-%d").to_string(),
        Timestamp::UtcMillis(ms) => format!("{ms} ms UTC"),
    };
    match (data.messages.first(), data.messages.last()) {
        (Some(first), Some(last)) => {
            format!("{} to {}", day(&first.timestamp), day(&last.timestamp))
        }
        _ => String::new(),
    }
}
