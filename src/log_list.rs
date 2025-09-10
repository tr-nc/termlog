use crate::log_parser::LogItem;
use ratatui::widgets::ListState;

pub struct LogList {
    pub items: Vec<LogItem>,
    pub state: ListState,
}

impl LogList {
    pub fn new(items: Vec<LogItem>) -> Self {
        Self {
            items,
            state: ListState::default(),
        }
    }
}
