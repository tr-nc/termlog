//! src/log_parser.rs
//! -------------------------------------------------------------------------
//! Converts an appended chunk of raw log text (“delta”) into structured
//! `LogItem`s.  The parser
//!   1. removes leading / inline timestamp headers,
//!   2. lets every `EventMatcher` carve out *special* blocks (pause, …),
//!   3. splits the remaining text into normal “## YYYY-MM-DD …” items,
//!   4. extracts `origin / level / tag` from every item’s content,
//!   5. finally deduplicates identical `(time, origin, level, tag, content)` pairs.
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
    ).unwrap();

    // Same header pattern but searched **everywhere** inside the delta
    static ref INLINE_HEADER_RE: Regex = Regex::new(
        r"\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}\] \[\w+\]\s*"
    ).unwrap();

    // Marks the start of a regular log item
    static ref ITEM_SEP_RE: Regex =
        Regex::new(r"## \d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}").unwrap();

    // Parses a regular log item into timestamp + body
    static ref ITEM_PARSE_RE: Regex =
        Regex::new(r"(?s)^## (\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\s*(.*)").unwrap();

    // Extracts:  [origin] LEVEL ## [TAG] message…
    // IMPORTANT: In (?x) mode, `#` starts a comment. Escape the hashes as \#\#.
    static ref CONTENT_HEADER_RE: Regex = Regex::new(
        r"(?xs)
          ^\[(?P<origin>[^\]]+)]\s*
          (?P<level>[A-Z]+)\s*
          \#\#\s*
          \[(?P<tag>[^\]]+)]\s*
          (?P<msg>.*)"
    ).unwrap();
}

/* ─────────────────────────  public struct  ────────────────────────────── */
#[derive(Debug, Clone)]
pub struct LogItem {
    pub time: String, // empty ⇒ not present
    pub origin: String,
    pub level: String,
    pub tag: String,
    pub content: String,
}

/* ───────────────────── special-event framework ────────────────────────── */
mod special_events {
    use super::*;

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
        fn pause_block_ranges(text: &str) -> Vec<Range<usize>> {
            lazy_static! {
                static ref PAUSE_RE: Regex = Regex::new("(?i)onpause").unwrap();
            }
            let mut ranges: Vec<Range<usize>> = PAUSE_RE
                .find_iter(text)
                .map(|m| {
                    let mut s = m.start();
                    let mut e = m.end();
                    s = text[..s].rfind('\n').map_or(0, |p| p + 1);
                    e += text[e..].find('\n').map_or(text.len() - e, |p| p + 1);
                    s..e
                })
                .collect();
            ranges.sort_by_key(|r| r.start);
            let mut merged = Vec::<Range<usize>>::new();
            for r in ranges {
                if let Some(last) = merged.last_mut() {
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
                        origin: String::new(),
                        level: String::new(),
                        tag: String::new(),
                        content: "DYEH PAUSE".to_string(),
                    },
                })
                .collect()
        }
    }

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

// Split “[origin] LEVEL ## [TAG] …” → (origin, level, tag, msg)
fn split_header(line: &str) -> (String, String, String, String) {
    // Be robust to BOM/control chars that might precede the first “[”.
    let line =
        line.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}' || c.is_control());

    if let Some(caps) = CONTENT_HEADER_RE.captures(line) {
        (
            caps["origin"].trim().to_owned(),
            caps["level"].trim().to_owned(),
            caps["tag"].trim().to_owned(),
            caps["msg"].trim().to_owned(),
        )
    } else {
        (
            String::new(),
            String::new(),
            String::new(),
            line.trim().to_owned(),
        )
    }
}

fn parse_structured(block: &str) -> Option<LogItem> {
    ITEM_PARSE_RE.captures(block).map(|caps| LogItem {
        time: caps.get(1).map_or("", |m| m.as_str()).to_string(),
        origin: String::new(),
        level: String::new(),
        tag: String::new(),
        content: caps.get(2).map_or("", |m| m.as_str()).trim().to_string(),
    })
}

/* ─────────────────────────────── API ──────────────────────────────────── */
pub fn process_delta(delta: &str) -> Vec<LogItem> {
    /* 1 ── initial cleaning --------------------------------------------- */
    let mut body = remove_inline_headers(strip_leading_header(delta))
        .trim()
        .to_string();
    if body.is_empty() {
        return Vec::new();
    }

    /* 2 ── special events ----------------------------------------------- */
    let mut specials = Vec::<LogItem>::new();
    let mut cut_ranges = Vec::<Range<usize>>::new();
    for m in MATCHERS.iter() {
        for MatchedEvent { span, item } in m.capture(&body) {
            specials.push(item);
            cut_ranges.push(span);
        }
    }

    /* 3 ── cut them out -------------------------------------------------- */
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

    /* 4 ── split into regular items ------------------------------------- */
    let mut items = Vec::<LogItem>::new();
    let mut starts: Vec<usize> = ITEM_SEP_RE.find_iter(&body).map(|m| m.start()).collect();
    if !starts.is_empty() {
        let len_total = body.len();
        starts.push(len_total); // sentinel
        for win in starts.windows(2) {
            if let [s, e] = *win {
                if let Some(mut it) = parse_structured(&body[s..e]) {
                    let (o, l, t, msg) = split_header(&it.content);
                    it.origin = o;
                    it.level = l;
                    it.tag = t;
                    it.content = msg;
                    items.push(it);
                }
            }
        }
    }

    /* 5 ── merge & deduplicate ------------------------------------------ */
    items.extend(specials);
    let mut seen = HashSet::<(String, String, String, String, String)>::new();
    items
        .into_iter()
        .filter(|it| {
            seen.insert((
                it.time.clone(),
                it.origin.clone(),
                it.level.clone(),
                it.tag.clone(),
                it.content.clone(),
            ))
        })
        .collect()
}
