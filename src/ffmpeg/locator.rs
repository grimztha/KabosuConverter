use std::path::PathBuf;
use std::env;

#[derive(Debug, Clone)]
pub struct FFmpegBinaries {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub ffplay: Option<PathBuf>,
}

impl FFmpegBinaries {
    /// Detects ffmpeg, ffprobe, and ffplay binaries strictly within the project root path.
    /// Does NOT search system PATH or use system-installed versions.
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
            exe_dir.join(".."),
            exe_dir.join("../.."),
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
}
