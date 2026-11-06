use std::env;
use std::fs;
use std::path::PathBuf;
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ffmpeg::{AudioCodecChoice, HwAccel, VideoCodec};

pub const APP_NAME: &str = "AnotherEncoder";

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
        }
    }
}

/// Checks if the application is running in standalone (portable) mode.
/// Standalone mode is triggered when a `portable` or `.standalone` marker exists
/// in the executable directory, or if `settings.json` is located next to the executable.
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

/// Returns the primary application directory:
/// - Standalone / portable: executable's parent directory
/// - Standard installation: OS-designated local app data folder
///   - Windows: `%LOCALAPPDATA%\AnotherEncoder`
///   - macOS: `~/Library/Application Support/AnotherEncoder`
///   - Linux: `~/.local/share/AnotherEncoder`
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

/// Resolves path to settings.json.
/// Uses the OS-designated app data folder, migrating from legacy config directory if present.
pub fn get_settings_path() -> Result<PathBuf> {
    let app_dir = get_app_dir();
    let local_settings = app_dir.join("settings.json");

    // If local settings don't exist yet, check legacy Roaming config directory for seamless migration
    if !local_settings.exists() && !is_standalone_mode() {
        if let Some(config_dir) = dirs::config_dir() {
            let legacy_path = config_dir.join(APP_NAME).join("settings.json");
            if legacy_path.exists() {
                return Ok(legacy_path);
            }
        }
    }

    Ok(local_settings)
}

/// Loads persistent application settings.
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

/// Saves persistent application settings atomically via temporary file replacement.
pub fn save_settings(settings: &AppSettings) -> Result<()> {
    let path = get_settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let content = serde_json::to_string_pretty(settings)?;
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, &content)?;

    // Atomic replace on Windows/POSIX
    if path.exists() {
        let _ = fs::remove_file(&path);
    }
    if let Err(e) = fs::rename(&tmp_path, &path) {
        // Fallback direct write if filesystem rename fails
        let _ = fs::remove_file(&tmp_path);
        fs::write(&path, content)?;
        return Err(e.into());
    }
    Ok(())
}

/// Uninstaller helper: cleanly wipes the application data directory (%LOCALAPPDATA%\AnotherEncoder).
/// Leaving the host machine completely clean.
pub fn wipe_app_data() -> std::io::Result<()> {
    let app_dir = get_app_dir();
    if app_dir.exists() && !is_standalone_mode() {
        fs::remove_dir_all(app_dir)?;
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
    }

    #[test]
    fn test_app_dir_resolves() {
        let app_dir = get_app_dir();
        assert!(!app_dir.as_os_str().is_empty());
    }
}
