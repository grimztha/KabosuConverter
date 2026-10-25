use std::fs;
use std::path::PathBuf;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::ffmpeg::{AudioCodecChoice, HwAccel, VideoCodec};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub default_output_dir: Option<String>,
    pub default_video_codec: VideoCodec,
    pub default_hw_accel: HwAccel,
    pub default_crf: u32,
    pub default_audio_codec: AudioCodecChoice,
    pub default_audio_bitrate_kbps: u32,
    pub open_folder_on_finish: bool,
    pub play_sound_on_finish: bool,
    pub theme_dark: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_output_dir: None,
            default_video_codec: VideoCodec::H264,
            default_hw_accel: HwAccel::Auto,
            default_crf: 23,
            default_audio_codec: AudioCodecChoice::AAC,
            default_audio_bitrate_kbps: 192,
            open_folder_on_finish: true,
            play_sound_on_finish: false,
            theme_dark: true,
        }
    }
}

pub fn get_settings_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir().context("No user config directory found")?;
    let path = config_dir.join("AnotherEncoder").join("settings.json");
    Ok(path)
}

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

pub fn save_settings(settings: &AppSettings) -> Result<()> {
    let path = get_settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let content = serde_json::to_string_pretty(settings)?;
    fs::write(&path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partial_json_deserialization_uses_defaults() {
        // Test that old/partial JSON without new fields deserializes properly instead of failing
        let partial_json = r#"{"default_crf": 18, "open_folder_on_finish": false}"#;
        let parsed: AppSettings = serde_json::from_str(partial_json).expect("Partial JSON must deserialize cleanly");
        assert_eq!(parsed.default_crf, 18);
        assert!(!parsed.open_folder_on_finish);
        // Default fields should be populated automatically
        assert_eq!(parsed.default_audio_bitrate_kbps, 192);
        assert_eq!(parsed.default_video_codec, VideoCodec::H264);
        assert!(parsed.theme_dark);
    }
}
