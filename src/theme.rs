use ratatui::{
    prelude::*,
    style::palette,
};

// colors
pub const NORMAL_ROW_BG_COLOR: Color = palette::tailwind::ZINC.c950;
pub const ALT_ROW_BG_COLOR: Color = palette::tailwind::ZINC.c900;
pub const TEXT_FG_COLOR: Color = palette::tailwind::ZINC.c200;

// styles
#[allow(dead_code)]
pub const LOG_HEADER_STYLE: Style = Style::new()
    .fg(palette::tailwind::ZINC.c100)
    .bg(palette::tailwind::ZINC.c400);
pub const SELECTED_STYLE: Style = Style::new()
    .bg(palette::tailwind::ZINC.c700)
    .add_modifier(Modifier::BOLD);
pub const INFO_STYLE: Style = Style::new().fg(palette::tailwind::SKY.c400);
pub const WARN_STYLE: Style = Style::new().fg(palette::tailwind::YELLOW.c400);
pub const ERROR_STYLE: Style = Style::new().fg(palette::tailwind::RED.c400);
pub const DEBUG_STYLE: Style = Style::new().fg(palette::tailwind::GREEN.c400);