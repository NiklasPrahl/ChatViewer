//! Generates synthetic test fixtures. Output never contains real data.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use cv_import_signal::writer::synth::{self, SynthOptions};
use cv_import_signal::writer::write_local_backup;
use rand::SeedableRng;
use rand::rngs::StdRng;

#[derive(Parser)]
#[command(about = "Generate synthetic chat archives for tests")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// A Signal local backup folder. Prints the synthetic recovery key.
    Signal {
        /// Output folder (becomes the backup root, like `SignalBackups`).
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Number of one-to-one chats.
        #[arg(long, default_value_t = 2)]
        chats: usize,
        /// Skip the group chat.
        #[arg(long)]
        no_group: bool,
        /// Total number of messages.
        #[arg(long, default_value_t = 50)]
        messages: usize,
        /// Number of messages with an attachment.
        #[arg(long, default_value_t = 3)]
        media: usize,
    },
    /// WhatsApp export ZIPs, one per supported format variant.
    Whatsapp {
        /// Output folder; one subfolder per variant.
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Messages per export.
        #[arg(long, default_value_t = 120)]
        messages: usize,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Signal {
            out,
            seed,
            chats,
            no_group,
            messages,
            media,
        } => signal(out, seed, chats, no_group, messages, media),
        Command::Whatsapp {
            out,
            seed,
            messages,
        } => whatsapp(&out, seed, messages),
    }
}

fn whatsapp(out: &std::path::Path, seed: u64, messages: usize) -> ExitCode {
    use cv_import_whatsapp::synth::{self, Variant};

    for (i, variant) in Variant::matrix().into_iter().enumerate() {
        let export = synth::generate(variant, seed + i as u64, messages);
        let dir = out.join(format!("{i:02}"));
        let result = std::fs::create_dir_all(&dir)
            .and_then(|()| synth::write_zip(&export, &dir.join(&export.zip_name)));
        if let Err(e) = result {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
        println!("{i:02} {variant:?}");
    }
    ExitCode::SUCCESS
}

fn signal(
    out: PathBuf,
    seed: u64,
    chats: usize,
    no_group: bool,
    messages: usize,
    media: usize,
) -> ExitCode {
    let backup = synth::generate(&SynthOptions {
        seed,
        direct_chats: chats,
        group: !no_group,
        messages,
        media,
    });
    let key = backup.recovery_key.parse().expect("generated key is valid");
    let mut rng = StdRng::seed_from_u64(seed ^ 0x5eed);
    match write_local_backup(
        &out,
        &key,
        &backup.aci,
        &backup.info,
        backup.frames,
        &backup.media,
        &mut rng,
    ) {
        Ok(snapshot) => {
            eprintln!("wrote {}", snapshot.display());
            // Synthetic key for a synthetic backup; printed for test scripts.
            println!("{}", backup.recovery_key);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
