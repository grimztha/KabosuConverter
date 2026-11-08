mod common;
use tempfile::tempdir;
use slint::ComponentHandle;
use kabosu_converter::ffmpeg::{AudioCodecChoice, HwAccel, VideoCodec};
use kabosu_converter::settings::AppSettings;

slint::include_modules!();

#[test]
fn test_settings_disk_save_and_reload_roundtrip() {
    let temp = tempdir().expect("Must create tempdir");
    let settings_file = temp.path().join("settings.json");

    let custom = AppSettings {
        default_output_dir: Some("D:/Exports".to_string()),
        default_ytdlp_output_dir: Some("D:/Downloads".to_string()),
        default_video_codec: VideoCodec::AV1,
        default_function_index: 2,
        default_hw_accel: HwAccel::Nvenc,
        default_hw_accel_index: 1,
        default_rate_control_index: 1,
        default_crf: 18,
        default_video_bitrate_index: 0,
        default_preset_index: 4,
        default_resolution_index: 1,
        default_fps_index: 8,
        default_deinterlace_index: 1,
        default_faststart: false,
        default_container_index: 1,
        default_audio_codec: AudioCodecChoice::Opus,
        default_audio_codec_index: 2,
        default_audio_bitrate_kbps: 256,
        default_audio_bitrate_index: 1,
        default_audio_sample_rate_index: 1,
        default_normalize_index: 2,
        open_folder_on_finish: false,
        play_sound_on_finish: true,
        show_volume_meter: false,
        player_volume: 0.85,
        edit_panel_expanded: false,
        crop_arrow_keys_enabled: false,
        crop_nudge_step: 30,
        ytdlp_selected_format_index: 2,
    };

    // Serialize and write to disk
    let json_content = serde_json::to_string_pretty(&custom).expect("Must serialize");
    std::fs::write(&settings_file, &json_content).expect("Must write settings file");

    // Reload from disk
    let reloaded_content = std::fs::read_to_string(&settings_file).expect("Must read settings file");
    let reloaded: AppSettings = serde_json::from_str(&reloaded_content).expect("Must deserialize");

    assert_eq!(reloaded.default_output_dir.as_deref(), Some("D:/Exports"));
    assert_eq!(reloaded.default_ytdlp_output_dir.as_deref(), Some("D:/Downloads"));
    assert_eq!(reloaded.default_video_codec, VideoCodec::AV1);
    assert_eq!(reloaded.default_function_index, 2);
    assert_eq!(reloaded.default_hw_accel, HwAccel::Nvenc);
    assert_eq!(reloaded.default_hw_accel_index, 1);
    assert_eq!(reloaded.default_rate_control_index, 1);
    assert_eq!(reloaded.default_crf, 18);
    assert_eq!(reloaded.default_video_bitrate_index, 0);
    assert_eq!(reloaded.default_preset_index, 4);
    assert_eq!(reloaded.default_resolution_index, 1);
    assert_eq!(reloaded.default_fps_index, 8);
    assert_eq!(reloaded.default_deinterlace_index, 1);
    assert!(!reloaded.default_faststart);
    assert_eq!(reloaded.default_container_index, 1);
    assert_eq!(reloaded.default_audio_codec, AudioCodecChoice::Opus);
    assert_eq!(reloaded.default_audio_codec_index, 2);
    assert_eq!(reloaded.default_audio_bitrate_kbps, 256);
    assert_eq!(reloaded.default_audio_bitrate_index, 1);
    assert_eq!(reloaded.default_audio_sample_rate_index, 1);
    assert_eq!(reloaded.default_normalize_index, 2);
    assert!(!reloaded.open_folder_on_finish);
    assert!(reloaded.play_sound_on_finish);
    assert!(!reloaded.show_volume_meter);
    assert!((reloaded.player_volume - 0.85).abs() < 0.001);
    assert!(!reloaded.edit_panel_expanded);
    assert!(!reloaded.crop_arrow_keys_enabled);
    assert_eq!(reloaded.crop_nudge_step, 30);
    assert_eq!(reloaded.ytdlp_selected_format_index, 2);
}

#[test]
fn test_ui_app_data_settings_sync_lifecycle() {
    common::init_test_backend();
    let ui = MainWindow::new().expect("MainWindow must instantiate");
    let app_data = ui.global::<AppData>();

    // Test default values
    assert!(app_data.get_open_folder_on_finish());
    assert!(!app_data.get_play_sound_on_finish());
    assert_eq!(app_data.get_function_index(), 0);
    assert_eq!(app_data.get_hw_accel_index(), 0);
    assert_eq!(app_data.get_rate_control_index(), 0);
    assert_eq!(app_data.get_crf_value(), 23.0);
    assert_eq!(app_data.get_video_bitrate_index(), 2);
    assert_eq!(app_data.get_preset_index(), 3);
    assert_eq!(app_data.get_resolution_index(), 0);
    assert_eq!(app_data.get_fps_index(), 0);
    assert_eq!(app_data.get_deinterlace_index(), 0);
    assert!(app_data.get_faststart());
    assert_eq!(app_data.get_container_index(), 0);
    assert_eq!(app_data.get_audio_codec_index(), 0);
    assert_eq!(app_data.get_audio_bitrate_index(), 2);
    assert_eq!(app_data.get_audio_sample_rate_index(), 0);
    assert_eq!(app_data.get_normalize_index(), 0);
    assert!(app_data.get_show_volume_meter());
    assert!((app_data.get_player_volume() - 1.0).abs() < 0.001);
    assert!(app_data.get_edit_panel_expanded());
    assert!(app_data.get_crop_arrow_keys_enabled());
    assert_eq!(app_data.get_crop_nudge_step(), 10);
    assert_eq!(app_data.get_ytdlp_selected_format_index(), 0);

    // Mutate properties simulating user interaction in the Settings tab
    app_data.set_open_folder_on_finish(false);
    app_data.set_play_sound_on_finish(true);
    app_data.set_function_index(3);
    app_data.set_hw_accel_index(1);
    app_data.set_rate_control_index(1);
    app_data.set_crf_value(16.0);
    app_data.set_video_bitrate_index(0);
    app_data.set_preset_index(5);
    app_data.set_resolution_index(3);
    app_data.set_fps_index(7);
    app_data.set_deinterlace_index(1);
    app_data.set_faststart(false);
    app_data.set_container_index(2);
    app_data.set_audio_codec_index(1);
    app_data.set_audio_bitrate_index(0);
    app_data.set_audio_sample_rate_index(1);
    app_data.set_normalize_index(2);
    app_data.set_show_volume_meter(false);
    app_data.set_player_volume(0.65);
    app_data.set_edit_panel_expanded(false);
    app_data.set_crop_arrow_keys_enabled(false);
    app_data.set_crop_nudge_step(15);
    app_data.set_ytdlp_selected_format_index(1);

    // Verify mutations took effect
    assert!(!app_data.get_open_folder_on_finish());
    assert!(app_data.get_play_sound_on_finish());
    assert_eq!(app_data.get_function_index(), 3);
    assert_eq!(app_data.get_hw_accel_index(), 1);
    assert_eq!(app_data.get_rate_control_index(), 1);
    assert_eq!(app_data.get_crf_value(), 16.0);
    assert_eq!(app_data.get_video_bitrate_index(), 0);
    assert_eq!(app_data.get_preset_index(), 5);
    assert_eq!(app_data.get_resolution_index(), 3);
    assert_eq!(app_data.get_fps_index(), 7);
    assert_eq!(app_data.get_deinterlace_index(), 1);
    assert!(!app_data.get_faststart());
    assert_eq!(app_data.get_container_index(), 2);
    assert_eq!(app_data.get_audio_codec_index(), 1);
    assert_eq!(app_data.get_audio_bitrate_index(), 0);
    assert_eq!(app_data.get_audio_sample_rate_index(), 1);
    assert_eq!(app_data.get_normalize_index(), 2);
    assert!(!app_data.get_show_volume_meter());
    assert!((app_data.get_player_volume() - 0.65).abs() < 0.001);
    assert!(!app_data.get_edit_panel_expanded());
    assert!(!app_data.get_crop_arrow_keys_enabled());
    assert_eq!(app_data.get_crop_nudge_step(), 15);
    assert_eq!(app_data.get_ytdlp_selected_format_index(), 1);
}
