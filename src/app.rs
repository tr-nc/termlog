use crate::{
    file_finder,
    log_list::LogList,
    log_parser::{LogItem, process_delta},
    metadata,
};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, MouseEvent};
use memmap2::MmapOptions;
use ratatui::{
    prelude::*,
    style::palette,
    symbols,
    widgets::{
        Block, Borders, HighlightSpacing, List, ListItem, Padding, Paragraph, StatefulWidget,
        Widget, Wrap,
    },
};
use std::{
    fs::File,
    path::{Path, PathBuf},
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

    let app_result = App::new(latest_file_path).run();

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

impl App {
    fn new(log_file_path: PathBuf) -> Self {
        Self {
            should_exit: false,
            log_list: LogList::new(Vec::new()),
            log_file_path,
            last_len: 0,
            prev_meta: None,
            autoscroll: true,
        }
    }

    fn run(mut self) -> Result<()> {
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
            Err(_) => return Ok(()),
        };

        if metadata::has_changed(&self.prev_meta, &current_meta) {
            // TODO: check if this branch works properly, it's pretty rare to happen, but it does
            if current_meta.len < self.last_len {
                // file was truncated, reset state
                self.log_list.items.clear();
                self.last_len = 0;
            }

            if current_meta.len > self.last_len {
                if let Ok(new_items) =
                    map_and_process_delta(&self.log_file_path, self.last_len, current_meta.len)
                {
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
            .title(Line::raw("LOGS").centered())
            .borders(Borders::TOP)
            .border_set(symbols::border::EMPTY)
            .border_style(LOG_HEADER_STYLE)
            .bg(NORMAL_ROW_BG_COLOR);

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
            .scroll_padding(1)
            .highlight_style(SELECTED_STYLE)
            .highlight_symbol(">")
            .highlight_spacing(HighlightSpacing::Always);

        StatefulWidget::render(list_widget, area, buf, &mut self.log_list.state);

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

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        // println!("Mouse event: {:?}", mouse);
        // TODO: this doesn't work
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
            KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                self.should_exit = true
            }
            KeyCode::Char('c') if !key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                self.log_list.items.clear();
                self.log_list.state.select(None);
            }
            KeyCode::Char('h') | KeyCode::Left => self.log_list.state.select(None),
            KeyCode::Char('j') | KeyCode::Down => self.log_list.state.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.log_list.state.select_previous(),
            KeyCode::Char('g') => self.log_list.state.select_first(),
            KeyCode::Char('G') => self.log_list.state.select_last(),
            KeyCode::Char('a') => self.autoscroll = !self.autoscroll, // Toggle autoscroll
            _ => {}
        }
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
            Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(main_area);

        self.render_header(header_area, buf);
        self.render_list(list_area, buf);
        self.render_selected_item(item_area, buf);
        App::render_footer(footer_area, buf);
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
