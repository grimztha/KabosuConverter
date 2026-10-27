use another_encoder::ytdlp::{
    build_output_template, detect_version, discover_media, find_ytdlp_binary, sanitize_filename_stem,
    DownloadFormatOption, YtDlpFormatItem, YtDlpMediaInfo,
};

#[test]
fn test_ytdlp_binary_locator() {
    let binary = find_ytdlp_binary();
    assert!(binary.is_some(), "yt-dlp binary should be located in project root path");
    let path = binary.unwrap();
    assert!(path.is_file(), "Located yt-dlp path must be a valid file");
    let path_str = path.to_string_lossy().to_lowercase();
    assert!(path_str.contains("another_encoder"), "yt-dlp must be located inside the project root");
}

#[test]
fn test_ytdlp_format_presets_structure() {
    let mock_info = YtDlpMediaInfo {
        id: Some("dQw4w9WgXcQ".to_string()),
        title: Some("Rick Astley - Never Gonna Give You Up".to_string()),
        uploader: Some("Rick Astley".to_string()),
        channel: Some("RickAstleyVEVO".to_string()),
        duration: Some(213.0),
        duration_string: Some("03:33".to_string()),
        thumbnail: None,
        webpage_url: Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string()),
        formats: Some(vec![
            YtDlpFormatItem {
                format_id: "137".to_string(),
                ext: Some("mp4".to_string()),
                resolution: Some("1920x1080".to_string()),
                width: Some(1920),
                height: Some(1080),
                fps: Some(30.0),
                vcodec: Some("avc1.640028".to_string()),
                acodec: Some("none".to_string()),
                filesize: Some(50000000),
                filesize_approx: None,
                tbr: Some(2500.0),
                format_note: Some("1080p".to_string()),
            },
            YtDlpFormatItem {
                format_id: "251".to_string(),
                ext: Some("webm".to_string()),
                resolution: Some("audio only".to_string()),
                width: None,
                height: None,
                fps: None,
                vcodec: Some("none".to_string()),
                acodec: Some("opus".to_string()),
                filesize: Some(3500000),
                filesize_approx: None,
                tbr: Some(130.0),
                format_note: Some("medium".to_string()),
            },
        ]),
    };

    assert_eq!(mock_info.id.as_deref(), Some("dQw4w9WgXcQ"));
    assert_eq!(mock_info.duration_string.as_deref(), Some("03:33"));

    // Verify format generation
    let options = [
        DownloadFormatOption {
            display_label: "Best Quality (Auto Best Video + Best Audio, Recommended)".to_string(),
            format_spec: "bv*+ba/b".to_string(),
            is_audio_only: false,
            audio_ext: None,
            container_ext: "mp4".to_string(),
        },
        DownloadFormatOption {
            display_label: "Audio Only — MP3 (High Quality 320 kbps)".to_string(),
            format_spec: "ba/b".to_string(),
            is_audio_only: true,
            audio_ext: Some("mp3".to_string()),
            container_ext: "mp3".to_string(),
        },
    ];

    assert_eq!(options.len(), 2);
    assert!(!options[0].is_audio_only);
    assert!(options[1].is_audio_only);
    assert_eq!(options[1].audio_ext.as_deref(), Some("mp3"));
}

#[test]
fn test_build_output_template_and_sanitization() {
    // 1. None or empty string defaults to standard template
    assert_eq!(build_output_template(None), "%(title)s [%(id)s].%(ext)s");
    assert_eq!(build_output_template(Some("")), "%(title)s [%(id)s].%(ext)s");
    assert_eq!(build_output_template(Some("   ")), "%(title)s [%(id)s].%(ext)s");

    // 2. Custom filename without extension
    assert_eq!(build_output_template(Some("My Vacation Video")), "My Vacation Video.%(ext)s");

    // 3. Custom filename with extension (extension stripped so %(ext)s can handle dynamically)
    assert_eq!(build_output_template(Some("My Podcast.mp3")), "My Podcast.%(ext)s");
    assert_eq!(build_output_template(Some("4k_gameplay.mp4")), "4k_gameplay.%(ext)s");

    // 4. Custom filename with Windows forbidden characters: \ / : * ? " < > |
    let template = build_output_template(Some("Illegal: Char * Test? <World> | Cool / Slanted\\Name"));
    assert_eq!(template, "Illegal_ Char _ Test_ _World_ _ Cool _ Slanted_Name.%(ext)s");

    // 5. Explicit %(ext)s preserved
    assert_eq!(build_output_template(Some("custom_pattern_%(id)s.%(ext)s")), "custom_pattern_%(id)s.%(ext)s");

    // 6. Sanitization edge cases
    assert_eq!(sanitize_filename_stem("   ...   "), "downloaded_media");
    assert_eq!(sanitize_filename_stem("Valid Name"), "Valid Name");
}

#[test]
fn test_ytdlp_url_validation_security() {
    let bin = find_ytdlp_binary().expect("yt-dlp binary exists in project root");

    // 1. Empty or whitespace URL
    let empty_res = discover_media(&bin, "");
    assert!(empty_res.is_err());
    assert!(empty_res.unwrap_err().to_string().contains("cannot be empty"));

    let whitespace_res = discover_media(&bin, "   \t\n  ");
    assert!(whitespace_res.is_err());
    assert!(whitespace_res.unwrap_err().to_string().contains("cannot be empty"));

    // 2. CLI flag injection attempts (CWE-88)
    let flag_res = discover_media(&bin, "--help");
    assert!(flag_res.is_err());
    assert!(flag_res.unwrap_err().to_string().contains("cannot begin with a command flag"));

    let exec_res = discover_media(&bin, "--exec id");
    assert!(exec_res.is_err());
    assert!(exec_res.unwrap_err().to_string().contains("cannot begin with a command flag"));

    let single_flag_res = discover_media(&bin, "-o evil.bat");
    assert!(single_flag_res.is_err());
    assert!(single_flag_res.unwrap_err().to_string().contains("cannot begin with a command flag"));
}

#[test]
fn test_parse_downloaded_filepath_bracket_filenames_and_tags() {
    use another_encoder::ytdlp::parse_downloaded_filepath;
    use std::path::{Path, PathBuf};

    let dest_dir = Path::new("C:\\Media\\Downloads");

    // 1. yt-dlp logging tags must NOT be parsed as filepaths
    assert_eq!(parse_downloaded_filepath("[download] Destination: video.mp4", dest_dir), None);
    assert_eq!(parse_downloaded_filepath("[download]  45.0% of 100.00MiB at 10.00MiB/s ETA 00:05", dest_dir), None);
    assert_eq!(parse_downloaded_filepath("[info] dQw4w9WgXcQ: Downloading webpage", dest_dir), None);
    assert_eq!(parse_downloaded_filepath("[youtube] Extracting URL: https://...", dest_dir), None);
    assert_eq!(parse_downloaded_filepath("[Merger] Merging formats into \"out.mp4\"", dest_dir), None);
    assert_eq!(parse_downloaded_filepath("[ExtractAudio] Destination: out.mp3", dest_dir), None);
    assert_eq!(parse_downloaded_filepath("[FixupM3u8] Fixing...", dest_dir), None);
    assert_eq!(parse_downloaded_filepath("[ffmpeg] Fixing container in \"out.mp4\"", dest_dir), None);
    assert_eq!(parse_downloaded_filepath("download:50.0%|10.0MiB/s|00:05|50.0MiB", dest_dir), None);
    assert_eq!(parse_downloaded_filepath("", dest_dir), None);
    assert_eq!(parse_downloaded_filepath("   ", dest_dir), None);

    // 2. Relative filename starting with brackets (e.g. [1080p] Video Title.mp4)
    let bracket_relative = "[1080p] Video Title [dQw4w9WgXcQ].mp4";
    let res = parse_downloaded_filepath(bracket_relative, dest_dir);
    assert_eq!(res, Some(dest_dir.join(bracket_relative)));

    // 3. Absolute path on disk
    let abs_path = if cfg!(windows) {
        "D:\\media\\[1080p] Video Title.mp4"
    } else {
        "/media/[1080p] Video Title.mp4"
    };
    let res_abs = parse_downloaded_filepath(abs_path, dest_dir);
    assert_eq!(res_abs, Some(PathBuf::from(abs_path)));

    // 4. Standard clean filenames
    let normal_rel = "Rick_Astley_Never_Gonna_Give_You_Up.mp4";
    assert_eq!(parse_downloaded_filepath(normal_rel, dest_dir), Some(dest_dir.join(normal_rel)));
}

#[test]
fn test_ytdlp_version_detection() {
    if let Some(path) = find_ytdlp_binary() {
        let (ver, badge) = detect_version(&path);
        assert!(!ver.is_empty(), "Version string must not be empty");
        assert!(badge.contains("yt-dlp"), "Badge should contain 'yt-dlp'");
        assert!(badge.contains(&ver), "Badge should contain version");
    }
}
