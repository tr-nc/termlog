mod log_parser;

use std::ffi::CString;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

mod log_processor {
    use super::*;
    use crate::log_parser::{LogItem, process_delta};
    use memmap2::MmapOptions;

    /// Memory-maps a file, processes the appended content, and returns parsed log items.
    pub fn map_and_process_delta(
        file_path: &Path,
        prev_len: u64,
        cur_len: u64,
    ) -> io::Result<Vec<LogItem>> {
        let file = File::open(file_path)?;
        // Safety: We map the file read-only. The length is based on a recent stat call.
        // Even if the file is truncated between stat and mmap, we handle the slice
        // bounds carefully below.
        let mmap = unsafe { MmapOptions::new().len(cur_len as usize).map(&file)? };

        let start = prev_len as usize;
        let end = cur_len as usize;

        // Ensure slice bounds are valid for the mapped region.
        let end = end.min(mmap.len());
        let start = start.min(end);

        let delta_bytes = &mmap[start..end];

        if delta_bytes.is_empty() {
            return Ok(Vec::new());
        }

        // Use lossy conversion as log files might occasionally have invalid UTF-8 sequences.
        let delta_str = String::from_utf8_lossy(delta_bytes);

        // Process the string delta to get structured log items.
        let log_items = process_delta(&delta_str);

        Ok(log_items)
    }
}

mod file_finder {
    use super::*;

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
                            return None;
                        }
                    }
                    Some(path)
                })
            })
            .collect();

        if live_log_files.is_empty() {
            return Err("No live log files found in the directory.".to_string());
        }

        live_log_files.sort();
        Ok(live_log_files.pop().unwrap())
    }
}

// --- Module: File Metadata (Unchanged) ---
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

    #[cfg(target_os = "macos")]
    pub fn stat_path(path: &Path) -> io::Result<MetaSnap> {
        use libc::{stat as stat_t, stat};

        let cpath = CString::new(path.to_str().unwrap())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

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

    pub fn has_changed(prev: &Option<MetaSnap>, cur: &MetaSnap) -> bool {
        match prev {
            None => true,
            Some(p) => p.len != cur.len || p.mtime != cur.mtime || p.ctime != cur.ctime,
        }
    }
}

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
                match log_processor::map_and_process_delta(
                    &latest_file_path,
                    last_len,
                    current_meta.len,
                ) {
                    Ok(log_items) => {
                        if !log_items.is_empty() {
                            for item in log_items {
                                println!("{:#?}", item);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("\n❌ Error processing file delta: {}\n", e);
                    }
                }
                last_len = current_meta.len;
            }

            prev_meta = Some(current_meta);
        }

        thread::sleep(poll_interval);
    }
}
