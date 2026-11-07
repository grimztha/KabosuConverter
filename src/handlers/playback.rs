use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer, SharedString};

use another_encoder::ffmpeg::{
    format_duration_hms, spawn_audio_playback, spawn_ffplay,
    spawn_playback_stream_transformed_with_duration, AudioPlaybackSession,
};
use crate::context::AppContext;
use crate::handlers::editor::{request_current_preview_frame, rotation_from_index};
use crate::{AppData, AppLogic, MainWindow};

/// Launches video decoding/rendering or audio playback stream starting at the given timestamp.
pub fn start_playback_from(ui: &MainWindow, ctx: &AppContext, start_at: f32) {
    let app_data = ui.global::<AppData>();
    if !app_data.get_player_has_media() {
        return;
    }

    ctx.stop_active_playback(&app_data);

    let cur = start_at;
    let dur = app_data.get_player_duration();
    let out_sec_val = app_data.get_out_point_seconds();
    let actual_start = if cur >= dur || (out_sec_val >= 0.0 && cur >= (out_sec_val - 0.05)) {
        let in_pos = app_data.get_in_point_seconds().max(0.0);
        app_data.set_player_current_time(in_pos);
        app_data.set_player_time_text(SharedString::from(format_duration_hms(in_pos as f64)));
        in_pos
    } else {
        cur
    };

    app_data.set_player_is_playing(true);

    if app_data.get_selected_is_video() {
        let path = app_data.get_selected_file_path().to_string();
        let start_sec = actual_start as f64;
        let out_sec = app_data.get_out_point_seconds() as f64;
        let dur_sec = app_data.get_player_duration() as f64;
        let ffmpeg_bin = ctx.binaries.ffmpeg.clone();

        let vol_float = if app_data.get_player_is_muted() {
            0.0
        } else {
            app_data.get_player_volume()
        };
        let vol_int = (vol_float * 100.0).round().clamp(0.0, 100.0) as i32;

        let audio_expected = app_data.get_selected_is_audio() && ctx.binaries.ffplay.is_some();
        let mut initial_audio_clock: Option<Arc<std::sync::atomic::AtomicU64>> = None;

        let dur_opt = if out_sec > start_sec {
            Some(out_sec - start_sec)
        } else {
            None
        };

        // Launch audio playback in parallel via ffplay in headless mode (-nodisp)
        if app_data.get_selected_is_audio() {
            if let Some(ref ffplay_bin) = ctx.binaries.ffplay {
                let edit_gain_db = app_data.get_audio_gain_db();
                let edit_gain_opt = if edit_gain_db.abs() > 0.05 {
                    Some(edit_gain_db)
                } else {
                    None
                };
                let edit_norm_opt = if app_data.get_audio_normalize_enabled() {
                    Some(app_data.get_audio_normalize_lufs())
                } else {
                    None
                };
                if let Ok(session) = spawn_audio_playback(
                    ffplay_bin,
                    &path,
                    start_sec,
                    dur_opt,
                    vol_int,
                    edit_gain_opt,
                    edit_norm_opt,
                ) {
                    initial_audio_clock = Some(session.master_clock.clone());
                    let mut lock = ctx.active_player_child.lock().unwrap_or_else(|e| e.into_inner());
                    *lock = Some(session);
                }
            }
        }

        let rot = rotation_from_index(app_data.get_video_rotation_index());
        let flip_h = app_data.get_video_flip_horizontal();
        let flip_v = app_data.get_video_flip_vertical();

        match spawn_playback_stream_transformed_with_duration(
            &ffmpeg_bin,
            &path,
            start_sec,
            dur_opt,
            rot,
            flip_h,
            flip_v,
        ) {
            Ok(mut child) => {
                let my_session = ctx.playback_session_id.fetch_add(1, Ordering::SeqCst) + 1;
                let session_check = ctx.playback_session_id.clone();
                let stream_child_ref = ctx.active_playback_child.clone();
                let ffplay_child_ref = ctx.active_player_child.clone();
                let ui_w = ui.as_weak();
                let mut stdout = child.stdout.take();

                let mut lock = ctx.active_playback_child.lock().unwrap_or_else(|e| e.into_inner());
                *lock = Some(child);

                thread::spawn(move || {
                    use std::io::Read;
                    let mut stdout = match stdout.take() {
                        Some(s) => s,
                        None => return,
                    };

                    let expected = 640 * 360 * 4;
                    let mut buf = vec![0u8; expected];
                    let mut frame_index: u64 = 0;
                    let frame_dur = 1.0 / 24.0;
                    let mut playback_start_instant = std::time::Instant::now();
                    let mut cached_audio_clock = initial_audio_clock;
                    let mut audio_wait_done = false;

                    loop {
                        if session_check.load(Ordering::SeqCst) != my_session {
                            break;
                        }

                        let final_time = if out_sec >= 0.0 {
                            out_sec.min(dur_sec)
                        } else {
                            dur_sec
                        };

                        if stdout.read_exact(&mut buf).is_err() {
                            let ui_w2 = ui_w.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w2.upgrade() {
                                    let app_data = ui.global::<AppData>();
                                    app_data.set_player_is_playing(false);
                                    app_data.set_player_current_time(final_time as f32);
                                    app_data.set_player_time_text(SharedString::from(format_duration_hms(final_time)));
                                }
                            });
                            break;
                        }

                        let mut audio_finished = false;
                        if let Ok(mut lock) = ffplay_child_ref.try_lock() {
                            if let Some(ref mut session) = *lock {
                                if session.is_finished() {
                                    audio_finished = true;
                                    cached_audio_clock = None;
                                } else {
                                    cached_audio_clock = Some(session.master_clock.clone());
                                }
                            } else {
                                cached_audio_clock = None;
                            }
                        }

                        let get_pts = |clock: &Option<Arc<std::sync::atomic::AtomicU64>>| -> Option<f64> {
                            if let Some(ref clk) = *clock {
                                let val = clk.load(Ordering::Acquire);
                                if val != u64::MAX {
                                    return Some((val as f64) / 1_000_000.0);
                                }
                            }
                            None
                        };

                        // On the initial frame, if audio is playing, wait up to 350ms
                        // for ffplay to initialize so video and audio start simultaneously.
                        if !audio_wait_done && audio_expected {
                            audio_wait_done = true;
                            let wait_start = std::time::Instant::now();
                            while get_pts(&cached_audio_clock).is_none() && wait_start.elapsed().as_millis() < 350 {
                                if session_check.load(Ordering::SeqCst) != my_session {
                                    break;
                                }
                                if let Ok(mut lock) = ffplay_child_ref.try_lock() {
                                    if let Some(ref mut session) = *lock {
                                        if !session.is_finished() {
                                            cached_audio_clock = Some(session.master_clock.clone());
                                        }
                                    }
                                }
                                thread::sleep(std::time::Duration::from_millis(5));
                            }
                            playback_start_instant = std::time::Instant::now();
                        }

                        let current_master_time = match get_pts(&cached_audio_clock) {
                            Some(pts) => pts,
                            None => {
                                start_sec + playback_start_instant.elapsed().as_secs_f64()
                            }
                        };

                        let frame_target_sec = start_sec + (frame_index as f64) * frame_dur;

                        let has_reached_out = out_sec >= 0.0 && (
                            current_master_time >= out_sec
                            || frame_target_sec >= out_sec
                            || (audio_finished && audio_wait_done)
                        );
                        let has_reached_end = current_master_time >= dur_sec || frame_target_sec >= dur_sec;

                        if has_reached_out || has_reached_end {
                            let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(640, 360);
                            pixel_buffer.make_mut_bytes().copy_from_slice(&buf);

                            let ui_w2 = ui_w.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w2.upgrade() {
                                    let app_data = ui.global::<AppData>();
                                    let img = slint::Image::from_rgba8(pixel_buffer);
                                    app_data.set_preview_frame(img);
                                    app_data.set_player_is_playing(false);
                                    app_data.set_player_current_time(final_time as f32);
                                    app_data.set_player_time_text(SharedString::from(format_duration_hms(final_time)));
                                }
                            });
                            break;
                        }

                        let diff = frame_target_sec - current_master_time;

                        frame_index += 1;

                        // Drop late frame (> 40ms) to stay in sync with audio
                        if diff < -0.040 {
                            continue;
                        }

                        if diff > 0.002 {
                            let sleep_sec = diff.min(0.045);
                            thread::sleep(std::time::Duration::from_secs_f64(sleep_sec));
                        }

                        if session_check.load(Ordering::SeqCst) != my_session {
                            break;
                        }

                        let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(640, 360);
                        pixel_buffer.make_mut_bytes().copy_from_slice(&buf);

                        let ui_w2 = ui_w.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_w2.upgrade() {
                                let app_data = ui.global::<AppData>();
                                if app_data.get_player_is_playing() {
                                    let img = slint::Image::from_rgba8(pixel_buffer);
                                    app_data.set_preview_frame(img);
                                    app_data.set_player_current_time(current_master_time as f32);
                                    app_data.set_player_time_text(SharedString::from(format_duration_hms(current_master_time)));
                                }
                            }
                        });
                    }

                    // Clean up process handles if still belonging to this session
                    if session_check.load(Ordering::SeqCst) == my_session {
                        let mut s_lock = stream_child_ref.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(mut c) = s_lock.take() {
                            let _ = c.kill();
                            let _ = c.wait();
                        }
                        let mut p_lock = ffplay_child_ref.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(mut s) = p_lock.take() {
                            let _ = s.kill();
                        }
                    }
                });
            }
            Err(_e) => {
                #[cfg(debug_assertions)]
                eprintln!("Failed to spawn playback stream: {}", _e);
                app_data.set_player_is_playing(false);
            }
        }
    } else {
        // Audio-only playback with ffplay
        let path = app_data.get_selected_file_path().to_string();
        let start_sec = actual_start as f64;
        let out_sec = app_data.get_out_point_seconds() as f64;

        let vol_float = if app_data.get_player_is_muted() {
            0.0
        } else {
            app_data.get_player_volume()
        };
        let vol_int = (vol_float * 100.0).round().clamp(0.0, 100.0) as i32;

        if let Some(ref ffplay_bin) = ctx.binaries.ffplay {
            let dur_opt = if out_sec > start_sec {
                Some(out_sec - start_sec)
            } else {
                None
            };
            let edit_gain_db = app_data.get_audio_gain_db();
            let edit_gain_opt = if edit_gain_db.abs() > 0.05 {
                Some(edit_gain_db)
            } else {
                None
            };
            let edit_norm_opt = if app_data.get_audio_normalize_enabled() {
                Some(app_data.get_audio_normalize_lufs())
            } else {
                None
            };
            if let Ok(session) = spawn_audio_playback(
                ffplay_bin,
                &path,
                start_sec,
                dur_opt,
                vol_int,
                edit_gain_opt,
                edit_norm_opt,
            ) {
                let mut lock = ctx.active_player_child.lock().unwrap_or_else(|e| e.into_inner());
                *lock = Some(session);
            }
        }

        let ui_w = ui.as_weak();
        let timer_ref = ctx.playback_timer.clone();
        let ffplay_child_ref = ctx.active_player_child.clone();
        let my_session = ctx.playback_session_id.fetch_add(1, Ordering::SeqCst) + 1;
        let session_check = ctx.playback_session_id.clone();
        let audio_start_instant = std::time::Instant::now();

        ctx.playback_timer.borrow().start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(30),
            move || {
                if session_check.load(Ordering::SeqCst) != my_session {
                    timer_ref.borrow().stop();
                    return;
                }

                if let Some(ui) = ui_w.upgrade() {
                    let app_data = ui.global::<AppData>();
                    if !app_data.get_player_is_playing() {
                        timer_ref.borrow().stop();
                        return;
                    }

                    let mut is_finished = false;
                    let mut audio_pts: Option<f64> = None;
                    if let Ok(mut lock) = ffplay_child_ref.try_lock() {
                        if let Some(ref mut session) = *lock {
                            is_finished = session.is_finished();
                            audio_pts = session.get_pts();
                        }
                    }

                    let dur = app_data.get_player_duration();
                    let out_sec = app_data.get_out_point_seconds();

                    if is_finished {
                        let final_time = if out_sec >= 0.0 { out_sec.min(dur) } else { dur };
                        app_data.set_player_is_playing(false);
                        app_data.set_player_current_time(final_time);
                        app_data.set_player_time_text(SharedString::from(format_duration_hms(final_time as f64)));
                        timer_ref.borrow().stop();
                        let mut lock = ffplay_child_ref.lock().unwrap_or_else(|e| e.into_inner());
                        let _ = lock.take();
                        return;
                    }

                    let cur = if let Some(pts) = audio_pts {
                        pts as f32
                    } else if vol_int == 0 || app_data.get_player_is_muted() {
                        let elapsed = audio_start_instant.elapsed().as_secs_f64();
                        (start_sec + elapsed) as f32
                    } else {
                        app_data.get_player_current_time()
                    };

                    if (out_sec >= 0.0 && cur >= out_sec) || cur >= dur {
                        let final_time = if out_sec >= 0.0 && cur >= out_sec {
                            out_sec
                        } else {
                            dur
                        };
                        app_data.set_player_is_playing(false);
                        app_data.set_player_current_time(final_time);
                        app_data.set_player_time_text(SharedString::from(format_duration_hms(final_time as f64)));
                        timer_ref.borrow().stop();
                        let mut lock = ffplay_child_ref.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(mut s) = lock.take() {
                            let _ = s.kill();
                        }
                    } else {
                        app_data.set_player_current_time(cur);
                        app_data.set_player_time_text(SharedString::from(format_duration_hms(cur as f64)));
                    }
                }
            },
        );
    }
}

pub fn register(ui: &MainWindow, ctx: &AppContext) {
    // Handle Player Seek (Scrubber slider / timeline click)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_player_seek(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let was_playing = app_data.get_player_is_playing()
                    || ctx.was_playing_before_scrub.swap(false, Ordering::SeqCst);

                ctx.stop_active_playback(&app_data);

                let dur = app_data.get_player_duration();
                let clamped = val.clamp(0.0, dur.max(0.0));
                app_data.set_player_current_time(clamped);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(clamped as f64)));

                if was_playing {
                    start_playback_from(&ui, &ctx, clamped);
                } else if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
                }
            }
        });
    }

    // Handle Player Scrub (Continuous dragging)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_player_scrub(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    ctx.was_playing_before_scrub.store(true, Ordering::SeqCst);
                    ctx.stop_active_playback(&app_data);
                }

                let dur = app_data.get_player_duration();
                let clamped = val.clamp(0.0, dur.max(0.0));
                app_data.set_player_current_time(clamped);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(clamped as f64)));

                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
                }
            }
        });
    }

    // Handle Player Step (Relative stepping: -1s, -0.1s, +0.1s, +1s)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_player_step(move |delta| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let was_playing = app_data.get_player_is_playing();
                if was_playing {
                    ctx.stop_active_playback(&app_data);
                }

                let cur = app_data.get_player_current_time();
                let dur = app_data.get_player_duration();
                let new_val = (cur + delta).clamp(0.0, dur.max(0.0));
                app_data.set_player_current_time(new_val);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(new_val as f64)));

                if was_playing {
                    start_playback_from(&ui, &ctx, new_val);
                } else if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
                }
            }
        });
    }

    // Handle Player Play/Pause
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_player_play_pause(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if !app_data.get_player_has_media() {
                    return;
                }

                let is_playing = app_data.get_player_is_playing();
                if is_playing {
                    ctx.was_playing_before_scrub.store(false, Ordering::SeqCst);
                    ctx.stop_active_playback(&app_data);
                } else {
                    let cur = app_data.get_player_current_time();
                    start_playback_from(&ui, &ctx, cur);
                }
            }
        });
    }

    // Handle Player Stop
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_player_stop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                ctx.stop_active_playback(&app_data);

                let reset_pos = app_data.get_in_point_seconds().max(0.0);
                app_data.set_player_current_time(reset_pos);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(reset_pos as f64)));

                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
                }
            }
        });
    }

    // Handle Play Trimmed Range
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_play_trimmed_range(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if !app_data.get_player_has_media() {
                    return;
                }

                let start = app_data.get_in_point_seconds().max(0.0);
                app_data.set_player_current_time(start);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(start as f64)));

                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
                }

                // If currently playing, stop first then restart at trimmed start
                if app_data.get_player_is_playing() {
                    ui.global::<AppLogic>().invoke_player_play_pause();
                }
                ui.global::<AppLogic>().invoke_player_play_pause();
            }
        });
    }

    // Handle Launch FFplay (Native full-fps audio/video preview)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_launch_ffplay(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if !app_data.get_player_has_media() {
                    return;
                }

                let media_path = app_data.get_selected_file_path().to_string();
                let cur = app_data.get_player_current_time() as f64;
                let is_video = app_data.get_selected_is_video();
                let out_sec = app_data.get_out_point_seconds();
                let duration = if out_sec >= 0.0 && (out_sec as f64) > cur {
                    Some(out_sec as f64 - cur)
                } else {
                    None
                };

                let ffplay_bin = ctx.binaries.ffplay.clone().unwrap_or_else(|| {
                    ctx.binaries
                        .ffmpeg
                        .with_file_name(if cfg!(windows) { "ffplay.exe" } else { "ffplay" })
                });

                let vol_float = if app_data.get_player_is_muted() {
                    0.0
                } else {
                    app_data.get_player_volume()
                };
                let vol_int = (vol_float * 100.0).round().clamp(0.0, 100.0) as i32;

                let mut lock = ctx.active_player_child.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(mut old) = lock.take() {
                    let _ = old.kill();
                }
                if let Ok(child) = spawn_ffplay(&ffplay_bin, &media_path, cur, duration, !is_video, Some(vol_int)) {
                    *lock = Some(AudioPlaybackSession::from_child(child));
                }
            }
        });
    }

    // Handle Player Volume Changed (Throttled real-time updates)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();

        ui.global::<AppLogic>().on_player_volume_changed(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let clamped = val.clamp(0.0, 1.0);
                app_data.set_player_volume(clamped);

                if clamped <= 0.001 {
                    app_data.set_player_is_muted(true);
                } else if app_data.get_player_is_muted() {
                    app_data.set_player_is_muted(false);
                }

                if app_data.get_player_is_playing() {
                    ctx.restart_audio_preview(&app_data);
                }
            }
        });
    }

    // Handle Player Volume Settled (Immediate guaranteed final update on release or click)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();

        ui.global::<AppLogic>().on_player_volume_settled(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let clamped = val.clamp(0.0, 1.0);
                app_data.set_player_volume(clamped);

                if clamped <= 0.001 {
                    app_data.set_player_is_muted(true);
                } else if app_data.get_player_is_muted() {
                    app_data.set_player_is_muted(false);
                }

                if app_data.get_player_is_playing() {
                    ctx.restart_audio_preview_immediate(&app_data);
                }
            }
        });
    }

    // Handle Player Toggle Mute
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_player_toggle_mute(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let is_muted = app_data.get_player_is_muted();
                if is_muted {
                    let mut restore_vol = app_data.get_player_pre_mute_volume();
                    if restore_vol < 0.05 {
                        restore_vol = 0.8;
                    }
                    app_data.set_player_volume(restore_vol);
                    app_data.set_player_is_muted(false);
                } else {
                    let cur_vol = app_data.get_player_volume();
                    if cur_vol > 0.01 {
                        app_data.set_player_pre_mute_volume(cur_vol);
                    }
                    app_data.set_player_is_muted(true);
                }

                // Keep ffplay running at volume=0 when muted so VU meters and clock sync stay active.
                if app_data.get_player_is_playing() {
                    if app_data.get_selected_is_audio() {
                        ctx.restart_audio_preview_immediate(&app_data);
                    } else if let Some(mut old) = ctx
                        .active_player_child
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .take()
                    {
                        let _ = old.kill();
                    }
                }
            }
        });
    }

    // Handle Toggle Volume Meter
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_toggle_volume_meter(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_show_volume_meter(!app_data.get_show_volume_meter());
                crate::handlers::settings::save_settings_from_app_data(&app_data);
            }
        });
    }

    // VU meter smoothing and peak-hold decay timer (30ms interval)
    {
        let ui_handle = ui.as_weak();
        let active_player_child = ctx.active_player_child.clone();
        let mut disp_l = -60.0f32;
        let mut disp_r = -60.0f32;
        let mut peak_l = -60.0f32;
        let mut peak_r = -60.0f32;
        let mut peak_hold_time_l = std::time::Instant::now();
        let mut peak_hold_time_r = std::time::Instant::now();

        #[inline]
        fn smooth_channel(
            target: f32,
            disp: &mut f32,
            peak: &mut f32,
            peak_hold_time: &mut std::time::Instant,
            now: std::time::Instant,
        ) {
            if target >= *disp {
                *disp = target;
            } else {
                *disp = (*disp - 2.5).max(target).max(-60.0);
            }

            if *disp >= *peak {
                *peak = *disp;
                *peak_hold_time = now;
            } else if now.duration_since(*peak_hold_time) > std::time::Duration::from_millis(800) {
                *peak = (*peak - 1.5).max(*disp).max(-60.0);
            }

            if *disp < -59.5 {
                *disp = -60.0;
            }
            if *peak < -59.5 {
                *peak = -60.0;
            }
        }

        ctx.meter_timer.borrow_mut().start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(30),
            move || {
                if let Some(ui) = ui_handle.upgrade() {
                    let app_data = ui.global::<AppData>();
                    let is_playing = app_data.get_player_is_playing();
                    let is_audio = app_data.get_selected_is_audio();

                    // Skip UI property writes when playback is stopped and meters have reached the noise floor.
                    if !(is_playing && is_audio)
                        && disp_l <= -59.5
                        && disp_r <= -59.5
                        && peak_l <= -59.5
                        && peak_r <= -59.5
                    {
                        return;
                    }

                    let (target_l, target_r) = if is_playing && is_audio {
                        if let Ok(lock) = active_player_child.try_lock() {
                            if let Some(ref session) = *lock {
                                session.get_meter_levels()
                            } else {
                                (-60.0, -60.0)
                            }
                        } else {
                            (disp_l, disp_r)
                        }
                    } else {
                        (-60.0, -60.0)
                    };

                    let now = std::time::Instant::now();
                    smooth_channel(target_l, &mut disp_l, &mut peak_l, &mut peak_hold_time_l, now);
                    smooth_channel(target_r, &mut disp_r, &mut peak_r, &mut peak_hold_time_r, now);

                    app_data.set_audio_meter_l_db(disp_l);
                    app_data.set_audio_meter_r_db(disp_r);
                    app_data.set_audio_meter_peak_l_db(peak_l);
                    app_data.set_audio_meter_peak_r_db(peak_r);
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, AtomicU64};
    use std::sync::{mpsc, Mutex};
    use std::time::Instant;
    use another_encoder::ffmpeg::FFmpegBinaries;

    fn create_test_context() -> AppContext {
        let (preview_tx, _) = mpsc::channel();
        let (waveform_tx, _) = mpsc::channel();

        let default_bin_dir = dirs::config_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("another_encoder")
            .join("bin");

        let binaries = FFmpegBinaries::locate().unwrap_or_else(|_| FFmpegBinaries {
            ffmpeg: default_bin_dir.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }),
            ffprobe: default_bin_dir.join(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }),
            ffplay: Some(default_bin_dir.join(if cfg!(windows) { "ffplay.exe" } else { "ffplay" })),
        });

        AppContext {
            binaries: Arc::new(binaries),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            active_player_child: Arc::new(Mutex::new(None)),
            active_playback_child: Arc::new(Mutex::new(None)),
            playback_session_id: Arc::new(AtomicU64::new(0)),
            preview_tx,
            latest_preview_id: Arc::new(AtomicU64::new(0)),
            waveform_tx,
            latest_waveform_id: Arc::new(AtomicU64::new(0)),
            ytdlp_binary: Arc::new(None),
            ytdlp_discovered_options: Arc::new(Mutex::new(Vec::new())),
            active_ytdlp_child: Arc::new(Mutex::new(None)),
            active_ytdlp_cancel: Arc::new(AtomicBool::new(false)),
            playback_timer: Rc::new(RefCell::new(slint::Timer::default())),
            meter_timer: Rc::new(RefCell::new(slint::Timer::default())),
            last_audio_restart: Arc::new(Mutex::new(Instant::now())),
            was_playing_before_scrub: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn test_playback_scrub_and_volume_lifecycle() {
        let ui = match MainWindow::new() {
            Ok(u) => u,
            Err(_) => return, // Slint Winit backend allows only one EventLoop per test process
        };
        let ctx = create_test_context();
        register(&ui, &ctx);

        let app_data = ui.global::<AppData>();

        // 1. Volume slider changes and settling
        ui.global::<AppLogic>().invoke_player_volume_changed(0.75);
        assert!((app_data.get_player_volume() - 0.75).abs() < 0.001);
        assert!(!app_data.get_player_is_muted());

        ui.global::<AppLogic>().invoke_player_volume_settled(0.50);
        assert!((app_data.get_player_volume() - 0.50).abs() < 0.001);
        assert!(!app_data.get_player_is_muted());

        // Setting volume to zero triggers mute
        ui.global::<AppLogic>().invoke_player_volume_settled(0.0);
        assert!(app_data.get_player_is_muted());

        // Toggle mute restores volume
        ui.global::<AppLogic>().invoke_player_toggle_mute();
        assert!(!app_data.get_player_is_muted());
        assert!(app_data.get_player_volume() > 0.05);

        // 2. Seek while paused vs playing
        app_data.set_player_has_media(true);
        app_data.set_player_duration(120.0);
        app_data.set_selected_is_video(false);
        app_data.set_selected_is_audio(true);

        // Seeking while paused remains paused
        app_data.set_player_is_playing(false);
        ui.global::<AppLogic>().invoke_player_seek(30.0);
        assert_eq!(app_data.get_player_current_time(), 30.0);
        assert!(!app_data.get_player_is_playing());

        // Seeking while playing continues playback seamlessly
        app_data.set_player_is_playing(true);
        ui.global::<AppLogic>().invoke_player_seek(75.0);
        assert_eq!(app_data.get_player_current_time(), 75.0);
        assert!(app_data.get_player_is_playing(), "Seeking while playing must continue playing");

        // 3. Scrubbing drag and release (seek) flow
        ui.global::<AppLogic>().invoke_player_scrub(40.0);
        assert_eq!(app_data.get_player_current_time(), 40.0);
        assert!(ctx.was_playing_before_scrub.load(Ordering::SeqCst));

        ui.global::<AppLogic>().invoke_player_scrub(55.0);
        assert_eq!(app_data.get_player_current_time(), 55.0);

        ui.global::<AppLogic>().invoke_player_seek(55.0);
        assert_eq!(app_data.get_player_current_time(), 55.0);
        assert!(app_data.get_player_is_playing(), "Releasing scrubber must resume playback");
        assert!(!ctx.was_playing_before_scrub.load(Ordering::SeqCst));

        ctx.stop_active_playback(&app_data);
    }
}
