use std::env;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

pub use crate::sanitize::{validate_stream_url, validate_youtube_url};

/// Finds the bundled or local yt-dlp binary.
/// Checks the app's local data directory, the current working directory, and the project root.
pub fn find_ytdlp_binary() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };

    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));

    let mut search_dirs = Vec::new();

    // 1. Primary app directory (portable `<exe_dir>/bin`, `%LOCALAPPDATA%\Kabosu\bin`, or `~/Library/Application Support/Kabosu/bin`)
    search_dirs.push(crate::settings::get_app_dir().join("bin"));
    if let Some(local_app) = dirs::data_local_dir() {
        search_dirs.push(local_app.join(crate::settings::APP_NAME).join("bin"));
        for legacy in crate::settings::LEGACY_APP_NAMES {
            search_dirs.push(local_app.join(legacy).join("bin"));
        }
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

    None
}

/// Queries `yt-dlp --version` to extract the release tag and formatted UI badge.
/// e.g. ("2026.11.23", "yt-dlp 2026.11.23 (Local Engine)")
pub fn detect_version(bin: &Path) -> (String, String) {
    let mut cmd = Command::new(bin);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    crate::set_no_window(&mut cmd);

    if let Ok(output) = cmd.output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let ver = stdout.trim();
        if !ver.is_empty() {
            return (ver.to_string(), format!("yt-dlp {} (Local Engine)", ver));
        }
    }

    ("2026.11.23".to_string(), "yt-dlp 2026.11.23 (Local Engine)".to_string())
}

/// JavaScript runtimes enabled for yt-dlp's External JS Challenge Solver (EJS).
/// Starting in 2026 releases, yt-dlp only enables `deno` by default; enabling all supported
/// runtimes lets yt-dlp automatically fall back to Node (v22+), QuickJS, or Bun if installed.
const SUPPORTED_JS_RUNTIMES: &[&str] = &["deno", "node", "quickjs", "bun"];

/// Prefix printed via `--print after_move:KABOSU_FILEPATH:%(filepath)s` for unambiguous path capture.
pub const PRINTED_FILEPATH_PREFIX: &str = "KABOSU_FILEPATH:";

fn apply_js_runtime_args(cmd: &mut Command) {
    for runtime in SUPPORTED_JS_RUNTIMES {
        cmd.arg("--js-runtimes").arg(runtime);
    }
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
    crate::sanitize::validate_stream_url(trimmed)?;

    let mut cmd = Command::new(ytdlp_bin);
    cmd.arg("--dump-single-json")
        .arg("--no-playlist")
        .arg("--no-warnings");
    apply_js_runtime_args(&mut cmd);
    cmd.arg("--").arg(trimmed);

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
                    // Security check: validate format_id contains only safe yt-dlp selector characters
                    if !crate::sanitize::validate_ytdlp_format_spec(&f.format_id) {
                        continue;
                    }

                    let vcodec = f.vcodec.as_deref().unwrap_or("video");
                    let ext = match f.ext.as_deref() {
                        Some("mp4") | Some("mkv") | Some("webm") | Some("mov") | Some("avi") => f.ext.as_deref().unwrap(),
                        _ => "mp4",
                    };
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
    crate::sanitize::sanitize_filename_stem(name)
}

/// Builds the yt-dlp output filename template, defaulting to `%(title)s [%(id)s].%(ext)s`.
pub fn build_output_template(custom_filename: Option<&str>) -> String {
    const DEFAULT_TEMPLATE: &str = "%(title)s [%(id)s].%(ext)s";

    let Some(custom) = custom_filename.map(str::trim).filter(|s| !s.is_empty()) else {
        return DEFAULT_TEMPLATE.to_string();
    };

    if custom.contains("%(ext)s") {
        let mut sanitized: String = custom
            .chars()
            .map(|c| {
                if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        sanitized = sanitized
            .trim_matches(|c: char| c.is_whitespace() || c == '.')
            .to_string();
        if sanitized.is_empty() {
            return DEFAULT_TEMPLATE.to_string();
        }
        if crate::sanitize::is_windows_reserved_name(&sanitized) {
            return format!("_{}", sanitized);
        }
        return sanitized;
    }

    let clean = sanitize_filename_stem(custom);
    let stem = Path::new(&clean)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&clean);
    format!("{}.%(ext)s", stem)
}

/// Parses a `download:<percent>|<speed>|<eta>|<size>` progress line emitted by yt-dlp.
pub fn parse_progress_line(line: &str) -> Option<YtDlpProgress> {
    let payload = line.strip_prefix("download:")?;
    let parts: Vec<&str> = payload.split('|').collect();
    if parts.len() < 4 {
        return None;
    }

    let pct_str = parts[0].trim().trim_end_matches('%');
    let pct = pct_str.parse::<f32>().unwrap_or(0.0).clamp(0.0, 100.0);

    let clean_metric = |raw: &str| -> String {
        let t = raw.trim();
        if t.is_empty()
            || t.eq_ignore_ascii_case("NA")
            || t.eq_ignore_ascii_case("N/A")
            || t.eq_ignore_ascii_case("Unknown")
            || t.eq_ignore_ascii_case("Unknown B/s")
        {
            String::new()
        } else {
            t.to_string()
        }
    };

    let speed = clean_metric(parts[1]);
    let eta = clean_metric(parts[2]);
    let size = clean_metric(parts[3]);

    Some(YtDlpProgress {
        percent: pct,
        speed,
        eta,
        size,
        status: format!("Downloading: {:.1}%", pct),
    })
}

/// Parses a line of yt-dlp stdout (from `--print after_move:KABOSU_FILEPATH:%(filepath)s` or plain `after_move:filepath`)
/// into a resolved file path, distinguishing internal yt-dlp log lines from actual target paths even when titles start with `[`.
pub fn parse_downloaded_filepath(line: &str, dest_dir: &Path) -> Option<PathBuf> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("download:") {
        return None;
    }

    if let Some(explicit) = trimmed.strip_prefix(PRINTED_FILEPATH_PREFIX) {
        let candidate = explicit.trim();
        if candidate.is_empty() {
            return None;
        }
        let p = PathBuf::from(candidate);
        return Some(if p.is_absolute() { p } else { dest_dir.join(p) });
    }

    let is_ytdlp_log = trimmed.starts_with("[download]")
        || trimmed.starts_with("[info]")
        || trimmed.starts_with("[youtube")
        || trimmed.starts_with("[jsc")
        || trimmed.starts_with("[Merger]")
        || trimmed.starts_with("[ExtractAudio]")
        || trimmed.starts_with("[Fixup")
        || trimmed.starts_with("[ffmpeg]")
        || trimmed.starts_with("[Metadata]")
        || trimmed.starts_with("[MoveFiles]")
        || trimmed.starts_with("[VideoConvertor]")
        || trimmed.starts_with("[VideoRemuxer]")
        || trimmed.starts_with("[generic]")
        || trimmed.starts_with("[twitter]")
        || trimmed.starts_with("[facebook]")
        || trimmed.starts_with("Deleting original file ")
        || trimmed.starts_with("WARNING:")
        || trimmed.starts_with("ERROR:");

    if !is_ytdlp_log {
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
    crate::sanitize::validate_stream_url(trimmed_url)?;

    // Validate format specifier
    if !format_opt.is_audio_only && !crate::sanitize::validate_ytdlp_format_spec(&format_opt.format_spec) {
        return Err(anyhow!("Invalid yt-dlp format specification: {}", format_opt.format_spec));
    }

    // Validate custom filename if provided
    if let Some(cf) = custom_filename {
        crate::sanitize::validate_subprocess_arg(cf)?;
    }

    // Validate destination path
    crate::sanitize::validate_path(dest_dir)?;

    // Ensure the destination folder exists on disk before starting
    let _ = std::fs::create_dir_all(dest_dir);

    let mut cmd = Command::new(ytdlp_bin);
    cmd.arg("--newline")
        .arg("--no-quiet")
        .arg("--progress")
        .arg("--no-playlist");
    apply_js_runtime_args(&mut cmd);
    cmd.arg("--progress-template")
        .arg("download:download:%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s|%(progress._total_bytes_str,progress._total_bytes_estimate_str)s")
        .arg("--print")
        .arg(format!("after_move:{}%(filepath)s", PRINTED_FILEPATH_PREFIX));

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
        let mut explicit_printed_path: Option<PathBuf> = None;
        let stdout_reader = BufReader::new(stdout);
        for line in stdout_reader.lines().map_while(Result::ok) {
            if cancel_clone.load(Ordering::SeqCst) {
                break;
            }
            let trimmed = line.trim();
            if !trimmed.starts_with("download:") && !trimmed.starts_with(PRINTED_FILEPATH_PREFIX) {
                on_log_out(line.clone());
            }

            // Capture after_move:filepath output (prefer explicit sentinel over heuristic lines)
            if trimmed.starts_with(PRINTED_FILEPATH_PREFIX) {
                if let Some(resolved) = parse_downloaded_filepath(trimmed, &dest_dir_clone) {
                    explicit_printed_path = Some(resolved);
                }
            } else if explicit_printed_path.is_none() {
                if let Some(resolved) = parse_downloaded_filepath(trimmed, &dest_dir_clone) {
                    printed_path = Some(resolved);
                }
            }

            if let Some(progress) = parse_progress_line(trimmed) {
                on_progress(progress);
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

        // Take ownership of the child handle before waiting so we never hold the mutex across blocking I/O.
        let mut child_opt = child_holder_inner
            .lock()
            .ok()
            .and_then(|mut lock| lock.take());

        // Kill before joining stderr_thread on cancel so the stderr pipe closes instead of hanging.
        if cancel_clone.load(Ordering::SeqCst) {
            if let Some(ref mut child) = child_opt {
                let _ = child.kill();
            }
        }

        let err_lines = stderr_thread.join().unwrap_or_default();
        let exit_status = child_opt.and_then(|mut child| child.wait().ok());

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
        if let Some(path) = explicit_printed_path.or(printed_path) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_youtube_url() {
        assert!(validate_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(validate_youtube_url("https://youtu.be/dQw4w9WgXcQ"));
        assert!(!validate_youtube_url("https://example.com/not_youtube"));
        assert!(!validate_youtube_url("http://www.youtube.com/watch?v=dQw4w9WgXcQ"));
    }

    #[test]
    fn test_validate_stream_url() {
        assert!(validate_stream_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ").is_ok());
        assert!(validate_stream_url("https://example.com/stream.m3u8").is_ok());
        assert!(validate_stream_url("-h").is_err());
        assert!(validate_stream_url("https://example.com/video;calc.exe").is_err());
    }

    #[test]
    fn test_validate_format_spec() {
        assert!(crate::sanitize::validate_ytdlp_format_spec("bv*+ba/b"));
        assert!(crate::sanitize::validate_ytdlp_format_spec("137+140"));
        assert!(!crate::sanitize::validate_ytdlp_format_spec("bv; reboot"));
    }

    #[test]
    fn test_sanitize_filename_stem() {
        assert_eq!(sanitize_filename_stem("My Video: Title"), "My Video_ Title");
        assert_eq!(sanitize_filename_stem("CON"), "_CON");
        assert_eq!(sanitize_filename_stem(""), "downloaded_media");
    }
}


