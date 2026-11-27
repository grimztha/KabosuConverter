use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::error::BinaryError;
use super::platform::BinaryDownloadTarget;

#[derive(Debug, Clone)]
pub enum DownloadProgress {
    Started {
        total_bytes: Option<u64>,
    },
    Transferred {
        chunk_size: usize,
        total_downloaded: u64,
    },
    VerifyingHash,
    Extracting,
    Complete,
}

const MAX_BINARY_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: usize = 1024 * 1024;

pub struct BinaryFetcher {
    client: Client,
}

impl BinaryFetcher {
    pub fn new() -> Result<Self, BinaryError> {
        let client = Client::builder()
            .user_agent(concat!("KabosuConverter/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .read_timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self { client })
    }

    /// Streams `target` to a temporary file in `target_dir` while computing its SHA-256 digest.
    pub async fn download_with_hash<F>(
        &self,
        target: &BinaryDownloadTarget,
        target_dir: &Path,
        on_progress: &F,
    ) -> Result<(PathBuf, String), BinaryError>
    where
        F: Fn(DownloadProgress) + Send + Sync,
    {
        std::fs::create_dir_all(target_dir)?;
        let tmp_file_path = target_dir.join(format!("{}.download.tmp", target.asset_filename));

        let download_url = match target.name {
            "yt-dlp" => std::env::var("KABOSU_YTDLP_URL")
                .unwrap_or_else(|_| target.download_url.to_string()),
            "ffmpeg" => std::env::var("KABOSU_FFMPEG_URL")
                .unwrap_or_else(|_| target.download_url.to_string()),
            _ => target.download_url.to_string(),
        };
        crate::updater::validate_remote_url(&download_url)
            .map_err(|e| BinaryError::Other(e.to_string()))?;

        let res = self.client.get(&download_url).send().await?;
        if !res.status().is_success() {
            return Err(BinaryError::Other(format!(
                "Failed to download asset '{}' from {}: HTTP {}",
                target.asset_filename,
                download_url,
                res.status()
            )));
        }

        let total_bytes = res.content_length();
        if total_bytes.is_some_and(|n| n > MAX_BINARY_DOWNLOAD_BYTES) {
            return Err(BinaryError::Other(format!(
                "Asset '{}' exceeds maximum allowed download size",
                target.asset_filename
            )));
        }
        on_progress(DownloadProgress::Started { total_bytes });

        let mut file = File::create(&tmp_file_path)?;
        let mut stream = res.bytes_stream();
        let mut hasher = Sha256::new();
        let mut total_downloaded: u64 = 0;

        while let Some(chunk_res) = stream.next().await {
            let chunk = match chunk_res {
                Ok(c) => c,
                Err(e) => {
                    drop(file);
                    let _ = std::fs::remove_file(&tmp_file_path);
                    return Err(e.into());
                }
            };
            let chunk_len = chunk.len();
            total_downloaded = total_downloaded.saturating_add(chunk_len as u64);
            if total_downloaded > MAX_BINARY_DOWNLOAD_BYTES {
                drop(file);
                let _ = std::fs::remove_file(&tmp_file_path);
                return Err(BinaryError::Other(format!(
                    "Asset '{}' exceeded maximum allowed download size",
                    target.asset_filename
                )));
            }
            if let Err(e) = file.write_all(&chunk) {
                drop(file);
                let _ = std::fs::remove_file(&tmp_file_path);
                return Err(e.into());
            }
            hasher.update(&chunk);

            on_progress(DownloadProgress::Transferred {
                chunk_size: chunk_len,
                total_downloaded,
            });
        }

        if let Err(e) = file.flush() {
            drop(file);
            let _ = std::fs::remove_file(&tmp_file_path);
            return Err(e.into());
        }
        drop(file);

        let calculated_hash = hex::encode(hasher.finalize());
        Ok((tmp_file_path, calculated_hash))
    }

    /// Fetches the remote checksum manifest and validates `calculated_hash`.
    pub async fn verify_checksum(
        &self,
        target: &BinaryDownloadTarget,
        calculated_hash: &str,
    ) -> Result<String, BinaryError> {
        let checksum_url_override = if target.name == "yt-dlp" {
            std::env::var("KABOSU_YTDLP_CHECKSUM_URL").ok()
        } else if target.name == "ffmpeg" {
            std::env::var("KABOSU_FFMPEG_CHECKSUM_URL").ok()
        } else {
            None
        };

        let checksum_url = checksum_url_override.as_deref().or(target.checksum_url);

        let Some(checksum_url) = checksum_url else {
            return Ok(calculated_hash.to_string());
        };
        crate::updater::validate_remote_url(checksum_url)
            .map_err(|e| BinaryError::Other(e.to_string()))?;

        let res = self.client.get(checksum_url).send().await?;
        if !res.status().is_success() {
            return Err(BinaryError::Other(format!(
                "Failed to fetch checksum manifest from {}: HTTP {}",
                checksum_url,
                res.status()
            )));
        }
        if res
            .content_length()
            .is_some_and(|n| n > MAX_CHECKSUM_BYTES as u64)
        {
            return Err(BinaryError::Other(
                "Checksum manifest exceeded size limit".to_string(),
            ));
        }

        let bytes = res.bytes().await?;
        if bytes.len() > MAX_CHECKSUM_BYTES {
            return Err(BinaryError::Other(
                "Checksum manifest exceeded size limit".to_string(),
            ));
        }
        let manifest_text = String::from_utf8_lossy(&bytes);
        let expected_hash = Self::find_hash_in_manifest(&manifest_text, target.asset_filename)
            .ok_or_else(|| BinaryError::ChecksumNotFound {
                asset_name: target.asset_filename.to_string(),
                url: checksum_url.to_string(),
            })?;

        if !calculated_hash.eq_ignore_ascii_case(&expected_hash) {
            return Err(BinaryError::ChecksumMismatch {
                binary: target.name.to_string(),
                expected: expected_hash,
                actual: calculated_hash.to_string(),
            });
        }

        Ok(expected_hash)
    }

    /// Parses standard checksum manifests (`<sha256>  <filename>` or `<sha256> *<filename>`).
    pub fn find_hash_in_manifest(manifest: &str, asset_name: &str) -> Option<String> {
        for line in manifest.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let mut parts = trimmed.split_whitespace();
            let Some(hash) = parts.next() else {
                continue;
            };
            let Some(filename) = parts.next().map(|s| s.trim_start_matches('*')) else {
                continue;
            };

            if filename == asset_name
                && hash.len() == 64
                && hash.chars().all(|c| c.is_ascii_hexdigit())
            {
                return Some(hash.to_lowercase());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ytdlp_manifest() {
        let manifest = r#"
# Comment line
b7af77f686e95f7845ce8d09d21957718febb8b48b30555adb9dd2292ad7bf14  yt-dlp
33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555  yt-dlp.exe
5f1e419f76e49c996061511ef9eacce50d4aa4959c34480539562e9e1295708d  yt-dlp.tar.gz
"#;

        let hash_win = BinaryFetcher::find_hash_in_manifest(manifest, "yt-dlp.exe");
        assert_eq!(
            hash_win.as_deref(),
            Some("33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555")
        );

        let hash_unix = BinaryFetcher::find_hash_in_manifest(manifest, "yt-dlp");
        assert_eq!(
            hash_unix.as_deref(),
            Some("b7af77f686e95f7845ce8d09d21957718febb8b48b30555adb9dd2292ad7bf14")
        );

        let hash_missing = BinaryFetcher::find_hash_in_manifest(manifest, "nonexistent");
        assert!(hash_missing.is_none());
    }

    #[test]
    fn test_parse_ffmpeg_builds_manifest() {
        let manifest = r#"
c8993fee4f28051bc07f472fd03f51d98a9c4054488500739c22b116a4f390d1  ffmpeg-master-latest-win32-gpl-shared.zip
d5410167dc4e59bac2c95f3588a8e2b5abe7777dd4cfdfff0cd72bdff99fd064  ffmpeg-master-latest-win64-gpl.zip
"#;

        let hash_win64 =
            BinaryFetcher::find_hash_in_manifest(manifest, "ffmpeg-master-latest-win64-gpl.zip");
        assert_eq!(
            hash_win64.as_deref(),
            Some("d5410167dc4e59bac2c95f3588a8e2b5abe7777dd4cfdfff0cd72bdff99fd064")
        );
    }
}
