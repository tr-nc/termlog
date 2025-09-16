use crate::{
    file_finder,
    log_list::LogList,
    log_parser::{LogItem, process_delta},
    metadata,
};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, MouseEvent};
use log::{Log, Metadata, Record};
use memmap2::MmapOptions;
use ratatui::{
    prelude::*,
    style::palette,
    symbols::{self, scrollbar},
    widgets::{
        Block, Borders, HighlightSpacing, List, ListItem, Padding, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget, Wrap,
    },
};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

// colors
const NORMAL_ROW_BG_COLOR: Color = palette::tailwind::ZINC.c950;
const ALT_ROW_BG_COLOR: Color = palette::tailwind::ZINC.c900;
const TEXT_FG_COLOR: Color = palette::tailwind::ZINC.c200;

// styles
const LOG_HEADER_STYLE: Style = Style::new()
    .fg(palette::tailwind::ZINC.c100)
    .bg(palette::tailwind::ZINC.c400);
const SELECTED_STYLE: Style = Style::new()
    .bg(palette::tailwind::ZINC.c700)
    .add_modifier(Modifier::BOLD);
const INFO_STYLE: Style = Style::new().fg(palette::tailwind::SKY.c400);
const WARN_STYLE: Style = Style::new().fg(palette::tailwind::YELLOW.c400);
const ERROR_STYLE: Style = Style::new().fg(palette::tailwind::RED.c400);
const DEBUG_STYLE: Style = Style::new().fg(palette::tailwind::GREEN.c400);

// Custom logger that writes to a buffer for display in UI
struct UiLogger {
    logs: Arc<Mutex<Vec<String>>>,
}

impl UiLogger {
    fn new(logs: Arc<Mutex<Vec<String>>>) -> Self {
        Self { logs }
    }
}

impl Log for UiLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let log_entry = format!("[{}] {}", record.level(), record.args());
            if let Ok(mut logs) = self.logs.lock() {
                logs.push(log_entry);
                // Keep only the last 50 entries to prevent memory bloat
                if logs.len() > 50 {
                    logs.remove(0);
                }
            }
        }
    }

    fn flush(&self) {}
}

pub fn start() -> Result<()> {
    color_eyre::install().or(Err(anyhow::anyhow!("Error installing color_eyre")))?;

    let log_dir_path = match dirs::home_dir() {
        Some(path) => path.join("Library/Application Support/DouyinAR/Logs/previewLog"),
        None => {
            return Err(anyhow::anyhow!("Error getting home directory"));
        }
    };

    let latest_file_path = match file_finder::find_latest_live_log(&log_dir_path) {
        Ok(path) => path,
        Err(e) => return Err(anyhow::anyhow!("Error finding latest log file: {}", e)),
    };

    App::new(latest_file_path).run()
}

struct App {
    should_exit: bool,
    log_list: LogList,
    filtered_log_list: Option<LogList>, // For filtered results
    log_file_path: PathBuf,
    last_len: u64,
    prev_meta: Option<metadata::MetaSnap>,
    autoscroll: bool,
    filter_mode: bool,                   // Whether we're in filter input mode
    filter_input: String,                // Current filter input text
    scrollbar_state: ScrollbarState,     // For the logs panel scrollbar
    detail_level: u8,                    // Detail level for log display (0-4, default 1)
    debug_logs: Arc<Mutex<Vec<String>>>, // Debug log messages for UI display
}

impl App {
    fn new(log_file_path: PathBuf) -> Self {
        // Set up logging
        let debug_logs = Arc::new(Mutex::new(Vec::new()));
        let logger = Box::new(UiLogger::new(debug_logs.clone()));

        // Try to set up the logger
        match log::set_logger(Box::leak(logger)) {
            Ok(_) => {
                log::set_max_level(log::LevelFilter::Debug);
                log::debug!("Debug logging initialized");
            }
            Err(_) => {
                // Logger might already be set, that's okay
            }
        }

        Self {
            should_exit: false,
            log_list: LogList::new(Vec::new()),
            filtered_log_list: None,
            log_file_path,
            last_len: 0,
            prev_meta: None,
            autoscroll: true,
            filter_mode: false,
            filter_input: String::new(),
            scrollbar_state: ScrollbarState::default(),
            detail_level: 1, // Default detail level (time content)
            debug_logs,
        }
    }

    fn run(mut self) -> Result<()> {
        log::info!("Starting dhlog application");
        log::debug!("Debug logging enabled");
        let mut terminal = ratatui::init();

        let poll_interval = Duration::from_millis(100);
        while !self.should_exit {
            self.update_logs()?;

            terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;

            if event::poll(poll_interval)? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key),
                    Event::Mouse(mouse) => self.handle_mouse(mouse),
                    _ => return Ok(()),
                }
            }
        }

        ratatui::restore();
        Ok(())
    }

    fn update_logs(&mut self) -> Result<()> {
        let current_meta = match metadata::stat_path(&self.log_file_path) {
            Ok(m) => m,
            Err(_) => {
                log::debug!("Failed to stat log file path");
                return Ok(());
            }
        };

        if metadata::has_changed(&self.prev_meta, &current_meta) {
            // TODO: check if this branch works properly, it's pretty rare to happen, but it does
            if current_meta.len < self.last_len {
                // file was truncated, reset state
                self.log_list.items.clear();
                self.last_len = 0;
            }

            if current_meta.len > self.last_len {
                log::debug!(
                    "Processing new log data from {} to {}",
                    self.last_len,
                    current_meta.len
                );
                if let Ok(new_items) =
                    map_and_process_delta(&self.log_file_path, self.last_len, current_meta.len)
                {
                    log::debug!("Found {} new log items", new_items.len());
                    self.log_list.items.extend(new_items);
                    if self.autoscroll {
                        self.log_list.state.select_last();
                    }
                }
                self.last_len = current_meta.len;
            }

            self.prev_meta = Some(current_meta);
        }
        return Ok(());

        fn map_and_process_delta(
            file_path: &Path,
            prev_len: u64,
            cur_len: u64,
        ) -> Result<Vec<LogItem>> {
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

    fn apply_filter(&mut self) {
        if self.filter_input.is_empty() {
            self.filtered_log_list = None;
        } else {
            let filtered_items: Vec<LogItem> = self
                .log_list
                .items
                .iter()
                .filter(|item| item.contains(&self.filter_input))
                .cloned()
                .collect();

            let mut filtered_log_list = LogList::new(filtered_items);
            // Select the last item to match the initial program behavior
            filtered_log_list.state.select_last();

            self.filtered_log_list = Some(filtered_log_list);
            self.update_scrollbar_state();
        }
    }

    fn exit_filter_mode(&mut self) {
        self.filter_mode = false;
        self.filter_input.clear();
        self.filtered_log_list = None;
    }

    fn update_scrollbar_state(&mut self) {
        let (items, selected_index) = if let Some(ref filtered) = self.filtered_log_list {
            (&filtered.items, filtered.state.selected())
        } else {
            (&self.log_list.items, self.log_list.state.selected())
        };

        let total_items = items.len();
        if total_items > 0 {
            let position = selected_index.unwrap_or(0);
            self.scrollbar_state = self
                .scrollbar_state
                .content_length(total_items)
                .position(position);
        } else {
            self.scrollbar_state = self.scrollbar_state.content_length(0).position(0);
        }
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        let autoscroll_status = if self.autoscroll { " ON" } else { " OFF" };
        let title = format!(
            "Ratatui Live Log Viewer (Autoscroll: {})",
            autoscroll_status
        );
        Paragraph::new(title).bold().centered().render(area, buf);
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        let help_text = if self.filter_mode {
            format!(
                "Filter: {} (Press Enter to apply, Esc to cancel)",
                self.filter_input
            )
        } else {
            "jk↑↓: nav | gG: top/bottom | f/: filter | a: autoscroll | []: detail | c: clear history | q: quit"
                .to_string()
        };
        Paragraph::new(help_text).centered().render(area, buf);
    }

    fn render_list(&mut self, area: Rect, buf: &mut Buffer) {
        // Update scrollbar state based on current selection
        self.update_scrollbar_state();

        // Create a horizontal layout: main list area + scrollbar area
        let [list_area, scrollbar_area] = Layout::horizontal([
            Constraint::Fill(1),   // Main list takes most space
            Constraint::Length(1), // Scrollbar is 1 character wide
        ])
        .margin(0)
        .areas(area);

        // Render the main list block with title
        let block = Block::new()
            .title(Line::raw("LOGS").centered())
            .borders(Borders::TOP)
            .border_set(symbols::border::EMPTY)
            .border_style(LOG_HEADER_STYLE)
            .bg(NORMAL_ROW_BG_COLOR);

        // Use filtered list if available, otherwise use the full list
        let (items_to_render, state_to_use) = if let Some(ref mut filtered) = self.filtered_log_list
        {
            (&filtered.items, &mut filtered.state)
        } else {
            (&self.log_list.items, &mut self.log_list.state)
        };

        let items: Vec<ListItem> = items_to_render
            .iter()
            .enumerate()
            .map(|(i, log_item)| {
                let color = alternate_colors(i);
                let detail_text = log_item.format_detail(self.detail_level);
                let level_style = match log_item.level.as_str() {
                    "ERROR" => ERROR_STYLE,
                    "WARN" => WARN_STYLE,
                    "INFO" => INFO_STYLE,
                    "DEBUG" => DEBUG_STYLE,
                    _ => Style::default().fg(TEXT_FG_COLOR),
                };
                ListItem::new(Line::styled(detail_text, level_style)).bg(color)
            })
            .collect();

        let list_widget = List::new(items)
            .block(block)
            .scroll_padding(1)
            .highlight_style(SELECTED_STYLE)
            .highlight_symbol(">")
            .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(list_widget, list_area, buf, state_to_use);

        // Render the scrollbar
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .symbols(scrollbar::VERTICAL)
            .style(Style::default().fg(palette::tailwind::ZINC.c500))
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"));

        StatefulWidget::render(scrollbar, scrollbar_area, buf, &mut self.scrollbar_state);

        fn alternate_colors(i: usize) -> Color {
            if i % 2 == 0 {
                NORMAL_ROW_BG_COLOR
            } else {
                ALT_ROW_BG_COLOR
            }
        }
    }

    fn render_selected_item(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::new()
            .title(Line::raw("LOG DETAILS").centered())
            .borders(Borders::TOP)
            .border_set(symbols::border::EMPTY)
            .border_style(LOG_HEADER_STYLE)
            .bg(NORMAL_ROW_BG_COLOR)
            .padding(Padding::horizontal(1));

        // Use filtered list if available, otherwise use the full list
        let (items, state) = if let Some(ref filtered) = self.filtered_log_list {
            (&filtered.items, &filtered.state)
        } else {
            (&self.log_list.items, &self.log_list.state)
        };

        let content = if let Some(i) = state.selected() {
            let item = &items[i];
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

    fn render_debug_logs(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::new()
            .title(Line::raw("DEBUG LOGS").centered())
            .borders(Borders::TOP)
            .border_set(symbols::border::EMPTY)
            .border_style(LOG_HEADER_STYLE)
            .bg(NORMAL_ROW_BG_COLOR);

        let debug_logs = if let Ok(logs) = self.debug_logs.lock() {
            if logs.is_empty() {
                vec![Line::from("No debug logs...".italic())]
            } else {
                logs.iter()
                    .rev() // Show most recent first
                    .take(5) // Show only last 5 entries
                    .map(|log_entry| {
                        let style = if log_entry.contains("ERROR") {
                            ERROR_STYLE
                        } else if log_entry.contains("WARN") {
                            WARN_STYLE
                        } else if log_entry.contains("DEBUG") {
                            DEBUG_STYLE
                        } else {
                            Style::default().fg(TEXT_FG_COLOR)
                        };
                        Line::styled(log_entry.clone(), style)
                    })
                    .collect()
            }
        } else {
            vec![Line::from("Failed to read debug logs...".italic())]
        };

        Paragraph::new(debug_logs)
            .block(block)
            .fg(TEXT_FG_COLOR)
            .render(area, buf);
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        // Handle mouse wheel scrolling with traditional navigation (no wrap)
        match mouse.kind {
            crossterm::event::MouseEventKind::ScrollDown => {
                let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                    filtered
                } else {
                    &mut self.log_list
                };
                target_list.select_next_traditional(); // Traditional navigation for mouse wheel
                self.update_scrollbar_state();
            }
            crossterm::event::MouseEventKind::ScrollUp => {
                let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                    filtered
                } else {
                    &mut self.log_list
                };
                target_list.select_previous_traditional(); // Traditional navigation for mouse wheel
                self.update_scrollbar_state();
            }
            _ => {
                // Other mouse events - could be implemented later for click-to-select
                // println!("Mouse event: {:?}", mouse);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        // Handle filter mode input
        if self.filter_mode {
            match key.code {
                KeyCode::Esc => {
                    self.exit_filter_mode();
                    return;
                }
                KeyCode::Enter => {
                    self.apply_filter();
                    self.filter_mode = false;
                    return;
                }
                KeyCode::Char(c) => {
                    self.filter_input.push(c);
                    return;
                }
                KeyCode::Backspace => {
                    self.filter_input.pop();
                    return;
                }
                _ => {}
            }
        }

        // When a key is pressed, disable autoscroll so the user can navigate freely.
        if !matches!(key.code, KeyCode::Char('a' | 'g' | 'G')) {
            self.autoscroll = false;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                log::debug!("Exit key pressed");
                self.should_exit = true;
            }
            KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                self.should_exit = true
            }
            KeyCode::Char('c') if !key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                self.log_list.items.clear();
                self.log_list.state.select(None);
                // Also clear filtered list if it exists
                self.filtered_log_list = None;
                self.filter_input.clear();
            }
            KeyCode::Char('h') | KeyCode::Left => {
                // Left arrow now scrolls left in expanded view
                // (no unselect functionality)
            }
            KeyCode::Char('j') => {
                let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                    filtered
                } else {
                    &mut self.log_list
                };
                target_list.select_next_circular(); // Circular navigation for j/k
                self.update_scrollbar_state();
            }
            KeyCode::Char('k') => {
                let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                    filtered
                } else {
                    &mut self.log_list
                };
                target_list.select_previous_circular(); // Circular navigation for j/k
                self.update_scrollbar_state();
            }
            KeyCode::Down => {
                let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                    filtered
                } else {
                    &mut self.log_list
                };
                target_list.select_next_traditional(); // Traditional navigation for arrow keys
                self.update_scrollbar_state();
            }
            KeyCode::Up => {
                let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                    filtered
                } else {
                    &mut self.log_list
                };
                target_list.select_previous_traditional(); // Traditional navigation for arrow keys
                self.update_scrollbar_state();
            }
            KeyCode::Char('g') => {
                let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                    filtered
                } else {
                    &mut self.log_list
                };
                target_list.state.select_first();
                self.update_scrollbar_state();
            }
            KeyCode::Char('G') => {
                let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                    filtered
                } else {
                    &mut self.log_list
                };
                target_list.state.select_last();
                self.update_scrollbar_state();
            }
            KeyCode::Char('a') => {
                self.autoscroll = !self.autoscroll; // Toggle autoscroll
                if self.autoscroll {
                    // When turning on autoscroll, instantly select the last item
                    let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                        filtered
                    } else {
                        &mut self.log_list
                    };
                    target_list.state.select_last();
                    self.update_scrollbar_state();
                }
            }
            KeyCode::Char('f') | KeyCode::Char('/') => {
                self.filter_mode = true;
                self.filter_input.clear();
            }
            KeyCode::Char('[') => {
                // Decrease detail level (show less info) - circular
                self.detail_level = if self.detail_level == 0 {
                    4
                } else {
                    self.detail_level - 1
                };
            }
            KeyCode::Char(']') => {
                // Increase detail level (show more info) - circular
                self.detail_level = if self.detail_level == 4 {
                    0
                } else {
                    self.detail_level + 1
                };
            }
            _ => {}
        }
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [header_area, main_area, debug_area, footer_area] = Layout::vertical([
            Constraint::Length(1), // Header
            Constraint::Fill(1),   // Main area (logs + details)
            Constraint::Length(7), // Debug logs block (5 lines + borders)
            Constraint::Length(1), // Footer
        ])
        .areas(area);

        let [list_area, item_area] =
            Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(main_area);

        self.render_header(header_area, buf);
        self.render_list(list_area, buf);
        self.render_selected_item(item_area, buf);
        self.render_debug_logs(debug_area, buf);
        self.render_footer(footer_area, buf);
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
