//! In-memory, redacted logging (ADR 0003).
//!
//! Log records are kept in a ring buffer that backs the "Copy log" button.
//! Code must never log message content, names, phone numbers, file names
//! or keys; use [`Redacted`] where a value would help debugging.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};

const CAPACITY: usize = 2000;

/// Shows only the length of a sensitive value.
pub struct Redacted<'a>(pub &'a str);

impl fmt::Display for Redacted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted {} chars>", self.0.chars().count())
    }
}

struct RingLogger {
    records: Mutex<VecDeque<String>>,
    echo_level: Mutex<Option<Level>>,
}

impl Log for RingLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{:<5} {}: {}",
            record.level(),
            record.target(),
            record.args()
        );
        if let Some(level) = *self.echo_level.lock().expect("log lock")
            && record.level() <= level
        {
            eprintln!("{line}");
        }
        let mut records = self.records.lock().expect("log lock");
        if records.len() == CAPACITY {
            records.pop_front();
        }
        records.push_back(line);
    }

    fn flush(&self) {}
}

fn logger() -> &'static RingLogger {
    static LOGGER: OnceLock<RingLogger> = OnceLock::new();
    LOGGER.get_or_init(|| RingLogger {
        records: Mutex::new(VecDeque::with_capacity(CAPACITY)),
        echo_level: Mutex::new(None),
    })
}

/// Installs the ring-buffer logger. Safe to call more than once.
/// `echo` additionally prints records up to that level to stderr.
pub fn init(echo: Option<Level>) {
    let logger = logger();
    *logger.echo_level.lock().expect("log lock") = echo;
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}

/// The current log, oldest record first (for "Copy log").
pub fn snapshot() -> Vec<String> {
    logger()
        .records
        .lock()
        .expect("log lock")
        .iter()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_shows_only_length() {
        assert_eq!(Redacted("Grüße").to_string(), "<redacted 5 chars>");
    }

    #[test]
    fn ring_buffer_keeps_records() {
        init(None);
        log::info!("hello {}", Redacted("secret"));
        let log = snapshot().join("\n");
        assert!(log.contains("<redacted 6 chars>"));
        assert!(!log.contains("secret"));
    }
}
