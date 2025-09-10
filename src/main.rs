mod file_finder;
mod log_parser;
mod metadata;

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

use crate::log_parser::LogItem;

mod log_processor {
    use super::*;
    use crate::log_parser::{LogItem, process_delta};
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
            KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => self.should_exit = true,
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
        Paragraph::new("↓↑: move | ←: unselect | g/G: top/bottom | a: autoscroll | q/Ctrl-C: quit")
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
