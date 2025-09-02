use std::fs::{self, File};
use std::io::{BufRead, BufReader};
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
    let entries = fs::read_dir(log_dir)
        .map_err(|e| format!("Failed to read directory '{}': {}", log_dir.display(), e))?;

    let mut live_log_files: Vec<PathBuf> = entries
        .filter_map(|entry_result| {
            entry_result.ok().and_then(|entry| {
                let path = entry.path();
                if !path.is_file() {
                    return None;
                }
                let file_name = path.file_name()?.to_str()?;
                if file_name.ends_with(".log") {
                    let base_name = file_name.strip_suffix(".log").unwrap();
                    if let Some(last_dot_pos) = base_name.rfind('.') {
                        let potential_chunk_num = &base_name[last_dot_pos + 1..];
                        if potential_chunk_num.parse::<u32>().is_ok() {
                            return None;
                        }
                    }
                    Some(path)
                } else {
                    None
                }
            })
        })
        .collect();

    if live_log_files.is_empty() {
        return Err("No live log files found in the directory.".to_string());
    }

    live_log_files.sort();
    Ok(live_log_files.pop().unwrap())
}

/// Opens a file and prints its first N lines to the console.
///
/// # Arguments
/// * `file_path` - The path to the file to read.
/// * `num_lines` - The number of lines to print from the beginning of the file.
///
/// # Returns
/// * `Ok(())` on success.
/// * `Err(String)` if the file cannot be opened or read.
fn print_file_head(file_path: &Path, num_lines: usize) -> Result<(), String> {
    // Attempt to open the file.
    let file = File::open(file_path)
        .map_err(|e| format!("Failed to open file '{}': {}", file_path.display(), e))?;

    // Use a BufReader for efficient, line-by-line reading.
    let reader = BufReader::new(file);

    println!("--- Displaying first {} lines of {} ---\n", num_lines, file_path.file_name().unwrap().to_string_lossy());

    // Iterate over the lines of the file, taking at most `num_lines`.
    // `enumerate()` gives us the line number.
    // The iterator will stop automatically if the file has fewer than `num_lines`.
    for (i, line_result) in reader.lines().take(num_lines).enumerate() {
        match line_result {
            Ok(line) => {
                // Print with a line number for context.
                println!("{:>4}: {}", i + 1, line);
            }
            Err(e) => {
                // If a specific line can't be read (e.g., invalid UTF-8), print an error for it.
                eprintln!("Error reading line {}: {}", i + 1, e);
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
    println!("---------------------------------------------------");

    // --- Main application logic ---
    // 1. Find the latest log file.
    match find_latest_live_log(&log_dir_path) {
        Ok(latest_file) => {
            // 2. If found, print its name.
            println!("✅ Latest live log file: {}\n", latest_file.display());

            // 3. Attempt to print the first 100 lines of that file.
            if let Err(e) = print_file_head(&latest_file, 100) {
                // Handle any errors that occur during file reading.
                eprintln!("❌ Error: {}", e);
            }
        }
        Err(e) => {
            // Handle errors from the file searching step.
            eprintln!("❌ Error: {}", e);
        }
    }
}
