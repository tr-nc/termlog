use std::fs;
use std::path::PathBuf;

fn main() {
    // --- 1. Find the user's home directory ---
    // We use the `dirs` crate to get the home directory path in a cross-platform way.
    // It returns an `Option<PathBuf>`, so we handle the case where it might not be found.
    let home_dir = match dirs::home_dir() {
        Some(path) => path,
        None => {
            eprintln!("Error: Could not determine the home directory.");
            // Exit if we can't find the home directory.
            std::process::exit(1);
        }
    };

    // --- 2. Construct the full path to the target log directory ---
    // We join the home directory path with the specific subdirectory you provided.
    let log_dir_path = home_dir.join("Library/Application Support/DouyinAR/Logs/previewLog");

    println!("🔍 Searching for .log files in: {}", log_dir_path.display());
    println!("---------------------------------------------------");

    // --- 3. Read the contents of the directory ---
    // `fs::read_dir` returns a `Result`, which we handle in case the directory
    // doesn't exist or we don't have permission to read it.
    let entries = match fs::read_dir(&log_dir_path) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("Error: Failed to read directory '{}'. Reason: {}", log_dir_path.display(), e);
            std::process::exit(1);
        }
    };

    let mut log_files_found = 0;

    // --- 4. Iterate, filter, and print log filenames ---
    // We loop through each entry found in the directory.
    for entry in entries {
        // The iterator yields `Result<DirEntry, io::Error>`, so we handle potential errors.
        if let Ok(entry) = entry {
            let path = entry.path();

            // We only care about entries that are files and have a ".log" extension.
            if path.is_file() {
                if let Some(extension) = path.extension() {
                    if extension == "log" {
                        // `path.file_name()` gives us just the filename part of the path.
                        if let Some(file_name) = path.file_name() {
                            // Print the filename to the terminal.
                            // `to_string_lossy()` is a safe way to convert OS-specific strings.
                            println!("📄 Found log file: {}", file_name.to_string_lossy());
                            log_files_found += 1;
                        }
                    }
                }
            }
        }
    }

    println!("---------------------------------------------------");
    if log_files_found == 0 {
        println!("✅ Search complete. No .log files were found.");
    } else {
        println!("✅ Search complete. Found {} log file(s).", log_files_found);
    }
}
