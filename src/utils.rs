use std::path::{Path, PathBuf};

/// Retrieve text from the system clipboard on Windows, or return None.
pub fn get_clipboard_string() -> Option<String> {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        extern "system" {
            fn OpenClipboard(hWndNewOwner: *mut std::ffi::c_void) -> i32;
            fn CloseClipboard() -> i32;
            fn GetClipboardData(uFormat: u32) -> *mut std::ffi::c_void;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalLock(hMem: *mut std::ffi::c_void) -> *const u16;
            fn GlobalUnlock(hMem: *mut std::ffi::c_void) -> i32;
            fn GlobalSize(hMem: *mut std::ffi::c_void) -> usize;
        }
        const CF_UNICODETEXT: u32 = 13;

        unsafe {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                let h_mem = GetClipboardData(CF_UNICODETEXT);
                let mut result = None;
                if !h_mem.is_null() {
                    let total_bytes = GlobalSize(h_mem);
                    let max_chars = (total_bytes / 2).min(32_768);
                    if max_chars > 0 {
                        let ptr = GlobalLock(h_mem);
                        if !ptr.is_null() {
                            let mut len = 0usize;
                            while len < max_chars && *ptr.add(len) != 0 {
                                len += 1;
                            }
                            let slice = std::slice::from_raw_parts(ptr, len);
                            let text = String::from_utf16_lossy(slice).trim().to_string();
                            GlobalUnlock(h_mem);
                            if !text.is_empty() {
                                result = Some(text);
                            }
                        }
                    }
                }
                CloseClipboard();
                if result.is_some() {
                    return result;
                }
            }
        }
    }
    None
}

/// Format a byte count into a human-readable size string (e.g. "12.34 MB", "1.50 GB").
pub fn format_bytes_fast(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else if bytes > 0 {
        format!("{} B", bytes)
    } else {
        "Unknown size".to_string()
    }
}

/// Recursively collect media files from a directory up to a max depth.
pub fn collect_media_files_from_dir(dir: &Path, depth: usize, out: &mut Vec<PathBuf>, exts: &[&str]) {
    if depth > 4 {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                collect_media_files_from_dir(&child, depth + 1, out, exts);
            } else if child.is_file() {
                if let Some(ext) = child.extension().and_then(|e| e.to_str()) {
                    if exts.iter().any(|&m| m.eq_ignore_ascii_case(ext)) {
                        out.push(child);
                    }
                }
            }
        }
    }
}

/// Normalize and clean a path for Windows Explorer / system openers.
pub fn normalize_path_for_explorer(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let resolved = std::fs::canonicalize(&abs).unwrap_or(abs);
    let s = resolved.to_string_lossy();
    let stripped = if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{}", unc)
    } else if let Some(local) = s.strip_prefix(r"\\?\") {
        local.to_string()
    } else {
        s.to_string()
    };
    let mut clean = stripped.replace('/', r"\");
    while clean.len() > 3 && clean.ends_with('\\') {
        clean.pop();
    }
    PathBuf::from(clean)
}

/// Open a directory in Windows Explorer (or xdg-open on Linux).
pub fn open_folder_in_explorer(folder: &Path) {
    let _ = std::fs::create_dir_all(folder);
    let clean_dir = normalize_path_for_explorer(folder);
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(&clean_dir)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(&clean_dir)
            .spawn();
    }
}

/// Open a file's containing folder in Windows Explorer with the file highlighted/selected.
pub fn reveal_in_explorer(file_path: &Path) {
    let clean = normalize_path_for_explorer(file_path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if clean.exists() && clean.is_file() {
            let path_str = clean.to_string_lossy().replace('"', "");
            // On Windows, explorer requires /select,"PATH" without quoting the /select switch itself.
            // .raw_arg() passes this verbatim so Explorer highlights the file properly.
            let spawned = std::process::Command::new("explorer")
                .raw_arg(format!("/select,\"{}\"", path_str))
                .spawn();
            if spawned.is_ok() {
                return;
            }
        }
        let target_dir = if clean.is_dir() {
            clean.clone()
        } else if let Some(parent) = clean.parent() {
            parent.to_path_buf()
        } else {
            clean.clone()
        };
        open_folder_in_explorer(&target_dir);
    }
    #[cfg(not(windows))]
    {
        let target_dir = if clean.is_dir() {
            clean.clone()
        } else if let Some(parent) = clean.parent() {
            parent.to_path_buf()
        } else {
            clean.clone()
        };
        open_folder_in_explorer(&target_dir);
    }
}

/// Plays an audible system notification chime when an export or download task finishes.
pub fn play_notification_sound() {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        extern "system" {
            fn MessageBeep(uType: u32) -> i32;
        }
        unsafe {
            // 0x00000040 = MB_ICONASTERISK (Windows notification chime)
            MessageBeep(0x00000040);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("afplay")
            .arg("/System/Library/Sounds/Glass.aiff")
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("paplay")
            .arg("/usr/share/sounds/freedesktop/stereo/complete.oga")
            .spawn();
    }
}
