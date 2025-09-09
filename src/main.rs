use std::fs;
use std::path::{Path, PathBuf};

/// Finds the most recently modified log file that does not have a numeric suffix.
/// This function remains unchanged from the original implementation.
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
                        if base_name[last_dot_pos + 1..].parse::<u32>().is_ok() {
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

use std::ffi::CString;
use std::os::raw::c_char;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TimeSpec {
    sec: i64,
    nsec: i64,
}

#[derive(Clone, Debug)]
struct MetaSnap {
    len: u64,
    mtime: TimeSpec,
    ctime: TimeSpec,
}

#[cfg(target_os = "macos")]
fn stat_path(path: &Path) -> std::io::Result<MetaSnap> {
    use libc::{stat as stat_t, stat};

    let cpath = CString::new(path.to_str().unwrap())?;
    let mut st: stat_t = unsafe { std::mem::zeroed() };
    let rc = unsafe { stat(cpath.as_ptr() as *const c_char, &mut st as *mut stat_t) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }

    let m = TimeSpec {
        sec: st.st_mtime as i64,
        nsec: st.st_mtime_nsec as i64,
    };
    let c = TimeSpec {
        sec: st.st_ctime as i64,
        nsec: st.st_ctime_nsec as i64,
    };

    Ok(MetaSnap {
        len: st.st_size as u64,
        mtime: m,
        ctime: c,
    })
}

fn changed(prev: &Option<MetaSnap>, cur: &MetaSnap) -> bool {
    match prev {
        None => true,
        Some(p) => p.len != cur.len || p.mtime != cur.mtime || p.ctime != cur.ctime,
    }
}

use std::fs::File;
use std::io::{self, Write};

fn map_and_print_delta(file_path: &Path, prev_len: u64, cur_len: u64) -> io::Result<()> {
    use memmap2::MmapOptions;

    // Open file read-only
    let file = File::open(file_path)?;
    // Safety: mapping read-only, size might have grown; we rely on cur_len as snapshot from stat.
    // We map the whole file of current length and then slice.
    let mmap = unsafe { MmapOptions::new().len(cur_len as usize).map(&file)? };

    let start = prev_len as usize;
    let end = cur_len as usize;
    if end > mmap.len() || start > end {
        // Guard: if the file changed between stat and mapping, just clamp
        let clamped_start = start.min(mmap.len());
        let clamped_end = end.min(mmap.len());
        if clamped_end > clamped_start {
            let delta = &mmap[clamped_start..clamped_end];
            print_delta(delta)?;
        }
        return Ok(());
    }

    let delta = &mmap[start..end];
    if !delta.is_empty() {
        print_delta(delta)?;
    }
    Ok(())
}

fn print_delta(delta: &[u8]) -> io::Result<()> {
    // Try UTF-8 first (typical logs). Fall back to lossy.
    match std::str::from_utf8(delta) {
        Ok(s) => {
            print!("{}", s);
        }
        Err(_) => {
            // If binary or partial UTF-8, print lossy to avoid panics.
            let s = String::from_utf8_lossy(delta);
            print!("{}", s);
        }
    }
    // Ensure immediate flush so tail-like behavior
    io::stdout().flush()
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
    println!(
        "🔍 Searching for the latest live log in: {}",
        log_dir_path.display()
    );

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

    let mut prev_meta: Option<MetaSnap> = None;
    let mut last_len: u64 = 0;

    loop {
        let cur = match stat_path(&latest_file_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("stat error: {}", e);
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
        };

        if changed(&prev_meta, &cur) {
            // Report meta change (optional)
            println!(
                "meta changed: len={} mtime={:?} ctime={:?}",
                cur.len, cur.mtime, cur.ctime
            );

            // If file truncated, reset last_len
            if cur.len < last_len {
                // Optional: indicate truncation
                eprintln!("⚠️ File was truncated. Resetting offset.");
                last_len = 0;
            }

            // Only output delta when file length grew
            if cur.len > last_len {
                if let Err(e) = map_and_print_delta(&latest_file_path, last_len, cur.len) {
                    eprintln!("delta read error: {}", e);
                }
                last_len = cur.len;
            }

            prev_meta = Some(cur);
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
