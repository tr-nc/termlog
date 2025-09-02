use std::fs::{self, File};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::Duration;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

// --- File Discovery (Unchanged) ---
// This function correctly finds the latest "live" log file.
fn find_latest_live_log(log_dir: &Path) -> Result<PathBuf, String> {
    let entries = fs::read_dir(log_dir)
        .map_err(|e| format!("Failed to read directory '{}': {}", log_dir.display(), e))?;

    let mut live_log_files: Vec<PathBuf> = entries
        .filter_map(|entry_result| {
            entry_result.ok().and_then(|entry| {
                let path = entry.path();
                if !path.is_file() { return None; }
                let file_name = path.file_name()?.to_str()?;
                if file_name.ends_with(".log") {
                    let base_name = file_name.strip_suffix(".log").unwrap();
                    if let Some(last_dot_pos) = base_name.rfind('.') {
                        if base_name[last_dot_pos + 1..].parse::<u32>().is_ok() {
                            return None;
                        }
                    }
                    Some(path)
                } else { None }
            })
        })
        .collect();

    if live_log_files.is_empty() {
        return Err("No live log files found in the directory.".to_string());
    }

    live_log_files.sort();
    Ok(live_log_files.pop().unwrap())
}

/// Watches a file for changes and prints any new content appended to it.
///
/// # Arguments
/// * `file_path` - The path to the file to be tailed.
///
/// # Returns
/// * `Ok(())` if the watcher exits gracefully.
/// * `Err(String)` if there is an error setting up the watcher or reading the file.
fn watch_and_tail_file(file_path: &Path) -> Result<(), String> {
    // --- 1. Setup the file reader ---
    let file = File::open(file_path)
        .map_err(|e| format!("Failed to open file '{}': {}", file_path.display(), e))?;

    // Use a BufReader for efficient reading.
    let mut reader = BufReader::new(file);

    // IMPORTANT: Move the cursor to the end of the file.
    // This ensures we only read content that is written *after* the program starts.
    reader.seek(SeekFrom::End(0))
        .map_err(|e| format!("Failed to seek to end of file: {}", e))?;

    // --- 2. Setup the file system watcher ---
    // Create a channel to receive events from the watcher thread.
    let (tx, rx) = channel();

    // Create a `RecommendedWatcher`, which is the best implementation for the current OS.
    let mut watcher = RecommendedWatcher::new(tx, Config::default())
        .map_err(|e| format!("Failed to create file watcher: {}", e))?;

    // Watch the specific file for any changes.
    watcher.watch(file_path, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Failed to start watching file: {}", e))?;

    println!("✅ Now tailing log file. Waiting for new content...\n");

    // --- 3. The Main Event Loop ---
    // This loop blocks until an event is received on the channel.
    for res in rx {
        match res {
            Ok(Event { kind, .. }) => {
                // We only care about events that indicate the file's data has been modified.
                if kind.is_modify() || kind.is_create() {
                    // Read all new lines from the reader's current position to the new end.
                    let mut line_buffer = String::new();
                    while let Ok(bytes_read) = reader.read_line(&mut line_buffer) {
                        if bytes_read == 0 {
                            // We've reached the new end of the file.
                            break;
                        }
                        // Print the new line and clear the buffer for the next one.
                        print!("{}", line_buffer);
                        line_buffer.clear();
                    }
                }
            }
            Err(e) => {
                return Err(format!("File watcher error: {}", e));
            }
        }
    }

    Ok(())
}

fn main() {
    let home_dir = match dirs::home_dir() {
        Some(path) => path,
        None => {
            eprintln!("Error: Could not determine the home directory.");
            std::process::exit(1);
        }
    };

    let log_dir_path = home_dir.join("Library/Application Support/DouyinAR/Logs/previewLog");
    println!("🔍 Searching for the latest live log in: {}", log_dir_path.display());

    // 1. Find the latest log file.
    let latest_file_path = match find_latest_live_log(&log_dir_path) {
        Ok(path) => {
            println!("✅ Found log file: {}", path.display());
            path
        }
        Err(e) => {
            eprintln!("❌ Error: {}", e);
            std::process::exit(1);
        }
    };

    // 2. Start tailing the file.
    if let Err(e) = watch_and_tail_file(&latest_file_path) {
        eprintln!("❌ A critical error occurred: {}", e);
    }
}
