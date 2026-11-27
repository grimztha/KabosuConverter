use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::thread;

use slint::{ComponentHandle, Model, SharedString};

use crate::context::AppContext;
use crate::handlers::editor::rotation_from_index;
use crate::utils::open_folder_in_explorer;
use crate::{AppData, AppLogic, MainWindow, QueueFile};
use kabosu_converter::ffmpeg::{
    format_duration_hms, parse_timecode_to_seconds, AudioCodecChoice, CropRect, DeinterlaceMode,
    EncodeJobConfig, EncoderFunction, FFmpegCommandBuilder, FFmpegRunner, HwAccel, ProResProfile,
    RateControlMode, ResolutionPreset, VideoCodec, VideoRotation,
};
use kabosu_converter::sanitize::{ensure_path_within_dir, sanitize_filename_stem, validate_path};

/// Finds a queue row by path, checking `preferred_idx` first for O(1) lookup.
fn find_queue_row(
    model: &slint::ModelRc<QueueFile>,
    preferred_idx: usize,
    expected_path: &str,
) -> Option<usize> {
    model
        .row_data(preferred_idx)
        .filter(|r| r.file_path.as_str() == expected_path)
        .map(|_| preferred_idx)
        .or_else(|| {
            (0..model.row_count()).find(|&i| {
                model
                    .row_data(i)
                    .is_some_and(|r| r.file_path.as_str() == expected_path)
            })
        })
}

/// Updates a queue row's status (and optional progress/summary) on the UI thread.
fn update_queue_row(
    ui_weak: &slint::Weak<MainWindow>,
    preferred_idx: usize,
    expected_path: &str,
    status: String,
    progress: Option<f32>,
    status_summary: Option<String>,
) {
    let ui_w = ui_weak.clone();
    let expected = expected_path.to_string();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui_w.upgrade() else {
            return;
        };
        let app_data = ui.global::<AppData>();
        if let Some(summary) = status_summary {
            app_data.set_status_summary(SharedString::from(summary));
        }
        let model = app_data.get_queue();
        if let Some(idx) = find_queue_row(&model, preferred_idx, &expected) {
            if let Some(mut row) = model.row_data(idx) {
                row.status = SharedString::from(status);
                if let Some(p) = progress {
                    row.progress = p;
                }
                model.set_row_data(idx, row);
            }
        }
    });
}

/// Maps UI function, container, and audio codec indices to the output file extension.
fn resolve_output_container(
    func_idx: i32,
    container_idx: i32,
    audio_codec_idx: i32,
) -> &'static str {
    if func_idx == 12 {
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
                4 => ".wav",
                _ => ".m4a",
            },
        }
    } else if func_idx == 3 || func_idx == 4 {
        // ProRes and DNxHD require QuickTime (.mov) or Matroska (.mkv).
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
    }
}

/// Ensures `is_processing` is cleared on the UI if the batch worker exits abnormally.
struct ProcessingResetGuard {
    ui_weak: slint::Weak<MainWindow>,
    disarmed: bool,
}

impl ProcessingResetGuard {
    fn new(ui_weak: slint::Weak<MainWindow>) -> Self {
        Self {
            ui_weak,
            disarmed: false,
        }
    }

    fn disarm(&mut self) {
        self.disarmed = true;
    }
}

impl Drop for ProcessingResetGuard {
    fn drop(&mut self) {
        if !self.disarmed {
            let ui_w = self.ui_weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_w.upgrade() {
                    let app_data = ui.global::<AppData>();
                    app_data.set_is_processing(false);
                    app_data.set_progress_text(SharedString::from("Encoding interrupted"));
                }
            });
        }
    }
}

/// Resolves a unique, non-existing output path inside `parent_dir`.
fn pick_unique_output_path(
    parent_dir: &Path,
    safe_stem: &str,
    out_container: &str,
) -> Result<PathBuf, &'static str> {
    for counter in 1..=1000u32 {
        let filename = if counter == 1 {
            format!("{}_encoded{}", safe_stem, out_container)
        } else {
            format!("{}_encoded_{}{}", safe_stem, counter, out_container)
        };
        if filename.len() > 255 {
            return Err("Filename exceeds 255 bytes");
        }
        let candidate = parent_dir.join(&filename);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Too many existing outputs (>1000)")
}

pub fn register(ui: &MainWindow, ctx: &AppContext) {
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_select_output_dir(move || {
            let (parent_hwnd, suggested) = if let Some(ui) = ui_handle.upgrade() {
                let hwnd = crate::dialogs::get_window_hwnd(ui.window());
                let cur = ui.global::<AppData>().get_output_directory().to_string();
                let dir = if !cur.is_empty() {
                    Some(PathBuf::from(cur))
                } else {
                    None
                };
                (hwnd, dir)
            } else {
                (std::ptr::null_mut(), None)
            };
            let ui_w = ui_handle.clone();
            crate::dialogs::pick_folder_async(
                parent_hwnd,
                suggested,
                "Select Output Directory",
                move |folder| {
                    if let Some(ui) = ui_w.upgrade() {
                        let folder_str = folder.to_string_lossy().to_string();
                        let data = ui.global::<AppData>();
                        data.set_output_directory(SharedString::from(&folder_str));
                        data.set_ytdlp_output_dir(SharedString::from(&folder_str));
                        crate::handlers::settings::save_settings_from_app_data(&data);
                    }
                },
            );
        });
    }

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
                            .unwrap_or_else(|| {
                                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                            })
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

    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_toggle_logs(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let cur = ui.global::<AppData>().get_show_logs();
                ui.global::<AppData>().set_show_logs(!cur);
            }
        });
    }

    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_switch_tab(move |tab_idx| {
            if let Some(ui) = ui_handle.upgrade() {
                ui.global::<AppData>().set_active_tab(tab_idx);
            }
        });
    }

    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_quick_lossless_cut(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if !app_data.get_has_active_trim() || !app_data.get_player_has_media() {
                    return;
                }
                // Function index 7 = LosslessCut
                app_data.set_function_index(7);
                ui.global::<AppLogic>().invoke_run_task();
            }
        });
    }

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

    // Batch encode queue runner
    {
        let ui_handle = ui.as_weak();
        let binaries = ctx.binaries.clone();
        let cancel_flag = ctx.cancel_flag.clone();

        ui.global::<AppLogic>().on_run_task(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.global::<AppData>().get_is_processing() {
                    return;
                }
                let items: Vec<QueueFile> = ui.global::<AppData>().get_queue().iter().collect();
                if items.is_empty() {
                    return;
                }

                cancel_flag.store(false, Ordering::SeqCst);
                ui.global::<AppData>().set_is_processing(true);
                ui.global::<AppData>().set_progress_percent(0.0);
                ui.global::<AppData>()
                    .set_progress_text(SharedString::from("Starting..."));

                // Snapshot UI parameters before spawning the worker thread
                let func_idx = ui.global::<AppData>().get_function_index();
                let hw_accel_idx = ui.global::<AppData>().get_hw_accel_index();
                let rate_ctrl_idx = ui.global::<AppData>().get_rate_control_index();
                let crf_raw = ui.global::<AppData>().get_crf_value().round() as u32;
                let crf = crf_raw.min(51);
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
                let edit_gain_opt = if edit_gain_val.abs() > 0.05 {
                    Some(edit_gain_val)
                } else {
                    None
                };
                let edit_norm_enabled = ui.global::<AppData>().get_audio_normalize_enabled();
                let edit_norm_lufs = ui.global::<AppData>().get_audio_normalize_lufs();
                let fade_in_val = ui.global::<AppData>().get_audio_fade_in_seconds();
                let fade_out_val = ui.global::<AppData>().get_audio_fade_out_seconds();
                let edit_fade_in_opt = if fade_in_val > 0.01 {
                    Some(fade_in_val)
                } else {
                    None
                };
                let edit_fade_out_opt = if fade_out_val > 0.01 {
                    Some(fade_out_val)
                } else {
                    None
                };
                let selected_idx = ui.global::<AppData>().get_selected_file_index();

                // Persist user encoder choices to settings.json
                crate::handlers::settings::save_settings_from_app_data(&ui.global::<AppData>());

                let ui_weak = ui_handle.clone();
                let bins = binaries.clone();
                let cancel = cancel_flag.clone();

                // Spawn worker thread for batch queue processing
                thread::spawn(move || {
                    let mut processing_guard = ProcessingResetGuard::new(ui_weak.clone());
                    let total_items = items.len();
                    let mut any_errors = false;
                    let mut last_completed_path: Option<PathBuf> = None;

                    for (item_idx, item) in items.iter().enumerate() {
                        if cancel.load(Ordering::SeqCst) {
                            break;
                        }

                        let input_path = item.file_path.to_string();
                        let in_p = Path::new(&input_path);
                        let out_container =
                            resolve_output_container(func_idx, container_idx, audio_codec_idx);

                        let parent_dir =
                            if out_dir_str != "Same as source" && !out_dir_str.is_empty() {
                                PathBuf::from(&out_dir_str)
                            } else {
                                in_p.parent()
                                    .map(|p| p.to_path_buf())
                                    .unwrap_or_else(|| PathBuf::from("."))
                            };

                        if let Err(e) = validate_path(&parent_dir) {
                            any_errors = true;
                            update_queue_row(
                                &ui_weak,
                                item_idx,
                                &input_path,
                                format!("Invalid dir: {}", e),
                                None,
                                None,
                            );
                            continue;
                        }

                        let _ = std::fs::create_dir_all(&parent_dir);

                        let stem = in_p.file_stem().unwrap_or_default().to_string_lossy();
                        let safe_stem = sanitize_filename_stem(&stem);

                        let output_path =
                            match pick_unique_output_path(&parent_dir, &safe_stem, out_container) {
                                Ok(p) => p,
                                Err(msg) => {
                                    any_errors = true;
                                    update_queue_row(
                                        &ui_weak,
                                        item_idx,
                                        &input_path,
                                        msg.to_string(),
                                        None,
                                        None,
                                    );
                                    continue;
                                }
                            };

                        if let Err(e) = ensure_path_within_dir(&output_path, &parent_dir) {
                            any_errors = true;
                            update_queue_row(
                                &ui_weak,
                                item_idx,
                                &input_path,
                                format!("Invalid path: {}", e),
                                None,
                                None,
                            );
                            continue;
                        }

                        // Clip-specific edits (crop, rotation, mirror flip) apply only to the
                        // active/edited clip, or when there is a single file in the queue.
                        let is_selected_or_single =
                            total_items == 1 || item_idx as i32 == selected_idx;

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

                        // Matches transcoder_tab.slint: None, EBU R128 (-23), YouTube (-14), Podcast (-16), Apple Music (-16)
                        let normalize_lufs = match normalize_idx {
                            1 => Some(-23.0),
                            2 => Some(-14.0),
                            3 | 4 => Some(-16.0),
                            _ if func_idx == 10 => Some(-23.0),
                            _ => None,
                        };

                        let item_fade_in = if is_selected_or_single && item.is_audio {
                            edit_fade_in_opt
                        } else {
                            None
                        };
                        let item_fade_out = if is_selected_or_single && item.is_audio {
                            edit_fade_out_opt
                        } else {
                            None
                        };

                        let has_audio_edits = (edit_gain_opt.is_some()
                            || edit_norm_enabled
                            || normalize_lufs.is_some()
                            || item_fade_in.is_some()
                            || item_fade_out.is_some())
                            && item.is_audio;
                        let has_video_edits =
                            rotation != VideoRotation::None || flip_h || flip_v || crop.is_some();

                        // Upgrade Rewrap/LosslessCut to Transcode if video or audio filters are active.
                        let function = match func_idx {
                            0..=5 => EncoderFunction::Transcode,
                            6 | 7 if has_video_edits || has_audio_edits => {
                                EncoderFunction::Transcode
                            }
                            6 => EncoderFunction::Rewrap,
                            7 => EncoderFunction::LosslessCut,
                            8 => EncoderFunction::ExtractVideo,
                            9 => EncoderFunction::ExtractAudio,
                            10 => EncoderFunction::AudioNormalize,
                            11 => EncoderFunction::ExtractAudio,
                            12 => EncoderFunction::ExtractSubtitles,
                            _ => EncoderFunction::Transcode,
                        };

                        let video_codec = match func_idx {
                            6 | 7 => VideoCodec::H264,
                            _ => VideoCodec::from_function_index(func_idx),
                        };
                        let hw_accel = HwAccel::from_index(hw_accel_idx);
                        let rate_control = RateControlMode::from_index(rate_ctrl_idx);

                        // Matches transcoder_tab.slint bitrate options (in kbps).
                        let video_bitrate_kbps = match video_br_idx {
                            0 => 12000,
                            1 => 8000,
                            2 => 6000,
                            3 => 4000,
                            4 => 2500,
                            _ => 1500,
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

                        let resolution = ResolutionPreset::from_index(res_idx);

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

                        let deinterlace = DeinterlaceMode::from_index(deinterlace_idx);

                        let prores_profile = match prores_idx {
                            0 => ProResProfile::Proxy,
                            1 => ProResProfile::LT,
                            2 => ProResProfile::Standard,
                            3 => ProResProfile::HQ,
                            4 => ProResProfile::FourFourFourFour,
                            5 => ProResProfile::FourFourFourFourXQ,
                            _ => ProResProfile::Standard,
                        };

                        let mut audio_codec = AudioCodecChoice::from_index(audio_codec_idx);
                        let audio_bitrate_kbps = AudioCodecChoice::bitrate_from_index(audio_br_idx);

                        let audio_sample_rate = match audio_sr_idx {
                            1 => Some(48000),
                            2 => Some(44100),
                            3 => Some(96000),
                            _ => None,
                        };

                        // Intelligent safety guard: For MP4/MOV containers, enforce mobile/messenger compatible audio (AAC).
                        // MP3, Opus, or copying non-AAC streams (e.g. from VP9/AV1 WebM) causes silent playback
                        // on Facebook Messenger, Signal, iOS (AVFoundation), and Android.
                        let is_mp4_like = out_container == ".mp4"
                            || out_container == ".mov"
                            || out_container == ".m4v";
                        if is_mp4_like {
                            if matches!(audio_codec, AudioCodecChoice::MP3 | AudioCodecChoice::Opus)
                            {
                                audio_codec = AudioCodecChoice::AAC;
                            } else if audio_codec == AudioCodecChoice::Copy {
                                // If source audio is not already AAC or ALAC, copying into MP4 will fail or play silently on mobile.
                                let source_codecs = item.codecs.to_lowercase();
                                let is_source_aac = source_codecs.contains("a: aac")
                                    || source_codecs.contains("a: alac");
                                if !is_source_aac {
                                    audio_codec = AudioCodecChoice::AAC;
                                }
                            }
                        }

                        let in_point = if is_selected_or_single && !in_point_str.trim().is_empty() {
                            Some(in_point_str.clone())
                        } else {
                            None
                        };

                        let out_point = if is_selected_or_single && !out_point_str.trim().is_empty()
                        {
                            Some(out_point_str.clone())
                        } else {
                            None
                        };

                        // Reuse already-probed duration if available, otherwise probe on demand
                        let total_duration = if item.duration_seconds > 0.0 {
                            item.duration_seconds as f64
                        } else {
                            match kabosu_converter::ffmpeg::probe_file(&input_path, &bins.ffprobe) {
                                Ok(m) => m.duration_seconds,
                                Err(_) => 0.0,
                            }
                        };

                        // Validate timecodes
                        let in_secs = in_point.as_ref().and_then(|s| parse_timecode_to_seconds(s));
                        let out_secs = out_point
                            .as_ref()
                            .and_then(|s| parse_timecode_to_seconds(s));

                        if let (Some(in_s), Some(out_s)) = (in_secs, out_secs) {
                            if in_s >= out_s {
                                any_errors = true;
                                update_queue_row(
                                    &ui_weak,
                                    item_idx,
                                    &input_path,
                                    "Skipped: In >= Out".to_string(),
                                    None,
                                    Some("In point must be before out point".to_string()),
                                );
                                continue;
                            }
                        }

                        if let Some(in_s) = in_secs {
                            if total_duration > 0.0 && in_s >= total_duration {
                                any_errors = true;
                                update_queue_row(
                                    &ui_weak,
                                    item_idx,
                                    &input_path,
                                    "Skipped: In > Dur".to_string(),
                                    None,
                                    Some(format!(
                                        "In point ({:.1}s) is beyond video duration ({:.1}s)",
                                        in_s, total_duration
                                    )),
                                );
                                continue;
                            }
                        }

                        if let Some(out_s) = out_secs {
                            if total_duration > 0.0 && out_s > total_duration {
                                any_errors = true;
                                update_queue_row(
                                    &ui_weak,
                                    item_idx,
                                    &input_path,
                                    "Skipped: Out > Dur".to_string(),
                                    None,
                                    Some(format!(
                                        "Out point ({:.1}s) is beyond video duration ({:.1}s)",
                                        out_s, total_duration
                                    )),
                                );
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
                            output_path: output_path.to_string_lossy().to_string(),

                            video_codec,
                            hw_accel,
                            rate_control,
                            crf,
                            video_bitrate_kbps,
                            max_bitrate_kbps: video_bitrate_kbps.saturating_mul(2),
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
                            audio_channels: if is_mp4_like { Some(2) } else { None },
                            audio_gain_db: final_gain_opt,
                            normalize_lufs: final_norm_opt,
                            audio_fade_in_sec: item_fade_in,
                            audio_fade_out_sec: item_fade_out,
                            effective_duration_sec: (effective_duration > 0.0)
                                .then_some(effective_duration),

                            in_point,
                            out_point,

                            rotation,
                            flip_horizontal: flip_h,
                            flip_vertical: flip_v,
                            crop,
                        };

                        // Build FFmpeg command safely (reject bad inputs without panicking the worker).
                        let cmd_args = match FFmpegCommandBuilder::try_build(&config, &bins.ffmpeg)
                        {
                            Ok(args) => args,
                            Err(e) => {
                                any_errors = true;
                                update_queue_row(
                                    &ui_weak,
                                    item_idx,
                                    &input_path,
                                    format!("Failed: {}", e),
                                    None,
                                    Some(format!("Invalid encoding configuration: {}", e)),
                                );
                                continue;
                            }
                        };
                        let cmd_str = cmd_args.join(" ");

                        update_queue_row(
                            &ui_weak,
                            item_idx,
                            &input_path,
                            "Encoding...".to_string(),
                            None,
                            None,
                        );

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
                        let expected_path_progress = input_path.clone();
                        let mut last_ui_tick = std::time::Instant::now()
                            .checked_sub(std::time::Duration::from_millis(100))
                            .unwrap_or_else(std::time::Instant::now);

                        let progress_callback =
                            move |p: kabosu_converter::ffmpeg::ProgressUpdate| {
                                let now = std::time::Instant::now();
                                if p.percentage < 1.0
                                    && now.duration_since(last_ui_tick).as_millis() < 45
                                {
                                    return;
                                }
                                last_ui_tick = now;

                                let ui_w = ui_w_progress.clone();
                                let expected = expected_path_progress.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_w.upgrade() {
                                        let app_data = ui.global::<AppData>();
                                        let model = app_data.get_queue();
                                        if let Some(target_idx) =
                                            find_queue_row(&model, idx, &expected)
                                        {
                                            if let Some(mut row) = model.row_data(target_idx) {
                                                row.progress = p.percentage;
                                                row.status = SharedString::from(format!(
                                                    "{:.1}%",
                                                    p.percentage * 100.0
                                                ));
                                                model.set_row_data(target_idx, row);
                                            }
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
                                            "{:.2}x",
                                            p.speed
                                        )));
                                        app_data.set_fps_text(SharedString::from(format!(
                                            "{:.0} fps",
                                            p.fps
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
                            // Skip in-place progress lines; progress_callback already handles them.
                            if trimmed.starts_with("frame=")
                                || (trimmed.starts_with("size=") && trimmed.contains("time="))
                            {
                                return;
                            }

                            let ui_w = ui_w_log.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    let app_data = ui.global::<AppData>();
                                    let mut cur = app_data.get_log_preview().to_string();
                                    crate::utils::append_log_line(&mut cur, &line, 4000, 3000);
                                    app_data.set_log_preview(SharedString::from(cur));
                                }
                            });
                        };

                        let result = FFmpegRunner::run_with_progress(
                            &cmd_args,
                            effective_duration,
                            cancel.clone(),
                            progress_callback,
                            log_callback,
                        );

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

                        // Remove partial output file when this job fails or is cancelled.
                        if !is_ok && output_path.exists() {
                            let _ = std::fs::remove_file(&output_path);
                        }

                        update_queue_row(
                            &ui_weak,
                            item_idx,
                            &input_path,
                            status_str,
                            is_ok.then_some(1.0),
                            None,
                        );
                    }

                    // Batch finished or cancelled; disarm panic-reset guard before posting final state.
                    processing_guard.disarm();
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
                                            crate::utils::open_folder_in_explorer(Path::new(
                                                &out_dir,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    });
                });
            }
        });
    }
}
