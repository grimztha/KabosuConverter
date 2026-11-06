use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;

use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer, SharedString};

use another_encoder::ffmpeg::{
    format_duration_hms, spawn_audio_playback, spawn_ffplay, spawn_playback_stream_transformed,
    AudioPlaybackSession,
};
use crate::context::AppContext;
use crate::handlers::editor::{request_current_preview_frame, rotation_from_index};
use crate::{AppData, AppLogic, MainWindow};

pub fn register(ui: &MainWindow, ctx: &AppContext) {
    // Handle Player Seek (Scrubber slider)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_player_seek(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
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
                if app_data.get_player_is_playing() {
                    ctx.stop_active_playback(&app_data);
                }

                let cur = app_data.get_player_current_time();
                let dur = app_data.get_player_duration();
                let new_val = (cur + delta).clamp(0.0, dur.max(0.0));
                app_data.set_player_current_time(new_val);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(new_val as f64)));

                if app_data.get_selected_is_video() {
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
                    // Pause
                    ctx.stop_active_playback(&app_data);
                } else {
                    // Play: first stop any previous playback
                    ctx.stop_active_playback(&app_data);

                    let cur = app_data.get_player_current_time();
                    let dur = app_data.get_player_duration();
                    let start_at = if cur >= dur {
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
                        let start_sec = start_at as f64;
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

                        // Launch audio playback in parallel via ffplay in headless mode (-nodisp)
                        // Runs even if muted or volume=0 so the render output audio meter continues measuring!
                        if app_data.get_selected_is_audio() {
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
                                    initial_audio_clock = Some(session.master_clock.clone());
                                    let mut lock = ctx.active_player_child.lock().unwrap_or_else(|e| e.into_inner());
                                    *lock = Some(session);
                                }
                            }
                        }

                        let rot = rotation_from_index(app_data.get_video_rotation_index());
                        let flip_h = app_data.get_video_flip_horizontal();
                        let flip_v = app_data.get_video_flip_vertical();

                        match spawn_playback_stream_transformed(&ffmpeg_bin, &path, start_sec, rot, flip_h, flip_v) {
                            Ok(mut child) => {
                                let my_session = ctx.playback_session_id.fetch_add(1, Ordering::SeqCst) + 1;
                                let session_check = ctx.playback_session_id.clone();
                                let stream_child_ref = ctx.active_playback_child.clone();
                                let ffplay_child_ref = ctx.active_player_child.clone();
                                let ui_w = ui_handle.clone();
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
                                    let mut playback_wall_start: Option<std::time::Instant> = None;
                                    let mut cached_audio_clock = initial_audio_clock;
                                    let mut audio_wait_done = false;

                                    loop {
                                        if session_check.load(Ordering::SeqCst) != my_session {
                                            break;
                                        }

                                        if stdout.read_exact(&mut buf).is_err() {
                                            let ui_w2 = ui_w.clone();
                                            let _ = slint::invoke_from_event_loop(move || {
                                                if let Some(ui) = ui_w2.upgrade() {
                                                    ui.global::<AppData>().set_player_is_playing(false);
                                                }
                                            });
                                            break;
                                        }

                                        let mut get_pts = || -> Option<f64> {
                                            if let Some(ref clk) = cached_audio_clock {
                                                let val = clk.load(Ordering::Acquire);
                                                if val != u64::MAX {
                                                    return Some((val as f64) / 1_000_000.0);
                                                }
                                            }
                                            if let Ok(lock) = ffplay_child_ref.try_lock() {
                                                if let Some(ref session) = *lock {
                                                    let clk = session.master_clock.clone();
                                                    let val = clk.load(Ordering::Acquire);
                                                    cached_audio_clock = Some(clk);
                                                    if val != u64::MAX {
                                                        return Some((val as f64) / 1_000_000.0);
                                                    }
                                                }
                                            }
                                            None
                                        };

                                        // On the initial frame, if audio is playing, wait up to 350ms
                                        // for ffplay to initialize so video and audio start simultaneously.
                                        if !audio_wait_done && audio_expected {
                                            audio_wait_done = true;
                                            let wait_start = std::time::Instant::now();
                                            while get_pts().is_none() && wait_start.elapsed().as_millis() < 350 {
                                                if session_check.load(Ordering::SeqCst) != my_session {
                                                    break;
                                                }
                                                thread::sleep(std::time::Duration::from_millis(5));
                                            }
                                        }

                                        let current_master_time = match get_pts() {
                                            Some(pts) => pts,
                                            None => {
                                                let inst = match playback_wall_start {
                                                    Some(i) => i,
                                                    None => {
                                                        let i = std::time::Instant::now();
                                                        playback_wall_start = Some(i);
                                                        i
                                                    }
                                                };
                                                start_sec + inst.elapsed().as_secs_f64()
                                            }
                                        };

                                        if (out_sec >= 0.0 && current_master_time >= out_sec) || current_master_time >= dur_sec {
                                            let final_time = if out_sec >= 0.0 && current_master_time >= out_sec {
                                                out_sec
                                            } else {
                                                dur_sec
                                            };
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

                                        let frame_target_sec = start_sec + (frame_index as f64) * frame_dur;
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
                        let start_sec = start_at as f64;
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

                        let ui_w = ui_handle.clone();
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
                                        start_sec as f32
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

    // Handle Player Volume Changed
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        let last_restart = Arc::new(std::sync::Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(1)));

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
                    let mut lr = last_restart.lock().unwrap_or_else(|e| e.into_inner());
                    let now = std::time::Instant::now();
                    if now.duration_since(*lr) >= std::time::Duration::from_millis(120) {
                        *lr = now;
                        let vol_int = if app_data.get_player_is_muted() {
                            0
                        } else {
                            (clamped * 100.0).round().clamp(0.0, 100.0) as i32
                        };

                        let mut lock = ctx.active_player_child.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(mut old) = lock.take() {
                            let _ = old.kill();
                        }
                        if app_data.get_selected_is_audio() {
                            if let Some(ref ffplay_bin) = ctx.binaries.ffplay {
                                let cur_time = app_data.get_player_current_time() as f64;
                                let out_sec = app_data.get_out_point_seconds() as f64;
                                let dur_opt = if out_sec > cur_time {
                                    Some(out_sec - cur_time)
                                } else {
                                    None
                                };
                                let path = app_data.get_selected_file_path().to_string();
                                let edit_gain_db = app_data.get_audio_gain_db();
                                let edit_gain_opt = if edit_gain_db.abs() > 0.05 { Some(edit_gain_db) } else { None };
                                let edit_norm_opt = if app_data.get_audio_normalize_enabled() { Some(app_data.get_audio_normalize_lufs()) } else { None };
                                if let Ok(session) = spawn_audio_playback(ffplay_bin, &path, cur_time, dur_opt, vol_int, edit_gain_opt, edit_norm_opt) {
                                    *lock = Some(session);
                                }
                            }
                        }
                    }
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
                    // Unmute
                    let mut restore_vol = app_data.get_player_pre_mute_volume();
                    if restore_vol < 0.05 {
                        restore_vol = 0.8;
                    }
                    app_data.set_player_volume(restore_vol);
                    app_data.set_player_is_muted(false);

                    if app_data.get_player_is_playing() && app_data.get_selected_is_audio() {
                        let cur_time = app_data.get_player_current_time() as f64;
                        let out_sec = app_data.get_out_point_seconds() as f64;
                        let dur_opt = if out_sec > cur_time {
                            Some(out_sec - cur_time)
                        } else {
                            None
                        };
                        let path = app_data.get_selected_file_path().to_string();
                        let vol_int = (restore_vol * 100.0).round().clamp(0.0, 100.0) as i32;

                        let mut lock = ctx.active_player_child.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(mut old) = lock.take() {
                            let _ = old.kill();
                        }
                        if let Some(ref ffplay_bin) = ctx.binaries.ffplay {
                            let edit_gain_db = app_data.get_audio_gain_db();
                            let edit_gain_opt = if edit_gain_db.abs() > 0.05 { Some(edit_gain_db) } else { None };
                            let edit_norm_opt = if app_data.get_audio_normalize_enabled() { Some(app_data.get_audio_normalize_lufs()) } else { None };
                            if let Ok(session) = spawn_audio_playback(ffplay_bin, &path, cur_time, dur_opt, vol_int, edit_gain_opt, edit_norm_opt) {
                                *lock = Some(session);
                            }
                        }
                    }
                } else {
                    // Mute
                    let cur_vol = app_data.get_player_volume();
                    if cur_vol > 0.01 {
                        app_data.set_player_pre_mute_volume(cur_vol);
                    }
                    app_data.set_player_is_muted(true);

                    // Silence audio output to speakers, but keep ffplay running at volume=0
                    // so render-output audio meter levels and clock synchronization continue accurately!
                    if app_data.get_player_is_playing() && app_data.get_selected_is_audio() {
                        let cur_time = app_data.get_player_current_time() as f64;
                        let out_sec = app_data.get_out_point_seconds() as f64;
                        let dur_opt = if out_sec > cur_time {
                            Some(out_sec - cur_time)
                        } else {
                            None
                        };
                        let path = app_data.get_selected_file_path().to_string();
                        let mut lock = ctx.active_player_child.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(mut old) = lock.take() {
                            let _ = old.kill();
                        }
                        if let Some(ref ffplay_bin) = ctx.binaries.ffplay {
                            let edit_gain_db = app_data.get_audio_gain_db();
                            let edit_gain_opt = if edit_gain_db.abs() > 0.05 { Some(edit_gain_db) } else { None };
                            let edit_norm_opt = if app_data.get_audio_normalize_enabled() { Some(app_data.get_audio_normalize_lufs()) } else { None };
                            if let Ok(session) = spawn_audio_playback(ffplay_bin, &path, cur_time, dur_opt, 0, edit_gain_opt, edit_norm_opt) {
                                *lock = Some(session);
                            }
                        }
                    } else {
                        let mut lock = ctx.active_player_child.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(mut old) = lock.take() {
                            let _ = old.kill();
                        }
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
            }
        });
    }

    // Meter Update & Decay Timer (30ms frequency for smooth VU meter movement & peak hold decay)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        let mut disp_l = -60.0f32;
        let mut disp_r = -60.0f32;
        let mut peak_l = -60.0f32;
        let mut peak_r = -60.0f32;
        let mut peak_hold_time_l = std::time::Instant::now();
        let mut peak_hold_time_r = std::time::Instant::now();

        let timer_clone = ctx.meter_timer.clone();

        ctx.meter_timer.borrow_mut().start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(30),
            move || {
                let _keep_alive = &timer_clone;
                if let Some(ui) = ui_handle.upgrade() {
                    let app_data = ui.global::<AppData>();
                    let is_playing = app_data.get_player_is_playing();
                    let is_audio = app_data.get_selected_is_audio();

                    let (target_l, target_r) = if is_playing && is_audio {
                        if let Ok(lock) = ctx.active_player_child.try_lock() {
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

                    // Left channel level smoothing (instant attack, smooth decay)
                    if target_l >= disp_l {
                        disp_l = target_l;
                    } else {
                        disp_l = (disp_l - 2.5).max(target_l).max(-60.0);
                    }

                    // Left channel peak hold (holds for 800ms, then drops)
                    if disp_l >= peak_l {
                        peak_l = disp_l;
                        peak_hold_time_l = now;
                    } else if now.duration_since(peak_hold_time_l) > std::time::Duration::from_millis(800) {
                        peak_l = (peak_l - 1.5).max(disp_l).max(-60.0);
                    }

                    // Right channel level smoothing (instant attack, smooth decay)
                    if target_r >= disp_r {
                        disp_r = target_r;
                    } else {
                        disp_r = (disp_r - 2.5).max(target_r).max(-60.0);
                    }

                    // Right channel peak hold (holds for 800ms, then drops)
                    if disp_r >= peak_r {
                        peak_r = disp_r;
                        peak_hold_time_r = now;
                    } else if now.duration_since(peak_hold_time_r) > std::time::Duration::from_millis(800) {
                        peak_r = (peak_r - 1.5).max(disp_r).max(-60.0);
                    }

                    // Clean floor clamping
                    if disp_l < -59.5 { disp_l = -60.0; }
                    if disp_r < -59.5 { disp_r = -60.0; }
                    if peak_l < -59.5 { peak_l = -60.0; }
                    if peak_r < -59.5 { peak_r = -60.0; }

                    app_data.set_audio_meter_l_db(disp_l);
                    app_data.set_audio_meter_r_db(disp_r);
                    app_data.set_audio_meter_peak_l_db(peak_l);
                    app_data.set_audio_meter_peak_r_db(peak_r);
                }
            },
        );
    }
}
