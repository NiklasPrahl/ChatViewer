//! Markdown for reading and sharing, grouped by day.

use std::io::Write;

use cv_core::{Exporter, Result};
use cv_model::{Author, ConversationData, SourceReport};

use crate::{author_name, describe, format_time, names};

pub struct MarkdownExporter;

impl Exporter for MarkdownExporter {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn extension(&self) -> &'static str {
        "md"
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
        writeln!(out, "# {}", escape(&data.conversation.title))?;
        writeln!(out)?;
        writeln!(
            out,
            "> Exported by ChatViewer from `{}` ({} messages). Markdown is lossy: \
             media are listed by name, reactions and formatting are summarised.",
            report.variant,
            data.messages.len()
        )?;
        let mut day = String::new();
        for m in &data.messages {
            let this_day = format_time(&m.timestamp, "%Y-%m-%d");
            if this_day != day {
                writeln!(out)?;
                writeln!(out, "## {this_day}")?;
                day = this_day;
            }
            writeln!(out)?;
            let time = format_time(&m.timestamp, "%H:%M");
            let (text, notes) = describe(&m.content);
            if matches!(m.author, Author::System) {
                writeln!(out, "*{time} · {}*", escape(&text).replace('\n', " "))?;
                continue;
            }
            let edited = if m.flags.edited { " · edited" } else { "" };
            writeln!(
                out,
                "**{}** · {time}{edited}  ",
                escape(author_name(m, &names))
            )?;
            if let Some(q) = &m.quote {
                for line in q.snippet.text.lines() {
                    writeln!(out, "> {}", escape(line))?;
                }
                // Without a blank line the reply would continue the quote.
                writeln!(out)?;
            }
            for note in &notes {
                writeln!(out, "{}  ", escape(note))?;
            }
            if !text.is_empty() {
                let lines: Vec<String> = text.lines().map(escape).collect();
                writeln!(out, "{}", lines.join("  \n"))?;
            }
            if !m.reactions.is_empty() {
                let r: Vec<&str> = m.reactions.iter().map(|r| r.emoji.as_str()).collect();
                writeln!(out, "{}", r.join(" "))?;
            }
        }
        Ok(())
    }
}

/// Escapes characters Markdown would interpret, so chat text shows as written.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|' | '~'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markdown_syntax() {
        assert_eq!(escape("*bold* <x> # 1"), "\\*bold\\* \\<x\\> \\# 1");
    }
}
