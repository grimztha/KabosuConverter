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
    /// Locates the local FFmpeg suite binaries (ffmpeg, ffprobe, ffplay).
    /// Searches local application storage, the working directory, and the project root.
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

        // macOS App Bundle standard paths
        search_dirs.push(exe_dir.join("../Resources"));
        search_dirs.push(exe_dir.join("../Resources/bin"));
        search_dirs.push(exe_dir.join("../MacOS"));

        #[cfg(debug_assertions)]
        search_dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

        let find_binary = |name: &str| -> Option<PathBuf> {
            // Check designated search directories
            for dir in &search_dirs {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate.canonicalize().unwrap_or(candidate));
                }
            }

            None
        };

        let ffmpeg = find_binary(&ffmpeg_name)
            .ok_or_else(|| anyhow::anyhow!("Could not find {} in app-local storage or project root path", ffmpeg_name))?;

        let ffprobe = find_binary(&ffprobe_name)
            .ok_or_else(|| anyhow::anyhow!("Could not find {} in app-local storage or project root path", ffprobe_name))?;

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

    /// Probes available hardware acceleration encoders on this machine.
    pub fn detect_hardware_accel(&self) -> (crate::ffmpeg::HwAccel, bool, String) {
        use std::process::{Command, Stdio};

        #[cfg(target_os = "macos")]
        {
            return (crate::ffmpeg::HwAccel::VideoToolbox, true, "Apple VideoToolbox".to_string());
        }

        #[cfg(not(target_os = "macos"))]
        {
            // Probe NVIDIA NVENC first
            let mut cmd = Command::new(&self.ffmpeg);
            cmd.args(["-f", "lavfi", "-i", "nullsrc=s=256x256:d=0.04", "-c:v", "h264_nvenc", "-f", "null", "-"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            crate::set_no_window(&mut cmd);
            if let Ok(status) = cmd.status() {
                if status.success() {
                    return (crate::ffmpeg::HwAccel::Nvenc, true, "NVIDIA NVENC".to_string());
                }
            }

            // Probe Intel QSV
            let mut cmd = Command::new(&self.ffmpeg);
            cmd.args(["-f", "lavfi", "-i", "nullsrc=s=256x256:d=0.04", "-c:v", "h264_qsv", "-f", "null", "-"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            crate::set_no_window(&mut cmd);
            if let Ok(status) = cmd.status() {
                if status.success() {
                    return (crate::ffmpeg::HwAccel::Qsv, true, "Intel QuickSync".to_string());
                }
            }

            // Probe AMD AMF
            let mut cmd = Command::new(&self.ffmpeg);
            cmd.args(["-f", "lavfi", "-i", "nullsrc=s=256x256:d=0.04", "-c:v", "h264_amf", "-f", "null", "-"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            crate::set_no_window(&mut cmd);
            if let Ok(status) = cmd.status() {
                if status.success() {
                    return (crate::ffmpeg::HwAccel::Amf, true, "AMD AMF".to_string());
                }
            }

            (crate::ffmpeg::HwAccel::Cpu, false, "CPU Software".to_string())
        }
    }
}
