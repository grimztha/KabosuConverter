use std::path::PathBuf;
use slint::{ComponentHandle, SharedString};
use crate::context::AppContext;
use crate::{AppData, AppLogic, MainWindow};
use kabosu_converter::ffmpeg::{AudioCodecChoice, HwAccel, VideoCodec};
use kabosu_converter::settings::{self, AppSettings};

/// Builds an `AppSettings` snapshot from the current Slint `AppData` state.
pub fn extract_settings_from_app_data(data: &AppData) -> AppSettings {
    let func_idx = data.get_function_index();
    let video_codec = VideoCodec::from_function_index(func_idx);

    let hw_accel_idx = data.get_hw_accel_index();
    let hw_accel = HwAccel::from_index(hw_accel_idx);

    let audio_codec_idx = data.get_audio_codec_index();
    let audio_codec = AudioCodecChoice::from_index(audio_codec_idx);

    let audio_br_idx = data.get_audio_bitrate_index();
    let audio_bitrate = AudioCodecChoice::bitrate_from_index(audio_br_idx);

    let out_dir = data.get_output_directory().trim().to_string();
    let default_output_dir = if !out_dir.is_empty() && out_dir != "Same as source" {
        Some(out_dir)
    } else {
        None
    };

    let ytdlp_dir = data.get_ytdlp_output_dir().trim().to_string();
    let default_ytdlp_output_dir = if !ytdlp_dir.is_empty() {
        Some(ytdlp_dir)
    } else {
        None
    };

    AppSettings {
        default_output_dir,
        default_ytdlp_output_dir,

        default_video_codec: video_codec,
        default_function_index: func_idx,
        default_hw_accel: hw_accel,
        default_hw_accel_index: hw_accel_idx,
        default_rate_control_index: data.get_rate_control_index(),
        default_crf: data.get_crf_value().round() as u32,
        default_video_bitrate_index: data.get_video_bitrate_index(),
        default_preset_index: data.get_preset_index(),
        default_resolution_index: data.get_resolution_index(),
        default_fps_index: data.get_fps_index(),
        default_deinterlace_index: data.get_deinterlace_index(),
        default_faststart: data.get_faststart(),
        default_container_index: data.get_container_index(),

        default_audio_codec: audio_codec,
        default_audio_codec_index: audio_codec_idx,
        default_audio_bitrate_kbps: audio_bitrate,
        default_audio_bitrate_index: audio_br_idx,
        default_audio_sample_rate_index: data.get_audio_sample_rate_index(),
        default_normalize_index: data.get_normalize_index(),

        open_folder_on_finish: data.get_open_folder_on_finish(),
        play_sound_on_finish: data.get_play_sound_on_finish(),

        show_volume_meter: data.get_show_volume_meter(),
        player_volume: data.get_player_volume(),
        edit_panel_expanded: data.get_edit_panel_expanded(),
        crop_arrow_keys_enabled: data.get_crop_arrow_keys_enabled(),
        crop_nudge_step: data.get_crop_nudge_step(),
        ytdlp_selected_format_index: data.get_ytdlp_selected_format_index(),
    }
}

/// Saves the current Slint AppData settings to disk.
pub fn save_settings_from_app_data(data: &AppData) {
    let s = extract_settings_from_app_data(data);
    let _ = settings::save_settings(&s);
}

/// Applies persistent AppSettings to Slint AppData.
pub fn apply_settings_to_app_data(data: &AppData, settings: &AppSettings) {
    data.set_open_folder_on_finish(settings.open_folder_on_finish);
    data.set_play_sound_on_finish(settings.play_sound_on_finish);

    if let Some(ref dir) = settings.default_output_dir {
        if !dir.is_empty() && dir != "Same as source" {
            data.set_output_directory(SharedString::from(dir));
        }
    }

    if let Some(ref dl_dir) = settings.default_ytdlp_output_dir {
        if !dl_dir.is_empty() {
            data.set_ytdlp_output_dir(SharedString::from(dl_dir));
        }
    } else if let Some(ref dir) = settings.default_output_dir {
        if !dir.is_empty() && dir != "Same as source" {
            data.set_ytdlp_output_dir(SharedString::from(dir));
        }
    } else {
        let default_dl_dir = dirs::download_dir()
            .or_else(dirs::video_dir)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        data.set_ytdlp_output_dir(SharedString::from(default_dl_dir.to_string_lossy().as_ref()));
    }

    let func_idx = if settings.default_function_index >= 0 {
        settings.default_function_index
    } else {
        settings.default_video_codec.to_function_index()
    };
    data.set_function_index(func_idx);

    let hw_idx = if settings.default_hw_accel_index >= 0 {
        settings.default_hw_accel_index
    } else {
        settings.default_hw_accel.to_index()
    };
    data.set_hw_accel_index(hw_idx);

    data.set_rate_control_index(settings.default_rate_control_index);
    data.set_crf_value(settings.default_crf as f32);
    data.set_video_bitrate_index(settings.default_video_bitrate_index);
    data.set_preset_index(settings.default_preset_index);
    data.set_resolution_index(settings.default_resolution_index);
    data.set_fps_index(settings.default_fps_index);
    data.set_deinterlace_index(settings.default_deinterlace_index);
    data.set_faststart(settings.default_faststart);
    data.set_container_index(settings.default_container_index);

    let audio_codec_idx = if settings.default_audio_codec_index >= 0 {
        settings.default_audio_codec_index
    } else {
        settings.default_audio_codec.to_index()
    };
    data.set_audio_codec_index(audio_codec_idx);

    let audio_br_idx = if settings.default_audio_bitrate_index >= 0 {
        settings.default_audio_bitrate_index
    } else {
        AudioCodecChoice::bitrate_to_index(settings.default_audio_bitrate_kbps)
    };
    data.set_audio_bitrate_index(audio_br_idx);
    data.set_audio_sample_rate_index(settings.default_audio_sample_rate_index);
    data.set_normalize_index(settings.default_normalize_index);

    data.set_show_volume_meter(settings.show_volume_meter);
    data.set_player_volume(settings.player_volume.clamp(0.0, 1.0));
    data.set_edit_panel_expanded(settings.edit_panel_expanded);
    data.set_crop_arrow_keys_enabled(settings.crop_arrow_keys_enabled);
    data.set_crop_nudge_step(settings.crop_nudge_step.max(1));
    data.set_ytdlp_selected_format_index(settings.ytdlp_selected_format_index.max(0));
}

/// Registers general settings handlers for user preferences.
pub fn register(ui: &MainWindow, _ctx: &AppContext) {
    // Save current settings callback (triggered when user alters any settings control)
    {
        let ui_save = ui.as_weak();
        ui.global::<AppLogic>().on_save_settings(move || {
            if let Some(ui) = ui_save.upgrade() {
                save_settings_from_app_data(&ui.global::<AppData>());
            }
        });
    }

    // Toggle and persist FastStart setting
    {
        let ui_fast = ui.as_weak();
        ui.global::<AppLogic>().on_set_faststart(move |val| {
            if let Some(ui) = ui_fast.upgrade() {
                let data = ui.global::<AppData>();
                data.set_faststart(val);
                save_settings_from_app_data(&data);
            }
        });
    }

    // Update and persist whether to open destination folder when export finishes
    {
        let ui_folder = ui.as_weak();
        ui.global::<AppLogic>().on_set_open_folder_on_finish(move |val| {
            if let Some(ui) = ui_folder.upgrade() {
                let data = ui.global::<AppData>();
                data.set_open_folder_on_finish(val);
                save_settings_from_app_data(&data);
            }
        });
    }

    // Update and persist whether to play a notification sound when export completes
    {
        let ui_sound = ui.as_weak();
        ui.global::<AppLogic>().on_set_play_sound_on_finish(move |val| {
            if let Some(ui) = ui_sound.upgrade() {
                let data = ui.global::<AppData>();
                data.set_play_sound_on_finish(val);
                save_settings_from_app_data(&data);
            }
        });
    }
}
