use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{
    DefaultTerminal,
    prelude::*,
    style::palette,
    symbols,
    widgets::{
        Block, Borders, HighlightSpacing, List, ListItem, ListState, Padding, Paragraph,
        StatefulWidget, Widget, Wrap,
    },
};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

mod log_parser {
    // A LogItem is the structured representation of a single log entry.
    #[derive(Debug, Clone)]
    pub struct LogItem {
        pub time: String,
        pub level: String,
        pub origin: String,
        pub tag: String,
        pub content: String,
    }

    // Parses a block of text (delta) containing multiple log entries.
    pub fn process_delta(delta: &str) -> Vec<LogItem> {
        let mut items = Vec::new();
        let mut current_item: Option<LogItem> = None;

        for line in delta.lines() {
            if line.starts_with("## ") {
                if let Some(item) = current_item.take() {
                    items.push(item);
                }
                current_item = Some(LogItem {
                    time: line.trim_start_matches("## ").to_string(),
                    level: String::new(),
                    origin: String::new(),
                    tag: String::new(),
                    content: String::new(),
                });
            } else if let Some(item) = &mut current_item {
                if item.level.is_empty() && line.contains("##") {
                    let parts: Vec<&str> = line.splitn(3, "##").collect();
                    if parts.len() == 3 {
                        let meta: Vec<&str> = parts[0]
                            .trim()
                            .trim_start_matches('[')
                            .trim_end_matches(']')
                            .splitn(2, ']')
                            .collect();
                        if meta.len() == 2 {
                            item.origin = meta[0].trim().to_string();
                            item.level = meta[1].trim().trim_start_matches('[').trim().to_string();
                        }
                        item.tag = parts[1].trim().to_string();
                        item.content = parts[2].trim().to_string();
                    }
                } else {
                    if !item.content.is_empty() {
                        item.content.push('\n');
                    }
                    item.content.push_str(line);
                }
            }
        }

        if let Some(item) = current_item {
            items.push(item);
        }

        items
    }
}
use crate::log_parser::LogItem;

// --- MODULE: File Finder (from monitoring script) ---
// This module is responsible for locating the most recent "live" log file in a directory.
mod file_finder {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    pub fn find_latest_live_log(log_dir: &Path) -> Result<PathBuf, String> {
        let entries = fs::read_dir(log_dir)
            .map_err(|e| format!("Failed to read directory '{}': {}", log_dir.display(), e))?;

        let mut live_log_files: Vec<PathBuf> = entries
            .filter_map(|entry_result| {
                entry_result.ok().and_then(|entry| {
                    let path = entry.path();
                    if !path.is_file() {
                        return None;
                    }

                    let file_name = path.file_name()?.to_str()?;
                    if !file_name.ends_with(".log") {
                        return None;
                    }

                    let base_name = file_name.strip_suffix(".log").unwrap();
                    if let Some(last_dot_pos) = base_name.rfind('.') {
                        let suffix = &base_name[last_dot_pos + 1..];
                        if suffix.parse::<u32>().is_ok() {
                            return None; // Exclude rotated logs like `file.1.log`
                        }
                    }
                    Some(path)
                })
            })
            .collect();

        if live_log_files.is_empty() {
            return Err("No live log files found in the directory.".to_string());
        }

        live_log_files.sort();
        Ok(live_log_files.pop().unwrap())
    }
}

// --- MODULE: File Metadata (from monitoring script, now multi-platform) ---
// This module provides functions to get file metadata (like size and modification time).
mod metadata {
    use super::*;
    use std::{ffi::CString, path::Path};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TimeSpec {
        pub sec: i64,
        pub nsec: i64,
    }

    #[derive(Clone, Debug)]
    pub struct MetaSnap {
        pub len: u64,
        pub mtime: TimeSpec,
    }

    #[cfg(target_os = "macos")]
    pub fn stat_path(path: &Path) -> io::Result<MetaSnap> {
        use libc::{stat as stat_t, stat};
        use std::mem;

        let cpath = CString::new(path.to_str().unwrap())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let mut st: stat_t = unsafe { mem::zeroed() };
        if unsafe { stat(cpath.as_ptr(), &mut st) } != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(MetaSnap {
            len: st.st_size as u64,
            mtime: TimeSpec {
                sec: st.st_mtime as i64,
                nsec: st.st_mtime_nsec as i64,
            },
        })
    }

    pub fn has_changed(prev: &Option<MetaSnap>, cur: &MetaSnap) -> bool {
        match prev {
            None => true,
            Some(p) => p.len != cur.len || p.mtime != cur.mtime,
        }
    }
}

// --- MODULE: Log Processor (from monitoring script) ---
// This module reads the new content from a file using memory-mapping.
mod log_processor {
    use super::*;
    use crate::log_parser::process_delta;
    use memmap2::MmapOptions;
    use std::fs::File;

    pub fn map_and_process_delta(
        file_path: &Path,
        prev_len: u64,
        cur_len: u64,
    ) -> io::Result<Vec<LogItem>> {
        let file = File::open(file_path)?;
        let mmap = unsafe { MmapOptions::new().len(cur_len as usize).map(&file)? };

        let start = (prev_len as usize).min(mmap.len());
        let end = (cur_len as usize).min(mmap.len());
        let delta_bytes = &mmap[start..end];

        if delta_bytes.is_empty() {
            return Ok(Vec::new());
        }

        let delta_str = String::from_utf8_lossy(delta_bytes);
        let log_items = process_delta(&delta_str);

        Ok(log_items)
    }
}

// --- TUI Styling Constants ---
const LOG_HEADER_STYLE: Style = Style::new()
    .fg(palette::tailwind::SLATE.c100)
    .bg(palette::tailwind::BLUE.c800);
const NORMAL_ROW_BG: Color = palette::tailwind::SLATE.c950;
const ALT_ROW_BG_COLOR: Color = palette::tailwind::SLATE.c900;
const SELECTED_STYLE: Style = Style::new()
    .bg(palette::tailwind::SLATE.c800)
    .add_modifier(Modifier::BOLD);
const TEXT_FG_COLOR: Color = palette::tailwind::SLATE.c200;
const INFO_STYLE: Style = Style::new().fg(palette::tailwind::SKY.c400);
const WARN_STYLE: Style = Style::new().fg(palette::tailwind::YELLOW.c400);
const ERROR_STYLE: Style = Style::new().fg(palette::tailwind::RED.c400);
const DEBUG_STYLE: Style = Style::new().fg(palette::tailwind::GREEN.c400);

// --- Main Application Entry Point ---
fn main() -> Result<()> {
    color_eyre::install()?;

    // -- ADDITION: Find the log file to monitor before starting the TUI.
    let log_dir_path = match dirs::home_dir() {
        Some(path) => path.join("Library/Application Support/DouyinAR/Logs/previewLog"),
        None => {
            eprintln!("Error: Could not determine the home directory.");
            std::process::exit(1);
        }
    };

    println!("🔍 Monitoring directory: {}", log_dir_path.display());
    let latest_file_path = match file_finder::find_latest_live_log(&log_dir_path) {
        Ok(path) => {
            println!("✅ Found log file to monitor: {}", path.display());
            path
        }
        Err(e) => {
            eprintln!("❌ Error: {}", e);
            eprintln!("Please ensure the directory exists and contains log files.");
            std::process::exit(1);
        }
    };
    // A brief pause to allow the user to see the startup messages.
    std::thread::sleep(Duration::from_secs(2));

    let terminal = ratatui::init();
    // -- REFACTOR: Create app with the path to the log file.
    let app_result = App::new(latest_file_path).run(terminal);
    ratatui::restore();
    app_result
}

// -- REFACTOR: App struct now holds state for file monitoring.
struct App {
    should_exit: bool,
    log_list: LogList,
    log_file_path: PathBuf,
    last_len: u64,
    prev_meta: Option<metadata::MetaSnap>,
    autoscroll: bool,
}

struct LogList {
    items: Vec<LogItem>,
    state: ListState,
}

// -- REFACTOR: `App::new` constructor replaces `Default`.
impl App {
    fn new(log_file_path: PathBuf) -> Self {
        Self {
            should_exit: false,
            log_list: LogList::new(Vec::new()), // Start with an empty list
            log_file_path,
            last_len: 0,
            prev_meta: None,
            autoscroll: true, // Auto-scroll to new logs by default
        }
    }
}

impl LogList {
    fn new(items: Vec<LogItem>) -> Self {
        Self {
            items,
            state: ListState::default(),
        }
    }
}

impl App {
    // -- REFACTOR: The main loop now uses `event::poll` for non-blocking event handling.
    fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let poll_interval = Duration::from_millis(100);
        while !self.should_exit {
            // --- Step 1: Check for file updates ---
            self.update_logs()?;

            // --- Step 2: Draw the UI ---
            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;

            // --- Step 3: Handle input events ---
            if event::poll(poll_interval)? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
            }
        }
        Ok(())
    }

    // -- ADDITION: New method to check for and process log file changes.
    fn update_logs(&mut self) -> Result<()> {
        let current_meta = match metadata::stat_path(&self.log_file_path) {
            Ok(m) => m,
            Err(_) => return Ok(()), // Ignore errors if file is temporarily unavailable
        };

        if metadata::has_changed(&self.prev_meta, &current_meta) {
            if current_meta.len < self.last_len {
                // File was truncated, reset state
                self.log_list.items.clear();
                self.last_len = 0;
            }

            if current_meta.len > self.last_len {
                if let Ok(new_items) = log_processor::map_and_process_delta(
                    &self.log_file_path,
                    self.last_len,
                    current_meta.len,
                ) {
                    self.log_list.items.extend(new_items);
                    if self.autoscroll {
                        self.select_last();
                    }
                }
                self.last_len = current_meta.len;
            }
            self.prev_meta = Some(current_meta);
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        // When a key is pressed, disable autoscroll so the user can navigate freely.
        if !matches!(key.code, KeyCode::Char('a' | 'g' | 'G')) {
            self.autoscroll = false;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_exit = true,
            KeyCode::Char('h') | KeyCode::Left => self.select_none(),
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_previous(),
            KeyCode::Char('g') => self.select_first(),
            KeyCode::Char('G') => self.select_last(),
            KeyCode::Char('a') => self.autoscroll = !self.autoscroll, // Toggle autoscroll
            _ => {}
        }
    }

    // --- Key handling methods for list navigation ---
    fn select_none(&mut self) {
        self.log_list.state.select(None);
    }
    fn select_next(&mut self) {
        self.log_list.state.select_next();
    }
    fn select_previous(&mut self) {
        self.log_list.state.select_previous();
    }
    fn select_first(&mut self) {
        self.log_list.state.select_first();
    }
    fn select_last(&mut self) {
        self.log_list.state.select_last();
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [header_area, main_area, footer_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .areas(area);

        let [list_area, item_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Fill(1)]).areas(main_area);

        self.render_header(header_area, buf);
        App::render_footer(footer_area, buf);
        self.render_list(list_area, buf);
        self.render_selected_item(item_area, buf);
    }
}

/// Rendering logic for the app
impl App {
    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        let autoscroll_status = if self.autoscroll { " ON" } else { " OFF" };
        let title = format!(
            "Ratatui Live Log Viewer (Autoscroll: {})",
            autoscroll_status
        );
        Paragraph::new(title).bold().centered().render(area, buf);
    }

    fn render_footer(area: Rect, buf: &mut Buffer) {
        Paragraph::new("↓↑: move | ←: unselect | g/G: top/bottom | a: autoscroll | q: quit")
            .centered()
            .render(area, buf);
    }

    fn render_list(&mut self, area: Rect, buf: &mut Buffer) {
        let block = Block::new()
            .title(Line::raw("Logs").centered())
            .borders(Borders::TOP)
            .border_set(symbols::border::EMPTY)
            .border_style(LOG_HEADER_STYLE)
            .bg(NORMAL_ROW_BG);

        let items: Vec<ListItem> = self
            .log_list
            .items
            .iter()
            .enumerate()
            .map(|(i, log_item)| {
                let color = alternate_colors(i);
                ListItem::from(log_item).bg(color)
            })
            .collect();

        let list_widget = List::new(items)
            .block(block)
            .highlight_style(SELECTED_STYLE)
            .highlight_symbol(">> ")
            .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(list_widget, area, buf, &mut self.log_list.state);
    }

    fn render_selected_item(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::new()
            .title(Line::raw("Log Details").centered())
            .borders(Borders::TOP)
            .border_set(symbols::border::EMPTY)
            .border_style(LOG_HEADER_STYLE)
            .bg(NORMAL_ROW_BG)
            .padding(Padding::horizontal(1));

        let content = if let Some(i) = self.log_list.state.selected() {
            let item = &self.log_list.items[i];
            vec![
                Line::from(vec!["Time:   ".bold(), item.time.clone().into()]),
                Line::from(vec!["Level:  ".bold(), item.level.clone().into()]),
                Line::from(vec!["Origin: ".bold(), item.origin.clone().into()]),
                Line::from(vec!["Tag:    ".bold(), item.tag.clone().into()]),
                Line::from(""),
                Line::from("Content:".bold()),
                Line::from(item.content.clone()),
            ]
        } else {
            vec![Line::from("Select a log item to see details...".italic())]
        };

        Paragraph::new(content)
            .block(block)
            .fg(TEXT_FG_COLOR)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}

const fn alternate_colors(i: usize) -> Color {
    if i % 2 == 0 {
        NORMAL_ROW_BG
    } else {
        ALT_ROW_BG_COLOR
    }
}

impl From<&LogItem> for ListItem<'_> {
    fn from(item: &LogItem) -> Self {
        let level_style = match item.level.as_str() {
            "ERROR" => ERROR_STYLE,
            "WARN" => WARN_STYLE,
            "INFO" => INFO_STYLE,
            "DEBUG" => DEBUG_STYLE,
            _ => Style::default().fg(TEXT_FG_COLOR),
        };

        let first_line = item.content.lines().next().unwrap_or("");
        let summary_text = format!("[{}] [{}] {}", item.level, item.origin, first_line);

        ListItem::new(Line::styled(summary_text, level_style))
    }
}
