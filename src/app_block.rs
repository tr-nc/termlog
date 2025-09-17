use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    prelude::Stylize,
    style::Style,
    symbols,
    widgets::{Block, Borders},
};
use uuid::Uuid;

pub type ClickCallback = Box<dyn Fn() + Send + Sync>;

pub struct AppBlock {
    #[allow(dead_code)]
    id: Uuid,
    title: Option<String>,
    click_callback: Option<ClickCallback>,
}

impl AppBlock {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            title: None,
            click_callback: None,
        }
    }

    pub fn set_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn on_click(mut self, callback: ClickCallback) -> Self {
        self.click_callback = Some(callback);
        self
    }

    pub fn update_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
    }

    #[allow(dead_code)]
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn build(&self, focused: bool) -> Block<'_> {
        let log_header_style = Style::new()
            .fg(ratatui::style::palette::tailwind::ZINC.c100)
            .bg(ratatui::style::palette::tailwind::ZINC.c400);

        let normal_row_bg_color = ratatui::style::palette::tailwind::ZINC.c950;

        let mut block = Block::default()
            .borders(Borders::TOP)
            .border_set(symbols::border::EMPTY)
            .border_style(log_header_style)
            .bg(normal_row_bg_color);

        if let Some(title) = &self.title {
            let title_style = if focused {
                Style::new().bold()
            } else {
                Style::new()
            };
            block = block.title(
                ratatui::prelude::Line::from(title.as_str())
                    .style(title_style)
                    .centered(),
            );
        }

        block
    }

    pub fn handle_mouse_event(
        &self,
        _event: &MouseEvent,
        area: Rect,
        mouse_event: Option<&MouseEvent>,
    ) -> bool {
        if let (Some(callback), Some(mouse_event)) = (&self.click_callback, mouse_event)
            && mouse_event.kind == MouseEventKind::Up(MouseButton::Left)
        {
            let inner_area = self.build(false).inner(area);
            if inner_area.contains(ratatui::layout::Position::new(
                mouse_event.column,
                mouse_event.row,
            )) {
                callback();
                return true;
            }
        }
        false
    }
}

impl Default for AppBlock {
    fn default() -> Self {
        Self::new()
    }
}
