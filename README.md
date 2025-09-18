# Termlog

A fast, minimal, terminal-based log inspector.

_[Screenshot/GIF of termlog in action, showing color-coded logs and the condensed view]_

`termlog` is a terminal-based tool for inspecting log files. It follows the KISS principle: open logs instantly, scroll smoothly, color by severity, and expand entries only when you need details. It's snappy, cross-platform, and designed to handle huge log files with ease.

## ✨ Key Features

- **Instant Open**: Jumps straight into the most recent log file.
- **Selection Mode**: Interactively pick a log file when needed.
- **Real-time Tailing**: Follows log updates, similar to `tail -f`.
- **Smooth Navigation**: Uses vim/arrow keys and mouse for scrolling and interaction.
- **Condensed & Expanded View**: Toggles between a one-line summary and the full log entry.
- **Color by Severity**: Clearly distinguishes between `DEBUG`, `INFO`, `WARN`, `ERROR`, and `FATAL`.
- **High Performance**: Uses virtualized rendering and a capped buffer to handle very large logs efficiently.
- **Cross-Platform**: Runs on Linux, macOS, and Windows.

## 🚀 Quick Start

#### Prerequisites

- **Rust Toolchain**: Version 1.77+ is recommended. You can install it via [rustup.rs](https://rustup.rs).

#### Installation & Build

TBD

## ⌨️ Usage

### Key Bindings

TBD

### Command-Line Options

TBD

## ❓ FAQ

TBD

## 📜 License

This project is licensed under the [MIT License](LICENSE-MIT)

## 🙏 Acknowledgments

`termlog` is built on the shoulders of giants. A huge thank you to the maintainers of `ratatui`, `crossterm`, `tokio`, and the broader Rust community.
