use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha256};

use super::error::BinaryError;
use super::platform::BinaryDownloadTarget;

#[derive(Debug, Clone)]
pub enum DownloadProgress {
    Started { total_bytes: Option<u64> },
    Transferred { chunk_size: usize, total_downloaded: u64 },
    VerifyingHash,
    Extracting,
    Complete,
}

pub struct BinaryFetcher {
    client: Client,
}

impl BinaryFetcher {
    pub fn new() -> Result<Self, BinaryError> {
        let client = Client::builder()
            .user_agent(concat!("AnotherEncoder/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { client })
    }

    /// Streams download of the target asset into `<target_dir>/<asset_filename>.download.tmp`,
    /// simultaneously calculating its SHA-256 hash and emitting progress callbacks.
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

        let res = self.client.get(target.download_url).send().await?;
        if !res.status().is_success() {
            return Err(BinaryError::Other(format!(
                "Failed to download asset '{}' from {}: HTTP {}",
                target.asset_filename,
                target.download_url,
                res.status()
            )));
        }

        let total_bytes = res.content_length();
        on_progress(DownloadProgress::Started { total_bytes });

        let mut file = File::create(&tmp_file_path)?;
        let mut stream = res.bytes_stream();
        let mut hasher = Sha256::new();
        let mut total_downloaded: u64 = 0;

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res?;
            let chunk_len = chunk.len();
            file.write_all(&chunk)?;
            hasher.update(&chunk);
            total_downloaded += chunk_len as u64;

            on_progress(DownloadProgress::Transferred {
                chunk_size: chunk_len,
                total_downloaded,
            });
        }

        file.flush()?;
        drop(file);

        let hash_result = hasher.finalize();
        let calculated_hash = hex::encode(hash_result).to_lowercase();

        Ok((tmp_file_path, calculated_hash))
    }

    /// Fetches the remote checksum manifest, parses it, and validates the calculated hash.
    pub async fn verify_checksum(
        &self,
        target: &BinaryDownloadTarget,
        calculated_hash: &str,
    ) -> Result<String, BinaryError> {
        let Some(checksum_url) = target.checksum_url else {
            // No remote checksum URL specified for this target
            return Ok(calculated_hash.to_string());
        };

        let res = self.client.get(checksum_url).send().await?;
        if !res.status().is_success() {
            return Err(BinaryError::Other(format!(
                "Failed to fetch checksum manifest from {}: HTTP {}",
                checksum_url,
                res.status()
            )));
        }

        let manifest_text = res.text().await?;
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

    /// Parses standard checksum manifests (such as SHA2-256SUMS or checksums.sha256).
    /// Lines format: `<hash>  <filename>` or `<hash> *<filename>`
    pub fn find_hash_in_manifest(manifest: &str, asset_name: &str) -> Option<String> {
        for line in manifest.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let mut parts = trimmed.split_whitespace();
            let hash = parts.next()?;
            let filename = parts.next()?.trim_start_matches('*');

            if filename == asset_name {
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

        let hash_win64 = BinaryFetcher::find_hash_in_manifest(manifest, "ffmpeg-master-latest-win64-gpl.zip");
        assert_eq!(
            hash_win64.as_deref(),
            Some("d5410167dc4e59bac2c95f3588a8e2b5abe7777dd4cfdfff0cd72bdff99fd064")
        );
    }
}
