use std::path::PathBuf;
use std::env;

/// Parses the output of `ffmpeg -version` into:
/// - (clean_version_string, short_display_badge)
///   e.g. ("8.1.3", "FFmpeg 8.x Engine")
pub fn parse_ffmpeg_version(output: &str) -> (String, String) {
    let first_line = output.lines().next().unwrap_or("").trim();
    if let Some(rest) = first_line.strip_prefix("ffmpeg version ") {
        let raw_ver = rest.split_whitespace().next().unwrap_or("");
        let clean_ver = raw_ver.trim_start_matches(['n', 'v']);
        let major = clean_ver.split('.').next().unwrap_or("");
        if let Ok(maj_num) = major.parse::<u32>() {
            return (clean_ver.to_string(), format!("FFmpeg {}.x Engine", maj_num));
        }
    }
    ("8.1".to_string(), "FFmpeg 8.x Engine".to_string())
}

#[derive(Debug, Clone)]
pub struct FFmpegBinaries {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub ffplay: Option<PathBuf>,
}

impl FFmpegBinaries {
    /// Locates ffmpeg, ffprobe, and ffplay binaries strictly within the project root
    /// or application bundle resources on macOS.
    pub fn locate() -> anyhow::Result<Self> {
        let exe_ext = if cfg!(windows) { ".exe" } else { "" };
        let ffmpeg_name = format!("ffmpeg{}", exe_ext);
        let ffprobe_name = format!("ffprobe{}", exe_ext);
        let ffplay_name = format!("ffplay{}", exe_ext);

        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let exe_dir = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let search_dirs = vec![
            cwd.clone(),
            cwd.join("bin"),
            exe_dir.clone(),
            exe_dir.join("bin"),
            exe_dir.join(".."),
            exe_dir.join("../.."),
            // macOS App Bundle standard paths
            exe_dir.join("../Resources"),
            exe_dir.join("../Resources/bin"),
            exe_dir.join("../MacOS"),
            #[cfg(debug_assertions)]
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ];

        let find_binary = |name: &str| -> Option<PathBuf> {
            for dir in &search_dirs {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate.canonicalize().unwrap_or(candidate));
                }
            }
            None
        };

        let ffmpeg = find_binary(&ffmpeg_name)
            .ok_or_else(|| anyhow::anyhow!("Could not find {} in project root path", ffmpeg_name))?;

        let ffprobe = find_binary(&ffprobe_name)
            .ok_or_else(|| anyhow::anyhow!("Could not find {} in project root path", ffprobe_name))?;

        let ffplay = find_binary(&ffplay_name);

        Ok(Self {
            ffmpeg,
            ffprobe,
            ffplay,
        })
    }

    /// Queries `ffmpeg -version` to get the version string and UI badge.
    pub fn detect_version(&self) -> (String, String) {
        use std::process::{Command, Stdio};

        let mut cmd = Command::new(&self.ffmpeg);
        cmd.arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        crate::set_no_window(&mut cmd);

        if let Ok(output) = cmd.output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return parse_ffmpeg_version(&stdout);
        }

        ("8.1".to_string(), "FFmpeg 8.x Engine".to_string())
    }
}
