//! Message header lines and their timestamps.
//!
//! ```text
//! Android: 31.12.23, 23:59 - Alice: text          12/31/23, 11:59 PM - Alice: text
//! iOS:     [31.12.23, 23:59:59] Alice: text       [12/31/23, 11:59:59 PM] Alice: text
//! ```
//!
//! Date order (DMY/MDY/YMD) cannot be read from one line; it is inferred
//! from the whole file in [`infer_date_order`].

use std::sync::LazyLock;

use chrono::{NaiveDate, NaiveDateTime};
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// iOS: `[date, time] rest`
    Bracket,
    /// Android: `date, time - rest`
    Dash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrder {
    Dmy,
    Mdy,
    Ymd,
}

impl DateOrder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dmy => "dmy",
            Self::Mdy => "mdy",
            Self::Ymd => "ymd",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "dmy" => Some(Self::Dmy),
            "mdy" => Some(Self::Mdy),
            "ymd" => Some(Self::Ymd),
            _ => None,
        }
    }
}

/// A recognised header line, before the date order is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub style: Style,
    /// The three numeric date fields in file order, with their digit counts.
    pub date: [(u32, usize); 3],
    pub hour: u32,
    pub minute: u32,
    pub second: Option<u32>,
    /// `Some(true)` for PM, `Some(false)` for AM, `None` for a 24-hour clock.
    pub pm: Option<bool>,
    /// Everything after the header (author and text).
    pub rest: String,
}

const DATE: &str = r"(\d{1,4})[./-]\s?(\d{1,2})[./-]\s?(\d{1,4})";
const TIME: &str = r"(\d{1,2})[:.](\d{2})(?:[:.](\d{2}))?";
const AMPM: &str = r"(?:\s?(AM|PM|am|pm|[aApP]\.\s?[mM]\.|vorm\.|nachm\.|午前|午後))?";

static BRACKET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^\[{DATE},?\s{TIME}{AMPM}\]\s(.*)$")).expect("valid regex")
});
static DASH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^{DATE},?\s{TIME}{AMPM}\s[-–]\s(.*)$")).expect("valid regex")
});

/// Removes directional and zero-width marks WhatsApp puts at line starts.
pub fn strip_marks(s: &str) -> &str {
    s.trim_start_matches([
        '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{feff}',
    ])
}

/// Maps Arabic-Indic, Persian and Devanagari digits to ASCII.
pub fn normalize_digits(s: &str) -> std::borrow::Cow<'_, str> {
    let map = |c: char| -> Option<char> {
        let base = match c {
            '\u{0660}'..='\u{0669}' => 0x0660,
            '\u{06f0}'..='\u{06f9}' => 0x06f0,
            '\u{0966}'..='\u{096f}' => 0x0966,
            _ => return None,
        };
        char::from_digit(c as u32 - base, 10)
    };
    if s.chars().any(|c| map(c).is_some()) {
        s.chars()
            .map(|c| map(c).unwrap_or(c))
            .collect::<String>()
            .into()
    } else {
        s.into()
    }
}

/// Parses a header line, or returns `None` for a continuation line.
pub fn parse_header(line: &str) -> Option<Header> {
    let line = strip_marks(line);
    // Only the header part needs digit normalization; keep the text as-is.
    let head_len = line.char_indices().nth(40).map_or(line.len(), |(i, _)| i);
    let normalized = format!(
        "{}{}",
        normalize_digits(&line[..head_len]),
        &line[head_len..]
    );
    let (style, caps) = if let Some(c) = BRACKET.captures(&normalized) {
        (Style::Bracket, c)
    } else {
        (Style::Dash, DASH.captures(&normalized)?)
    };
    let num = |i: usize| {
        caps.get(i)
            .map(|m| (m.as_str().parse::<u32>().ok(), m.as_str().len()))
    };
    let field = |i: usize| -> Option<(u32, usize)> {
        let (n, len) = num(i)?;
        Some((n?, len))
    };
    let pm = caps.get(7).map(|m| {
        let s = m.as_str().to_lowercase();
        s.starts_with('p') || s == "nachm." || s == "午後"
    });
    Some(Header {
        style,
        date: [field(1)?, field(2)?, field(3)?],
        hour: field(4)?.0,
        minute: field(5)?.0,
        second: field(6).map(|f| f.0),
        pm,
        rest: caps.get(8)?.as_str().to_owned(),
    })
}

/// Result of date order inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderGuess {
    pub order: DateOrder,
    /// 0–100.
    pub confidence: u8,
    /// True if the data alone could not decide.
    pub ambiguous: bool,
}

/// Infers the date order from all headers of a file. `prefer_mdy` is the
/// tie-breaker (e.g. a 12-hour clock suggests a US locale).
pub fn infer_date_order(headers: &[&Header], prefer_mdy: bool) -> OrderGuess {
    let sure = |order| OrderGuess {
        order,
        confidence: 100,
        ambiguous: false,
    };
    if headers.is_empty() {
        return OrderGuess {
            order: DateOrder::Dmy,
            confidence: 0,
            ambiguous: true,
        };
    }
    if headers.iter().any(|h| h.date[0].1 == 4 || h.date[0].0 > 31) {
        return sure(DateOrder::Ymd);
    }
    let first_over_12 = headers.iter().any(|h| h.date[0].0 > 12);
    let second_over_12 = headers.iter().any(|h| h.date[1].0 > 12);
    match (first_over_12, second_over_12) {
        (true, false) => return sure(DateOrder::Dmy),
        (false, true) => return sure(DateOrder::Mdy),
        _ => {}
    }
    // Undecidable from ranges: prefer the order that keeps time monotonic.
    let inversions = |order| {
        let times: Vec<_> = headers
            .iter()
            .filter_map(|h| to_datetime(h, order))
            .collect();
        times.windows(2).filter(|w| w[1] < w[0]).count()
    };
    let (dmy, mdy) = (inversions(DateOrder::Dmy), inversions(DateOrder::Mdy));
    if dmy != mdy {
        let order = if dmy < mdy {
            DateOrder::Dmy
        } else {
            DateOrder::Mdy
        };
        return OrderGuess {
            order,
            confidence: 80,
            ambiguous: false,
        };
    }
    OrderGuess {
        order: if prefer_mdy {
            DateOrder::Mdy
        } else {
            DateOrder::Dmy
        },
        confidence: 50,
        ambiguous: true,
    }
}

/// Builds the local timestamp of a header for a given date order.
pub fn to_datetime(h: &Header, order: DateOrder) -> Option<NaiveDateTime> {
    let [a, b, c] = h.date;
    let (y, m, d) = match order {
        DateOrder::Dmy => (c, b.0, a.0),
        DateOrder::Mdy => (c, a.0, b.0),
        DateOrder::Ymd => (a, b.0, c.0),
    };
    let year = if y.1 <= 2 {
        2000 + y.0 as i32
    } else {
        y.0 as i32
    };
    let hour = match h.pm {
        None => h.hour,
        Some(false) if h.hour == 12 => 0,
        Some(true) if h.hour < 12 => h.hour + 12,
        Some(_) => h.hour,
    };
    NaiveDate::from_ymd_opt(year, m, d)?.and_hms_opt(hour, h.minute, h.second.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn parses_android_and_ios_headers() {
        let h = parse_header("31.12.23, 23:59 - Alice: Hi").unwrap();
        assert_eq!((h.style, h.rest.as_str()), (Style::Dash, "Alice: Hi"));
        assert_eq!(
            to_datetime(&h, DateOrder::Dmy),
            Some(dt("2023-12-31 23:59:00"))
        );

        let h = parse_header("\u{200e}[12/31/23, 11:59:58\u{202f}PM] Bob: \u{200e}x").unwrap();
        assert_eq!(h.style, Style::Bracket);
        assert_eq!(h.pm, Some(true));
        assert_eq!(
            to_datetime(&h, DateOrder::Mdy),
            Some(dt("2023-12-31 23:59:58"))
        );

        let h = parse_header("2023-01-05, 12:01 a.m. - System text").unwrap();
        assert_eq!(
            to_datetime(&h, DateOrder::Ymd),
            Some(dt("2023-01-05 00:01:00"))
        );
    }

    #[test]
    fn continuation_lines_are_not_headers() {
        assert!(parse_header("just text").is_none());
        assert!(parse_header("12.03.23 is my birthday").is_none());
        assert!(parse_header("").is_none());
    }

    #[test]
    fn normalizes_non_latin_digits() {
        let h = parse_header("٣١/١٢/٢٣, ٢٣:٥٩ - A: hi").unwrap();
        assert_eq!(
            to_datetime(&h, DateOrder::Dmy),
            Some(dt("2023-12-31 23:59:00"))
        );
    }

    #[test]
    fn infers_date_order() {
        let parse = |lines: &[&str]| -> Vec<Header> {
            lines.iter().map(|l| parse_header(l).unwrap()).collect()
        };
        let dmy = parse(&["13/01/24, 10:00 - a: x", "14/01/24, 10:00 - a: x"]);
        let mdy = parse(&["01/13/24, 10:00 - a: x"]);
        let mono = parse(&["02/01/24, 10:00 - a: x", "01/02/24, 10:00 - a: x"]);
        let one = parse(&["01/02/24, 10:00 - a: x"]);
        fn refs(v: &[Header]) -> Vec<&Header> {
            v.iter().collect()
        }
        assert_eq!(infer_date_order(&refs(&dmy), true).order, DateOrder::Dmy);
        assert_eq!(infer_date_order(&refs(&mdy), false).order, DateOrder::Mdy);
        // 02/01 → 01/02 only stays monotonic as DMY.
        let g = infer_date_order(&refs(&mono), true);
        assert_eq!((g.order, g.ambiguous), (DateOrder::Dmy, false));
        let g = infer_date_order(&refs(&one), true);
        assert_eq!((g.order, g.ambiguous), (DateOrder::Mdy, true));
    }
}
