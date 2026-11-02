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
}

fn main() -> ExitCode {
    let Command::Signal {
        out,
        seed,
        chats,
        no_group,
        messages,
        media,
    } = Cli::parse().command;
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
