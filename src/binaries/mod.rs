pub mod error;
pub mod extract;
pub mod fetcher;
pub mod manifest;
pub mod platform;

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::timeout;

pub use error::BinaryError;
pub use fetcher::DownloadProgress;
pub use platform::PlatformInfo;

#[derive(Debug, Clone)]
pub struct BinaryPaths {
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub ffplay: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DependencyManager {
    bin_dir: PathBuf,
    app_data_dir: PathBuf,
    is_standalone: bool,
    platform: PlatformInfo,
}

impl DependencyManager {
    /// Initializes DependencyManager for the specified application name.
    /// Follows OS conventions:
    /// - Standalone / portable: executable's parent directory (`<exe_dir>/bin`)
    /// - Standard installation: OS local app data folder
    ///   - Windows: `%LOCALAPPDATA%\<app_name>\bin`
    ///   - macOS: `~/Library/Application Support/<app_name>/bin`
    ///   - Linux: `~/.local/share/<app_name>/bin`
    pub fn new(app_name: &str) -> Result<Self, BinaryError> {
        let platform = PlatformInfo::current()?;
        let (app_data_dir, is_standalone) = Self::determine_app_data_dir(app_name);
        let bin_dir = app_data_dir.join("bin");

        std::fs::create_dir_all(&bin_dir)?;

        Ok(Self {
            bin_dir,
            app_data_dir,
            is_standalone,
            platform,
        })
    }

    pub fn bin_dir(&self) -> &Path {
        &self.bin_dir
    }

    pub fn app_data_dir(&self) -> &Path {
        &self.app_data_dir
    }

    pub fn is_standalone(&self) -> bool {
        self.is_standalone
    }

    /// Checks if required binaries (ffmpeg, ffprobe, yt-dlp) exist and are executable
    /// without making any network calls.
    ///
    /// Resolution order:
    /// 1. App-Local Folder (%LOCALAPPDATA%\<app_name>\bin or standalone `<exe_dir>/bin`)
    /// 2. Project root / executable directory
    /// 3. System $PATH (Optional fallback for power users)
    pub async fn check_existing(&self) -> Result<Option<BinaryPaths>, BinaryError> {
        let ffmpeg = self.resolve_binary(self.platform.ffmpeg_filename(), "-version").await;
        let ffprobe = self.resolve_binary(self.platform.ffprobe_filename(), "-version").await;
        let ytdlp = self.resolve_binary(self.platform.ytdlp_filename(), "--version").await;

        if let (Some(ffmpeg), Some(ffprobe), Some(yt_dlp)) = (ffmpeg, ffprobe, ytdlp) {
            let ffplay = self.resolve_binary(self.platform.ffplay_filename(), "-version").await;
            return Ok(Some(BinaryPaths {
                yt_dlp,
                ffmpeg,
                ffprobe,
                ffplay,
            }));
        }

        Ok(None)
    }

    /// Resolves binary paths, downloading and hash-verifying any missing binaries into
    /// the app-local folder.
    pub async fn ensure_dependencies<F>(&self, on_progress: F) -> Result<BinaryPaths, BinaryError>
    where
        F: Fn(&str, DownloadProgress) + Send + Sync + 'static,
    {
        let fetcher = fetcher::BinaryFetcher::new()?;
        let mut manifest = manifest::LocalBinaryManifest::load(&self.bin_dir);

        // 1. Resolve or Download FFmpeg suite
        let ffmpeg_existing = self.resolve_binary(self.platform.ffmpeg_filename(), "-version").await;
        let ffprobe_existing = self.resolve_binary(self.platform.ffprobe_filename(), "-version").await;

        let (ffmpeg_path, ffprobe_path, ffplay_path) = match (ffmpeg_existing, ffprobe_existing) {
            (Some(f), Some(p)) => {
                let play = self.resolve_binary(self.platform.ffplay_filename(), "-version").await;
                (f, p, play)
            }
            _ => {
                let target = self.platform.ffmpeg_target();
                let progress_name = "ffmpeg";

                // Step A: Download with simultaneous SHA-256 calculation
                let (tmp_file, calculated_hash) = fetcher
                    .download_with_hash(&target, &self.bin_dir, &|prog| {
                        on_progress(progress_name, prog);
                    })
                    .await?;

                // Step B: Verify SHA-256 against official release checksum manifest
                on_progress(progress_name, DownloadProgress::VerifyingHash);
                let verified_hash = match fetcher.verify_checksum(&target, &calculated_hash).await {
                    Ok(hash) => hash,
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp_file);
                        return Err(e);
                    }
                };

                // Step C: Extract archive into app-local bin directory
                on_progress(progress_name, DownloadProgress::Extracting);
                extract::BinaryExtractor::extract(&tmp_file, &self.bin_dir, &target.format, &self.platform)?;

                let ffmpeg = self.bin_dir.join(self.platform.ffmpeg_filename());
                let ffprobe = self.bin_dir.join(self.platform.ffprobe_filename());
                let ffplay = self.bin_dir.join(self.platform.ffplay_filename());

                // Step D: Verify executable smoke test
                let ver_output = Self::verify_binary_executable(&ffmpeg, "-version").await?;
                let _ = Self::verify_binary_executable(&ffprobe, "-version").await?;

                // Step E: Persist metadata
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                manifest.update(
                    "ffmpeg",
                    manifest::BinaryMetadata {
                        installed_at: now,
                        sha256: Some(verified_hash),
                        version: Some(ver_output.lines().next().unwrap_or("").trim().to_string()),
                        path: Some(ffmpeg.to_string_lossy().to_string()),
                    },
                );
                let _ = manifest.save(&self.bin_dir);

                on_progress(progress_name, DownloadProgress::Complete);

                let play_opt = if ffplay.is_file() { Some(ffplay) } else { None };
                (ffmpeg, ffprobe, play_opt)
            }
        };

        // 2. Resolve or Download yt-dlp
        let ytdlp_path = match self.resolve_binary(self.platform.ytdlp_filename(), "--version").await {
            Some(p) => p,
            None => {
                let target = self.platform.ytdlp_target();
                let progress_name = "yt-dlp";

                // Step A: Download with simultaneous SHA-256 calculation
                let (tmp_file, calculated_hash) = fetcher
                    .download_with_hash(&target, &self.bin_dir, &|prog| {
                        on_progress(progress_name, prog);
                    })
                    .await?;

                // Step B: Verify SHA-256 against official release SHA2-256SUMS
                on_progress(progress_name, DownloadProgress::VerifyingHash);
                let verified_hash = match fetcher.verify_checksum(&target, &calculated_hash).await {
                    Ok(hash) => hash,
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp_file);
                        return Err(e);
                    }
                };

                // Step C: Atomically install raw executable into app-local bin directory
                on_progress(progress_name, DownloadProgress::Extracting);
                extract::BinaryExtractor::extract(&tmp_file, &self.bin_dir, &target.format, &self.platform)?;

                let ytdlp = self.bin_dir.join(self.platform.ytdlp_filename());

                // Step D: Verify executable smoke test
                let ver_output = Self::verify_binary_executable(&ytdlp, "--version").await?;

                // Step E: Persist metadata
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                manifest.update(
                    "yt-dlp",
                    manifest::BinaryMetadata {
                        installed_at: now,
                        sha256: Some(verified_hash),
                        version: Some(ver_output.trim().to_string()),
                        path: Some(ytdlp.to_string_lossy().to_string()),
                    },
                );
                let _ = manifest.save(&self.bin_dir);

                on_progress(progress_name, DownloadProgress::Complete);

                ytdlp
            }
        };

        Ok(BinaryPaths {
            yt_dlp: ytdlp_path,
            ffmpeg: ffmpeg_path,
            ffprobe: ffprobe_path,
            ffplay: ffplay_path,
        })
    }

    /// Resolves an individual binary across the 3 designated locations in priority order:
    /// 1. App-Local Folder (%LOCALAPPDATA%\<app_name>\bin or standalone `<exe_dir>/bin`)
    /// 2. Project root / executable directory
    /// 3. System $PATH (Optional fallback)
    pub async fn resolve_binary(&self, filename: &str, test_arg: &str) -> Option<PathBuf> {
        // Priority 1: Check App-Local bin directory
        let local_path = self.bin_dir.join(filename);
        if local_path.is_file() && Self::verify_binary_executable(&local_path, test_arg).await.is_ok() {
            return Some(local_path.canonicalize().unwrap_or(local_path));
        }

        // Priority 1b: Check application root / executable directory
        let exe_dir = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));

        let candidate_dirs = [
            exe_dir.clone(),
            exe_dir.join("bin"),
            env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            #[cfg(debug_assertions)]
            PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        ];

        for dir in &candidate_dirs {
            let candidate = dir.join(filename);
            if candidate.is_file() && Self::verify_binary_executable(&candidate, test_arg).await.is_ok() {
                return Some(candidate.canonicalize().unwrap_or(candidate));
            }
        }

        None
    }

    /// Checks if binary is available in the user's system $PATH.
    pub fn find_in_system_path(filename: &str) -> Option<PathBuf> {
        if let Some(paths) = env::var_os("PATH") {
            for dir in env::split_paths(&paths) {
                let candidate = dir.join(filename);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    /// Runs smoke test `<binary> <arg>` with a 2-second timeout to verify executable integrity.
    pub async fn verify_binary_executable(path: &Path, arg: &str) -> Result<String, BinaryError> {
        if !path.is_file() {
            return Err(BinaryError::ExecutionFailed {
                name: path.display().to_string(),
                path: path.to_path_buf(),
                details: "File does not exist or is not a regular file".to_string(),
            });
        }

        let mut cmd = tokio::process::Command::new(path);
        cmd.arg(arg)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        #[cfg(windows)]
        cmd.creation_flags(crate::CREATE_NO_WINDOW);

        let output_fut = cmd.output();
        let timeout_fut = timeout(Duration::from_secs(2), output_fut);

        match timeout_fut.await {
            Ok(Ok(output)) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                Ok(stdout)
            }
            Ok(Ok(output)) => {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                Err(BinaryError::ExecutionFailed {
                    name: path.display().to_string(),
                    path: path.to_path_buf(),
                    details: format!("Process exited with status {}: {}", output.status, stderr),
                })
            }
            Ok(Err(e)) => Err(BinaryError::ExecutionFailed {
                name: path.display().to_string(),
                path: path.to_path_buf(),
                details: e.to_string(),
            }),
            Err(_) => Err(BinaryError::ExecutionFailed {
                name: path.display().to_string(),
                path: path.to_path_buf(),
                details: "Smoke test timed out after 2 seconds".to_string(),
            }),
        }
    }

    /// Determines primary application data directory:
    /// - Standalone / portable if marker or local config is detected next to the exe
    /// - Otherwise, OS standard local app data folder
    fn determine_app_data_dir(app_name: &str) -> (PathBuf, bool) {
        if let Ok(exe_path) = env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                if exe_dir.join("portable").exists()
                    || exe_dir.join(".standalone").exists()
                    || exe_dir.join("settings.json").exists()
                {
                    return (exe_dir.to_path_buf(), true);
                }
            }
        }

        let local_dir = dirs::data_local_dir()
            .map(|p| p.join(app_name))
            .unwrap_or_else(|| {
                dirs::config_dir()
                    .map(|p| p.join(app_name))
                    .unwrap_or_else(|| PathBuf::from("."))
            });

        (local_dir, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dependency_manager_init() {
        let dm = DependencyManager::new("AnotherEncoderTest").expect("Init DependencyManager");
        assert!(dm.bin_dir().exists());
    }

    #[tokio::test]
    async fn test_check_existing_finds_project_binaries() {
        let dm = DependencyManager::new("AnotherEncoder").expect("Init DependencyManager");
        let existing = dm.check_existing().await.expect("check_existing");
        // Binaries exist in current project root
        if let Some(paths) = existing {
            assert!(paths.ffmpeg.is_file());
            assert!(paths.ffprobe.is_file());
            assert!(paths.yt_dlp.is_file());
        }
    }

    #[tokio::test]
    async fn test_verify_binary_smoke_test() {
        let exe_ext = if cfg!(windows) { ".exe" } else { "" };
        let ffmpeg_name = format!("ffmpeg{}", exe_ext);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(ffmpeg_name);
        if path.is_file() {
            let output = DependencyManager::verify_binary_executable(&path, "-version").await;
            assert!(output.is_ok());
            let out_str = output.unwrap();
            assert!(out_str.contains("ffmpeg version"));
        }
    }
}
