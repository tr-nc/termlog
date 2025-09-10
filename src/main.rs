mod app;
mod file_finder;
mod log_list;
mod log_parser;
mod metadata;

fn main() {
    app::start().unwrap();
}
