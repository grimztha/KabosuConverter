use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Sha256, Digest};

pub const GITHUB_REPO: &str = "grimztha/KabosuConverter";
/// GitHub API base URL - externalized for potential testing, mirroring, or environment overrides.
pub const GITHUB_API_BASE: &str = "https://api.github.com";

pub fn github_api_base() -> String {
    std::env::var("KABOSU_GITHUB_API_BASE")
        .unwrap_or_else(|_| GITHUB_API_BASE.to_string())
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GithubRelease {
    pub tag_name: String,
    pub html_url: String,
    pub name: Option<String>,
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

/// Checks the latest GitHub release for the repository.
pub async fn check_latest_release(repo: &str) -> Result<GithubRelease> {
    let client = Client::builder()
        .user_agent(concat!("KabosuConverter/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let url = format!("{}/repos/{}/releases/latest", github_api_base(), repo);
    let res = client
        .get(&url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(anyhow!("GitHub API returned HTTP {}", res.status()));
    }

    let bytes = res.bytes().await?;
    let release: GithubRelease = serde_json::from_slice(&bytes)?;
    Ok(release)
}

/// Parses a semver-like version string into (major, minor, patch).
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let s = v.trim().trim_start_matches(|c| c == 'v' || c == 'V');
    let mut parts = s.split(|c: char| !c.is_ascii_digit());
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok().unwrap_or(0);
    let patch = parts.next().unwrap_or("0").parse().ok().unwrap_or(0);
    Some((major, minor, patch))
}

/// Returns true if remote version is strictly newer than current version.
pub fn is_newer_version(current: &str, remote: &str) -> bool {
    match (parse_version(current), parse_version(remote)) {
        (Some(c), Some(r)) => r > c,
        _ => false,
    }
}

/// Finds the best matching release asset for the current OS and architecture.
pub fn find_platform_asset<'a>(assets: &'a [GithubAsset]) -> Option<&'a GithubAsset> {
    #[cfg(target_os = "windows")]
    {
        assets.iter().find(|a| {
            let n = a.name.to_lowercase();
            n.contains("windows") && n.ends_with(".zip")
        }).or_else(|| {
            assets.iter().find(|a| a.name.to_lowercase().ends_with(".zip"))
        })
    }
    #[cfg(target_os = "macos")]
    {
        #[cfg(target_arch = "aarch64")]
        let arch_match = "aarch64";
        #[cfg(not(target_arch = "aarch64"))]
        let arch_match = "x86_64";

        assets.iter().find(|a| {
            let n = a.name.to_lowercase();
            (n.contains("apple") || n.contains("darwin") || n.contains("macos"))
                && n.contains(arch_match)
                && (n.ends_with(".tar.gz") || n.ends_with(".zip"))
        }).or_else(|| {
            assets.iter().find(|a| {
                let n = a.name.to_lowercase();
                (n.contains("apple") || n.contains("darwin") || n.contains("macos"))
                    && (n.ends_with(".tar.gz") || n.ends_with(".zip"))
            })
        })
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        assets.iter().find(|a| {
            let n = a.name.to_lowercase();
            (n.contains("linux") || n.contains("ubuntu")) && (n.ends_with(".tar.gz") || n.ends_with(".zip"))
        })
    }
}

/// Computes the SHA-256 hash of a file, returning a lowercase hex string.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    let result = hasher.finalize();
    Ok(hex::encode(result).to_lowercase())
}

/// Verifies the SHA-256 hash of a downloaded file against the expected hash.
pub fn verify_sha256(path: &Path, expected: &str) -> Result<bool> {
    let actual = sha256_file(path)?;
    let expected_lower = expected.to_lowercase();
    Ok(actual == expected_lower)
}

/// Attempts to discover the expected SHA-256 checksum for `target_asset_name` from GitHub release assets.
pub async fn fetch_release_checksum(
    client: &Client,
    assets: &[GithubAsset],
    target_asset_name: &str,
) -> Option<String> {
    for asset in assets {
        let name_lower = asset.name.to_lowercase();
        let is_target_checksum = name_lower == format!("{}.sha256", target_asset_name.to_lowercase())
            || name_lower == format!("{}.sha256sum", target_asset_name.to_lowercase());
        let is_manifest = name_lower == "sha256sums"
            || name_lower == "checksums.sha256"
            || name_lower == "checksums.txt";

        if is_target_checksum || is_manifest {
            if let Ok(res) = client.get(&asset.browser_download_url).send().await {
                if res.status().is_success() {
                    if let Ok(text) = res.text().await {
                        if is_manifest {
                            if let Some(hash) = crate::binaries::BinaryFetcher::find_hash_in_manifest(&text, target_asset_name) {
                                return Some(hash);
                            }
                        } else {
                            let trimmed = text.trim();
                            let hash = trimmed.split_whitespace().next().unwrap_or(trimmed);
                            if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                                return Some(hash.to_lowercase());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Streams download of the asset to destination path, reporting progress.
/// If expected_hash is provided, verifies the download's SHA-256 hash.
pub async fn download_release_asset<F>(
    url: &str,
    dest_path: &Path,
    expected_hash: Option<&str>,
    on_progress: F,
) -> Result<()>
where
    F: Fn(f32, u64, u64) + Send + Sync + 'static,
{
    let client = Client::builder()
        .user_agent(concat!("KabosuConverter/", env!("CARGO_PKG_VERSION")))
        .build()?;

    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = dest_path.with_extension("download.tmp");

    let res = client.get(url).send().await?;
    if !res.status().is_success() {
        return Err(anyhow!("Failed to download update: HTTP {}", res.status()));
    }

    let total_bytes = res.content_length().unwrap_or(0);
    on_progress(0.0, 0, total_bytes);

    let mut file = File::create(&tmp_path)?;
    let mut stream = res.bytes_stream();
    let mut downloaded: u64 = 0;

    while let Some(chunk_res) = stream.next().await {
        let chunk = match chunk_res {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(anyhow!("Download stream failed: {}", e));
            }
        };
        if let Err(e) = file.write_all(&chunk) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(anyhow!("Failed writing update file: {}", e));
        }
        downloaded += chunk.len() as u64;

        let ratio = if total_bytes > 0 {
            (downloaded as f32 / total_bytes as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        on_progress(ratio, downloaded, total_bytes);
    }

    if let Err(e) = file.flush() {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(anyhow!("Failed flushing file: {}", e));
    }
    drop(file);

    // Verify SHA-256 hash if provided
    if let Some(hash) = expected_hash {
        if let Ok(is_valid) = verify_sha256(&tmp_path, hash) {
            if !is_valid {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(anyhow!("SHA-256 hash verification failed"));
            }
        }
    }

    if dest_path.exists() {
        let _ = std::fs::remove_file(dest_path);
    }
    if let Err(e) = std::fs::rename(&tmp_path, dest_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(anyhow!("Failed renaming update to target path: {}", e));
    }

    on_progress(1.0, downloaded, total_bytes.max(downloaded));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version_variants() {
        assert_eq!(parse_version("0.14.6"), Some((0, 14, 6)));
        assert_eq!(parse_version("v0.14.6"), Some((0, 14, 6)));
        assert_eq!(parse_version("V1.0.0"), Some((1, 0, 0)));
        assert_eq!(parse_version("2.1"), Some((2, 1, 0)));
        assert_eq!(parse_version("v3"), Some((3, 0, 0)));
        assert_eq!(parse_version("invalid"), None);
    }

    #[test]
    fn test_is_newer_version() {
        assert!(!is_newer_version("0.14.6", "0.14.6"));
        assert!(!is_newer_version("0.14.6", "v0.14.6"));
        assert!(!is_newer_version("0.14.6", "v0.14.5"));
        assert!(!is_newer_version("0.14.6", "0.13.99"));

        assert!(is_newer_version("0.14.6", "0.14.7"));
        assert!(is_newer_version("0.14.6", "v0.15.0"));
        assert!(is_newer_version("0.14.6", "v1.0.0"));
    }

    #[test]
    fn test_find_platform_asset_selection() {
        let assets = vec![
            GithubAsset {
                name: "kabosu-converter-v0.15.0-aarch64-apple-darwin.tar.gz".into(),
                browser_download_url: "https://example.com/darwin.tar.gz".into(),
                size: 1000,
            },
            GithubAsset {
                name: "kabosu-converter-v0.15.0-ubuntu-26.04-x86_64.tar.gz".into(),
                browser_download_url: "https://example.com/linux.tar.gz".into(),
                size: 2000,
            },
            GithubAsset {
                name: "kabosu-converter-v0.15.0-x86_64-windows.zip".into(),
                browser_download_url: "https://example.com/windows.zip".into(),
                size: 3000,
            },
        ];

        let found = find_platform_asset(&assets);
        assert!(found.is_some());
        #[cfg(target_os = "windows")]
        assert_eq!(found.unwrap().name, "kabosu-converter-v0.15.0-x86_64-windows.zip");
        #[cfg(target_os = "macos")]
        assert!(found.unwrap().name.contains("darwin") || found.unwrap().name.contains("apple"));
        #[cfg(all(unix, not(target_os = "macos")))]
        assert!(found.unwrap().name.contains("ubuntu") || found.unwrap().name.contains("linux"));
    }
}
