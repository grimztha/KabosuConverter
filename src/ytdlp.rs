use std::env;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// Locate yt-dlp binary following the priority order:
/// 1. App-Local Folder (%LOCALAPPDATA%\<app_name>\bin or macOS ~/Library/Application Support/<app_name>/bin)
/// 2. Standalone / portable directory or project root
/// 3. System $PATH fallback (for power users maintaining their own builds)
pub fn find_ytdlp_binary() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut search_dirs = Vec::new();

    // 1. App-Local OS folder (e.g. %LOCALAPPDATA%\AnotherEncoder\bin)
    if let Some(local_app) = dirs::data_local_dir() {
        search_dirs.push(local_app.join(crate::settings::APP_NAME).join("bin"));
    }

    // 2. Standalone / portable & project directory
    search_dirs.push(cwd.clone());
    search_dirs.push(cwd.join("bin"));
    search_dirs.push(exe_dir.clone());
    search_dirs.push(exe_dir.join("bin"));
    search_dirs.push(exe_dir.join(".."));
    search_dirs.push(exe_dir.join("../.."));

    #[cfg(debug_assertions)]
    search_dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

    for dir in &search_dirs {
        let candidate = dir.join(exe_name);
        if candidate.is_file() {
            return Some(candidate.canonicalize().unwrap_or(candidate));
        }
    }

    // 3. System $PATH (Optional fallback)
    if let Some(system_candidate) = crate::binaries::DependencyManager::find_in_system_path(exe_name) {
        return Some(system_candidate);
    }

    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtDlpFormatItem {
    pub format_id: String,
    pub ext: Option<String>,
    pub resolution: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub filesize: Option<u64>,
    pub filesize_approx: Option<u64>,
    pub tbr: Option<f32>,
    pub format_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtDlpMediaInfo {
    pub id: Option<String>,
    pub title: Option<String>,
    pub uploader: Option<String>,
    pub channel: Option<String>,
    pub duration: Option<f64>,
    pub duration_string: Option<String>,
    pub thumbnail: Option<String>,
    pub webpage_url: Option<String>,
    pub formats: Option<Vec<YtDlpFormatItem>>,
}

#[derive(Debug, Clone)]
pub struct DownloadFormatOption {
    pub display_label: String,
    pub format_spec: String,
    pub is_audio_only: bool,
    pub audio_ext: Option<String>,
    pub container_ext: String,
}

/// Run yt-dlp --dump-single-json to extract title, duration, author, and available stream formats.
#[allow(clippy::vec_init_then_push)]
pub fn discover_media(ytdlp_bin: &Path, url: &str) -> Result<(YtDlpMediaInfo, Vec<DownloadFormatOption>)> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Stream URL cannot be empty"));
    }
    if trimmed.starts_with('-') {
        return Err(anyhow!("Invalid stream URL: input cannot begin with a command flag"));
    }

    let mut cmd = Command::new(ytdlp_bin);
    cmd.arg("--dump-single-json")
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--")
        .arg(trimmed);

    let output = crate::ffmpeg::spawn_with_output(cmd, "yt-dlp")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("yt-dlp detection failed: {}", stderr.trim()));
    }

    let info: YtDlpMediaInfo = serde_json::from_slice(&output.stdout)
        .map_err(|e| anyhow!("Failed to parse stream metadata JSON: {}", e))?;

    let mut options = Vec::new();

    // 1. High-level curated smart presets
    options.push(DownloadFormatOption {
        display_label: "[Video] Best Quality (Auto Best Video + Best Audio, Recommended)".to_string(),
        format_spec: "bv*+ba/b".to_string(),
        is_audio_only: false,
        audio_ext: None,
        container_ext: "mp4".to_string(),
    });

    options.push(DownloadFormatOption {
        display_label: "[Video] 4K UHD 2160p (Best Video <= 2160p + Best Audio)".to_string(),
        format_spec: "bv*[height<=2160]+ba/b[height<=2160]".to_string(),
        is_audio_only: false,
        audio_ext: None,
        container_ext: "mp4".to_string(),
    });

    options.push(DownloadFormatOption {
        display_label: "[Video] 1440p QHD (Best Video <= 1440p + Best Audio)".to_string(),
        format_spec: "bv*[height<=1440]+ba/b[height<=1440]".to_string(),
        is_audio_only: false,
        audio_ext: None,
        container_ext: "mp4".to_string(),
    });

    options.push(DownloadFormatOption {
        display_label: "[Video] 1080p Full HD (Best Video <= 1080p + Best Audio)".to_string(),
        format_spec: "bv*[height<=1080]+ba/b[height<=1080]".to_string(),
        is_audio_only: false,
        audio_ext: None,
        container_ext: "mp4".to_string(),
    });

    options.push(DownloadFormatOption {
        display_label: "[Video] 720p HD (Best Video <= 720p + Best Audio)".to_string(),
        format_spec: "bv*[height<=720]+ba/b[height<=720]".to_string(),
        is_audio_only: false,
        audio_ext: None,
        container_ext: "mp4".to_string(),
    });

    options.push(DownloadFormatOption {
        display_label: "[Video] 480p SD (Fast / Mobile Compact)".to_string(),
        format_spec: "bv*[height<=480]+ba/b[height<=480]".to_string(),
        is_audio_only: false,
        audio_ext: None,
        container_ext: "mp4".to_string(),
    });

    options.push(DownloadFormatOption {
        display_label: "[Audio] MP3 (High Quality 320 kbps)".to_string(),
        format_spec: "ba/b".to_string(),
        is_audio_only: true,
        audio_ext: Some("mp3".to_string()),
        container_ext: "mp3".to_string(),
    });

    options.push(DownloadFormatOption {
        display_label: "[Audio] M4A / AAC (Best Source Audio)".to_string(),
        format_spec: "ba/b".to_string(),
        is_audio_only: true,
        audio_ext: Some("m4a".to_string()),
        container_ext: "m4a".to_string(),
    });

    options.push(DownloadFormatOption {
        display_label: "[Audio] FLAC (Lossless Audio)".to_string(),
        format_spec: "ba/b".to_string(),
        is_audio_only: true,
        audio_ext: Some("flac".to_string()),
        container_ext: "flac".to_string(),
    });

    options.push(DownloadFormatOption {
        display_label: "[Audio] WAV (Lossless PCM)".to_string(),
        format_spec: "ba/b".to_string(),
        is_audio_only: true,
        audio_ext: Some("wav".to_string()),
        container_ext: "wav".to_string(),
    });

    // 2. Discover distinct video-only stream resolutions if provided
    if let Some(ref formats) = info.formats {
        let mut seen_heights = std::collections::HashSet::new();
        for f in formats.iter().rev() {
            if let Some(h) = f.height {
                if h >= 360 && seen_heights.insert(h) {
                    let vcodec = f.vcodec.as_deref().unwrap_or("video");
                    let ext = f.ext.as_deref().unwrap_or("mp4");
                    let fps_str = f.fps.map(|fps| format!("{}fps", fps.round())).unwrap_or_default();
                    let label = format!("[Video] Stream {}p {} ({}) — Format ID {}", h, fps_str, vcodec, f.format_id);
                    options.push(DownloadFormatOption {
                        display_label: label,
                        format_spec: format!("{}+ba/b", f.format_id),
                        is_audio_only: false,
                        audio_ext: None,
                        container_ext: ext.to_string(),
                    });
                }
            }
        }
    }

    Ok((info, options))
}

#[derive(Debug, Clone, Default)]
pub struct YtDlpProgress {
    pub percent: f32,
    pub speed: String,
    pub eta: String,
    pub size: String,
    pub status: String,
}

pub struct DownloadJob {
    pub cancel_flag: Arc<AtomicBool>,
}

/// Sanitize filename characters forbidden in Windows / NTFS filesystems.
pub fn sanitize_filename_stem(name: &str) -> String {
    let mut sanitized = name.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_");
    sanitized = sanitized.trim_matches(|c: char| c.is_whitespace() || c == '.').to_string();
    if sanitized.is_empty() {
        "downloaded_media".to_string()
    } else {
        sanitized
    }
}

/// Build the output filename template for yt-dlp.
/// If user supplies a custom filename, it uses that sanitized stem and appends `%(ext)s`.
/// If no custom name is supplied or is blank, it falls back to standard `%(title)s [%(id)s].%(ext)s`.
pub fn build_output_template(custom_filename: Option<&str>) -> String {
    if let Some(custom) = custom_filename.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if custom.contains("%(ext)s") {
            let sanitized = custom.replace(['\\', '/', ':'], "_");
            return sanitized;
        }
        let clean = sanitize_filename_stem(custom);
        let stem = Path::new(&clean)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&clean);
        format!("{}.%(ext)s", stem)
    } else {
        "%(title)s [%(id)s].%(ext)s".to_string()
    }
}

/// Parses a line of yt-dlp stdout (from `--print after_move:filepath`) into a resolved file path.
/// Distinguishes between internal yt-dlp progress tags and actual target paths, even when video titles start with `[`.
pub fn parse_downloaded_filepath(line: &str, dest_dir: &Path) -> Option<PathBuf> {
    let trimmed = line.trim();
    let is_ytdlp_tag = trimmed.starts_with("[download]")
        || trimmed.starts_with("[info]")
        || trimmed.starts_with("[youtube]")
        || trimmed.starts_with("[Merger]")
        || trimmed.starts_with("[ExtractAudio]")
        || trimmed.starts_with("[Fixup")
        || trimmed.starts_with("[ffmpeg]");

    if !trimmed.is_empty() && !trimmed.starts_with("download:") && !is_ytdlp_tag {
        let p = PathBuf::from(trimmed);
        let resolved = if p.is_absolute() {
            p.clone()
        } else {
            dest_dir.join(&p)
        };
        if resolved.exists() || p.is_absolute() || p.extension().is_some() {
            return Some(resolved);
        }
    }
    None
}

/// Start an asynchronous download with real-time progress parsing and log streaming.
#[allow(clippy::too_many_arguments)]
pub fn start_download(
    ytdlp_bin: &Path,
    ffmpeg_dir: Option<&Path>,
    url: &str,
    format_opt: &DownloadFormatOption,
    dest_dir: &Path,
    custom_filename: Option<&str>,
    child_holder: Arc<Mutex<Option<Child>>>,
    cancel_flag: Arc<AtomicBool>,
    on_progress: impl Fn(YtDlpProgress) + Send + 'static,
    on_log: impl Fn(String) + Send + Sync + 'static,
    on_complete: impl Fn(Result<PathBuf>) + Send + 'static,
) -> Result<DownloadJob> {
    let trimmed_url = url.trim();
    if trimmed_url.is_empty() {
        return Err(anyhow!("Stream URL cannot be empty"));
    }
    if trimmed_url.starts_with('-') {
        return Err(anyhow!("Invalid stream URL: input cannot begin with a command flag"));
    }

    // Ensure the destination folder exists on disk before starting
    let _ = std::fs::create_dir_all(dest_dir);

    let mut cmd = Command::new(ytdlp_bin);
    cmd.arg("--newline")
        .arg("--no-playlist")
        .arg("--progress-template")
        .arg("download:%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s|%(progress._total_bytes_estimate_str)s")
        .arg("--print").arg("after_move:filepath");

    if let Some(f_dir) = ffmpeg_dir {
        cmd.arg("--ffmpeg-location").arg(f_dir);
    }

    if format_opt.is_audio_only {
        let audio_fmt = format_opt.audio_ext.as_deref().unwrap_or("mp3");
        cmd.arg("-f").arg("ba/b")
            .arg("-x")
            .arg("--audio-format").arg(audio_fmt);
        if audio_fmt == "mp3" {
            cmd.arg("--audio-quality").arg("0");
        }
    } else {
        cmd.arg("-f").arg(&format_opt.format_spec)
            .arg("--merge-output-format").arg("mp4");
    }

    // Explicit path and filename template
    cmd.arg("-P").arg(dest_dir);
    let output_template = build_output_template(custom_filename);
    cmd.arg("-o").arg(output_template);
    cmd.arg("--");
    cmd.arg(trimmed_url);

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    crate::set_no_window(&mut cmd);

    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| anyhow!("Failed to capture yt-dlp stdout"))?;
    let stderr = child.stderr.take().ok_or_else(|| anyhow!("Failed to capture yt-dlp stderr"))?;

    let child_holder_inner = child_holder.clone();
    if let Ok(mut lock) = child_holder.lock() {
        *lock = Some(child);
    }

    let cancel_clone = Arc::clone(&cancel_flag);
    let dest_dir_clone = dest_dir.to_path_buf();
    let on_log = Arc::new(on_log);

    thread::spawn(move || {
        let on_log_err = Arc::clone(&on_log);
        let on_log_out = Arc::clone(&on_log);
        let stderr_thread = thread::spawn(move || {
            let reader = BufReader::new(stderr);
            let mut err_lines = Vec::new();
            for line in reader.lines().map_while(Result::ok) {
                on_log_err(format!("[yt-dlp stderr] {}", line));
                let trimmed = line.trim();
                if trimmed.starts_with("ERROR:") || trimmed.starts_with("error:") || trimmed.contains("Forbidden") {
                    err_lines.push(trimmed.to_string());
                }
            }
            err_lines
        });

        let mut printed_path: Option<PathBuf> = None;
        let stdout_reader = BufReader::new(stdout);
        for line in stdout_reader.lines().map_while(Result::ok) {
            if cancel_clone.load(Ordering::SeqCst) {
                break;
            }
            on_log_out(line.clone());

            let trimmed = line.trim();
            // Capture after_move:filepath output
            if let Some(resolved) = parse_downloaded_filepath(trimmed, &dest_dir_clone) {
                printed_path = Some(resolved);
            }

            if line.starts_with("download:") {
                let parts: Vec<&str> = line.trim_start_matches("download:").split('|').collect();
                if parts.len() >= 4 {
                    let pct_str = parts[0].trim().trim_end_matches('%');
                    let pct = pct_str.parse::<f32>().unwrap_or(0.0);
                    let speed = parts[1].trim().to_string();
                    let eta = parts[2].trim().to_string();
                    let size = parts[3].trim().to_string();

                    on_progress(YtDlpProgress {
                        percent: pct,
                        speed,
                        eta,
                        size,
                        status: format!("Downloading: {:.1}%", pct),
                    });
                }
            } else if line.contains("[Merger]") {
                on_progress(YtDlpProgress {
                    percent: 98.0,
                    speed: "Merging".to_string(),
                    eta: "Almost done".to_string(),
                    size: String::new(),
                    status: "Merging audio and video tracks via FFmpeg...".to_string(),
                });
            } else if line.contains("[ExtractAudio]") {
                on_progress(YtDlpProgress {
                    percent: 95.0,
                    speed: "Extracting".to_string(),
                    eta: "Almost done".to_string(),
                    size: String::new(),
                    status: "Extracting audio format via FFmpeg...".to_string(),
                });
            }
        }

        let err_lines = stderr_thread.join().unwrap_or_default();

        let exit_status = {
            if let Ok(mut lock) = child_holder_inner.lock() {
                lock.as_mut().and_then(|c| c.wait().ok())
            } else {
                None
            }
        };

        if let Ok(mut lock) = child_holder_inner.lock() {
            let _ = lock.take();
        }

        if cancel_clone.load(Ordering::SeqCst) {
            on_complete(Err(anyhow!("Download cancelled by user.")));
            return;
        }

        let is_success = exit_status.map(|s| s.success()).unwrap_or(false);
        if !is_success {
            let error_summary = if !err_lines.is_empty() {
                err_lines.join(" | ")
            } else {
                format!(
                    "yt-dlp download failed with exit code {:?}",
                    exit_status.and_then(|s| s.code())
                )
            };
            on_complete(Err(anyhow!("{}", error_summary)));
            return;
        }

        // Check if after_move printed path exists
        if let Some(path) = printed_path {
            if path.exists() {
                on_complete(Ok(path));
                return;
            }
        }

        // Scan destination directory for most recently modified media file
        if let Ok(entries) = std::fs::read_dir(&dest_dir_clone) {
            let mut newest_file: Option<(PathBuf, std::time::SystemTime)> = None;
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
                    if ["mp4", "mkv", "webm", "mov", "avi", "flv", "mp3", "m4a", "flac", "wav", "aac", "opus"].contains(&ext.as_str()) {
                        if let Ok(meta) = p.metadata() {
                            if let Ok(mod_time) = meta.modified() {
                                if newest_file.as_ref().map(|(_, t)| mod_time > *t).unwrap_or(true) {
                                    newest_file = Some((p, mod_time));
                                }
                            }
                        }
                    }
                }
            }
            if let Some((p, mod_time)) = newest_file {
                if let Ok(elapsed) = mod_time.elapsed() {
                    if elapsed.as_secs() < 600 {
                        on_complete(Ok(p));
                        return;
                    }
                }
            }
        }

        on_complete(Err(anyhow!(
            "Download completed without error, but no saved output file was found in {}",
            dest_dir_clone.display()
        )));
    });

    Ok(DownloadJob {
        cancel_flag,
    })
}

