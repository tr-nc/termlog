//! src/log_parser.rs
//! -------------------------------------------------------------------------
//! Converts an appended chunk of raw log text (“delta”) into structured
//! `LogItem`s.  The parser
//!   1. removes leading / inline timestamp headers,
//!   2. lets every `EventMatcher` carve out *special* blocks (pause, …),
//!   3. splits the remaining text into normal “## YYYY-MM-DD …” items,
//!   4. finally deduplicates identical `(time, content)` pairs.
//!
//! You can add more special events by implementing the `EventMatcher` trait
//! and pushing a boxed instance into the `MATCHERS` list.
//! -------------------------------------------------------------------------
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::ops::Range;

/* ──────────────────────────── regexes ─────────────────────────────────── */
lazy_static! {
    // Leading header that can appear right at the beginning of the delta
    static ref LEADING_HEADER_RE: Regex = Regex::new(
        r"^\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}\] \[\w+\]\s*\n?"
    )
    .unwrap();

    // Same header pattern but searched **everywhere** inside the delta
    static ref INLINE_HEADER_RE: Regex = Regex::new(
        r"\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}\] \[\w+\]\s*"
    )
    .unwrap();

    // Marks the start of a regular log item
    static ref ITEM_SEP_RE: Regex =
        Regex::new(r"## \d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}").unwrap();

    // Parses a regular log item into timestamp + body
    static ref ITEM_PARSE_RE: Regex =
        Regex::new(r"(?s)^## (\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\s*(.*)").unwrap();
}

/* ─────────────────────────  public struct  ────────────────────────────── */
#[derive(Debug, Clone)]
pub struct LogItem {
    pub time: String,    // empty ⇒ event has no timestamp
    pub content: String, // trimmed, multiline if necessary
}

/* ───────────────────── special-event framework ────────────────────────── */
mod special_events {
    use super::*;

    // Returned by a matcher: the byte range it occupied + the generated item
    pub struct MatchedEvent {
        pub span: Range<usize>,
        pub item: LogItem,
    }

    pub trait EventMatcher: Sync + Send {
        fn capture(&self, text: &str) -> Vec<MatchedEvent>;
    }

    /* ------------------------------- Pause ------------------------------ */
    struct PauseMatcher;

    impl PauseMatcher {
        // Return byte ranges of *blocks* that belong to a pause event
        fn pause_block_ranges(text: &str) -> Vec<Range<usize>> {
            lazy_static! {
                static ref PAUSE_RE: Regex = Regex::new("(?i)onpause").unwrap();
            }

            // First gather per-line ranges that contain “onPause”
            let mut ranges: Vec<Range<usize>> = PAUSE_RE
                .find_iter(text)
                .map(|m| {
                    // Expand to full line (incl. trailing \n if present)
                    let mut start = m.start();
                    let mut end = m.end();

                    start = text[..start].rfind('\n').map_or(0, |p| p + 1);
                    end += text[end..].find('\n').map_or(text.len() - end, |p| p + 1);

                    start..end
                })
                .collect();

            // Merge overlapping *or directly adjacent* ranges
            ranges.sort_by_key(|r| r.start);
            let mut merged: Vec<Range<usize>> = Vec::new();

            for r in ranges {
                if let Some(last) = merged.last_mut() {
                    // Touching ranges (gap ≤1 byte) belong together
                    if r.start <= last.end + 1 {
                        last.end = last.end.max(r.end);
                        continue;
                    }
                }
                merged.push(r.clone());
            }
            merged
        }
    }

    impl EventMatcher for PauseMatcher {
        fn capture(&self, text: &str) -> Vec<MatchedEvent> {
            Self::pause_block_ranges(text)
                .into_iter()
                .map(|span| MatchedEvent {
                    span,
                    item: LogItem {
                        time: String::new(),
                        content: "DYEH PAUSE".to_string(),
                    },
                })
                .collect()
        }
    }

    /* ----------------------- register matchers -------------------------- */
    lazy_static! {
        pub static ref MATCHERS: Vec<Box<dyn EventMatcher>> = vec![Box::new(PauseMatcher)];
    }
}
use special_events::{MATCHERS, MatchedEvent};

/* ─────────────────────── helper functions ─────────────────────────────── */
fn strip_leading_header(s: &str) -> &str {
    LEADING_HEADER_RE
        .find(s)
        .map(|m| &s[m.end()..])
        .unwrap_or(s)
}

fn remove_inline_headers(s: &str) -> String {
    INLINE_HEADER_RE.replace_all(s, "").into_owned()
}

fn parse_structured(block: &str) -> Option<LogItem> {
    ITEM_PARSE_RE.captures(block).map(|caps| LogItem {
        time: caps.get(1).map_or("", |m| m.as_str()).to_string(),
        content: caps.get(2).map_or("", |m| m.as_str()).trim().to_string(),
    })
}

/* ─────────────────────────────── API ──────────────────────────────────── */
pub fn process_delta(delta: &str) -> Vec<LogItem> {
    /* 1 ── initial cleaning: remove leading + inline headers ------------- */
    let mut body = remove_inline_headers(strip_leading_header(delta))
        .trim()
        .to_string();
    if body.is_empty() {
        return Vec::new();
    }

    /* 2 ── run special-event matchers, collect ranges to cut ------------- */
    let mut specials: Vec<LogItem> = Vec::new();
    let mut cut_ranges: Vec<Range<usize>> = Vec::new();

    for matcher in MATCHERS.iter() {
        for MatchedEvent { span, item } in matcher.capture(&body) {
            specials.push(item);
            cut_ranges.push(span);
        }
    }

    /* 3 ── physically remove the special-event ranges -------------------- */
    if !cut_ranges.is_empty() {
        cut_ranges.sort_by_key(|r| r.start);
        let mut cleaned = String::with_capacity(body.len());
        let mut last = 0;
        for r in cut_ranges {
            cleaned.push_str(&body[last..r.start]);
            last = r.end;
        }
        cleaned.push_str(&body[last..]);
        body = cleaned;
    }

    /* 4 ── split remaining text into regular items ----------------------- */
    let mut items: Vec<LogItem> = Vec::new();
    let mut starts: Vec<usize> = ITEM_SEP_RE.find_iter(&body).map(|m| m.start()).collect();

    if !starts.is_empty() {
        let len_total = body.len();
        starts.push(len_total); // sentinel for the last slice
        for window in starts.windows(2) {
            if let [s, e] = *window {
                if let Some(item) = parse_structured(&body[s..e]) {
                    items.push(item);
                }
            }
        }
    }

    /* 5 ── add specials and deduplicate identical items ------------------ */
    items.extend(specials);

    let mut seen = HashSet::<(String, String)>::new();
    items
        .into_iter()
        .filter(|it| seen.insert((it.time.clone(), it.content.clone())))
        .collect()
}
