use super::error::BinaryError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOS {
    Windows,
    MacOS,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetArch {
    X86_64,
    Aarch64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveFormat {
    RawExecutable,
    Zip,
    TarXz,
}

#[derive(Debug, Clone)]
pub struct BinaryDownloadTarget {
    pub name: &'static str,
    pub binary_filename: &'static str,
    pub download_url: &'static str,
    pub asset_filename: &'static str,
    pub checksum_url: Option<&'static str>,
    pub format: ArchiveFormat,
}

#[derive(Debug, Clone)]
pub struct PlatformInfo {
    pub os: TargetOS,
    pub arch: TargetArch,
}

impl PlatformInfo {
    /// Detects current host operating system and architecture.
    pub fn current() -> Result<Self, BinaryError> {
        let os = if cfg!(target_os = "windows") {
            TargetOS::Windows
        } else if cfg!(target_os = "macos") {
            TargetOS::MacOS
        } else if cfg!(target_os = "linux") {
            TargetOS::Linux
        } else {
            return Err(BinaryError::PlatformNotSupported {
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
            });
        };

        let arch = if cfg!(target_arch = "x86_64") {
            TargetArch::X86_64
        } else if cfg!(target_arch = "aarch64") {
            TargetArch::Aarch64
        } else {
            return Err(BinaryError::PlatformNotSupported {
                os: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
            });
        };

        Ok(Self { os, arch })
    }

    /// Executable filename for ffmpeg on this platform.
    pub fn ffmpeg_filename(&self) -> &'static str {
        match self.os {
            TargetOS::Windows => "ffmpeg.exe",
            TargetOS::MacOS | TargetOS::Linux => "ffmpeg",
        }
    }

    /// Executable filename for ffprobe on this platform.
    pub fn ffprobe_filename(&self) -> &'static str {
        match self.os {
            TargetOS::Windows => "ffprobe.exe",
            TargetOS::MacOS | TargetOS::Linux => "ffprobe",
        }
    }

    /// Executable filename for ffplay on this platform.
    pub fn ffplay_filename(&self) -> &'static str {
        match self.os {
            TargetOS::Windows => "ffplay.exe",
            TargetOS::MacOS | TargetOS::Linux => "ffplay",
        }
    }

    /// Executable filename for yt-dlp on this platform.
    pub fn ytdlp_filename(&self) -> &'static str {
        match self.os {
            TargetOS::Windows => "yt-dlp.exe",
            TargetOS::MacOS | TargetOS::Linux => "yt-dlp",
        }
    }

    /// Resolution for yt-dlp binary target.
    pub fn ytdlp_target(&self) -> BinaryDownloadTarget {
        match (self.os, self.arch) {
            (TargetOS::Windows, TargetArch::X86_64) => BinaryDownloadTarget {
                name: "yt-dlp",
                binary_filename: "yt-dlp.exe",
                download_url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe",
                asset_filename: "yt-dlp.exe",
                checksum_url: Some("https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS"),
                format: ArchiveFormat::RawExecutable,
            },
            (TargetOS::Windows, TargetArch::Aarch64) => BinaryDownloadTarget {
                name: "yt-dlp",
                binary_filename: "yt-dlp.exe",
                download_url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_arm64.exe",
                asset_filename: "yt-dlp_arm64.exe",
                checksum_url: Some("https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS"),
                format: ArchiveFormat::RawExecutable,
            },
            (TargetOS::MacOS, _) => BinaryDownloadTarget {
                name: "yt-dlp",
                binary_filename: "yt-dlp",
                download_url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos",
                asset_filename: "yt-dlp_macos",
                checksum_url: Some("https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS"),
                format: ArchiveFormat::RawExecutable,
            },
            (TargetOS::Linux, TargetArch::X86_64) => BinaryDownloadTarget {
                name: "yt-dlp",
                binary_filename: "yt-dlp",
                download_url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp",
                asset_filename: "yt-dlp",
                checksum_url: Some("https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS"),
                format: ArchiveFormat::RawExecutable,
            },
            (TargetOS::Linux, TargetArch::Aarch64) => BinaryDownloadTarget {
                name: "yt-dlp",
                binary_filename: "yt-dlp",
                download_url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64",
                asset_filename: "yt-dlp_linux_aarch64",
                checksum_url: Some("https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS"),
                format: ArchiveFormat::RawExecutable,
            },
        }
    }

    /// Resolution for FFmpeg suite target (targeting FFmpeg 8.1+ latest builds).
    pub fn ffmpeg_target(&self) -> BinaryDownloadTarget {
        match (self.os, self.arch) {
            (TargetOS::Windows, TargetArch::X86_64) => BinaryDownloadTarget {
                name: "ffmpeg",
                binary_filename: "ffmpeg.exe",
                download_url: "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-win64-gpl.zip",
                asset_filename: "ffmpeg-master-latest-win64-gpl.zip",
                checksum_url: Some("https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/checksums.sha256"),
                format: ArchiveFormat::Zip,
            },
            (TargetOS::Windows, TargetArch::Aarch64) => BinaryDownloadTarget {
                name: "ffmpeg",
                binary_filename: "ffmpeg.exe",
                download_url: "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-winarm64-gpl.zip",
                asset_filename: "ffmpeg-master-latest-winarm64-gpl.zip",
                checksum_url: Some("https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/checksums.sha256"),
                format: ArchiveFormat::Zip,
            },
            (TargetOS::MacOS, TargetArch::Aarch64) => BinaryDownloadTarget {
                name: "ffmpeg",
                binary_filename: "ffmpeg",
                download_url: "https://github.com/Tyrrrz/FFmpegBin/releases/latest/download/ffmpeg-osx-arm64.zip",
                asset_filename: "ffmpeg-osx-arm64.zip",
                checksum_url: None,
                format: ArchiveFormat::Zip,
            },
            (TargetOS::MacOS, TargetArch::X86_64) => BinaryDownloadTarget {
                name: "ffmpeg",
                binary_filename: "ffmpeg",
                download_url: "https://github.com/Tyrrrz/FFmpegBin/releases/latest/download/ffmpeg-osx-x64.zip",
                asset_filename: "ffmpeg-osx-x64.zip",
                checksum_url: None,
                format: ArchiveFormat::Zip,
            },
            (TargetOS::Linux, TargetArch::X86_64) => BinaryDownloadTarget {
                name: "ffmpeg",
                binary_filename: "ffmpeg",
                download_url: "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-linux64-gpl.tar.xz",
                asset_filename: "ffmpeg-master-latest-linux64-gpl.tar.xz",
                checksum_url: Some("https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/checksums.sha256"),
                format: ArchiveFormat::TarXz,
            },
            (TargetOS::Linux, TargetArch::Aarch64) => BinaryDownloadTarget {
                name: "ffmpeg",
                binary_filename: "ffmpeg",
                download_url: "https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
                asset_filename: "ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
                checksum_url: Some("https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download/checksums.sha256"),
                format: ArchiveFormat::TarXz,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_current_detection() {
        let platform = PlatformInfo::current().expect("Host platform must be supported");
        assert!(!platform.ffmpeg_filename().is_empty());
        assert!(!platform.ffprobe_filename().is_empty());
        assert!(!platform.ytdlp_filename().is_empty());
    }

    #[test]
    fn test_targets_have_valid_urls() {
        let platform = PlatformInfo::current().expect("Host platform");
        let ytdlp = platform.ytdlp_target();
        assert!(ytdlp.download_url.starts_with("https://"));
        assert!(ytdlp.checksum_url.unwrap().starts_with("https://"));

        let ffmpeg = platform.ffmpeg_target();
        assert!(ffmpeg.download_url.starts_with("https://"));
    }
}
