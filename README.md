# dhlog

> A fast, minimal, terminal-based log inspector.

[Crates.io](https://img.shields.io/crates/v/dhlog.svg)
[Build Status](https://img.shields.io/travis/yourname/dhlog.svg)
[License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)

*[Screenshot/GIF of dhlog in action, showing color-coded logs and the condensed view]*

`dhlog` is a terminal-based tool for inspecting log files. It follows the KISS principle: open logs instantly, scroll smoothly, color by severity, and expand entries only when you need details. It's snappy, cross-platform, and designed to handle huge log files with ease.

## ✨ Key Features

*   **Instant Open**: Jumps straight into the most recent log file.
*   **Selection Mode**: Interactively pick a log file when needed.
*   **Real-time Tailing**: Follows log updates, similar to `tail -f`.
*   **Smooth Navigation**: Uses vim/arrow keys and mouse for scrolling and interaction.
*   **Condensed & Expanded View**: Toggles between a one-line summary and the full log entry.
*   **Color by Severity**: Clearly distinguishes between `DEBUG`, `INFO`, `WARN`, `ERROR`, and `FATAL`.
*   **High Performance**: Uses virtualized rendering and a capped buffer to handle very large logs efficiently.
*   **Cross-Platform**: Runs on Linux, macOS, and Windows.

## 🚀 Quick Start

#### Prerequisites

*   **Rust Toolchain**: Version 1.77+ is recommended. You can install it via [rustup.rs](https://rustup.rs).

#### Installation & Build

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/yourname/dhlog.git
    cd dhlog
    ```

2.  **Build the release binary:**
    ```bash
    cargo build --release
    ```
    The executable will be located at `target/release/dhlog`.

#### Running

*   **Open the most recent log automatically:**
    ```bash
    dhlog
    ```

*   **Open in selection mode to choose a log file:**
    ```bash
    dhlog -a
    ```

## ⌨️ Usage

### Key Bindings

| Key(s) | Action |
| :--- | :--- |
| `↑` / `k` | Move selection up |
| `↓` / `j` | Move selection down |
| `PgUp` | Scroll one page up |
| `PgDn` | Scroll one page down |
| `g` / `Home` | Go to the top |
| `G` / `End` | Go to the bottom |
| `←` / `h` | Scroll left (in expanded view) |
| `→` / `l` | Scroll right (in expanded view) |
| `Enter` / `e` | Expand or collapse the selected log entry |
| `f` | Toggle follow mode (`tail -f`) |
| `p` | Pause or resume log ingestion |
| `/` | Enter search mode (use `n`/`N` for next/previous) |
| `s` | Open selection mode to switch files |
| `c` | Clear the current view history |
| `?` | Show the help popup |
| `q` | Quit the application |

### Command-Line Options

| Option | Alias | Description |
| :--- | :--- | :--- |
| `--all` | `-a` | Start in selection mode to choose a log file. |
| `--from-start` | | Read files from the beginning instead of tailing from the end. |
| `--min-level <level>` | | Minimum severity level to display (`debug`, `info`, `warn`, `error`, `fatal`). |
| `--no-color` | | Disable all ANSI color output. |
| `--max-lines <N>` | | Maximum number of lines to keep in the scrollback buffer (e.g., `100000`). |
| `--json` | | Force all logs to be parsed as JSON. |
| `--regex <PATTERN>` | | Provide a regex for parsing plain-text logs. Must use named groups like `ts`, `level`, `msg`. |
| `--config <FILE>` | `-c` | Load a specific configuration file. |
| `--verbose` | `-v` | Increase internal logging level (can be repeated, e.g., `-vv`). |
| `--help` | `-h` | Display the help message. |
| `--version` | `-V` | Display the application version. |

## ⚙️ Configuration

`dhlog` can be configured via a TOML file located at `~/.config/dhlog/config.toml` (or specified with `--config`).

Here is an example configuration with all available options:
```toml
# Log file discovery settings
[logs]
search_paths = ["/var/log", "./logs"]
filename_patterns = ["*.log", "*.jsonl"]

# User interface settings
[ui]
theme = "default"         # "default", "dark", or "light"
max_lines = 100000        # Scrollback buffer size
truncate_width = 120      # Width for one-line summary view
mouse = true              # Enable or disable mouse support

# Default filters
[filters]
min_level = "info"

# Log parsing behavior
[parsing]
mode = "auto"             # "auto", "json", or "regex"
# Example regex for a pattern like: "2024-08-27 INFO This is a message"
regex = '^(?P<ts>[^ ]+) (?P<level>[A-Z]+) (?P<msg>.*)$'

# Tailing and file rotation settings
[follow]
enabled = true
detect_rotation = true
```

## 🧠 Core Concepts

#### How "Most Recent Log" is Chosen

By default, `dhlog` scans the `search_paths` for files matching `filename_patterns` and opens the one with the most recent modification time. You can override this by starting in selection mode (`-a`).

#### Severity and Colors

*   **DEBUG**: Dim Gray
*   **INFO**: Cyan/Blue
*   **WARN**: Yellow
*   **ERROR**: Red
*   **FATAL**: Bold Red

Colors adapt to the configured theme and can be disabled entirely with `--no-color`.

#### Performance & Large Files

`dhlog` is designed for speed and low memory usage.
*   **Virtualized Rendering**: Only the visible rows are drawn to the screen, ensuring smooth scrolling.
*   **Bounded Buffer**: The in-memory log buffer is capped (`--max-lines`) to prevent excessive memory consumption.

## 🧑‍💻 For Developers

### Project Structure

```
.
├── Cargo.toml
├── src/
│   ├── main.rs         # CLI parsing and application startup
│   ├── app.rs          # Core application state and logic
│   ├── ui.rs           # TUI rendering with ratatui
│   ├── input.rs        # Keyboard and mouse event handling
│   ├── select.rs       # File selection mode UI
│   ├── reader/         # File reading and tailing
│   └── parsing/        # Log parsing (JSON, regex)
└── tests/
    └── integration_tests.rs
```

### Key Dependencies

*   **`clap`**: Command-line argument parsing.
*   **`tokio`**: Asynchronous runtime.
*   **`ratatui` & `crossterm`**: Terminal UI rendering and backend.
*   **`notify`**: Filesystem events for rotation detection.
*   **`serde` & `serde_json`**: JSON deserialization.
*   **`regex`**: Regex-based log parsing.
*   **`anyhow`**: Error handling.
*   **`tracing`**: Internal application logging.

### Development Workflow

*   **Run in debug mode:**
    ```bash
    # Start in selection mode
    cargo run -- -a
    # Open a specific sample file
    cargo run -- ./sample.log
    ```

*   **Format and Lint:**
    ```bash
    cargo fmt
    cargo clippy --all-targets --all-features -D warnings
    ```

*   **Run Tests:**
    ```bash
    cargo test
    ```

## ❓ FAQ

*   **Does it support the mouse?**
    Yes, if your terminal (e.g., iTerm2, Windows Terminal) supports it. This can be disabled in the config file.

*   **Can I read from stdin?**
    Yes. You can pipe output from another command directly into `dhlog`: `somecommand | dhlog`.

*   **How does it handle huge logs?**
    `dhlog` keeps a capped in-memory buffer and only renders what's visible, so it can open multi-gigabyte files without crashing.

## 📜 License

This project is dual-licensed under either the [MIT License](LICENSE-MIT) or [Apache License, Version 2.0](LICENSE-APACHE).

## 🙏 Acknowledgments

`dhlog` is built on the shoulders of giants. A huge thank you to the maintainers of `ratatui`, `crossterm`, `tokio`, and the broader Rust community.
