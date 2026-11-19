use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const GITHUB_REPO: &str = "grimztha/KabosuConverter";
/// Default GitHub API base URL (can be overridden via `KABOSU_GITHUB_API_BASE` for testing/mirrors).
pub const GITHUB_API_BASE: &str = "https://api.github.com";

const USER_AGENT: &str = concat!("KabosuConverter/", env!("CARGO_PKG_VERSION"));
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const METADATA_TIMEOUT: Duration = Duration::from_secs(30);

pub fn github_api_base() -> String {
    std::env::var("KABOSU_GITHUB_API_BASE").unwrap_or_else(|_| GITHUB_API_BASE.to_string())
}

/// Builds an HTTP client configured with our user-agent and connection timeout.
pub fn build_http_client(request_timeout: Option<Duration>) -> Result<Client> {
    let mut builder = Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT);
    if let Some(timeout) = request_timeout {
        builder = builder.timeout(timeout);
    }
    Ok(builder.build()?)
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

/// Validates a remote release asset filename so it can never escape the download folder.
pub fn sanitize_asset_filename(name: &str) -> Result<&str> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
        || trimmed.chars().any(char::is_control)
        || crate::sanitize::is_windows_reserved_name(trimmed)
    {
        return Err(anyhow!("Refusing unsafe release asset filename: {:?}", name));
    }
    Ok(trimmed)
}

/// Queries the GitHub Releases API for the latest published release.
pub async fn check_latest_release(repo: &str) -> Result<GithubRelease> {
    let client = build_http_client(Some(METADATA_TIMEOUT))?;
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

/// Parses a semver-like string (with optional leading `v`/`V`) into `(major, minor, patch)`.
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let s = v.trim().trim_start_matches(['v', 'V']);
    let mut parts = s.split(|c: char| !c.is_ascii_digit());
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// Returns true when `remote` is strictly newer than `current`.
pub fn is_newer_version(current: &str, remote: &str) -> bool {
    match (parse_version(current), parse_version(remote)) {
        (Some(c), Some(r)) => r > c,
        _ => false,
    }
}

/// Picks the best matching release archive for the current OS and CPU architecture.
pub fn find_platform_asset(assets: &[GithubAsset]) -> Option<&GithubAsset> {
    #[cfg(target_os = "windows")]
    {
        assets
            .iter()
            .find(|a| {
                let n = a.name.to_lowercase();
                n.contains("windows") && n.ends_with(".zip")
            })
            .or_else(|| assets.iter().find(|a| a.name.to_lowercase().ends_with(".zip")))
    }
    #[cfg(target_os = "macos")]
    {
        #[cfg(target_arch = "aarch64")]
        let arch_match = "aarch64";
        #[cfg(not(target_arch = "aarch64"))]
        let arch_match = "x86_64";

        assets
            .iter()
            .find(|a| {
                let n = a.name.to_lowercase();
                (n.contains("apple") || n.contains("darwin") || n.contains("macos"))
                    && n.contains(arch_match)
                    && (n.ends_with(".tar.gz") || n.ends_with(".zip"))
            })
            .or_else(|| {
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
            (n.contains("linux") || n.contains("ubuntu"))
                && (n.ends_with(".tar.gz") || n.ends_with(".zip"))
        })
    }
}

/// Computes the lowercase hex SHA-256 digest of a file on disk.
pub fn sha256_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Returns true if the file's SHA-256 digest matches `expected` (case-insensitive).
pub fn verify_sha256(path: &Path, expected: &str) -> Result<bool> {
    let actual = sha256_file(path)?;
    Ok(actual.eq_ignore_ascii_case(expected.trim()))
}

/// Looks for a matching `.sha256` file or checksum manifest in `assets` and extracts the expected hash.
pub async fn fetch_release_checksum(
    client: &Client,
    assets: &[GithubAsset],
    target_asset_name: &str,
) -> Option<String> {
    let target_lower = target_asset_name.to_lowercase();
    let expected_sha256 = format!("{}.sha256", target_lower);
    let expected_sha256sum = format!("{}.sha256sum", target_lower);

    for asset in assets {
        let name_lower = asset.name.to_lowercase();
        let is_target_checksum =
            name_lower == expected_sha256 || name_lower == expected_sha256sum;
        let is_manifest = matches!(
            name_lower.as_str(),
            "sha256sums" | "checksums.sha256" | "checksums.txt"
        );

        if !(is_target_checksum || is_manifest) {
            continue;
        }

        let Ok(res) = client.get(&asset.browser_download_url).send().await else {
            continue;
        };
        if !res.status().is_success() {
            continue;
        }
        let Ok(text) = res.text().await else {
            continue;
        };

        if is_manifest {
            if let Some(hash) =
                crate::binaries::BinaryFetcher::find_hash_in_manifest(&text, target_asset_name)
            {
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
    None
}

/// Streams a release asset to `dest_path`, verifying its SHA-256 digest if `expected_hash` is provided.
pub async fn download_release_asset<F>(
    url: &str,
    dest_path: &Path,
    expected_hash: Option<&str>,
    on_progress: F,
) -> Result<()>
where
    F: Fn(f32, u64, u64) + Send + Sync + 'static,
{
    let client = build_http_client(None)?;

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
                drop(file);
                let _ = std::fs::remove_file(&tmp_path);
                return Err(anyhow!("Download stream failed: {}", e));
            }
        };
        if let Err(e) = file.write_all(&chunk) {
            drop(file);
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
        drop(file);
        let _ = std::fs::remove_file(&tmp_path);
        return Err(anyhow!("Failed flushing file: {}", e));
    }
    drop(file);

    // Always propagate verification errors instead of silently accepting an unverified file.
    if let Some(hash) = expected_hash {
        match verify_sha256(&tmp_path, hash) {
            Ok(true) => {}
            Ok(false) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(anyhow!("SHA-256 hash verification failed"));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(anyhow!("Could not verify SHA-256 hash: {}", e));
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
    fn test_sanitize_asset_filename_blocks_traversal() {
        assert_eq!(
            sanitize_asset_filename("kabosu-converter-v0.15.0-x86_64-windows.zip").unwrap(),
            "kabosu-converter-v0.15.0-x86_64-windows.zip"
        );
        assert!(sanitize_asset_filename("../evil.exe").is_err());
        assert!(sanitize_asset_filename("..\\evil.exe").is_err());
        assert!(sanitize_asset_filename("subdir/file.zip").is_err());
        assert!(sanitize_asset_filename("CON.zip").is_err());
        assert!(sanitize_asset_filename("").is_err());
    }

    #[test]
    fn test_verify_sha256_missing_file_returns_err() {
        let res = verify_sha256(Path::new("nonexistent_file_for_sha_test.bin"), "0".repeat(64).as_str());
        assert!(res.is_err());
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
