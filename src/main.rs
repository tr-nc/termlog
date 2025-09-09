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
struct Timespec {
    sec: i64,
    nsec: i64,
}

#[derive(Clone, Debug)]
struct MetaSnap {
    len: u64,
    mtime: Timespec,
    ctime: Timespec,
}

#[cfg(target_os = "macos")]
fn stat_path(path: &Path) -> std::io::Result<MetaSnap> {
    use libc::{stat as stat_t, stat, timespec};

    let cpath = CString::new(path.to_str().unwrap())?;
    let mut st: stat_t = unsafe { std::mem::zeroed() };
    let rc = unsafe { stat(cpath.as_ptr() as *const c_char, &mut st as *mut stat_t) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // On macOS: st_mtimespec and st_ctimespec exist.
    // let m = unsafe { std::ptr::addr_of!(st.st_mtime).read_unaligned() as timespec };
    // let c = unsafe { std::ptr::addr_of!(st.st_ctime).read_unaligned() as timespec };
    //

    let m = Timespec {
        sec: st.st_mtime as i64,
        nsec: st.st_mtime_nsec as i64,
    };
    let c = Timespec {
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

    let mut prev: Option<MetaSnap> = None;

    loop {
        let cur = stat_path(&latest_file_path).unwrap();
        if changed(&prev, &cur) {
            println!(
                "meta changed: len={} mtime={:?} ctime={:?}",
                cur.len, cur.mtime, cur.ctime
            );
            prev = Some(cur);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
