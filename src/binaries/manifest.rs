use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};

use super::error::BinaryError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryMetadata {
    pub installed_at: u64,
    pub sha256: Option<String>,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalBinaryManifest {
    pub yt_dlp: Option<BinaryMetadata>,
    pub ffmpeg: Option<BinaryMetadata>,
    pub ffprobe: Option<BinaryMetadata>,
}

impl LocalBinaryManifest {
    pub const FILE_NAME: &'static str = ".metadata.json";

    /// Loads the local binary manifest from the given binary directory.
    /// Returns default manifest if file does not exist or fails to parse.
    pub fn load(bin_dir: &Path) -> Self {
        let manifest_path = bin_dir.join(Self::FILE_NAME);
        if manifest_path.is_file() {
            if let Ok(content) = fs::read_to_string(&manifest_path) {
                if let Ok(manifest) = serde_json::from_str::<Self>(&content) {
                    return manifest;
                }
            }
        }
        Self::default()
    }

    /// Saves the manifest to disk via a temporary file first.
    pub fn save(&self, bin_dir: &Path) -> Result<(), BinaryError> {
        fs::create_dir_all(bin_dir)?;
        let manifest_path = bin_dir.join(Self::FILE_NAME);
        let tmp_path = bin_dir.join(format!("{}.tmp", Self::FILE_NAME));

        let content = serde_json::to_string_pretty(self)?;
        fs::write(&tmp_path, &content)?;
        if manifest_path.exists() {
            let _ = fs::remove_file(&manifest_path);
        }
        if fs::rename(&tmp_path, &manifest_path).is_err() {
            let _ = fs::remove_file(&tmp_path);
            fs::write(&manifest_path, content)?;
        }

        Ok(())
    }

    /// Updates metadata for a specific binary key ("yt-dlp", "ffmpeg", "ffprobe").
    pub fn update(&mut self, key: &str, meta: BinaryMetadata) {
        match key {
            "yt-dlp" | "yt_dlp" => self.yt_dlp = Some(meta),
            "ffmpeg" => self.ffmpeg = Some(meta),
            "ffprobe" => self.ffprobe = Some(meta),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_manifest_save_and_load() {
        let dir = tempdir().unwrap();
        let mut manifest = LocalBinaryManifest::default();
        manifest.update(
            "yt-dlp",
            BinaryMetadata {
                installed_at: 1729891200,
                sha256: Some("33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555".into()),
                version: Some("2026.10.21".into()),
                path: Some("/test/bin/yt-dlp.exe".into()),
            },
        );

        manifest.save(dir.path()).expect("save manifest");

        let loaded = LocalBinaryManifest::load(dir.path());
        assert!(loaded.yt_dlp.is_some());
        let ytdlp = loaded.yt_dlp.unwrap();
        assert_eq!(ytdlp.installed_at, 1729891200);
        assert_eq!(
            ytdlp.sha256.as_deref(),
            Some("33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555")
        );
        assert_eq!(ytdlp.version.as_deref(), Some("2026.10.21"));
    }
}
