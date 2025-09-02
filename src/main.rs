use std::fs;
use std::path::{Path, PathBuf};

/// Finds the latest "live" log file in a given directory.
///
/// This function scans the specified directory for files ending in `.log`. It specifically
/// ignores "chunked" or "archived" logs, which are identified by having a numeric suffix
/// before the extension (e.g., `filename.1.log`, `filename.2.log`).
///
/// It determines the "latest" file by sorting the names alphabetically, which works because
/// the timestamps are in `YYYY-MM-DD-HH-MM-SS` format.
///
/// # Arguments
/// * `log_dir` - A reference to the path of the directory to search.
///
/// # Returns
/// * `Ok(PathBuf)` containing the path to the latest log file if one is found.
/// * `Err(String)` if the directory cannot be read or if no suitable log files are found.
fn find_latest_live_log(log_dir: &Path) -> Result<PathBuf, String> {
    // Read all entries in the directory, handling potential errors like permissions or non-existence.
    let entries = fs::read_dir(log_dir)
        .map_err(|e| format!("Failed to read directory '{}': {}", log_dir.display(), e))?;

    // --- Filter and collect all valid "live" log files ---
    let mut live_log_files: Vec<PathBuf> = entries
        .filter_map(|entry_result| {
            // We only care about entries that we can successfully read.
            entry_result.ok().and_then(|entry| {
                let path = entry.path();
                // The entry must be a file.
                if !path.is_file() {
                    return None;
                }

                // Get the filename as a string for analysis.
                let file_name = path.file_name()?.to_str()?;

                // --- Core Filtering Logic ---
                // 1. The file must end with ".log".
                // 2. It must NOT be a chunked file (e.g., "name.1.log").
                if file_name.ends_with(".log") {
                    // Get the part of the name before the ".log" extension.
                    let base_name = file_name.strip_suffix(".log").unwrap();

                    // Check if the base name contains a dot, which might indicate a chunk.
                    if let Some(last_dot_pos) = base_name.rfind('.') {
                        // Get the part after the last dot.
                        let potential_chunk_num = &base_name[last_dot_pos + 1..];
                        // If that part can be parsed as a number, it's a chunk file, so we ignore it.
                        if potential_chunk_num.parse::<u32>().is_ok() {
                            return None; // It's a chunk file, ignore.
                        }
                    }

                    // If we get here, it's a live log file.
                    Some(path)
                } else {
                    None // Not a .log file.
                }
            })
        })
        .collect();

    // After filtering, check if we found any live logs.
    if live_log_files.is_empty() {
        return Err("No live log files found in the directory.".to_string());
    }

    // Sort the files lexicographically. Because the filenames contain sortable timestamps,
    // the last file in the sorted list will be the most recent one.
    live_log_files.sort();

    // The last element is the latest file. `pop()` removes and returns it.
    Ok(live_log_files.pop().unwrap())
}

fn main() {
    // Find the user's home directory.
    let home_dir = match dirs::home_dir() {
        Some(path) => path,
        None => {
            eprintln!("Error: Could not determine the home directory.");
            std::process::exit(1);
        }
    };

    // Construct the full path to the target log directory.
    let log_dir_path = home_dir.join("Library/Application Support/DouyinAR/Logs/previewLog");

    println!("🔍 Searching for the latest live log in: {}", log_dir_path.display());
    println!("---------------------------------------------------");

    // Call the function to find the latest log file.
    match find_latest_live_log(&log_dir_path) {
        Ok(latest_file) => {
            // On success, print the name of the file found.
            if let Some(file_name) = latest_file.file_name() {
                println!("✅ Latest live log file: {}", file_name.to_string_lossy());
            } else {
                eprintln!("Error: Found a path but could not extract the filename.");
            }
        }
        Err(e) => {
            // If an error occurred (e.g., no files found), print the error message.
            eprintln!("❌ Error: {}", e);
        }
    }
}
