//! log_parser.rs
use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    /* ────────── regular-item regexes ────────── */
    static ref HEADER_RE: Regex =
        Regex::new(r"^\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}\] \[\w+\]\s*\n?")
            .unwrap();

    static ref INLINE_HEADER_RE: Regex =
        // same pattern, *not* anchored → remove everywhere
        Regex::new(r"\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}\] \[\w+\]\s*")
            .unwrap();

    static ref LOG_ITEM_SEPARATOR_RE: Regex =
        Regex::new(r"## \d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}").unwrap();

    static ref LOG_ITEM_PARSE_RE: Regex =
        Regex::new(r"(?s)^## (\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\s*(.*)").unwrap();
}

/* ────────── public type ────────── */
#[derive(Debug, Clone)]
pub struct LogItem {
    pub time: String,
    pub content: String,
}

/* ────────── special-event framework (unchanged) ────────── */
mod special_events {
    use super::*;
    pub trait EventMatcher: Sync + Send {
        fn try_match(&self, block: &str) -> Option<LogItem>;
    }

    struct PauseMatcher;
    impl EventMatcher for PauseMatcher {
        fn try_match(&self, block: &str) -> Option<LogItem> {
            lazy_static! {
                static ref PAUSE_RE: Regex = Regex::new("(?i)\\bonpause\\b").unwrap();
            }
            if PAUSE_RE.is_match(block) {
                Some(LogItem {
                    time: String::new(),
                    content: "DYEH PAUSE".to_string(),
                })
            } else {
                None
            }
        }
    }

    lazy_static! {
        pub static ref MATCHERS: Vec<Box<dyn EventMatcher>> = vec![Box::new(PauseMatcher)];
    }

    pub fn detect_specials(block: &str) -> Vec<LogItem> {
        MATCHERS.iter().filter_map(|m| m.try_match(block)).collect()
    }
}
use special_events::detect_specials;

/* ────────── helpers ────────── */
fn strip_first_header(delta: &str) -> &str {
    HEADER_RE
        .find(delta)
        .map(|m| &delta[m.end()..])
        .unwrap_or(delta)
}

fn strip_inline_headers(s: &str) -> String {
    INLINE_HEADER_RE.replace_all(s, "").into_owned()
}

fn parse_structured(entry: &str) -> Option<LogItem> {
    LOG_ITEM_PARSE_RE.captures(entry).map(|caps| LogItem {
        time: caps.get(1).map_or("", |m| m.as_str()).to_string(),
        content: caps.get(2).map_or("", |m| m.as_str()).trim().to_string(),
    })
}

/* ────────── public API ────────── */
pub fn process_delta(delta: &str) -> Vec<LogItem> {
    // 1. remove the *leading* header, if present …
    let body = strip_first_header(delta).trim();
    if body.is_empty() {
        return Vec::new();
    }

    // 2. … then kill every inline “[YYYY-MM-DD hh:mm:ss.mmm] [info]” string
    let body = strip_inline_headers(body);

    // 3. split into regular “## …” blocks
    let starts: Vec<usize> = LOG_ITEM_SEPARATOR_RE
        .find_iter(&body)
        .map(|m| m.start())
        .collect();

    let mut items = Vec::new();
    if !starts.is_empty() {
        let len_total = body.len();
        for (s, e) in starts
            .iter()
            .zip(starts.iter().skip(1).chain(std::iter::once(&len_total)))
        {
            if let Some(item) = parse_structured(&body[*s..*e]) {
                items.push(item);
            }
        }
    }

    // 4. ask the special-event matchers
    items.extend(detect_specials(&body));
    items
}
