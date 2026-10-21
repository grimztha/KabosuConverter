use another_encoder::ytdlp::{
    build_output_template, discover_media, find_ytdlp_binary, sanitize_filename_stem,
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
