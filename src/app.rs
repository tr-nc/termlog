use crate::{
    app_block::AppBlock,
    content_line_maker::wrap_content_to_lines,
    file_finder,
    log_list::LogList,
    log_parser::{LogItem, process_delta},
    metadata, theme,
};
use anyhow::{Result, anyhow};
use arboard::Clipboard;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use log::{Log, Metadata, Record};
use memmap2::MmapOptions;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    prelude::*,
    widgets::{Padding, Paragraph, StatefulWidget, Widget},
};
use std::{
    collections::HashMap,
    fs::File,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

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
    color_eyre::install().or(Err(anyhow!("Error installing color_eyre")))?;

    let log_dir_path = match dirs::home_dir() {
        Some(path) => path.join("Library/Application Support/DouyinAR/Logs/previewLog"),
        None => {
            return Err(anyhow!("Error getting home directory"));
        }
    };

    let latest_file_path = match file_finder::find_latest_live_log(&log_dir_path) {
        Ok(path) => path,
        Err(e) => return Err(anyhow!("Error finding latest log file: {}", e)),
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
    filter_mode: bool,                        // Whether we're in filter input mode
    filter_input: String,                     // Current filter input text
    detail_level: u8,                         // Detail level for log display (0-4, default 1)
    debug_logs: Arc<Mutex<Vec<String>>>,      // Debug log messages for UI display
    focused_block_id: Option<uuid::Uuid>,     // Currently focused block ID
    blocks: HashMap<String, AppBlock>, // Named blocks with persistent IDs (logs, details, debug)
    prev_selected_log_id: Option<uuid::Uuid>, // Track previous selected log item ID for details reset

    event: Option<MouseEvent>,
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
            detail_level: 1, // Default detail level (time content)
            debug_logs,
            focused_block_id: None,     // No block focused initially
            blocks: HashMap::new(),     // Initialize empty blocks map
            prev_selected_log_id: None, // No previous selection initially

            event: None,
        }
    }

    fn run(mut self) -> Result<()> {
        // Enhanced terminal setup with explicit cleanup
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

        let poll_interval = Duration::from_millis(100);

        // Use a guard to ensure cleanup happens
        struct TerminalGuard;
        impl Drop for TerminalGuard {
            fn drop(&mut self) {
                let _ = disable_raw_mode();
                let _ = execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
            }
        }
        let _guard = TerminalGuard;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
            while !self.should_exit {
                self.poll_event(poll_interval)?;
                self.update_logs()?;

                terminal.draw(|frame| frame.render_widget(&mut self, frame.area()))?;
            }
            Ok(())
        }));

        // Explicit cleanup
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;

        match result {
            Ok(r) => r,
            Err(_) => {
                eprintln!("Application panicked, terminal restored");
                std::process::exit(1);
            }
        }
    }

    fn poll_event(&mut self, poll_interval: Duration) -> Result<()> {
        if event::poll(poll_interval)? {
            let event = event::read()?;
            match event {
                Event::Key(key) => self.handle_key(key),
                Event::Mouse(mouse) => {
                    match mouse.kind {
                        MouseEventKind::ScrollDown => {
                            if self.is_log_block_focused()? {
                                self.autoscroll = false; // Disable autoscroll on manual scroll
                                self.handle_logs_view_scrolling(true);
                            }
                            if self.is_details_block_focused()? {
                                self.handle_details_block_scrolling(true);
                            }
                            if self.is_debug_block_focused()? {
                                self.handle_debug_logs_scrolling(true);
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if self.is_log_block_focused()? {
                                self.autoscroll = false; // Disable autoscroll on manual scroll
                                self.handle_logs_view_scrolling(false);
                            }
                            if self.is_details_block_focused()? {
                                self.handle_details_block_scrolling(false);
                            }
                            if self.is_debug_block_focused()? {
                                self.handle_debug_logs_scrolling(false);
                            }
                        }
                        MouseEventKind::Moved => {
                            // Mouse moved - the render methods will handle hover focus
                            // Just store the event so blocks can check if mouse is hovering
                        }
                        _ => {}
                    }
                    self.event = Some(mouse);
                }
                Event::Resize(width, height) => {
                    // Terminal was resized, ratatui will handle the layout automatically
                    log::debug!("Terminal resized to {}x{}", width, height);
                }
                _ => {}
            }
        }

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
            // later: check if this branch works properly, it's pretty rare to happen, but it does
            if current_meta.len < self.last_len {
                // file was truncated, reset state
                self.log_list.items.clear();
                self.last_len = 0;
            }

            if current_meta.len > self.last_len {
                if let Ok(new_items) =
                    map_and_process_delta(&self.log_file_path, self.last_len, current_meta.len)
                {
                    log::debug!("Found {} new log items", new_items.len());
                    self.log_list.items.extend(new_items);
                    if self.autoscroll {
                        self.log_list.state.select_first();
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
            // Select the first item to match the reversed program behavior (newest at top)
            filtered_log_list.state.select_first();

            self.filtered_log_list = Some(filtered_log_list);
            self.update_logs_scrollbar_state();
        }
    }

    fn exit_filter_mode(&mut self) {
        self.filter_mode = false;
        self.filter_input.clear();
        self.filtered_log_list = None;
    }

    fn update_logs_scrollbar_state(&mut self) {
        // if we have filtered list, use it directly, otherwise, use the default log list
        let items = if let Some(ref filtered) = self.filtered_log_list {
            &filtered.items
        } else {
            &self.log_list.items
        };

        // Update the logs block scrollbar state
        if let Some(logs_block) = self.blocks.get_mut("logs") {
            if items.len() > 0 {
                // Don't automatically sync scroll position with selection
                // Keep current scroll position and just update scrollbar state
                let current_scroll_pos = logs_block.get_scroll_position();
                logs_block.update_scrollbar_state(items.len(), Some(current_scroll_pos));
            } else {
                logs_block.set_scroll_position(0);
                logs_block.update_scrollbar_state(0, Some(0));
            }
        }
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) -> Result<()> {
        let autoscroll_status = if self.autoscroll { " ON" } else { " OFF" };
        let title = format!("Termlog | Autoscroll: {}", autoscroll_status);
        Paragraph::new(title).bold().centered().render(area, buf);
        Ok(())
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) -> Result<()> {
        let help_text = if self.filter_mode {
            format!(
                "Filter: {} (Press Enter to apply, Esc to cancel)",
                self.filter_input
            )
        } else {
            "jk↑↓: nav | gG: top/bottom | f/: filter | a: autoscroll | []: detail | y: yank | JK: scroll focused | c: clear history | q: quit"
                .to_string()
        };
        Paragraph::new(help_text).centered().render(area, buf);
        Ok(())
    }

    fn render_logs(&mut self, area: Rect, buf: &mut Buffer) -> Result<()> {
        // Update scrollbar state based on current selection
        self.update_logs_scrollbar_state();

        // Create a horizontal layout: main content area + scrollbar area
        let [content_area, scrollbar_area] = Layout::horizontal([
            Constraint::Fill(1),   // Main content takes most space
            Constraint::Length(1), // Scrollbar is 1 character wide
        ])
        .margin(0)
        .areas(area);

        // Initialize blocks if not already done
        if self.blocks.is_empty() {
            self.initialize_blocks();
        }

        // Check focus status before mutable borrow
        let is_log_focused = self.is_log_block_focused()?;

        // Get the LOGS block from storage and update its title
        let (logs_block_id, should_focus, clicked_row) = if let Some(logs_block) =
            self.blocks.get_mut("logs")
        {
            // Update the title with current detail level (preserving the same block ID)
            logs_block.update_title(format!("LOGS | Detail Level: {}", self.detail_level));

            let logs_block_id = logs_block.id();

            // Handle click and set focus, also check for click position
            let (should_focus, clicked_row) = if let Some(event) = self.event {
                let was_clicked =
                    logs_block.handle_mouse_event(&event, content_area, self.event.as_ref());
                // Check if this is a left click event, regardless of was_clicked (which is mainly for focus)
                let is_left_click = event.kind
                    == crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left);

                // For click processing, we need to check if the click is within the logs block area
                let inner_area = logs_block.build(false).inner(content_area);
                let is_within_bounds =
                    inner_area.contains(ratatui::layout::Position::new(event.column, event.row));

                let click_row = if is_left_click && is_within_bounds {
                    Some(event.row)
                } else {
                    None
                };
                (was_clicked, click_row)
            } else {
                (false, None)
            };

            (logs_block_id, should_focus, clicked_row)
        } else {
            return Err(anyhow!("No logs block available"));
        };

        if should_focus {
            self.set_focused_block(logs_block_id);
        }

        // Use filtered list if available, otherwise use the full list
        let (items_to_render, state_to_use) = if let Some(ref filtered) = self.filtered_log_list {
            (&filtered.items, &filtered.state)
        } else {
            (&self.log_list.items, &self.log_list.state)
        };

        // Convert log items to lines with highlighting for selected item
        let mut content_lines = Vec::new();
        let selected_index = state_to_use.selected();

        for (index, log_item) in items_to_render.iter().rev().enumerate() {
            let detail_text = log_item.format_detail(self.detail_level);
            let level_style = match log_item.level.as_str() {
                "ERROR" => theme::ERROR_STYLE,
                "WARN" => theme::WARN_STYLE,
                "INFO" => theme::INFO_STYLE,
                "DEBUG" => theme::DEBUG_STYLE,
                _ => Style::default().fg(theme::TEXT_FG_COLOR),
            };

            // Apply selection highlighting if this is the selected item
            let final_style = if let Some(sel_idx) = selected_index {
                if index == sel_idx {
                    level_style.patch(theme::SELECTED_STYLE)
                } else {
                    level_style
                }
            } else {
                level_style
            };

            // Add selection indicator for selected item
            let display_text = if let Some(sel_idx) = selected_index
                && index == sel_idx
            {
                format!("> {}", detail_text)
            } else {
                format!("  {}", detail_text)
            };

            content_lines.push(Line::styled(display_text, final_style));
        }

        // Handle click on LOGS block to calculate exact log item number
        if let Some(click_row) = clicked_row {
            // Get the inner area for the logs block to calculate relative position
            if let Some(logs_block) = self.blocks.get("logs") {
                let inner_area = logs_block.build(false).inner(content_area);
                let relative_row = click_row.saturating_sub(inner_area.y);

                // Get current scroll position from the logs block
                let scroll_position = if let Some(logs_block) = self.blocks.get("logs") {
                    logs_block.get_scroll_position()
                } else {
                    0
                };

                // Calculate the exact log item number
                // The formula: exact_item = scroll_position + relative_row
                let exact_item_number = scroll_position + relative_row as usize;

                // Ensure the calculated item number is within bounds
                if exact_item_number < items_to_render.len() {
                    // Select the corresponding log item
                    if let Some(ref mut filtered) = self.filtered_log_list {
                        filtered.state.select(Some(exact_item_number));
                    } else {
                        self.log_list.state.select(Some(exact_item_number));
                    }
                    // log::debug!("Selected log item #{}", exact_item_number);
                } else {
                    return Err(anyhow!("Click outside valid item range"));
                }
            }
        }

        // Update the logs block with lines count and scrollbar state
        let scroll_position = if let Some(logs_block) = self.blocks.get_mut("logs") {
            logs_block.set_lines_count(content_lines.len());
            let current_pos = logs_block.get_scroll_position();
            logs_block.update_scrollbar_state(content_lines.len(), Some(current_pos));
            current_pos
        } else {
            0
        };

        // Build the block after mutable operations
        let block = if let Some(logs_block) = self.blocks.get("logs") {
            logs_block.build(is_log_focused)
        } else {
            return Err(anyhow!("No logs block available"));
        };

        // Render using Paragraph widget like the other blocks
        Paragraph::new(content_lines)
            .block(block)
            .fg(theme::TEXT_FG_COLOR)
            .scroll((scroll_position as u16, 0))
            .render(content_area, buf);

        let scrollbar = AppBlock::create_scrollbar(is_log_focused);

        // Use AppBlock's scrollbar state for logs
        if let Some(logs_block) = self.blocks.get_mut("logs") {
            StatefulWidget::render(
                scrollbar,
                scrollbar_area,
                buf,
                logs_block.get_scrollbar_state(),
            );
        }
        Ok(())
    }

    fn render_details(&mut self, area: Rect, buf: &mut Buffer) -> Result<()> {
        // Initialize blocks if not already done
        if self.blocks.is_empty() {
            self.initialize_blocks();
        }

        // Get the DETAILS block ID and check if focused
        let (details_block_id, is_focused, should_focus) =
            if let Some(details_block) = self.blocks.get_mut("details") {
                let details_block_id = details_block.id();
                let is_focused = self.focused_block_id == Some(details_block_id);

                // Handle click and set focus
                let should_focus = if let Some(event) = self.event {
                    details_block.handle_mouse_event(&event, area, self.event.as_ref())
                } else {
                    false
                };

                (details_block_id, is_focused, should_focus)
            } else {
                return Err(anyhow!("No details block available"));
            };

        if should_focus {
            self.set_focused_block(details_block_id);
        }

        // Create a horizontal layout: main content area + scrollbar area
        let [content_area, scrollbar_area] = Layout::horizontal([
            Constraint::Fill(1),   // Main content takes most space
            Constraint::Length(1), // Scrollbar is 1 character wide
        ])
        .margin(0)
        .areas(area);

        // Use filtered list if available, otherwise use the full list
        let (items, state) = if let Some(ref filtered) = self.filtered_log_list {
            (&filtered.items, &filtered.state)
        } else {
            (&self.log_list.items, &self.log_list.state)
        };

        let content = if let Some(i) = state.selected() {
            // Access items in reverse order to match the LOGS panel display order
            let reversed_index = items.len().saturating_sub(1).saturating_sub(i);
            let item = &items[reversed_index];

            // Check if the selected log item has changed and reset scroll position if needed
            if self.prev_selected_log_id != Some(item.id) {
                self.prev_selected_log_id = Some(item.id);
                if let Some(details_block) = self.blocks.get_mut("details") {
                    details_block.set_scroll_position(0);
                }
            }

            let mut content_lines = vec![
                Line::from(vec!["Time:   ".bold(), item.time.clone().into()]),
                Line::from(vec!["Level:  ".bold(), item.level.clone().into()]),
                Line::from(vec!["Origin: ".bold(), item.origin.clone().into()]),
                Line::from(vec!["Tag:    ".bold(), item.tag.clone().into()]),
                Line::from("Content:".bold()),
            ];
            // Get the actual content rect accounting for borders
            let content_rect = if let Some(details_block) = self.blocks.get("details") {
                let inner_rect = details_block.get_content_rect(content_area, is_focused);
                inner_rect
            } else {
                content_area
            };
            content_lines.extend(wrap_content_to_lines(&item.content, content_rect.width));
            content_lines
        } else {
            // No log item selected - clear the previous selection tracking
            if self.prev_selected_log_id.is_some() {
                self.prev_selected_log_id = None;
                if let Some(details_block) = self.blocks.get_mut("details") {
                    details_block.set_scroll_position(0);
                    log::debug!("No log item selected - resetting details scroll position");
                }
            }
            vec![Line::from("Select a log item to see details...".italic())]
        };

        // The content vector already contains properly wrapped lines
        let lines_count = content.len();

        // Update the details block with lines count and scrollbar state
        let scroll_position = if let Some(details_block) = self.blocks.get_mut("details") {
            details_block.set_lines_count(lines_count);
            let current_pos = details_block.get_scroll_position();
            details_block.update_scrollbar_state(lines_count, Some(current_pos));
            current_pos
        } else {
            0
        };

        // Build the block after mutable operations
        let block = if let Some(details_block) = self.blocks.get("details") {
            details_block.build(is_focused)
        } else {
            return Err(anyhow!("No details block available"));
        };

        Paragraph::new(content)
            .block(block)
            .fg(theme::TEXT_FG_COLOR)
            .scroll((scroll_position as u16, 0))
            .render(content_area, buf);

        let scrollbar = AppBlock::create_scrollbar(is_focused);

        // Use AppBlock's scrollbar state
        if let Some(details_block) = self.blocks.get_mut("details") {
            StatefulWidget::render(
                scrollbar,
                scrollbar_area,
                buf,
                details_block.get_scrollbar_state(),
            );
        }
        Ok(())
    }

    fn render_debug_logs(&mut self, area: Rect, buf: &mut Buffer) -> Result<()> {
        // Initialize blocks if not already done
        if self.blocks.is_empty() {
            self.initialize_blocks();
        }

        // Get the DEBUG block ID and check if focused
        let (debug_block_id, is_focused, should_focus) =
            if let Some(debug_block) = self.blocks.get_mut("debug") {
                let debug_block_id = debug_block.id();
                let is_focused = self.focused_block_id == Some(debug_block_id);

                // Handle click and set focus
                let should_focus = if let Some(event) = self.event {
                    debug_block.handle_mouse_event(&event, area, self.event.as_ref())
                } else {
                    false
                };

                (debug_block_id, is_focused, should_focus)
            } else {
                return Err(anyhow!("No debug block available"));
            };

        if should_focus {
            self.set_focused_block(debug_block_id);
        }

        // Create a horizontal layout: main content area + scrollbar area
        let [content_area, scrollbar_area] = Layout::horizontal([
            Constraint::Fill(1),   // Main content takes most space
            Constraint::Length(1), // Scrollbar is 1 character wide
        ])
        .margin(0)
        .areas(area);

        // Build the block after getting focus info
        let _block = if let Some(debug_block) = self.blocks.get("debug") {
            debug_block.build(is_focused)
        } else {
            return Err(anyhow!("No debug block available"));
        };

        let debug_logs_lines = if let Ok(logs) = self.debug_logs.lock() {
            if logs.is_empty() {
                vec![Line::from("No debug logs...".italic())]
            } else {
                logs.iter()
                    .rev() // Show most recent first
                    .map(|log_entry| {
                        let style = if log_entry.contains("ERROR") {
                            theme::ERROR_STYLE
                        } else if log_entry.contains("WARN") {
                            theme::WARN_STYLE
                        } else if log_entry.contains("DEBUG") {
                            theme::DEBUG_STYLE
                        } else {
                            Style::default().fg(theme::TEXT_FG_COLOR)
                        };
                        Line::styled(log_entry.clone(), style)
                    })
                    .collect()
            }
        } else {
            vec![Line::from("Failed to read debug logs...".italic())]
        };

        // The debug_logs_lines vector already contains properly wrapped lines
        let lines_count = debug_logs_lines.len();

        // Update the debug block with lines count and scrollbar state
        let scroll_position = if let Some(debug_block) = self.blocks.get_mut("debug") {
            debug_block.set_lines_count(lines_count);
            if !is_focused {
                debug_block.set_scroll_position(0);
            }
            let current_pos = debug_block.get_scroll_position();
            debug_block.update_scrollbar_state(lines_count, Some(current_pos));
            current_pos
        } else {
            0
        };

        // Build the block after mutable operations
        let _block = if let Some(debug_block) = self.blocks.get("debug") {
            debug_block.build(is_focused)
        } else {
            return Err(anyhow!("No debug block available"));
        };

        Paragraph::new(debug_logs_lines)
            .block(_block)
            .fg(theme::TEXT_FG_COLOR)
            .scroll((scroll_position as u16, 0))
            .render(content_area, buf);

        let scrollbar = AppBlock::create_scrollbar(is_focused);

        // Use AppBlock's scrollbar state
        if let Some(debug_block) = self.blocks.get_mut("debug") {
            StatefulWidget::render(
                scrollbar,
                scrollbar_area,
                buf,
                debug_block.get_scrollbar_state(),
            );
        }
        Ok(())
    }

    fn is_log_block_focused(&self) -> Result<bool> {
        if let (Some(focused_id), Some(logs_block)) =
            (self.focused_block_id, self.blocks.get("logs"))
        {
            Ok(focused_id == logs_block.id())
        } else {
            Err(anyhow!("No logs block available"))
        }
    }

    fn is_debug_block_focused(&self) -> Result<bool> {
        if let (Some(focused_id), Some(debug_block)) =
            (self.focused_block_id, self.blocks.get("debug"))
        {
            Ok(focused_id == debug_block.id())
        } else {
            Err(anyhow!("No debug block available"))
        }
    }

    fn is_details_block_focused(&self) -> Result<bool> {
        if let (Some(focused_id), Some(details_block)) =
            (self.focused_block_id, self.blocks.get("details"))
        {
            Ok(focused_id == details_block.id())
        } else {
            Err(anyhow!("No details block available"))
        }
    }

    fn handle_log_item_scrolling(&mut self, move_next: bool, circular: bool) -> Result<()> {
        let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
            filtered
        } else {
            &mut self.log_list
        };

        // Handle selection changes using the original LogList logic
        match (move_next, circular) {
            (true, true) => {
                target_list.select_next_circular();
            }
            (true, false) => {
                target_list.select_next();
            }
            (false, true) => {
                target_list.select_previous_circular();
            }
            (false, false) => {
                target_list.select_previous();
            }
        }

        // Don't update scroll position automatically - let selection and scrolling be independent
        self.update_logs_scrollbar_state();
        Ok(())
    }

    fn handle_logs_view_scrolling(&mut self, move_down: bool) -> Result<()> {
        // Handle pure view scrolling without changing selection
        if let Some(logs_block) = self.blocks.get_mut("logs") {
            let lines_count = logs_block.get_lines_count();
            let current_position = logs_block.get_scroll_position();

            let new_position = if move_down {
                if current_position >= lines_count.saturating_sub(1) {
                    current_position // Stay at bottom
                } else {
                    current_position.saturating_add(1)
                }
            } else {
                current_position.saturating_sub(1)
            };

            logs_block.set_scroll_position(new_position);
            logs_block.update_scrollbar_state(lines_count, Some(new_position));
        }
        Ok(())
    }

    fn handle_details_block_scrolling(&mut self, move_next: bool) -> Result<()> {
        if let Some(details_block) = self.blocks.get_mut("details") {
            let lines_count = details_block.get_lines_count();
            let current_position = details_block.get_scroll_position();

            let new_position = if move_next {
                if current_position == lines_count - 1 {
                    current_position
                } else {
                    current_position.saturating_add(1)
                }
            } else {
                current_position.saturating_sub(1)
            };

            details_block.set_scroll_position(new_position);
            details_block.update_scrollbar_state(lines_count, Some(new_position));
        } else {
            return Err(anyhow!("No details block available"));
        }
        Ok(())
    }

    fn handle_debug_logs_scrolling(&mut self, move_next: bool) -> Result<()> {
        if let Some(debug_block) = self.blocks.get_mut("debug") {
            let lines_count = debug_block.get_lines_count();
            let current_position = debug_block.get_scroll_position();

            let new_position = if move_next {
                // should stop when it reaches the end
                if current_position == lines_count - 1 {
                    current_position
                } else {
                    current_position.saturating_add(1)
                }
            } else {
                current_position.saturating_sub(1)
            };

            debug_block.set_scroll_position(new_position);
            debug_block.update_scrollbar_state(lines_count, Some(new_position));
        } else {
            return Err(anyhow!("No debug block available"));
        }
        Ok(())
    }

    fn make_yank_content(&self, item: &LogItem) -> String {
        format!(
            "# Formatted Log\n\n## Time:\n\n{}\n\n## Level:\n\n{}\n\n## Origin:\n\n{}\n\n## Tag:\n\n{}\n\n## Content:\n\n{}\n\n# Raw Log\n\n{}",
            item.time, item.level, item.origin, item.tag, item.content, item.raw_content
        )
    }

    fn yank_current_log(&self) -> Result<()> {
        // Use filtered list if available, otherwise use the full list
        let (items, state) = if let Some(ref filtered) = self.filtered_log_list {
            (&filtered.items, &filtered.state)
        } else {
            (&self.log_list.items, &self.log_list.state)
        };

        if let Some(i) = state.selected() {
            // Access items in reverse order to match the LOGS panel display order
            let reversed_index = items.len().saturating_sub(1).saturating_sub(i);
            let item = &items[reversed_index];

            let mut clipboard = Clipboard::new()?;
            let yank_content = self.make_yank_content(item);
            clipboard.set_text(&yank_content)?;

            log::debug!(
                "Yanked log content to clipboard: {} chars",
                yank_content.len()
            );
        } else {
            log::debug!("No log item selected for yanking");
        }

        Ok(())
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
            KeyCode::Char('j') | KeyCode::Down => {
                self.handle_log_item_scrolling(true, true);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.handle_log_item_scrolling(false, true);
            }
            KeyCode::Char('g') => {
                let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                    filtered
                } else {
                    &mut self.log_list
                };
                target_list.state.select_first();
                self.update_logs_scrollbar_state();
            }
            KeyCode::Char('G') => {
                let target_list = if let Some(ref mut filtered) = self.filtered_log_list {
                    filtered
                } else {
                    &mut self.log_list
                };
                target_list.state.select_last();
                self.update_logs_scrollbar_state();
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
                    target_list.state.select_first();
                    self.update_logs_scrollbar_state();
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
            KeyCode::Char('y') => {
                // Yank (copy) the current log item content to clipboard
                if let Err(e) = self.yank_current_log() {
                    log::debug!("Failed to yank log content: {}", e);
                }
            }
            _ => {}
        }
    }

    fn set_focused_block(&mut self, block_id: uuid::Uuid) {
        self.focused_block_id = Some(block_id);
    }

    fn initialize_blocks(&mut self) {
        // Create LOGS block - basic click logging + detailed handling in render_logs method
        let logs_block =
            AppBlock::new().set_title(format!("LOGS | Detail Level: {}", self.detail_level));
        let logs_block_id = logs_block.id();
        self.blocks.insert("logs".to_string(), logs_block);

        // Create LOG DETAILS block with horizontal padding
        let details_block = AppBlock::new()
            .set_title("LOG DETAILS")
            .set_padding(Padding::horizontal(1))
            .on_click(Box::new(|_column, _row, _area| {
                log::debug!("Clicked on log details area");
            }));
        self.blocks.insert("details".to_string(), details_block);

        // Create DEBUG LOGS block with horizontal padding
        let debug_block = AppBlock::new()
            .set_title("DEBUG LOGS")
            .set_padding(Padding::horizontal(1))
            .on_click(Box::new(|_column, _row, _area| {
                log::debug!("Clicked on debug logs areas");
            }));
        self.blocks.insert("debug".to_string(), debug_block);

        // Auto-focus the LOGS block when the app opens
        self.set_focused_block(logs_block_id);
    }

    fn clear_event(&mut self) {
        self.event = None;
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [header_area, main_area, debug_area, footer_area] = Layout::vertical([
            Constraint::Length(1), // Header
            Constraint::Fill(1),   // Main area (logs + details)
            Constraint::Length(6), // Debug logs block (2 lines + borders)
            Constraint::Length(1), // Footer
        ])
        .areas(area);

        let [list_area, item_area] =
            Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(main_area);

        self.render_header(header_area, buf);
        self.render_logs(list_area, buf);
        self.render_details(item_area, buf);
        self.render_debug_logs(debug_area, buf);
        self.render_footer(footer_area, buf);

        self.clear_event();
    }
}
