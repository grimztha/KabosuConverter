use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::thread;

use slint::{ComponentHandle, Model, SharedString};

use another_encoder::ffmpeg::{
    format_duration_hms, parse_timecode_to_seconds, AudioCodecChoice, CropRect, DeinterlaceMode,
    EncodeJobConfig, EncoderFunction, FFmpegCommandBuilder, FFmpegRunner, HwAccel, ProResProfile,
    RateControlMode, ResolutionPreset, VideoCodec, VideoRotation,
};
use crate::context::AppContext;
use crate::handlers::editor::rotation_from_index;
use crate::utils::open_folder_in_explorer;
use crate::{AppData, AppLogic, MainWindow, QueueFile};

pub fn register(ui: &MainWindow, ctx: &AppContext) {
    // Handle Select Output Directory
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_select_output_dir(move || {
            if let Some(folder) = rfd::FileDialog::new()
                .set_title("Select Output Directory")
                .pick_folder()
            {
                if let Some(ui) = ui_handle.upgrade() {
                    let folder_str = folder.to_string_lossy().to_string();
                    ui.global::<AppData>().set_output_directory(SharedString::from(
                        &folder_str,
                    ));
                    ui.global::<AppData>().set_ytdlp_output_dir(SharedString::from(
                        &folder_str,
                    ));
                    let mut s = another_encoder::settings::load_settings();
                    s.default_output_dir = Some(folder_str);
                    let _ = another_encoder::settings::save_settings(&s);
                }
            }
        });
    }

    // Handle Open Output Directory
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_open_output_dir(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let dir_str = app_data.get_output_directory().trim().to_string();
                let target = if dir_str == "Same as source" || dir_str.is_empty() {
                    let sel_path = app_data.get_selected_file_path().to_string();
                    if !sel_path.is_empty() {
                        Path::new(&sel_path)
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
                    } else {
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                    }
                } else {
                    PathBuf::from(dir_str)
                };

                open_folder_in_explorer(&target);
            }
        });
    }

    // Handle Toggle Logs
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_toggle_logs(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let cur = ui.global::<AppData>().get_show_logs();
                ui.global::<AppData>().set_show_logs(!cur);
            }
        });
    }

    // Handle Switch Tab
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_switch_tab(move |tab_idx| {
            if let Some(ui) = ui_handle.upgrade() {
                ui.global::<AppData>().set_active_tab(tab_idx);
            }
        });
    }

    // Handle Quick Lossless Cut
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_quick_lossless_cut(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if !app_data.get_has_active_trim() || !app_data.get_player_has_media() {
                    return;
                }
                // Function 7 corresponds to LosslessCut
                app_data.set_function_index(7);
                ui.global::<AppLogic>().invoke_run_task();
            }
        });
    }

    // Handle Cancel Processing
    {
        let cancel_flag = ctx.cancel_flag.clone();
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_cancel_processing(move || {
            cancel_flag.store(true, Ordering::SeqCst);
            if let Some(ui) = ui_handle.upgrade() {
                ui.global::<AppData>()
                    .set_progress_text(SharedString::from("Cancelling..."));
            }
        });
    }

    // Handle Run Task (formerly Start Processing)
    {
        let ui_handle = ui.as_weak();
        let binaries = ctx.binaries.clone();
        let cancel_flag = ctx.cancel_flag.clone();

        ui.global::<AppLogic>().on_run_task(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.global::<AppData>().get_is_processing() {
                    return;
                }
                let items: Vec<QueueFile> =
                    ui.global::<AppData>().get_queue().iter().collect();
                if items.is_empty() {
                    return;
                }

                // Reset cancellation flag
                cancel_flag.store(false, Ordering::SeqCst);

                // Set processing state
                ui.global::<AppData>().set_is_processing(true);
                ui.global::<AppData>().set_progress_percent(0.0);
                ui.global::<AppData>()
                    .set_progress_text(SharedString::from("Starting..."));

                // Read all parameters from UI
                let func_idx = ui.global::<AppData>().get_function_index();
                let hw_accel_idx = ui.global::<AppData>().get_hw_accel_index();
                let rate_ctrl_idx = ui.global::<AppData>().get_rate_control_index();
                let crf_raw = ui.global::<AppData>().get_crf_value().round() as u32;
                let crf = crf_raw.min(51); // Clamp to valid range [0, 51]
                let video_br_idx = ui.global::<AppData>().get_video_bitrate_index();
                let preset_idx = ui.global::<AppData>().get_preset_index();
                let res_idx = ui.global::<AppData>().get_resolution_index();
                let fps_idx = ui.global::<AppData>().get_fps_index();
                let deinterlace_idx = ui.global::<AppData>().get_deinterlace_index();
                let prores_idx = ui.global::<AppData>().get_prores_profile_index();
                let faststart = ui.global::<AppData>().get_faststart();

                let audio_codec_idx = ui.global::<AppData>().get_audio_codec_index();
                let audio_br_idx = ui.global::<AppData>().get_audio_bitrate_index();
                let audio_sr_idx = ui.global::<AppData>().get_audio_sample_rate_index();
                let normalize_idx = ui.global::<AppData>().get_normalize_index();

                let in_point_str = ui.global::<AppData>().get_in_point().to_string();
                let out_point_str = ui.global::<AppData>().get_out_point().to_string();
                let container_idx = ui.global::<AppData>().get_container_index();
                let out_dir_str = ui.global::<AppData>().get_output_directory().to_string();

                let rot_idx = ui.global::<AppData>().get_video_rotation_index();
                let flip_h = ui.global::<AppData>().get_video_flip_horizontal();
                let flip_v = ui.global::<AppData>().get_video_flip_vertical();
                let crop_enabled = ui.global::<AppData>().get_crop_enabled();
                let crop_x = ui.global::<AppData>().get_crop_x();
                let crop_y = ui.global::<AppData>().get_crop_y();
                let crop_w = ui.global::<AppData>().get_crop_width();
                let crop_h = ui.global::<AppData>().get_crop_height();

                let edit_gain_val = ui.global::<AppData>().get_audio_gain_db();
                let edit_gain_opt = if edit_gain_val.abs() > 0.05 { Some(edit_gain_val) } else { None };
                let edit_norm_enabled = ui.global::<AppData>().get_audio_normalize_enabled();
                let edit_norm_lufs = ui.global::<AppData>().get_audio_normalize_lufs();
                let selected_idx = ui.global::<AppData>().get_selected_file_index();

                // Persist user encoder choices to settings.json
                let chosen_video_codec = match func_idx {
                    0 => VideoCodec::H264,
                    1 => VideoCodec::H265,
                    2 => VideoCodec::AV1,
                    3 => VideoCodec::AppleProRes,
                    4 => VideoCodec::AvidDNxHD,
                    5 => VideoCodec::VP9,
                    6 => VideoCodec::Copy,
                    _ => VideoCodec::H264,
                };
                let chosen_hw_accel = match hw_accel_idx {
                    1 => HwAccel::Nvenc,
                    2 => HwAccel::Qsv,
                    3 => HwAccel::Amf,
                    4 => HwAccel::VideoToolbox,
                    5 => HwAccel::Cpu,
                    _ => HwAccel::Auto,
                };
                let chosen_audio_codec = match audio_codec_idx {
                    1 => AudioCodecChoice::MP3,
                    2 => AudioCodecChoice::Opus,
                    3 => AudioCodecChoice::FLAC,
                    4 => AudioCodecChoice::WavPCM24,
                    5 => AudioCodecChoice::WavPCM16,
                    6 => AudioCodecChoice::AC3,
                    7 => AudioCodecChoice::Copy,
                    _ => AudioCodecChoice::AAC,
                };
                let chosen_audio_bitrate = match audio_br_idx {
                    0 => 320,
                    1 => 256,
                    2 => 192,
                    3 => 128,
                    4 => 96,
                    _ => 64,
                };
                let mut current_settings = another_encoder::settings::load_settings();
                current_settings.default_video_codec = chosen_video_codec;
                current_settings.default_hw_accel = chosen_hw_accel;
                current_settings.default_crf = crf;
                current_settings.default_audio_codec = chosen_audio_codec;
                current_settings.default_audio_bitrate_kbps = chosen_audio_bitrate;
                if out_dir_str != "Same as source" && !out_dir_str.is_empty() {
                    current_settings.default_output_dir = Some(out_dir_str.clone());
                }
                let _ = another_encoder::settings::save_settings(&current_settings);

                let ui_weak = ui_handle.clone();
                let bins = binaries.clone();
                let cancel = cancel_flag.clone();

                // Spawn worker thread for batch queue processing
                thread::spawn(move || {
                    let total_items = items.len();
                    let mut any_errors = false;
                    let mut last_completed_path: Option<PathBuf> = None;

                    for (item_idx, item) in items.iter().enumerate() {
                        if cancel.load(Ordering::SeqCst) {
                            break;
                        }

                        let input_path = item.file_path.to_string();
                        let in_p = Path::new(&input_path);

                        // Determine destination directory and filename
                        let out_container = if func_idx == 12 {
                            ".srt"
                        } else if func_idx == 9 || func_idx == 11 {
                            match container_idx {
                                6 => ".mp3",
                                7 => ".wav",
                                8 => ".flac",
                                _ => match audio_codec_idx {
                                    1 => ".mp3",
                                    2 => ".opus",
                                    3 => ".flac",
                                    4 | 5 => ".wav",
                                    _ => ".m4a",
                                },
                            }
                        } else if func_idx == 3 || func_idx == 4 {
                            // Professional editing codecs (Apple ProRes & Avid DNxHD) require QuickTime (.mov) or Matroska (.mkv) container
                            match container_idx {
                                1 => ".mkv",
                                _ => ".mov",
                            }
                        } else {
                            match container_idx {
                                1 => ".mkv",
                                2 => ".mov",
                                3 => ".webm",
                                4 => ".avi",
                                5 => ".ts",
                                6 => ".mp3",
                                7 => ".wav",
                                8 => ".flac",
                                _ => ".mp4",
                            }
                        };

                        let parent_dir =
                            if out_dir_str != "Same as source" && !out_dir_str.is_empty() {
                                PathBuf::from(&out_dir_str)
                            } else {
                                in_p.parent()
                                    .map(|p| p.to_path_buf())
                                    .unwrap_or_else(|| PathBuf::from("."))
                            };
                        let _ = std::fs::create_dir_all(&parent_dir);

                        let stem = in_p.file_stem().unwrap_or_default().to_string_lossy();
                        let mut output_path =
                            parent_dir.join(format!("{}_encoded{}", stem, out_container));

                        // Avoid overwriting existing output or input file by appending counter if needed
                        let mut counter = 1;
                        while output_path.exists() {
                            output_path = parent_dir
                                .join(format!("{}_encoded_{}{}", stem, counter, out_container));
                            counter += 1;
                        }

                        // Clip-specific edits (crop, rotation, mirror flip) are only applied to the
                        // currently selected/edited item, or when there is only a single item in the queue.
                        // Non-selected items in a batch queue transcode their full frames with the batch codec settings.
                        let is_selected_or_single = total_items == 1 || item_idx as i32 == selected_idx;

                        let (rotation, flip_h, flip_v, crop) = if is_selected_or_single {
                            let rot = rotation_from_index(rot_idx);
                            let c = if crop_enabled && crop_w >= 2 && crop_h >= 2 {
                                Some(CropRect {
                                    x: crop_x.max(0) as u32,
                                    y: crop_y.max(0) as u32,
                                    width: crop_w.max(2) as u32,
                                    height: crop_h.max(2) as u32,
                                })
                            } else {
                                None
                            };
                            (rot, flip_h, flip_v, c)
                        } else {
                            (VideoRotation::None, false, false, None)
                        };

                        let has_audio_edits =
                            (edit_gain_opt.is_some() || edit_norm_enabled) && item.is_audio;
                        let has_video_edits =
                            rotation != VideoRotation::None || flip_h || flip_v || crop.is_some();

                        // Determine function (upgrade Rewrap/LosslessCut to Transcode if video or audio filters are active)
                        let function = match func_idx {
                            0..=5 => EncoderFunction::Transcode,
                            6 | 7 if has_video_edits || has_audio_edits => EncoderFunction::Transcode,
                            6 => EncoderFunction::Rewrap,
                            7 => EncoderFunction::LosslessCut,
                            8 => EncoderFunction::ExtractVideo,
                            9 => EncoderFunction::ExtractAudio,
                            10 => EncoderFunction::AudioNormalize,
                            11 => EncoderFunction::ExtractAudio, // Audio conversion: drop video cleanly without streamcopy conflict
                            12 => EncoderFunction::ExtractSubtitles,
                            _ => EncoderFunction::Transcode,
                        };

                        let video_codec = match func_idx {
                            0 => VideoCodec::H264,
                            1 => VideoCodec::H265,
                            2 => VideoCodec::AV1,
                            3 => VideoCodec::AppleProRes,
                            4 => VideoCodec::AvidDNxHD,
                            5 => VideoCodec::VP9,
                            11 => VideoCodec::Copy,
                            _ => VideoCodec::H264,
                        };

                        let hw_accel = match hw_accel_idx {
                            1 => HwAccel::Nvenc,
                            2 => HwAccel::Qsv,
                            3 => HwAccel::Amf,
                            4 => HwAccel::VideoToolbox,
                            5 => HwAccel::Cpu,
                            _ => HwAccel::Auto,
                        };

                        let rate_control = match rate_ctrl_idx {
                            1 => RateControlMode::CBR,
                            2 => RateControlMode::VBR,
                            _ => RateControlMode::CRF,
                        };

                        let video_bitrate_kbps = match video_br_idx {
                            0 => 15000,
                            1 => 10000,
                            2 => 6000,
                            3 => 4000,
                            4 => 2500,
                            _ => 1200,
                        };

                        let preset = match preset_idx {
                            0 => "ultrafast",
                            1 => "faster",
                            2 => "fast",
                            3 => "medium",
                            4 => "slow",
                            5 => "veryslow",
                            _ => "medium",
                        }
                        .to_string();

                        let resolution = match res_idx {
                            1 => ResolutionPreset::UHD4K,
                            2 => ResolutionPreset::QHD1440p,
                            3 => ResolutionPreset::FHD1080p,
                            4 => ResolutionPreset::HD720p,
                            5 => ResolutionPreset::SD480p,
                            _ => ResolutionPreset::Original,
                        };

                        let fps = match fps_idx {
                            1 => Some(23.976),
                            2 => Some(24.0),
                            3 => Some(25.0),
                            4 => Some(29.97),
                            5 => Some(30.0),
                            6 => Some(50.0),
                            7 => Some(59.94),
                            8 => Some(60.0),
                            _ => None,
                        };

                        let deinterlace = match deinterlace_idx {
                            1 => DeinterlaceMode::Yadif,
                            2 => DeinterlaceMode::Bwdif,
                            _ => DeinterlaceMode::None,
                        };

                        let prores_profile = match prores_idx {
                            0 => ProResProfile::Proxy,
                            1 => ProResProfile::LT,
                            2 => ProResProfile::Standard,
                            3 => ProResProfile::HQ,
                            4 => ProResProfile::FourFourFourFour,
                            5 => ProResProfile::FourFourFourFourXQ,
                            _ => ProResProfile::Standard,
                        };

                        let audio_codec = match audio_codec_idx {
                            1 => AudioCodecChoice::MP3,
                            2 => AudioCodecChoice::Opus,
                            3 => AudioCodecChoice::FLAC,
                            4 => AudioCodecChoice::WavPCM24,
                            5 => AudioCodecChoice::WavPCM16,
                            6 => AudioCodecChoice::AC3,
                            7 => AudioCodecChoice::Copy,
                            _ => AudioCodecChoice::AAC,
                        };

                        let audio_bitrate_kbps = match audio_br_idx {
                            0 => 320,
                            1 => 256,
                            2 => 192,
                            3 => 128,
                            4 => 96,
                            _ => 64,
                        };

                        let audio_sample_rate = match audio_sr_idx {
                            1 => Some(48000),
                            2 => Some(44100),
                            3 => Some(96000),
                            _ => None,
                        };

                        let normalize_lufs = match normalize_idx {
                            1 => Some(-23.0),
                            2 => Some(-14.0),
                            3 => Some(-16.0),
                            4 => Some(-12.0),
                            _ => {
                                if func_idx == 10 {
                                    Some(-23.0)
                                } else {
                                    None
                                }
                            }
                        };

                        let in_point = if is_selected_or_single && !in_point_str.trim().is_empty() {
                            Some(in_point_str.clone())
                        } else {
                            None
                        };

                        let out_point = if is_selected_or_single && !out_point_str.trim().is_empty() {
                            Some(out_point_str.clone())
                        } else {
                            None
                        };

                        // Reuse already-probed duration if available, otherwise probe on demand
                        let total_duration = if item.duration_seconds > 0.0 {
                            item.duration_seconds as f64
                        } else {
                            match another_encoder::ffmpeg::probe_file(&input_path, &bins.ffprobe) {
                                Ok(m) => m.duration_seconds,
                                Err(_) => 0.0,
                            }
                        };

                        // Validate timecodes
                        let in_secs = in_point.as_ref().and_then(|s| parse_timecode_to_seconds(s));
                        let out_secs = out_point.as_ref().and_then(|s| parse_timecode_to_seconds(s));

                        if let (Some(in_s), Some(out_s)) = (in_secs, out_secs) {
                            if in_s >= out_s {
                                any_errors = true;
                                let ui_w = ui_weak.clone();
                                let idx = item_idx;
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_w.upgrade() {
                                        let app_data = ui.global::<AppData>();
                                        app_data.set_status_summary(SharedString::from("In point must be before out point"));
                                        let model = app_data.get_queue();
                                        if let Some(mut row) = model.row_data(idx) {
                                            row.status = SharedString::from("Skipped: In >= Out");
                                            model.set_row_data(idx, row);
                                        }
                                    }
                                });
                                continue;
                            }
                        }

                        if let Some(in_s) = in_secs {
                            if total_duration > 0.0 && in_s >= total_duration {
                                any_errors = true;
                                let ui_w = ui_weak.clone();
                                let in_s_c = in_s;
                                let dur_c = total_duration;
                                let idx = item_idx;
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_w.upgrade() {
                                        let app_data = ui.global::<AppData>();
                                        app_data.set_status_summary(SharedString::from(format!("In point ({:.1}s) is beyond video duration ({:.1}s)", in_s_c, dur_c)));
                                        let model = app_data.get_queue();
                                        if let Some(mut row) = model.row_data(idx) {
                                            row.status = SharedString::from("Skipped: In > Dur");
                                            model.set_row_data(idx, row);
                                        }
                                    }
                                });
                                continue;
                            }
                        }

                        if let Some(out_s) = out_secs {
                            if total_duration > 0.0 && out_s > total_duration {
                                any_errors = true;
                                let ui_w = ui_weak.clone();
                                let out_s_c = out_s;
                                let dur_c = total_duration;
                                let idx = item_idx;
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_w.upgrade() {
                                        let app_data = ui.global::<AppData>();
                                        app_data.set_status_summary(SharedString::from(format!("Out point ({:.1}s) is beyond video duration ({:.1}s)", out_s_c, dur_c)));
                                        let model = app_data.get_queue();
                                        if let Some(mut row) = model.row_data(idx) {
                                            row.status = SharedString::from("Skipped: Out > Dur");
                                            model.set_row_data(idx, row);
                                        }
                                    }
                                });
                                continue;
                            }
                        }

                        let start_sec = in_secs.unwrap_or(0.0).max(0.0);
                        let end_sec = out_secs.unwrap_or(total_duration);
                        let effective_duration = if end_sec > start_sec {
                            end_sec - start_sec
                        } else {
                            total_duration
                        };

                        let (final_gain_opt, final_norm_opt) = if item.is_audio {
                            let norm = if edit_norm_enabled {
                                Some(edit_norm_lufs)
                            } else {
                                normalize_lufs
                            };
                            (edit_gain_opt, norm)
                        } else {
                            (None, None)
                        };

                        let config = EncodeJobConfig {
                            function,
                            input_path: input_path.clone(),
                            secondary_input_path: None,
                            output_path: output_path.to_string_lossy().to_string(),

                            video_codec,
                            hw_accel,
                            rate_control,
                            crf,
                            video_bitrate_kbps,
                            max_bitrate_kbps: video_bitrate_kbps * 2,
                            preset,
                            resolution,
                            custom_resolution: None,
                            fps,
                            deinterlace,
                            prores_profile,
                            faststart,

                            audio_codec,
                            audio_bitrate_kbps,
                            audio_sample_rate,
                            audio_channels: None,
                            audio_gain_db: final_gain_opt,
                            normalize_lufs: final_norm_opt,

                            in_point,
                            out_point,

                            rotation,
                            flip_horizontal: flip_h,
                            flip_vertical: flip_v,
                            crop,
                        };

                        // Build FFmpeg command
                        let cmd_args = FFmpegCommandBuilder::build(&config, &bins.ffmpeg);
                        let cmd_str = cmd_args.join(" ");

                        // Update current file status to Encoding in-place
                        {
                            let ui_w = ui_weak.clone();
                            let idx = item_idx;
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    let app_data = ui.global::<AppData>();
                                    let model = app_data.get_queue();
                                    if let Some(mut row) = model.row_data(idx) {
                                        row.status = SharedString::from("Encoding...");
                                        model.set_row_data(idx, row);
                                    }
                                }
                            });
                        }

                        // Display command preview in UI
                        {
                            let ui_w = ui_weak.clone();
                            let c_str = cmd_str.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    ui.global::<AppData>().set_log_preview(SharedString::from(
                                        format!("$ {}\n", c_str),
                                    ));
                                }
                            });
                        }

                        let ui_w_progress = ui_weak.clone();
                        let idx = item_idx;
                        let mut last_ui_tick = std::time::Instant::now()
                            .checked_sub(std::time::Duration::from_millis(100))
                            .unwrap_or_else(std::time::Instant::now);

                        let progress_callback =
                            move |p: another_encoder::ffmpeg::ProgressUpdate| {
                                let now = std::time::Instant::now();
                                if p.percentage < 1.0
                                    && now.duration_since(last_ui_tick).as_millis() < 45
                                {
                                    return;
                                }
                                last_ui_tick = now;

                                let ui_w = ui_w_progress.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_w.upgrade() {
                                        let app_data = ui.global::<AppData>();
                                        let model = app_data.get_queue();
                                        if let Some(mut row) = model.row_data(idx) {
                                            row.progress = p.percentage;
                                            row.status = SharedString::from(format!(
                                                "{:.1}%",
                                                p.percentage * 100.0
                                            ));
                                            model.set_row_data(idx, row);
                                        }

                                        let overall =
                                            (idx as f32 + p.percentage) / (total_items as f32);
                                        app_data.set_progress_percent(overall);
                                        app_data.set_progress_text(SharedString::from(format!(
                                            "File {}/{} — {:.1}%",
                                            idx + 1,
                                            total_items,
                                            p.percentage * 100.0
                                        )));
                                        app_data.set_speed_text(SharedString::from(format!(
                                            "{:.2}x", p.speed
                                        )));
                                        app_data.set_fps_text(SharedString::from(format!(
                                            "{:.0} fps", p.fps
                                        )));
                                        app_data.set_time_text(SharedString::from(format!(
                                            "{} / {}",
                                            format_duration_hms(p.current_seconds),
                                            format_duration_hms(p.total_seconds)
                                        )));
                                    }
                                });
                            };

                        let ui_w_log = ui_weak.clone();
                        let log_callback = move |line: String| {
                            let trimmed = line.trim();
                            // Filter out high-frequency in-place progress updates (frame=... or size=... time=...)
                            // These are already handled by progress_callback and would flood the Slint UI event loop.
                            if trimmed.starts_with("frame=")
                                || (trimmed.starts_with("size=") && trimmed.contains("time="))
                            {
                                return;
                            }

                            let ui_w = ui_w_log.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    let mut cur =
                                        ui.global::<AppData>().get_log_preview().to_string();
                                    if cur.len() > 4000 {
                                        let mut split_idx = cur.len() - 3000;
                                        while split_idx < cur.len() && !cur.is_char_boundary(split_idx) {
                                            split_idx += 1;
                                        }
                                        cur = cur[split_idx..].to_string();
                                    }
                                    cur.push_str(&line);
                                    cur.push('\n');
                                    ui.global::<AppData>()
                                        .set_log_preview(SharedString::from(cur));
                                }
                            });
                        };

                        // Run execution
                        let result = FFmpegRunner::run_with_progress(
                            &cmd_args,
                            effective_duration,
                            cancel.clone(),
                            progress_callback,
                            log_callback,
                        );

                        let ui_w_done = ui_weak.clone();
                        let status_str = match &result {
                            Ok(_) => "Completed".to_string(),
                            Err(e) => {
                                if cancel.load(Ordering::SeqCst) {
                                    "Cancelled".to_string()
                                } else {
                                    format!("Failed: {}", e)
                                }
                            }
                        };
                        let is_ok = result.is_ok();
                        if !is_ok {
                            any_errors = true;
                        } else if output_path.exists() {
                            last_completed_path = Some(output_path.clone());
                        }

                        // Clean up output file only if this specific processing job failed or was cancelled in-flight
                        if !is_ok && output_path.exists() {
                            let _ = std::fs::remove_file(&output_path);
                        }

                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_w_done.upgrade() {
                                let app_data = ui.global::<AppData>();
                                let model = app_data.get_queue();
                                if let Some(mut row) = model.row_data(idx) {
                                    row.status = SharedString::from(status_str);
                                    if is_ok {
                                        row.progress = 1.0;
                                    }
                                    model.set_row_data(idx, row);
                                }
                            }
                        });
                    }

                    // Batch finished or cancelled
                    let ui_w = ui_weak.clone();
                    let was_cancelled = cancel.load(Ordering::SeqCst);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            ui.global::<AppData>().set_is_processing(false);
                            if was_cancelled {
                                ui.global::<AppData>()
                                    .set_progress_text(SharedString::from("Cancelled"));
                            } else if any_errors {
                                ui.global::<AppData>().set_progress_text(SharedString::from(
                                    "Finished with errors / skipped files",
                                ));
                            } else {
                                let app_data = ui.global::<AppData>();
                                app_data.set_progress_percent(1.0);
                                app_data.set_progress_text(SharedString::from(
                                    "All tasks finished successfully!",
                                ));

                                // Play notification sound if enabled in Settings
                                if app_data.get_play_sound_on_finish() {
                                    crate::utils::play_notification_sound();
                                }

                                // Open destination folder if enabled in Settings
                                if app_data.get_open_folder_on_finish() {
                                    if let Some(ref path) = last_completed_path {
                                        crate::utils::reveal_in_explorer(path);
                                    } else {
                                        let out_dir = app_data.get_output_directory().to_string();
                                        if !out_dir.is_empty() && out_dir != "Same as source" {
                                            crate::utils::open_folder_in_explorer(Path::new(&out_dir));
                                        }
                                    }
                                }
                            }
                        }
                    });
                });
            }
        });

        // Backward compatibility callback forwarding
        let ui_handle_fwd = ui.as_weak();
        ui.global::<AppLogic>().on_start_processing(move || {
            if let Some(ui) = ui_handle_fwd.upgrade() {
                ui.global::<AppLogic>().invoke_run_task();
            }
        });
    }
}
