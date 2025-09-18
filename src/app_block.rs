use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::Rect,
    prelude::Stylize,
    style::Style,
    widgets::BorderType,
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
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded);

        if focused {
            block =
                block.border_style(Style::new().fg(ratatui::style::palette::tailwind::ZINC.c100));
        } else {
            block =
                block.border_style(Style::new().fg(ratatui::style::palette::tailwind::ZINC.c600));
        }

        if let Some(title) = &self.title {
            let title_style = if focused {
                Style::new().bold().underlined()
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
        if let Some(mouse_event) = mouse_event {
            let inner_area = self.build(false).inner(area);
            let is_hovering = inner_area.contains(ratatui::layout::Position::new(
                mouse_event.column,
                mouse_event.row,
            ));

            // Handle hover focus - return true if mouse is hovering over this block
            if is_hovering && mouse_event.kind == MouseEventKind::Moved {
                return true;
            }

            // Handle click events (existing functionality)
            if let Some(callback) = &self.click_callback {
                if is_hovering && mouse_event.kind == MouseEventKind::Up(MouseButton::Left) {
                    callback();
                    return true;
                }
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
