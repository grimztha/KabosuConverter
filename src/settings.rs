use std::env;
use std::fs;
use std::path::PathBuf;
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ffmpeg::{AudioCodecChoice, HwAccel, VideoCodec};

pub const APP_NAME: &str = "Kabosu";
pub const LEGACY_APP_NAMES: &[&str] = &["KabosuConverter", "AnotherEncoder"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    // Directories
    pub default_output_dir: Option<String>,
    pub default_ytdlp_output_dir: Option<String>,

    // Video & Transcoder Options
    pub default_video_codec: VideoCodec,
    pub default_function_index: i32,
    pub default_hw_accel: HwAccel,
    pub default_hw_accel_index: i32,
    pub default_rate_control_index: i32,
    pub default_crf: u32,
    pub default_video_bitrate_index: i32,
    pub default_preset_index: i32,
    pub default_resolution_index: i32,
    pub default_fps_index: i32,
    pub default_deinterlace_index: i32,
    pub default_faststart: bool,
    pub default_container_index: i32,

    // Audio Options
    pub default_audio_codec: AudioCodecChoice,
    pub default_audio_codec_index: i32,
    pub default_audio_bitrate_kbps: u32,
    pub default_audio_bitrate_index: i32,
    pub default_audio_sample_rate_index: i32,
    pub default_normalize_index: i32,

    // General Preferences
    pub open_folder_on_finish: bool,
    pub play_sound_on_finish: bool,

    // UI & Workspace State
    pub show_volume_meter: bool,
    pub player_volume: f32,
    pub edit_panel_expanded: bool,
    pub crop_arrow_keys_enabled: bool,
    pub crop_nudge_step: i32,
    pub ytdlp_selected_format_index: i32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_output_dir: None,
            default_ytdlp_output_dir: None,

            default_video_codec: VideoCodec::H264,
            default_function_index: 0,
            default_hw_accel: HwAccel::Auto,
            default_hw_accel_index: 0,
            default_rate_control_index: 0,
            default_crf: 23,
            default_video_bitrate_index: 2, // 6000 kbps
            default_preset_index: 3,        // medium
            default_resolution_index: 0,    // original source
            default_fps_index: 0,           // original source
            default_deinterlace_index: 0,   // none
            default_faststart: true,
            default_container_index: 0,     // .mp4

            default_audio_codec: AudioCodecChoice::AAC,
            default_audio_codec_index: 0,   // AAC
            default_audio_bitrate_kbps: 192,
            default_audio_bitrate_index: 2, // 192 kbps
            default_audio_sample_rate_index: 0, // keep source
            default_normalize_index: 0,     // disabled

            open_folder_on_finish: true,
            play_sound_on_finish: false,

            show_volume_meter: true,
            player_volume: 1.0,
            edit_panel_expanded: true,
            crop_arrow_keys_enabled: true,
            crop_nudge_step: 10,
            ytdlp_selected_format_index: 0,
        }
    }
}

/// Returns `true` when the app is running in portable/standalone mode.
/// This is enabled if `portable`, `.standalone`, or `settings.json` sits next to the executable.
pub fn is_standalone_mode() -> bool {
    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            if exe_dir.join("portable").exists()
                || exe_dir.join(".standalone").exists()
                || exe_dir.join("settings.json").exists()
            {
                return true;
            }
        }
    }
    false
}

/// Returns the root folder for app data and binaries.
/// Uses the executable folder in standalone mode, or the OS local app data folder otherwise.
pub fn get_app_dir() -> PathBuf {
    if is_standalone_mode() {
        if let Ok(exe_path) = env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                return exe_dir.to_path_buf();
            }
        }
    }

    dirs::data_local_dir()
        .map(|p| p.join(APP_NAME))
        .unwrap_or_else(|| {
            dirs::config_dir()
                .map(|p| p.join(APP_NAME))
                .unwrap_or_else(|| PathBuf::from("."))
        })
}

/// Returns the path to `settings.json`, falling back to the legacy config folder if present.
pub fn get_settings_path() -> Result<PathBuf> {
    let app_dir = get_app_dir();
    let local_settings = app_dir.join("settings.json");

    if !local_settings.exists() && !is_standalone_mode() {
        if let Some(config_dir) = dirs::config_dir() {
            let app_path = config_dir.join(APP_NAME).join("settings.json");
            if app_path.exists() {
                return Ok(app_path);
            }
            for legacy in LEGACY_APP_NAMES {
                let legacy_path = config_dir.join(legacy).join("settings.json");
                if legacy_path.exists() {
                    return Ok(legacy_path);
                }
            }
        }
    }

    Ok(local_settings)
}

/// Loads saved settings from disk, or returns defaults if the file is missing or unreadable.
pub fn load_settings() -> AppSettings {
    if let Ok(path) = get_settings_path() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(settings) = serde_json::from_str(&content) {
                return settings;
            }
        }
    }
    AppSettings::default()
}

/// Writes settings to a temp file first, then swaps it into place to avoid half-written files.
pub fn save_settings(settings: &AppSettings) -> Result<()> {
    let path = get_settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let content = serde_json::to_string_pretty(settings)?;
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, &content)?;

    // Windows rename fails if the target already exists, so remove it first.
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    if fs::rename(&tmp_path, &path).is_err() {
        let _ = fs::remove_file(&tmp_path);
        fs::write(&path, content)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partial_json_deserialization_uses_defaults() {
        let partial_json = r#"{"default_crf": 18, "open_folder_on_finish": false}"#;
        let parsed: AppSettings = serde_json::from_str(partial_json).expect("Partial JSON must deserialize cleanly");
        assert_eq!(parsed.default_crf, 18);
        assert!(!parsed.open_folder_on_finish);
        assert_eq!(parsed.default_audio_bitrate_kbps, 192);
        assert_eq!(parsed.default_video_codec, VideoCodec::H264);
        assert_eq!(parsed.default_function_index, 0);
        assert_eq!(parsed.default_preset_index, 3);
        assert_eq!(parsed.default_container_index, 0);
        assert_eq!(parsed.player_volume, 1.0);
        assert!(parsed.show_volume_meter);
        assert!(parsed.edit_panel_expanded);
    }

    #[test]
    fn test_full_settings_serialization_roundtrip() {
        let settings = AppSettings {
            default_output_dir: Some("D:/Exports".to_string()),
            default_ytdlp_output_dir: Some("D:/Downloads".to_string()),
            default_video_codec: VideoCodec::H265,
            default_function_index: 1,
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
            player_volume: 0.75,
            edit_panel_expanded: false,
            crop_arrow_keys_enabled: false,
            crop_nudge_step: 25,
            ytdlp_selected_format_index: 3,
        };

        let json = serde_json::to_string_pretty(&settings).expect("Must serialize cleanly");
        let reloaded: AppSettings = serde_json::from_str(&json).expect("Must deserialize cleanly");

        assert_eq!(reloaded.default_output_dir.as_deref(), Some("D:/Exports"));
        assert_eq!(reloaded.default_ytdlp_output_dir.as_deref(), Some("D:/Downloads"));
        assert_eq!(reloaded.default_video_codec, VideoCodec::H265);
        assert_eq!(reloaded.default_function_index, 1);
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
        assert!((reloaded.player_volume - 0.75).abs() < f32::EPSILON);
        assert!(!reloaded.edit_panel_expanded);
        assert!(!reloaded.crop_arrow_keys_enabled);
        assert_eq!(reloaded.crop_nudge_step, 25);
        assert_eq!(reloaded.ytdlp_selected_format_index, 3);
    }

    #[test]
    fn test_app_dir_resolves() {
        let app_dir = get_app_dir();
        assert!(!app_dir.as_os_str().is_empty());
    }

    #[test]
    fn test_settings_toggle_switches_serialization_roundtrip() {
        let mut settings = AppSettings::default();
        assert!(settings.open_folder_on_finish);
        assert!(!settings.play_sound_on_finish);

        settings.open_folder_on_finish = false;
        settings.play_sound_on_finish = true;

        let json = serde_json::to_string(&settings).expect("Must serialize");
        let reloaded: AppSettings = serde_json::from_str(&json).expect("Must deserialize");

        assert!(!reloaded.open_folder_on_finish);
        assert!(reloaded.play_sound_on_finish);
    }
}
