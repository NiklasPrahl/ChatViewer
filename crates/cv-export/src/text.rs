//! Plain text in a fixed, locale-independent, WhatsApp-like layout.

use std::io::Write;

use cv_core::{Exporter, Result};
use cv_model::{ConversationData, SourceReport};

use crate::{author_name, describe, format_time, names};

pub struct TextExporter;

impl Exporter for TextExporter {
    fn id(&self) -> &'static str {
        "text"
    }

    fn extension(&self) -> &'static str {
        "txt"
    }

    fn lossless(&self) -> bool {
        false
    }

    fn export(
        &self,
        data: &ConversationData,
        report: &SourceReport,
        out: &mut dyn Write,
    ) -> Result<()> {
        let names = names(data);
        writeln!(out, "# {}", data.conversation.title)?;
        writeln!(
            out,
            "# Exported by ChatViewer from {} ({} messages). Plain text is lossy:",
            report.variant,
            data.messages.len()
        )?;
        writeln!(
            out,
            "# media, reactions and formatting are summarised or left out."
        )?;
        writeln!(out)?;
        for m in &data.messages {
            let (text, notes) = describe(&m.content);
            let mut body = notes.join(" ");
            if !text.is_empty() {
                if !body.is_empty() {
                    body.push(' ');
                }
                body.push_str(&text);
            }
            if let Some(q) = &m.quote {
                body = format!("[reply to: {}] {body}", first_line(&q.snippet.text));
            }
            if m.flags.edited {
                body.push_str(" (edited)");
            }
            if !m.reactions.is_empty() {
                let r: Vec<&str> = m.reactions.iter().map(|r| r.emoji.as_str()).collect();
                body.push_str(&format!(" [reactions: {}]", r.join(" ")));
            }
            writeln!(
                out,
                "[{}] {}: {body}",
                format_time(&m.timestamp, "%Y-%m-%d %H:%M:%S"),
                author_name(m, &names)
            )?;
        }
        Ok(())
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}
