//! Input sanitization and validation for subprocess arguments, file paths, URLs, and filters.
//!
//! Provides secure validation and sanitization for all user-supplied values that are passed to
//! external processes (FFmpeg, ffprobe, ffplay, yt-dlp) or used in file system operations.

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Windows reserved device names that cannot be used as filenames or stems.
pub const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Maximum allowed path length across supported platforms.
pub const MAX_PATH_LENGTH: usize = 1024;

/// Maximum allowed filename stem length.
pub const MAX_FILENAME_STEM_LENGTH: usize = 200;

#[derive(Debug, Error, PartialEq)]
pub enum SanitizeError {
    #[error("Path is empty")]
    EmptyPath,

    #[error("Path exceeds maximum length of {max} characters (got {actual})")]
    PathTooLong { actual: usize, max: usize },

    #[error("Path contains null character")]
    ContainsNull,

    #[error("Value contains forbidden control character")]
    ControlCharacters,

    #[error("Path or filename contains reserved Windows device name: '{0}'")]
    ReservedWindowsName(String),

    #[error("Path traversal detected: target '{target}' is outside destination '{allowed_dir}'")]
    PathTraversal { target: String, allowed_dir: String },

    #[error("Invalid timecode format: '{0}'")]
    InvalidTimecode(String),

    #[error("Timecode out of range: {0} seconds (max 24 hours)")]
    TimecodeOutOfRange(f64),

    #[error("Filter contains dangerous character '{0}'")]
    DangerousFilterChar(char),

    #[error("Value {value} out of range [{min}, {max}]")]
    NumericOutOfRange { value: String, min: String, max: String },

    #[error("Invalid stream URL: {0}")]
    InvalidUrl(String),

    #[error("Invalid yt-dlp format specification: '{0}'")]
    InvalidFormatSpec(String),

    #[error("Subprocess argument contains null byte or control characters: '{0}'")]
    InvalidSubprocessArg(String),
}

/// Checks if a string (or stem) matches a Windows reserved device name case-insensitively.
pub fn is_windows_reserved_name(name: &str) -> bool {
    let clean = name.trim().trim_end_matches('.');
    let stem = clean.split('.').next().unwrap_or(clean);
    WINDOWS_RESERVED_NAMES.iter().any(|&r| r.eq_ignore_ascii_case(stem))
}

/// Validates and sanitizes a filename stem for safe use in output paths across all platforms.
///
/// Strips characters forbidden in Windows / NTFS and POSIX filesystems, collapses redundant
/// separators/underscores, prevents Windows reserved device names, and clamps length to prevent
/// buffer/path truncation.
pub fn sanitize_filename_stem(name: &str) -> String {
    // Replace characters that are problematic in filenames across platforms
    let mut sanitized = name
        .replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_")
        .trim()
        .to_string();

    // Replace control characters with underscores
    sanitized = sanitized
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();

    // Collapse multiple underscores
    while sanitized.contains("__") {
        sanitized = sanitized.replace("__", "_");
    }

    // Trim leading/trailing underscores, whitespace, and dots
    sanitized = sanitized
        .trim_matches(|c: char| c == '_' || c == '.' || c.is_whitespace())
        .to_string();

    // Clamp length to prevent OS filename limits (255 bytes max on most filesystems)
    if sanitized.chars().count() > MAX_FILENAME_STEM_LENGTH {
        sanitized = sanitized.chars().take(MAX_FILENAME_STEM_LENGTH).collect();
        sanitized = sanitized.trim_end_matches(|c: char| c == '_' || c == '.').to_string();
    }

    // If the name is empty, return a safe placeholder
    if sanitized.is_empty() {
        return "downloaded_media".to_string();
    }

    // If the name is a reserved Windows device name, prefix with an underscore
    if is_windows_reserved_name(&sanitized) {
        return format!("_{}", sanitized);
    }

    sanitized
}

/// Validates a file path for safe use as a subprocess argument or filesystem path.
///
/// Ensures:
/// 1. Path is not empty.
/// 2. Path length does not exceed maximum bounds.
/// 3. Path contains no null bytes (`\0`) or control characters.
/// 4. Path components do not use reserved Windows device names (`CON`, `PRN`, `AUX`, etc.).
pub fn validate_path<P: AsRef<Path>>(path: P) -> Result<(), SanitizeError> {
    let p = path.as_ref();
    let s = p.to_string_lossy();

    if s.trim().is_empty() {
        return Err(SanitizeError::EmptyPath);
    }

    if s.len() > MAX_PATH_LENGTH {
        return Err(SanitizeError::PathTooLong {
            actual: s.len(),
            max: MAX_PATH_LENGTH,
        });
    }

    if s.contains('\0') {
        return Err(SanitizeError::ContainsNull);
    }

    if s.chars().any(|c| c.is_control() && c != '\t') {
        return Err(SanitizeError::ControlCharacters);
    }

    // Check components for Windows reserved device names
    for comp in p.components() {
        let comp_str = comp.as_os_str().to_string_lossy();
        if is_windows_reserved_name(&comp_str) {
            return Err(SanitizeError::ReservedWindowsName(comp_str.to_string()));
        }
    }

    Ok(())
}

/// Ensures an output path is strictly contained within the allowed output directory,
/// preventing path traversal attacks.
pub fn ensure_path_within_dir(target: &Path, allowed_dir: &Path) -> Result<PathBuf, SanitizeError> {
    validate_path(target)?;
    validate_path(allowed_dir)?;

    let abs_target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        allowed_dir.join(target)
    };

    let abs_allowed = if allowed_dir.is_absolute() {
        allowed_dir.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(allowed_dir)
    };

    // Normalize paths by removing `.` and evaluating `..` lexically
    let norm_target = normalize_lexically(&abs_target);
    let norm_allowed = normalize_lexically(&abs_allowed);

    if !norm_target.starts_with(&norm_allowed) {
        return Err(SanitizeError::PathTraversal {
            target: target.to_string_lossy().to_string(),
            allowed_dir: allowed_dir.to_string_lossy().to_string(),
        });
    }

    Ok(norm_target)
}

/// Normalizes path components lexically without touching the filesystem.
pub fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// Validates a subprocess argument before passing it to `std::process::Command`.
///
/// Ensures no null bytes or raw control characters exist that could be misinterpreted
/// by `CreateProcessW` on Windows or `execvp` on Unix.
pub fn validate_subprocess_arg(arg: &str) -> Result<(), SanitizeError> {
    if arg.contains('\0') {
        return Err(SanitizeError::ContainsNull);
    }
    if arg.chars().any(|c| c.is_control() && c != '\t' && c != '\n') {
        return Err(SanitizeError::InvalidSubprocessArg(arg.to_string()));
    }
    Ok(())
}

/// Validates a YouTube URL format.
///
/// Returns true only if the URL strictly matches a known YouTube URL pattern.
/// Returns false for any non-YouTube URLs, unencrypted http:// schemes, or malformed queries.
pub fn validate_youtube_url(url: &str) -> bool {
    let url_trimmed = url.trim();
    let url_lower = url_trimmed.to_lowercase();

    // Must start with https://
    if !url_lower.starts_with("https://") {
        return false;
    }

    // Short URL: https://youtu.be/VIDEO_ID
    if url_lower.starts_with("https://youtu.be/") {
        let path = &url_lower[17..];
        let video_id = path.split(['/', '?', '&']).next().unwrap_or(path);
        if video_id.len() != 11 {
            return false;
        }
        return video_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    }

    // Standard URL: https://www.youtube.com/watch?v=VIDEO_ID or https://youtube.com/watch?v=VIDEO_ID
    for prefix in &["https://www.youtube.com/watch?", "https://youtube.com/watch?"] {
        if url_lower.starts_with(prefix) {
            let query = &url_lower[prefix.len()..];
            for param in query.split('&') {
                if let Some(vid) = param.strip_prefix("v=") {
                    let video_id = vid.split('&').next().unwrap_or(vid);
                    if video_id.len() == 11 && video_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                        return true;
                    }
                }
            }
            return false;
        }
    }

    // Embed URL: https://www.youtube.com/embed/VIDEO_ID
    for prefix in &["https://www.youtube.com/embed/", "https://youtube.com/embed/"] {
        if url_lower.starts_with(prefix) {
            let path = &url_lower[prefix.len()..];
            let video_id = path.split(['/', '?', '&']).next().unwrap_or(path);
            if video_id.len() != 11 {
                return false;
            }
            return video_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        }
    }

    // Music URL: https://music.youtube.com/watch?v=VIDEO_ID
    if url_lower.starts_with("https://music.youtube.com/watch?") {
        let query = &url_lower["https://music.youtube.com/watch?".len()..];
        for param in query.split('&') {
            if let Some(vid) = param.strip_prefix("v=") {
                let video_id = vid.split('&').next().unwrap_or(vid);
                if video_id.len() == 11 && video_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                    return true;
                }
            }
        }
        return false;
    }

    // Playlist URL: https://www.youtube.com/playlist?list=LIST_ID
    for prefix in &["https://www.youtube.com/playlist?", "https://youtube.com/playlist?"] {
        if url_lower.starts_with(prefix) {
            let query = &url_lower[prefix.len()..];
            for param in query.split('&') {
                if let Some(lid) = param.strip_prefix("list=") {
                    let list_id = lid.split('&').next().unwrap_or(lid);
                    return list_id.len() >= 12 && list_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
                }
            }
            return false;
        }
    }

    false
}

/// Validates a stream URL for safe passing to yt-dlp.
///
/// Ensures the URL starts with https:// (or http:// for localhost), contains no command-line flag
/// indicators (`-`), no control characters, and no shell metacharacters.
pub fn validate_stream_url(url: &str) -> Result<(), SanitizeError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(SanitizeError::InvalidUrl("Stream URL cannot be empty".to_string()));
    }
    if trimmed.len() > 2048 {
        return Err(SanitizeError::InvalidUrl("Stream URL is too long".to_string()));
    }
    if trimmed.starts_with('-') {
        return Err(SanitizeError::InvalidUrl(
            "Invalid stream URL: input cannot begin with a command flag".to_string(),
        ));
    }
    if trimmed.chars().any(|c| c.is_control() || c == '\0') {
        return Err(SanitizeError::InvalidUrl(
            "Invalid stream URL: input cannot contain control characters".to_string(),
        ));
    }
    if trimmed.chars().any(|c| c.is_whitespace()) {
        return Err(SanitizeError::InvalidUrl(
            "Invalid stream URL: input cannot contain whitespace".to_string(),
        ));
    }

    // Reject dangerous shell metacharacters
    let dangerous = [';', '&', '|', '$', '`', '<', '>', '"', '\'', '{', '}', '^'];
    for ch in dangerous {
        if trimmed.contains(ch) {
            return Err(SanitizeError::InvalidUrl(format!(
                "URL contains disallowed character '{}'",
                ch
            )));
        }
    }

    if !trimmed.starts_with("https://") && !trimmed.starts_with("http://") {
        return Err(SanitizeError::InvalidUrl(
            "URL must use http:// or https:// protocol".to_string(),
        ));
    }

    Ok(())
}

/// Validates a yt-dlp format specification string.
///
/// Only permits standard yt-dlp format selector characters:
/// alphanumerics, `+`, `/`, `[`, `]`, `(`, `)`, `<=`, `>=`, `<`, `>`, `=`, `*`, `-`, `_`, `.`, `?`, `~`, `,`.
/// Prevents shell injection or argument injection via format parameters.
pub fn validate_ytdlp_format_spec(spec: &str) -> bool {
    let trimmed = spec.trim();
    if trimmed.is_empty() || trimmed.len() > 256 {
        return false;
    }
    trimmed.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '+' | '/' | '[' | ']' | '(' | ')' | '<' | '>' | '=' | '*' | '-' | '_' | '.' | '?' | '~' | ',' | ':'
            )
    })
}

/// Validates and parses a timecode string for FFmpeg.
///
/// Accepts formats: `HH:MM:SS`, `HH:MM:SS.ms`, `MM:SS`, `MM:SS.ms`, or plain numeric seconds.
/// Rejects negative seconds or durations exceeding 24 hours.
pub fn validate_timecode(tc: &str) -> Result<(), SanitizeError> {
    let tc = tc.trim();
    if tc.is_empty() {
        return Err(SanitizeError::InvalidTimecode("Timecode is empty".to_string()));
    }

    let seconds = parse_timecode_to_seconds(tc)
        .ok_or_else(|| SanitizeError::InvalidTimecode(tc.to_string()))?;

    if seconds < 0.0 || seconds > 24.0 * 60.0 * 60.0 {
        return Err(SanitizeError::TimecodeOutOfRange(seconds));
    }

    Ok(())
}

/// Parses a timecode string to seconds.
///
/// Accepts formats: `HH:MM:SS`, `HH:MM:SS.ms`, `MM:SS`, `MM:SS.ms`, or plain numeric seconds.
pub fn parse_timecode_to_seconds(tc: &str) -> Option<f64> {
    let tc = tc.trim();
    if tc.is_empty() {
        return None;
    }

    // Try plain seconds first
    if let Ok(secs) = tc.parse::<f64>() {
        return if secs >= 0.0 { Some(secs) } else { None };
    }

    let parts: Vec<&str> = tc.split(':').collect();
    match parts.len() {
        2 => {
            let min: f64 = parts[0].parse().ok()?;
            let sec: f64 = parts[1].parse().ok()?;
            if min >= 0.0 && sec >= 0.0 && sec < 60.0 {
                Some(min * 60.0 + sec)
            } else {
                None
            }
        }
        3 => {
            let h: f64 = parts[0].parse().ok()?;
            let m: f64 = parts[1].parse().ok()?;
            let s: f64 = parts[2].parse().ok()?;
            if h >= 0.0 && m >= 0.0 && m < 60.0 && s >= 0.0 && s < 60.0 {
                Some(h * 3600.0 + m * 60.0 + s)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Formats duration in seconds to HH:MM:SS string for display.
pub fn format_duration_hms(seconds: f64) -> String {
    let total_secs = seconds.max(0.0) as u64;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// Validates an FFmpeg filter argument for safe use.
///
/// Ensures the filter string contains only safe filter syntax characters and contains
/// no shell metacharacters or command injection primitives.
pub fn validate_ffmpeg_filter(filter: &str) -> Result<(), SanitizeError> {
    if filter.contains('\0') {
        return Err(SanitizeError::ContainsNull);
    }

    let dangerous = [';', '&', '|', '$', '`', '{', '}', '!'];
    for ch in dangerous {
        if filter.contains(ch) {
            return Err(SanitizeError::DangerousFilterChar(ch));
        }
    }

    Ok(())
}

/// Validates a numeric value is within a safe range for FFmpeg arguments.
pub fn validate_numeric_range(value: f64, min: f64, max: f64) -> Result<(), SanitizeError> {
    if value < min || value > max || value.is_nan() {
        return Err(SanitizeError::NumericOutOfRange {
            value: value.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        });
    }
    Ok(())
}

/// Masks a portion of a string, showing only the first `visible` and last `visible` characters.
/// Useful for hiding secrets or sensitive portions of strings in logs.
pub fn mask_sensitive(value: &str, visible: usize) -> String {
    let value_len = value.chars().count();
    if value_len <= visible * 2 {
        return value.to_string();
    }
    let first: String = value.chars().take(visible).collect();
    let last: String = value.chars().skip(value_len - visible).collect();
    format!("{}...{}", first, last)
}

/// Redacts sensitive query parameters (such as tokens, signatures, keys) from a URL for logging.
pub fn redact_url_query(url: &str) -> String {
    if let Some((base, query)) = url.split_once('?') {
        let sensitive_keys = ["token", "sig", "signature", "key", "auth", "secret", "session", "pass"];
        let mut sanitized_params = Vec::new();
        for param in query.split('&') {
            if let Some((k, _)) = param.split_once('=') {
                if sensitive_keys.iter().any(|&s| k.eq_ignore_ascii_case(s)) {
                    sanitized_params.push(format!("{}=[REDACTED]", k));
                    continue;
                }
            }
            sanitized_params.push(param.to_string());
        }
        format!("{}?{}", base, sanitized_params.join("&"))
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename_stem_basic() {
        assert_eq!(sanitize_filename_stem("my video"), "my video");
        assert_eq!(sanitize_filename_stem("file.txt"), "file.txt");
    }

    #[test]
    fn test_sanitize_filename_stem_special_chars() {
        assert_eq!(sanitize_filename_stem("file:name"), "file_name");
        assert_eq!(sanitize_filename_stem("a/b"), "a_b");
        assert_eq!(sanitize_filename_stem("a\\b"), "a_b");
        assert_eq!(sanitize_filename_stem("a?b*c<d>e|f"), "a_b_c_d_e_f");
    }

    #[test]
    fn test_sanitize_filename_stem_empty() {
        assert_eq!(sanitize_filename_stem(""), "downloaded_media");
        assert_eq!(sanitize_filename_stem("   "), "downloaded_media");
        assert_eq!(sanitize_filename_stem("___"), "downloaded_media");
        assert_eq!(sanitize_filename_stem("..."), "downloaded_media");
    }

    #[test]
    fn test_sanitize_filename_stem_windows_reserved() {
        assert_eq!(sanitize_filename_stem("con"), "_con");
        assert_eq!(sanitize_filename_stem("CON"), "_CON");
        assert_eq!(sanitize_filename_stem("aux.mp4"), "_aux.mp4");
        assert_eq!(sanitize_filename_stem("com1"), "_com1");
        assert_eq!(sanitize_filename_stem("prn"), "_prn");
    }

    #[test]
    fn test_validate_path_basic() {
        assert!(validate_path("C:\\Users\\test\\file.mp4").is_ok());
        assert!(validate_path("/home/user/file.mp4").is_ok());
        assert!(validate_path("relative/path/to/file.mkv").is_ok());
        assert!(validate_path("").is_err());
        assert!(validate_path("a\0b").is_err());
        assert!(validate_path("con.mp4").is_err());
        assert!(validate_path("C:\\folder\\aux\\test.mp4").is_err());
    }

    #[test]
    fn test_ensure_path_within_dir() {
        let base = Path::new("C:\\Users\\test\\output");
        let safe = Path::new("C:\\Users\\test\\output\\video.mp4");
        assert!(ensure_path_within_dir(safe, base).is_ok());

        let traversal = Path::new("C:\\Users\\test\\output\\..\\windows\\system32.exe");
        assert!(ensure_path_within_dir(traversal, base).is_err());
    }

    #[test]
    fn test_validate_youtube_url_valid() {
        assert!(validate_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(validate_youtube_url("https://youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(validate_youtube_url("https://youtu.be/dQw4w9WgXcQ"));
        assert!(validate_youtube_url("https://www.youtube.com/embed/dQw4w9WgXcQ"));
        assert!(validate_youtube_url("https://music.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(validate_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42s"));
    }

    #[test]
    fn test_validate_youtube_url_invalid() {
        assert!(!validate_youtube_url("https://example.com"));
        assert!(!validate_youtube_url("http://www.youtube.com/watch?v=dQw4w9WgXcQ"));
        assert!(!validate_youtube_url(""));
        assert!(!validate_youtube_url("https://www.youtube.com/watch?v=tooshort"));
        assert!(!validate_youtube_url("https://www.youtube.com/watch?v=toolongvideoidhere"));
        assert!(!validate_youtube_url("https://youtu.be/invalid"));
        assert!(!validate_youtube_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ;rm=rf"));
        assert!(!validate_youtube_url("https://vimeo.com/12345"));
        assert!(!validate_youtube_url("https://dailymotion.com/video/abc123"));
    }

    #[test]
    fn test_validate_stream_url() {
        assert!(validate_stream_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ").is_ok());
        assert!(validate_stream_url("https://example.com/stream.m3u8").is_ok());
        assert!(validate_stream_url("-flag").is_err());
        assert!(validate_stream_url("https://example.com/video; rm -rf /").is_err());
        assert!(validate_stream_url("ftp://example.com").is_err());
    }

    #[test]
    fn test_validate_ytdlp_format_spec() {
        assert!(validate_ytdlp_format_spec("bv*+ba/b"));
        assert!(validate_ytdlp_format_spec("bv*[height<=1080]+ba/b[height<=1080]"));
        assert!(validate_ytdlp_format_spec("bestvideo[ext=mp4]+bestaudio[ext=m4a]/best"));
        assert!(validate_ytdlp_format_spec("137+140"));
        assert!(!validate_ytdlp_format_spec("bv; rm -rf /"));
        assert!(!validate_ytdlp_format_spec("bv&calc.exe"));
    }

    #[test]
    fn test_parse_timecode() {
        assert_eq!(parse_timecode_to_seconds("10").unwrap(), 10.0);
        assert_eq!(parse_timecode_to_seconds("00:10").unwrap(), 10.0);
        assert_eq!(parse_timecode_to_seconds("00:00:10").unwrap(), 10.0);
        assert_eq!(parse_timecode_to_seconds("00:01:05").unwrap(), 65.0);
        assert_eq!(parse_timecode_to_seconds("01:00:00").unwrap(), 3600.0);
        assert!(parse_timecode_to_seconds("invalid").is_none());
    }

    #[test]
    fn test_mask_sensitive() {
        assert_eq!(mask_sensitive("secret", 0), "...");
        assert_eq!(mask_sensitive("short", 5), "short");
        assert_eq!(mask_sensitive("my_password", 3), "my_...ord");
    }

    #[test]
    fn test_redact_url_query() {
        let url = "https://example.com/video?token=supersecret123&v=1080p";
        let redacted = redact_url_query(url);
        assert_eq!(redacted, "https://example.com/video?token=[REDACTED]&v=1080p");
    }
}
