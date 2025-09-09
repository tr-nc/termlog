use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

mod file_finder {
    use super::*;

    /// Finds the most recently modified log file that does not have a numeric suffix (e.g., ".1.log").
    pub fn find_latest_live_log(log_dir: &Path) -> Result<PathBuf, String> {
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
                    if !file_name.ends_with(".log") {
                        return None;
                    }

                    let base_name = file_name.strip_suffix(".log").unwrap();
                    if let Some(last_dot_pos) = base_name.rfind('.') {
                        let suffix = &base_name[last_dot_pos + 1..];
                        if suffix.parse::<u32>().is_ok() {
                            return None; // Exclude files with numeric suffixes like ".1", ".2"
                        }
                    }
                    Some(path)
                })
            })
            .collect();

        if live_log_files.is_empty() {
            return Err("No live log files found in the directory.".to_string());
        }

        // Sort to get a consistent, albeit simple, ordering.
        live_log_files.sort();
        Ok(live_log_files.pop().unwrap())
    }
}

// --- Module: File Metadata ---
mod metadata {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TimeSpec {
        pub sec: i64,
        pub nsec: i64,
    }

    #[derive(Clone, Debug)]
    pub struct MetaSnap {
        pub len: u64,
        pub mtime: TimeSpec,
        pub ctime: TimeSpec,
    }

    /// Gets a file's metadata snapshot using the `libc::stat` call for high-resolution timestamps.
    #[cfg(target_os = "macos")]
    pub fn stat_path(path: &Path) -> io::Result<MetaSnap> {
        use libc::{stat as stat_t, stat};

        let cpath = CString::new(path.to_str().unwrap())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        // Safety: `st` is zeroed, and we're passing a valid C-string pointer.
        let mut st: stat_t = unsafe { std::mem::zeroed() };
        if unsafe { stat(cpath.as_ptr(), &mut st) } != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(MetaSnap {
            len: st.st_size as u64,
            mtime: TimeSpec {
                sec: st.st_mtime as i64,
                nsec: st.st_mtime_nsec as i64,
            },
            ctime: TimeSpec {
                sec: st.st_ctime as i64,
                nsec: st.st_ctime_nsec as i64,
            },
        })
    }

    /// Returns true if the file's metadata (size or timestamps) has changed.
    pub fn has_changed(prev: &Option<MetaSnap>, cur: &MetaSnap) -> bool {
        match prev {
            None => true,
            Some(p) => p.len != cur.len || p.mtime != cur.mtime || p.ctime != cur.ctime,
        }
    }
}

// --- Module: Delta Content Printer ---
mod delta_printer {
    use super::*;
    use memmap2::MmapOptions;

    /// Memory-maps a file and prints the content that has been appended since the last check.
    ///
    /// # Safety
    /// The `mmap` call is unsafe because file size can change between `stat` and `mmap`.
    /// This is handled by opening the file read-only and carefully slicing the mapped
    /// region within the bounds of the last known length.
    pub fn map_and_print_delta(file_path: &Path, prev_len: u64, cur_len: u64) -> io::Result<()> {
        let file = File::open(file_path)?;
        let mmap = unsafe { MmapOptions::new().len(cur_len as usize).map(&file)? };

        let start = prev_len as usize;
        let end = cur_len as usize;

        // Clamp slice to actual mapped length in case the file was truncated
        // between the `stat` and `mmap` calls.
        let end = end.min(mmap.len());
        let start = start.min(end);

        let delta = &mmap[start..end];
        if !delta.is_empty() {
            print_bytes(delta)?;
        }

        Ok(())
    }

    /// Prints a byte slice to stdout, using lossy UTF-8 conversion as a fallback.
    fn print_bytes(delta: &[u8]) -> io::Result<()> {
        match std::str::from_utf8(delta) {
            Ok(s) => print!("{}", s),
            Err(_) => print!("{}", String::from_utf8_lossy(delta)),
        }
        // Flush to ensure tail-like behavior.
        io::stdout().flush()
    }
}

// --- Main Application ---
fn main() {
    let log_dir_path = match dirs::home_dir() {
        Some(path) => path.join("Library/Application Support/DouyinAR/Logs/previewLog"),
        None => {
            eprintln!("Error: Could not determine the home directory.");
            std::process::exit(1);
        }
    };

    println!("🔍 Monitoring directory: {}", log_dir_path.display());

    let latest_file_path = match file_finder::find_latest_live_log(&log_dir_path) {
        Ok(path) => {
            println!("✅ Found log file: {}", path.display());
            path
        }
        Err(e) => {
            eprintln!("❌ Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut prev_meta: Option<metadata::MetaSnap> = None;
    let mut last_len: u64 = 0;
    let poll_interval = Duration::from_millis(50);

    println!("🚀 Starting to monitor file for changes...\n---");

    loop {
        let current_meta = match metadata::stat_path(&latest_file_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("⚠️ Error stating file: {}", e);
                thread::sleep(poll_interval);
                continue;
            }
        };

        if metadata::has_changed(&prev_meta, &current_meta) {
            if current_meta.len < last_len {
                eprintln!("\n⚠️ File was truncated. Resetting read offset.\n");
                last_len = 0;
            }

            if current_meta.len > last_len {
                if let Err(e) = delta_printer::map_and_print_delta(
                    &latest_file_path,
                    last_len,
                    current_meta.len,
                ) {
                    eprintln!("\n❌ Error reading file delta: {}\n", e);
                }
                last_len = current_meta.len;
            }

            prev_meta = Some(current_meta);
        }

        thread::sleep(poll_interval);
    }
}
