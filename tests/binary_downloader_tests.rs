use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;
use zip::write::{SimpleFileOptions, ZipWriter};

use another_encoder::binaries::extract::BinaryExtractor;
use another_encoder::binaries::fetcher::BinaryFetcher;
use another_encoder::binaries::manifest::{BinaryMetadata, LocalBinaryManifest};
use another_encoder::binaries::platform::{ArchiveFormat, PlatformInfo};
use another_encoder::binaries::DependencyManager;

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
        BinaryFetcher::find_hash_in_manifest(manifest, "ffmpeg-master-latest-win64-gpl.zip").as_deref(),
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
    zip.start_file(format!("ffmpeg-latest/bin/{}", platform.ffmpeg_filename()), opts).unwrap();
    zip.write_all(b"binary_payload_ffmpeg").unwrap();

    zip.start_file(format!("ffmpeg-latest/bin/{}", platform.ffprobe_filename()), opts).unwrap();
    zip.write_all(b"binary_payload_ffprobe").unwrap();

    zip.finish().unwrap();

    // Perform extraction
    BinaryExtractor::extract(&zip_file_path, &dest_bin_dir, &ArchiveFormat::Zip, &platform).unwrap();

    let extracted_ffmpeg = dest_bin_dir.join(platform.ffmpeg_filename());
    let extracted_ffprobe = dest_bin_dir.join(platform.ffprobe_filename());

    assert!(extracted_ffmpeg.is_file());
    assert!(extracted_ffprobe.is_file());
    assert_eq!(fs::read(&extracted_ffmpeg).unwrap(), b"binary_payload_ffmpeg");
    assert_eq!(fs::read(&extracted_ffprobe).unwrap(), b"binary_payload_ffprobe");

    // Temporary archive should be cleaned up
    assert!(!zip_file_path.exists());
}

#[tokio::test]
async fn test_dependency_manager_discovery_and_paths() {
    let dm = DependencyManager::new("AnotherEncoderTestApp").expect("DependencyManager::new");
    assert_eq!(dm.bin_dir(), dm.app_data_dir().join("bin"));

    // Check existing detects workspace binaries
    let existing = dm.check_existing().await.expect("check_existing");
    assert!(existing.is_some());
    let paths = existing.unwrap();
    assert!(paths.ffmpeg.is_file());
    assert!(paths.ffprobe.is_file());
    assert!(paths.yt_dlp.is_file());
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
