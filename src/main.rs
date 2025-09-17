mod app;
mod app_block;
mod file_finder;
mod log_list;
mod log_parser;
mod metadata;

fn enable_mouse_capture() {
    ratatui::crossterm::terminal::enable_raw_mode().unwrap();
    let mut stdout = std::io::stdout();
    ratatui::crossterm::execute!(stdout, ratatui::crossterm::event::EnableMouseCapture).unwrap();
}

fn disable_mouse_capture() {
    let mut stdout = std::io::stdout();
    ratatui::crossterm::execute!(stdout, ratatui::crossterm::event::DisableMouseCapture).unwrap();
    ratatui::crossterm::terminal::disable_raw_mode().unwrap();
}

fn main() {
    enable_mouse_capture();
    app::start().unwrap();
    disable_mouse_capture();
}
