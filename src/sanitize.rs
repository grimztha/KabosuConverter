//! Input validation and sanitization for file paths, URLs, timecodes, and CLI arguments.

use std::path::{Component, Path, PathBuf};
use thiserror::Error;

/// Device names reserved by Windows that cannot be used as file or folder stems.
pub const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Upper bound on accepted file path lengths.
pub const MAX_PATH_LENGTH: usize = 1024;

/// Maximum safe filename stem length in UTF-8 bytes (leaves room for suffixes and extensions within the 255-byte OS limit).
pub const MAX_FILENAME_STEM_LENGTH: usize = 200;

const MAX_TIMECODE_SECONDS: f64 = 24.0 * 60.0 * 60.0;

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
    NumericOutOfRange {
        value: String,
        min: String,
        max: String,
    },

    #[error("Invalid stream URL: {0}")]
    InvalidUrl(String),

    #[error("Invalid yt-dlp format specification: '{0}'")]
    InvalidFormatSpec(String),

    #[error("Subprocess argument contains null byte or control characters: '{0}'")]
    InvalidSubprocessArg(String),
}

/// Returns true if the filename stem matches a reserved Windows device name (case-insensitive).
pub fn is_windows_reserved_name(name: &str) -> bool {
    let clean = name.trim().trim_end_matches('.');
    let stem = clean.split('.').next().unwrap_or(clean);
    WINDOWS_RESERVED_NAMES
        .iter()
        .any(|&reserved| reserved.eq_ignore_ascii_case(stem))
}

/// Cleans a filename stem so it is safe on Windows, macOS, and Linux.
///
/// Replaces forbidden path and control characters, collapses repeated underscores,
/// enforces a safe UTF-8 byte cap, and prefixes reserved Windows device names.
pub fn sanitize_filename_stem(name: &str) -> String {
    let mut sanitized = String::with_capacity(name.len().min(MAX_FILENAME_STEM_LENGTH));
    let mut prev_underscore = false;

    for ch in name.trim().chars() {
        let forbidden =
            matches!(ch, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || ch.is_control();
        let mapped = if forbidden { '_' } else { ch };

        if mapped == '_' {
            if prev_underscore {
                continue;
            }
            prev_underscore = true;
        } else {
            prev_underscore = false;
        }
        sanitized.push(mapped);
    }

    let mut trimmed = sanitized
        .trim_matches(|c: char| c == '_' || c == '.' || c.is_whitespace())
        .to_string();

    // Clamp by UTF-8 byte length so multi-byte characters never break 255-byte filesystem limits.
    if trimmed.len() > MAX_FILENAME_STEM_LENGTH {
        let mut end = MAX_FILENAME_STEM_LENGTH;
        while end > 0 && !trimmed.is_char_boundary(end) {
            end -= 1;
        }
        trimmed.truncate(end);
        trimmed = trimmed.trim_end_matches(['_', '.']).to_string();
    }

    if trimmed.is_empty() {
        return "downloaded_media".to_string();
    }

    if is_windows_reserved_name(&trimmed) {
        return format!("_{}", trimmed);
    }

    trimmed
}

/// Validates a path before passing it to the filesystem or external CLI tools.
pub fn validate_path<P: AsRef<Path>>(path: P) -> Result<(), SanitizeError> {
    let s = path.as_ref().to_string_lossy();

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

    // Split on both '/' and '\\' so Windows reserved device names are caught on Unix CI too.
    for part in s.split(['/', '\\']) {
        let trimmed = part.trim();
        if !trimmed.is_empty() && is_windows_reserved_name(trimmed) {
            return Err(SanitizeError::ReservedWindowsName(trimmed.to_string()));
        }
    }

    Ok(())
}

/// Returns true if `path` is absolute on the current OS or starts with a Windows drive prefix (e.g. `C:\`).
pub fn is_absolute_path(path: &Path) -> bool {
    if path.is_absolute() {
        return true;
    }
    let s = path.to_string_lossy();
    let bytes = s.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

/// Verifies that `target` resolves inside `allowed_dir` without escaping via `..` traversal.
pub fn ensure_path_within_dir(target: &Path, allowed_dir: &Path) -> Result<PathBuf, SanitizeError> {
    validate_path(target)?;
    validate_path(allowed_dir)?;

    #[cfg(not(windows))]
    let (target_buf, allowed_buf) = (
        normalize_separators(target),
        normalize_separators(allowed_dir),
    );
    #[cfg(windows)]
    let (target_buf, allowed_buf) = (target.to_path_buf(), allowed_dir.to_path_buf());

    let cwd = || std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let abs_allowed = if is_absolute_path(&allowed_buf) {
        allowed_buf.clone()
    } else {
        cwd().join(&allowed_buf)
    };

    let abs_target = if is_absolute_path(&target_buf) {
        target_buf
    } else if target_buf.starts_with(&allowed_buf) {
        if is_absolute_path(&allowed_buf) {
            target_buf
        } else {
            cwd().join(target_buf)
        }
    } else {
        abs_allowed.join(target_buf)
    };

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

#[cfg(not(windows))]
fn normalize_separators(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.contains('\\') {
        PathBuf::from(s.replace('\\', "/"))
    } else {
        path.to_path_buf()
    }
}

/// Resolves `.` and `..` components purely in memory without touching the filesystem.
pub fn normalize_lexically(path: &Path) -> PathBuf {
    #[cfg(not(windows))]
    let normalized = normalize_separators(path);
    #[cfg(windows)]
    let normalized = path.to_path_buf();

    let mut out = PathBuf::new();
    for comp in normalized.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// Checks that a CLI argument contains no null bytes or unexpected control characters.
pub fn validate_subprocess_arg(arg: &str) -> Result<(), SanitizeError> {
    if arg.contains('\0') {
        return Err(SanitizeError::ContainsNull);
    }
    if arg
        .chars()
        .any(|c| c.is_control() && c != '\t' && c != '\n')
    {
        return Err(SanitizeError::InvalidSubprocessArg(arg.to_string()));
    }
    Ok(())
}

fn is_valid_youtube_id(id: &str, exact_len: Option<usize>, min_len: usize) -> bool {
    let len_ok = match exact_len {
        Some(len) => id.len() == len,
        None => id.len() >= min_len,
    };
    len_ok
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|param| param.strip_prefix(key))
}

/// Returns true only when `url` matches a known HTTPS YouTube video, embed, music, or playlist URL.
pub fn validate_youtube_url(url: &str) -> bool {
    let url_lower = url.trim().to_lowercase();
    if !url_lower.starts_with("https://") {
        return false;
    }

    // Path-based video IDs (youtu.be and /embed/)
    for prefix in [
        "https://youtu.be/",
        "https://www.youtube.com/embed/",
        "https://youtube.com/embed/",
    ] {
        if let Some(rest) = url_lower.strip_prefix(prefix) {
            let video_id = rest.split(['/', '?', '&']).next().unwrap_or(rest);
            return is_valid_youtube_id(video_id, Some(11), 11);
        }
    }

    // Query-based watch URLs (?v=VIDEO_ID)
    for prefix in [
        "https://www.youtube.com/watch?",
        "https://youtube.com/watch?",
        "https://music.youtube.com/watch?",
    ] {
        if let Some(query) = url_lower.strip_prefix(prefix) {
            return query_param(query, "v=")
                .is_some_and(|id| is_valid_youtube_id(id, Some(11), 11));
        }
    }

    // Playlist URLs (?list=LIST_ID)
    for prefix in [
        "https://www.youtube.com/playlist?",
        "https://youtube.com/playlist?",
    ] {
        if let Some(query) = url_lower.strip_prefix(prefix) {
            return query_param(query, "list=").is_some_and(|id| is_valid_youtube_id(id, None, 12));
        }
    }

    false
}

/// Validates a stream URL before passing it to `yt-dlp`.
///
/// Requires `http://` or `https://` and blocks flag injection, control characters,
/// whitespace, and dangerous metacharacters while permitting standard query separators like `&`.
pub fn validate_stream_url(url: &str) -> Result<(), SanitizeError> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(SanitizeError::InvalidUrl(
            "Stream URL cannot be empty".to_string(),
        ));
    }
    if trimmed.len() > 2048 {
        return Err(SanitizeError::InvalidUrl(
            "Stream URL is too long".to_string(),
        ));
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
    if trimmed.chars().any(char::is_whitespace) {
        return Err(SanitizeError::InvalidUrl(
            "Invalid stream URL: input cannot contain whitespace".to_string(),
        ));
    }

    // Allow `&` for standard multi-parameter query strings; yt-dlp is invoked directly without a shell.
    const DISALLOWED_URL_CHARS: &[char] = &[';', '|', '$', '`', '<', '>', '"', '\'', '{', '}', '^'];
    for &ch in DISALLOWED_URL_CHARS {
        if trimmed.contains(ch) {
            return Err(SanitizeError::InvalidUrl(format!(
                "URL contains disallowed character '{}'",
                ch
            )));
        }
    }

    let after_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .ok_or_else(|| {
            SanitizeError::InvalidUrl("URL must use http:// or https:// protocol".to_string())
        })?;

    if after_scheme.is_empty() || after_scheme.starts_with('/') {
        return Err(SanitizeError::InvalidUrl(
            "URL must include a valid host".to_string(),
        ));
    }

    Ok(())
}

/// Checks that a `yt-dlp` format selector contains only valid selector syntax characters.
pub fn validate_ytdlp_format_spec(spec: &str) -> bool {
    let trimmed = spec.trim();
    if trimmed.is_empty() || trimmed.len() > 256 {
        return false;
    }
    trimmed.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '+' | '/'
                    | '['
                    | ']'
                    | '('
                    | ')'
                    | '<'
                    | '>'
                    | '='
                    | '*'
                    | '-'
                    | '_'
                    | '.'
                    | '?'
                    | '~'
                    | ','
                    | ':'
            )
    })
}

/// Validates a timecode string and ensures it falls within `[0, 24h]`.
pub fn validate_timecode(tc: &str) -> Result<(), SanitizeError> {
    let tc = tc.trim();
    if tc.is_empty() {
        return Err(SanitizeError::InvalidTimecode(
            "Timecode is empty".to_string(),
        ));
    }

    let seconds = parse_timecode_to_seconds(tc)
        .ok_or_else(|| SanitizeError::InvalidTimecode(tc.to_string()))?;

    if !(0.0..=MAX_TIMECODE_SECONDS).contains(&seconds) {
        return Err(SanitizeError::TimecodeOutOfRange(seconds));
    }

    Ok(())
}

/// Parses `HH:MM:SS(.ms)`, `MM:SS(.ms)`, or plain seconds into a finite non-negative `f64`.
pub fn parse_timecode_to_seconds(tc: &str) -> Option<f64> {
    let tc = tc.trim();
    if tc.is_empty() {
        return None;
    }

    let parts: Vec<&str> = tc.split(':').collect();
    match parts.len() {
        1 => {
            let secs: f64 = parts[0].trim().parse().ok()?;
            (secs.is_finite() && secs >= 0.0).then_some(secs)
        }
        2 => {
            let min: f64 = parts[0].trim().parse().ok()?;
            let sec: f64 = parts[1].trim().parse().ok()?;
            if min.is_finite() && sec.is_finite() && min >= 0.0 && (0.0..60.0).contains(&sec) {
                Some(min * 60.0 + sec)
            } else {
                None
            }
        }
        3 => {
            let h: f64 = parts[0].trim().parse().ok()?;
            let m: f64 = parts[1].trim().parse().ok()?;
            let s: f64 = parts[2].trim().parse().ok()?;
            if h.is_finite()
                && m.is_finite()
                && s.is_finite()
                && h >= 0.0
                && (0.0..60.0).contains(&m)
                && (0.0..60.0).contains(&s)
            {
                Some(h * 3600.0 + m * 60.0 + s)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Formats seconds as `HH:MM:SS`.
pub fn format_duration_hms(seconds: f64) -> String {
    let total_secs = if seconds.is_finite() {
        seconds.max(0.0) as u64
    } else {
        0
    };
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// Checks an FFmpeg filter expression for null bytes and unsafe metacharacters.
pub fn validate_ffmpeg_filter(filter: &str) -> Result<(), SanitizeError> {
    if filter.contains('\0') {
        return Err(SanitizeError::ContainsNull);
    }

    const DANGEROUS_FILTER_CHARS: &[char] = &[';', '&', '|', '$', '`', '{', '}', '!'];
    for &ch in DANGEROUS_FILTER_CHARS {
        if filter.contains(ch) {
            return Err(SanitizeError::DangerousFilterChar(ch));
        }
    }

    Ok(())
}

/// Checks that a numeric value is finite and within `[min, max]`.
pub fn validate_numeric_range(value: f64, min: f64, max: f64) -> Result<(), SanitizeError> {
    if !value.is_finite() || value < min || value > max {
        return Err(SanitizeError::NumericOutOfRange {
            value: value.to_string(),
            min: min.to_string(),
            max: max.to_string(),
        });
    }
    Ok(())
}

/// Masks the middle of a string, leaving `visible` characters at each end for safe logging.
pub fn mask_sensitive(value: &str, visible: usize) -> String {
    let value_len = value.chars().count();
    if value_len <= visible * 2 {
        return value.to_string();
    }
    let first: String = value.chars().take(visible).collect();
    let last: String = value.chars().skip(value_len - visible).collect();
    format!("{}...{}", first, last)
}

/// Redacts sensitive query parameter values for safe logging.
pub fn redact_url_query(url: &str) -> String {
    const SENSITIVE_SUBSTRINGS: &[&str] = &[
        "token", "sig", "key", "auth", "secret", "session", "pass", "cookie",
    ];

    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };

    let sanitized_params: Vec<String> = query
        .split('&')
        .map(|param| {
            if let Some((k, _)) = param.split_once('=') {
                let k_lower = k.to_ascii_lowercase();
                if SENSITIVE_SUBSTRINGS.iter().any(|&s| k_lower.contains(s)) {
                    return format!("{k}=[REDACTED]");
                }
            }
            param.to_string()
        })
        .collect();

    format!("{base}?{}", sanitized_params.join("&"))
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
    fn test_sanitize_filename_stem_multibyte_byte_cap() {
        let long_cjk = "動画テスト".repeat(60);
        let sanitized = sanitize_filename_stem(&long_cjk);
        assert!(sanitized.len() <= MAX_FILENAME_STEM_LENGTH);
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
        assert!(validate_path("/var/log/aux/output.mp4").is_err());
    }

    #[test]
    fn test_ensure_path_within_dir() {
        let base_win = Path::new("C:\\Users\\test\\output");
        let safe_win = Path::new("C:\\Users\\test\\output\\video.mp4");
        assert!(ensure_path_within_dir(safe_win, base_win).is_ok());

        let traversal_win = Path::new("C:\\Users\\test\\output\\..\\windows\\system32.exe");
        assert!(ensure_path_within_dir(traversal_win, base_win).is_err());

        let base_unix = Path::new("/home/user/output");
        let safe_unix = Path::new("/home/user/output/video.mp4");
        assert!(ensure_path_within_dir(safe_unix, base_unix).is_ok());

        let traversal_unix = Path::new("/home/user/output/../system/secret");
        assert!(ensure_path_within_dir(traversal_unix, base_unix).is_err());

        let base_rel = Path::new("output_folder");
        let safe_rel = Path::new("output_folder/video.mp4");
        assert!(ensure_path_within_dir(safe_rel, base_rel).is_ok());

        let traversal_rel = Path::new("output_folder/../secret.txt");
        assert!(ensure_path_within_dir(traversal_rel, base_rel).is_err());
    }

    #[test]
    fn test_validate_youtube_url_valid() {
        assert!(validate_youtube_url(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(validate_youtube_url(
            "https://youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(validate_youtube_url("https://youtu.be/dQw4w9WgXcQ"));
        assert!(validate_youtube_url(
            "https://www.youtube.com/embed/dQw4w9WgXcQ"
        ));
        assert!(validate_youtube_url(
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(validate_youtube_url(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42s"
        ));
    }

    #[test]
    fn test_validate_youtube_url_invalid() {
        assert!(!validate_youtube_url("https://example.com"));
        assert!(!validate_youtube_url(
            "http://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(!validate_youtube_url(""));
        assert!(!validate_youtube_url(
            "https://www.youtube.com/watch?v=tooshort"
        ));
        assert!(!validate_youtube_url(
            "https://www.youtube.com/watch?v=toolongvideoidhere"
        ));
        assert!(!validate_youtube_url("https://youtu.be/invalid"));
        assert!(!validate_youtube_url(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ;rm=rf"
        ));
        assert!(!validate_youtube_url("https://vimeo.com/12345"));
        assert!(!validate_youtube_url(
            "https://dailymotion.com/video/abc123"
        ));
    }

    #[test]
    fn test_validate_stream_url() {
        assert!(validate_stream_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ").is_ok());
        assert!(validate_stream_url(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42s&list=PL1234567890ab"
        )
        .is_ok());
        assert!(validate_stream_url("https://example.com/stream.m3u8").is_ok());
        assert!(validate_stream_url("-flag").is_err());
        assert!(validate_stream_url("https://example.com/video; rm -rf /").is_err());
        assert!(validate_stream_url("ftp://example.com").is_err());
        assert!(validate_stream_url("https://").is_err());
        assert!(validate_stream_url("http:///path").is_err());
    }

    #[test]
    fn test_validate_ytdlp_format_spec() {
        assert!(validate_ytdlp_format_spec("bv*+ba/b"));
        assert!(validate_ytdlp_format_spec(
            "bv*[height<=1080]+ba/b[height<=1080]"
        ));
        assert!(validate_ytdlp_format_spec(
            "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best"
        ));
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
        assert!(parse_timecode_to_seconds("inf").is_none());
        assert!(parse_timecode_to_seconds("NaN").is_none());
    }

    #[test]
    fn test_mask_sensitive() {
        assert_eq!(mask_sensitive("secret", 0), "...");
        assert_eq!(mask_sensitive("short", 5), "short");
        assert_eq!(mask_sensitive("my_password", 3), "my_...ord");
    }

    #[test]
    fn test_redact_url_query() {
        let url = "https://example.com/video?token=supersecret123&access_token=abc&api_key=xyz&client_secret=s3cr3t&v=1080p";
        let redacted = redact_url_query(url);
        assert_eq!(
            redacted,
            "https://example.com/video?token=[REDACTED]&access_token=[REDACTED]&api_key=[REDACTED]&client_secret=[REDACTED]&v=1080p"
        );
    }
}
