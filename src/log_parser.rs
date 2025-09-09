use lazy_static::lazy_static;
use regex::Regex;

// Using lazy_static to compile regexes only once for performance.
lazy_static! {
    /// Matches and removes the optional header, e.g., `[2025-09-09 16:48:50.561] [info]`
    static ref HEADER_RE: Regex = Regex::new(r"^\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3}\] \[\w+\]\s*\n?").unwrap();

    /// Finds the starting pattern of each log item, e.g., `## 2025-09-09 16:54:44`
    static ref LOG_ITEM_SEPARATOR_RE: Regex = Regex::new(r"## \d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}").unwrap();

    /// Captures the time and content from a single log item string.
    /// The `(?s)` flag (dot all) allows `.` to match newline characters, capturing multi-line content.
    static ref LOG_ITEM_PARSE_RE: Regex = Regex::new(r"(?s)^## (\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2})\s*(.*)").unwrap();
}

/// # LogItem
/// Represents a single, structured log entry.
///
/// ## Fields
/// * `time`: The timestamp of the log entry (e.g., "2025-09-09 16:54:44").
/// * `content`: The full text of the log message, including any stack traces or other multi-line data.
#[derive(Debug, Clone)]
pub struct LogItem {
    pub time: String,
    pub content: String,
}

/// Strips the optional informational header from the beginning of a log delta.
/// If no header is found, it returns the original string slice.
fn strip_header(delta_str: &str) -> &str {
    if let Some(mat) = HEADER_RE.find(delta_str) {
        // Return the string slice that comes after the matched header.
        &delta_str[mat.end()..]
    } else {
        // No header was found, so return the original.
        delta_str
    }
}

/// Parses a string slice representing a single log item into a `LogItem` struct.
/// Returns `None` if the string does not match the expected format.
fn parse_log_item(entry_str: &str) -> Option<LogItem> {
    LOG_ITEM_PARSE_RE.captures(entry_str).map(|caps| {
        // Group 1: The timestamp
        let time = caps.get(1).map_or("", |m| m.as_str()).to_string();
        // Group 2: The rest of the content
        let content = caps.get(2).map_or("", |m| m.as_str()).trim().to_string();
        LogItem { time, content }
    })
}

/// ## process_delta
/// Processes a raw log delta string into a vector of `LogItem`s.
///
/// This function performs the following steps:
/// 1. Removes an optional leading header line (e.g., `[timestamp] [level]`).
/// 2. Splits the remaining content into chunks, where each chunk is a full log item starting with `## YYYY-MM-DD...`.
/// 3. Parses each chunk into a `LogItem` struct.
///
/// ### Arguments
/// * `delta_str`: A string slice (`&str`) containing the new chunk of log data.
///
/// ### Returns
/// A `Vec<LogItem>` containing all the log entries parsed from the delta.
pub fn process_delta(delta_str: &str) -> Vec<LogItem> {
    // Step 1: Strip the optional header from the raw delta.
    let content_to_parse = strip_header(delta_str).trim();

    if content_to_parse.is_empty() {
        return Vec::new();
    }

    // Step 2: Find the start indices of all log items to correctly split them.
    let start_indices: Vec<usize> = LOG_ITEM_SEPARATOR_RE
        .find_iter(content_to_parse)
        .map(|mat| mat.start())
        .collect();

    // If no log item separators are found, there's nothing to parse.
    if start_indices.is_empty() {
        return Vec::new();
    }

    // Create an iterator of (start, end) pairs for slicing the log string.
    // This correctly handles the last item by chaining the total length of the string.
    let len = content_to_parse.len();
    let slices = start_indices
        .iter()
        .zip(start_indices.iter().skip(1).chain(Some(&len)));

    // Step 3: Iterate over the slices, parse each one, and collect the results.
    slices
        .filter_map(|(&start, &end)| {
            let log_entry_str = &content_to_parse[start..end];
            parse_log_item(log_entry_str)
        })
        .collect()
}
