mod common;

use kabosu_converter::updater::{self, GithubAsset, GithubRelease};
use slint::ComponentHandle;

slint::include_modules!();

#[test]
fn test_version_parsing_and_semver_comparisons() {
    assert_eq!(updater::parse_version("0.14.6"), Some((0, 14, 6)));
    assert_eq!(updater::parse_version("v0.14.6"), Some((0, 14, 6)));
    assert_eq!(updater::parse_version("V0.15.0"), Some((0, 15, 0)));
    assert_eq!(updater::parse_version("1.0.0"), Some((1, 0, 0)));
    assert_eq!(updater::parse_version("0.14"), Some((0, 14, 0)));
    assert_eq!(updater::parse_version("v1"), Some((1, 0, 0)));

    assert_eq!(updater::parse_version(""), None);
    assert_eq!(updater::parse_version("not-a-version"), None);
}

#[test]
fn test_is_newer_version_matrix() {
    let current = "0.14.6";

    // Same or older versions -> false
    assert!(!updater::is_newer_version(current, "0.14.6"));
    assert!(!updater::is_newer_version(current, "v0.14.6"));
    assert!(!updater::is_newer_version(current, "v0.14.5"));
    assert!(!updater::is_newer_version(current, "0.13.99"));
    assert!(!updater::is_newer_version(current, "0.1.0"));

    // Newer versions -> true
    assert!(updater::is_newer_version(current, "0.14.7"));
    assert!(updater::is_newer_version(current, "v0.14.7"));
    assert!(updater::is_newer_version(current, "v0.15.0"));
    assert!(updater::is_newer_version(current, "1.0.0"));
    assert!(updater::is_newer_version(current, "v2.0.0"));

    // Invalid remote version -> false
    assert!(!updater::is_newer_version(current, "unknown"));
}

#[test]
fn test_find_platform_asset_resolution() {
    let assets = vec![
        GithubAsset {
            name: "kabosu-converter-v0.15.0-x86_64-windows.zip".into(),
            browser_download_url: "https://github.com/grimztha/KabosuConverter/releases/download/v0.15.0/kabosu-converter-v0.15.0-x86_64-windows.zip".into(),
            size: 12_500_000,
        },
        GithubAsset {
            name: "kabosu-converter-v0.15.0-x86_64-windows.zip.sha256".into(),
            browser_download_url: "https://github.com/grimztha/KabosuConverter/releases/download/v0.15.0/kabosu-converter-v0.15.0-x86_64-windows.zip.sha256".into(),
            size: 110,
        },
        GithubAsset {
            name: "kabosu-converter-v0.15.0-aarch64-apple-darwin.tar.gz".into(),
            browser_download_url: "https://github.com/grimztha/KabosuConverter/releases/download/v0.15.0/kabosu-converter-v0.15.0-aarch64-apple-darwin.tar.gz".into(),
            size: 10_800_000,
        },
        GithubAsset {
            name: "kabosu-converter-v0.15.0-ubuntu-26.04-x86_64.tar.gz".into(),
            browser_download_url: "https://github.com/grimztha/KabosuConverter/releases/download/v0.15.0/kabosu-converter-v0.15.0-ubuntu-26.04-x86_64.tar.gz".into(),
            size: 11_200_000,
        },
    ];

    let found = updater::find_platform_asset(&assets);
    assert!(found.is_some(), "Should find a matching asset for host platform");

    #[cfg(target_os = "windows")]
    assert_eq!(found.unwrap().name, "kabosu-converter-v0.15.0-x86_64-windows.zip");

    #[cfg(target_os = "macos")]
    assert!(found.unwrap().name.contains("apple") || found.unwrap().name.contains("darwin"));

    #[cfg(all(unix, not(target_os = "macos")))]
    assert!(found.unwrap().name.contains("ubuntu") || found.unwrap().name.contains("linux"));
}

#[test]
fn test_github_release_deserialization() {
    let json_data = r#"{
        "tag_name": "v0.14.6",
        "html_url": "https://github.com/grimztha/KabosuConverter/releases/tag/v0.14.6",
        "name": "v0.14.6",
        "body": "Release description",
        "assets": [
            {
                "name": "kabosu-converter-v0.14.6-x86_64-windows.zip",
                "browser_download_url": "https://example.com/download.zip",
                "size": 12555929
            }
        ]
    }"#;

    let release: Result<GithubRelease, _> = serde_json::from_str(json_data);
    assert!(release.is_ok());
    let r = release.unwrap();
    assert_eq!(r.tag_name, "v0.14.6");
    assert_eq!(r.assets.len(), 1);
    assert_eq!(r.assets[0].name, "kabosu-converter-v0.14.6-x86_64-windows.zip");
    assert_eq!(r.assets[0].size, 12555929);
}

#[test]
fn test_ui_update_state_lifecycle() {
    common::init_test_backend();

    let ui = MainWindow::new().expect("MainWindow creation");
    let data = ui.global::<AppData>();

    // Initial default state
    data.set_app_version("0.14.6".into());
    assert_eq!(data.get_app_version().as_str(), "0.14.6");
    assert_eq!(data.get_update_status(), 0);

    // State 1: Checking
    data.set_update_status(1);
    data.set_update_status_text("Checking for updates...".into());
    assert_eq!(data.get_update_status(), 1);
    assert_eq!(data.get_update_status_text().as_str(), "Checking for updates...");

    // State 2: Up to date
    data.set_update_status(2);
    data.set_update_status_text("Up to date".into());
    assert_eq!(data.get_update_status(), 2);
    assert_eq!(data.get_update_status_text().as_str(), "Up to date");

    // State 3: Update available
    data.set_has_update_available(true);
    data.set_update_status(3);
    data.set_update_latest_version("v0.15.0".into());
    data.set_update_release_url("https://github.com/grimztha/KabosuConverter/releases/tag/v0.15.0".into());
    data.set_update_status_text("Update to v0.15.0".into());
    assert_eq!(data.get_update_status(), 3);
    assert!(data.get_has_update_available());
    assert_eq!(data.get_update_latest_version().as_str(), "v0.15.0");
    assert_eq!(data.get_update_status_text().as_str(), "Update to v0.15.0");

    // State 4: Downloading
    data.set_update_status(4);
    data.set_update_download_progress(0.45);
    data.set_update_status_text("Downloading... 45% (5.6/12.5 MB)".into());
    assert_eq!(data.get_update_status(), 4);
    assert!((data.get_update_download_progress() - 0.45).abs() < 1e-4);

    // State 5: Downloaded
    data.set_update_status(5);
    data.set_update_download_progress(1.0);
    data.set_update_downloaded_path("C:\\Users\\User\\Downloads\\kabosu-converter-v0.15.0-x86_64-windows.zip".into());
    data.set_update_status_text("Update downloaded (Click to open)".into());
    assert_eq!(data.get_update_status(), 5);
    assert_eq!(data.get_update_status_text().as_str(), "Update downloaded (Click to open)");

    // State 6: Error / retry
    data.set_update_status(6);
    data.set_update_status_text("Check failed (Click to retry)".into());
    assert_eq!(data.get_update_status(), 6);
    assert_eq!(data.get_update_status_text().as_str(), "Check failed (Click to retry)");
}
