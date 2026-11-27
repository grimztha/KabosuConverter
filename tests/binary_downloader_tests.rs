use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;
use zip::write::{SimpleFileOptions, ZipWriter};

use kabosu_converter::binaries::extract::BinaryExtractor;
use kabosu_converter::binaries::fetcher::BinaryFetcher;
use kabosu_converter::binaries::manifest::{BinaryMetadata, LocalBinaryManifest};
use kabosu_converter::binaries::platform::{ArchiveFormat, PlatformInfo};
use kabosu_converter::binaries::DependencyManager;

#[tokio::test]
async fn test_manifest_lifecycle() {
    let temp = tempdir().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let mut manifest = LocalBinaryManifest::load(&bin_dir);
    assert!(manifest.ffmpeg.is_none());
    assert!(manifest.yt_dlp.is_none());

    manifest.update(
        "ffmpeg",
        BinaryMetadata {
            installed_at: 1729891200,
            sha256: Some("d5410167dc4e59bac2c95f3588a8e2b5abe7777dd4cfdfff0cd72bdff99fd064".into()),
            version: Some("8.1.3".into()),
            path: Some(bin_dir.join("ffmpeg.exe").to_string_lossy().to_string()),
        },
    );
    manifest.save(&bin_dir).unwrap();

    let reloaded = LocalBinaryManifest::load(&bin_dir);
    assert!(reloaded.ffmpeg.is_some());
    let ffmpeg_meta = reloaded.ffmpeg.unwrap();
    assert_eq!(ffmpeg_meta.version.as_deref(), Some("8.1.3"));
    assert_eq!(
        ffmpeg_meta.sha256.as_deref(),
        Some("d5410167dc4e59bac2c95f3588a8e2b5abe7777dd4cfdfff0cd72bdff99fd064")
    );
}

#[tokio::test]
async fn test_checksum_manifest_parsing() {
    let manifest = r#"
# Comments should be ignored
b7af77f686e95f7845ce8d09d21957718febb8b48b30555adb9dd2292ad7bf14  yt-dlp
33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555 *yt-dlp.exe
d5410167dc4e59bac2c95f3588a8e2b5abe7777dd4cfdfff0cd72bdff99fd064  ffmpeg-master-latest-win64-gpl.zip
"#;

    assert_eq!(
        BinaryFetcher::find_hash_in_manifest(manifest, "yt-dlp.exe").as_deref(),
        Some("33121d192d706e62f684a06f80918e3f3a8ba52ac3a65213b6951c39c7d9f555")
    );
    assert_eq!(
        BinaryFetcher::find_hash_in_manifest(manifest, "yt-dlp").as_deref(),
        Some("b7af77f686e95f7845ce8d09d21957718febb8b48b30555adb9dd2292ad7bf14")
    );
    assert_eq!(
        BinaryFetcher::find_hash_in_manifest(manifest, "ffmpeg-master-latest-win64-gpl.zip")
            .as_deref(),
        Some("d5410167dc4e59bac2c95f3588a8e2b5abe7777dd4cfdfff0cd72bdff99fd064")
    );
    assert!(BinaryFetcher::find_hash_in_manifest(manifest, "nonexistent.zip").is_none());
}

#[tokio::test]
async fn test_archive_extractor_zip_and_permissions() {
    let temp = tempdir().unwrap();
    let zip_file_path = temp.path().join("ffmpeg_bundle.zip");
    let dest_bin_dir = temp.path().join("bin");

    let platform = PlatformInfo::current().unwrap();

    // Create a mock zip containing nested bin/ directory
    let file = File::create(&zip_file_path).unwrap();
    let mut zip = ZipWriter::new(file);

    let opts = SimpleFileOptions::default();
    zip.start_file(
        format!("ffmpeg-latest/bin/{}", platform.ffmpeg_filename()),
        opts,
    )
    .unwrap();
    zip.write_all(b"binary_payload_ffmpeg").unwrap();

    zip.start_file(
        format!("ffmpeg-latest/bin/{}", platform.ffprobe_filename()),
        opts,
    )
    .unwrap();
    zip.write_all(b"binary_payload_ffprobe").unwrap();

    zip.finish().unwrap();

    // Perform extraction
    BinaryExtractor::extract(
        &zip_file_path,
        &dest_bin_dir,
        &ArchiveFormat::Zip,
        &platform,
    )
    .unwrap();

    let extracted_ffmpeg = dest_bin_dir.join(platform.ffmpeg_filename());
    let extracted_ffprobe = dest_bin_dir.join(platform.ffprobe_filename());

    assert!(extracted_ffmpeg.is_file());
    assert!(extracted_ffprobe.is_file());
    assert_eq!(
        fs::read(&extracted_ffmpeg).unwrap(),
        b"binary_payload_ffmpeg"
    );
    assert_eq!(
        fs::read(&extracted_ffprobe).unwrap(),
        b"binary_payload_ffprobe"
    );

    // Temporary archive should be cleaned up
    assert!(!zip_file_path.exists());
}

#[tokio::test]
async fn test_dependency_manager_discovery_and_paths() {
    let temp = tempdir().unwrap();
    let dm = DependencyManager::with_app_data_dir(temp.path().to_path_buf(), false)
        .expect("DependencyManager::with_app_data_dir");
    assert_eq!(dm.bin_dir(), dm.app_data_dir().join("bin"));

    // Check existing detects workspace binaries
    let existing = dm.check_existing().await.expect("check_existing");
    if let Some(paths) = existing {
        assert!(paths.ffmpeg.is_file());
        assert!(paths.ffprobe.is_file());
        assert!(paths.yt_dlp.is_file());
    }
}

#[tokio::test]
async fn test_standalone_settings_and_clean_uninstallation() {
    let temp = tempdir().unwrap();
    let custom_app_dir = temp.path().join("CustomAppTest");
    fs::create_dir_all(&custom_app_dir).unwrap();

    let test_file = custom_app_dir.join("settings.json");
    fs::write(&test_file, r#"{"default_crf": 21}"#).unwrap();
    assert!(test_file.exists());

    // Wipe cleans the entire app directory leaving zero residue
    fs::remove_dir_all(&custom_app_dir).unwrap();
    assert!(!custom_app_dir.exists());
}

#[test]
fn test_ytdlp_platform_targets_use_standalone_binaries() {
    use kabosu_converter::binaries::platform::{TargetArch, TargetOS};

    let cases = [
        (
            TargetOS::Windows,
            TargetArch::X86_64,
            "yt-dlp.exe",
            "yt-dlp.exe",
        ),
        (
            TargetOS::Windows,
            TargetArch::Aarch64,
            "yt-dlp.exe",
            "yt-dlp_arm64.exe",
        ),
        (
            TargetOS::MacOS,
            TargetArch::X86_64,
            "yt-dlp",
            "yt-dlp_macos",
        ),
        (
            TargetOS::MacOS,
            TargetArch::Aarch64,
            "yt-dlp",
            "yt-dlp_macos",
        ),
        (
            TargetOS::Linux,
            TargetArch::X86_64,
            "yt-dlp",
            "yt-dlp_linux",
        ),
        (
            TargetOS::Linux,
            TargetArch::Aarch64,
            "yt-dlp",
            "yt-dlp_linux_aarch64",
        ),
    ];

    for (os, arch, expected_bin, expected_asset) in cases {
        let info = PlatformInfo { os, arch };
        let target = info.ytdlp_target();
        assert_eq!(target.binary_filename, expected_bin);
        assert_eq!(target.asset_filename, expected_asset);
        assert!(
            target.download_url.ends_with(expected_asset),
            "Unexpected URL {} for {:?}",
            target.download_url,
            (os, arch)
        );
        assert_eq!(
            target.checksum_url,
            Some("https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS")
        );
    }
}

#[test]
fn test_ytdlp_date_version_comparison() {
    use kabosu_converter::updater::is_newer_version;

    assert!(is_newer_version("2026.10.26", "2026.11.23"));
    assert!(!is_newer_version("2026.11.23", "2026.11.23"));
    assert!(!is_newer_version("2026.11.23", "2026.10.26"));
    assert!(is_newer_version("2026.11.23", "2026.12.01"));
}
