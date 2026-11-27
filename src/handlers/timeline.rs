use slint::{ComponentHandle, SharedString};

use crate::context::AppContext;
use crate::handlers::editor::{
    request_current_preview_frame, reset_audio_fades, update_audio_edit_indicators,
};
use crate::{AppData, AppLogic, MainWindow};
use kabosu_converter::ffmpeg::{format_duration_hms, parse_timecode_to_seconds};

fn effective_trim_span(app_data: &AppData) -> (f32, f32, f32) {
    let dur = app_data.get_player_duration().max(0.0);
    let in_sec = app_data.get_in_point_seconds();
    let out_sec = app_data.get_out_point_seconds();
    let start = if in_sec >= 0.0 { in_sec.min(dur) } else { 0.0 };
    let end = if out_sec >= 0.0 {
        out_sec.clamp(start, dur)
    } else {
        dur
    };
    (start, end, (end - start).max(0.0))
}

fn format_fade_seconds(sec: f32) -> SharedString {
    let clean = if sec.is_finite() { sec.max(0.0) } else { 0.0 };
    SharedString::from(format!("{:.2}s", clean))
}

fn clamp_and_round_fade(raw: f32, max_sec: f32) -> f32 {
    if !raw.is_finite() || max_sec <= 0.01 {
        return 0.0;
    }
    let clamped = raw.clamp(0.0, max_sec);
    if clamped < 0.03 {
        return 0.0;
    }
    ((clamped * 100.0).round() / 100.0).min(max_sec)
}

pub fn update_trim_indicators(app_data: &AppData) {
    let dur = app_data.get_player_duration();
    let in_sec = app_data.get_in_point_seconds();
    let out_sec = app_data.get_out_point_seconds();

    let has_trim = in_sec >= 0.0 || out_sec >= 0.0;
    app_data.set_has_active_trim(has_trim);

    let norm_in = if in_sec >= 0.0 && dur > 0.0 {
        (in_sec / dur).clamp(0.0, 1.0)
    } else {
        0.0
    };
    app_data.set_in_point_normalized(norm_in);

    let norm_out = if out_sec >= 0.0 && dur > 0.0 {
        (out_sec / dur).clamp(norm_in, 1.0)
    } else {
        1.0
    };
    app_data.set_out_point_normalized(norm_out);

    let start = if in_sec >= 0.0 { in_sec } else { 0.0 };
    let end = if out_sec >= 0.0 {
        out_sec.max(start)
    } else {
        dur
    };
    let trim_len = (end - start).max(0.0);

    // Clamp active fade durations so fade-in + fade-out never exceed the trimmed region length.
    let cur_fade_in = app_data.get_audio_fade_in_seconds();
    let cur_fade_out = app_data.get_audio_fade_out_seconds();
    let clamped_in = clamp_and_round_fade(cur_fade_in, trim_len);
    let clamped_out = clamp_and_round_fade(cur_fade_out, (trim_len - clamped_in).max(0.0));
    if (clamped_in - cur_fade_in).abs() > 0.0001 {
        app_data.set_audio_fade_in_seconds(clamped_in);
        app_data.set_audio_fade_in_text(format_fade_seconds(clamped_in));
    }
    if (clamped_out - cur_fade_out).abs() > 0.0001 {
        app_data.set_audio_fade_out_seconds(clamped_out);
        app_data.set_audio_fade_out_text(format_fade_seconds(clamped_out));
    }
    update_audio_edit_indicators(app_data);

    if has_trim {
        let length_str = format_duration_hms(trim_len as f64);
        app_data.set_trim_duration_text(SharedString::from(length_str));
    } else {
        app_data.set_trim_duration_text(SharedString::from(format!(
            "Full ({})",
            app_data.get_player_duration_text()
        )));
    }
}

pub fn register(ui: &MainWindow, ctx: &AppContext) {
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_set_in_point_current(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let cur = app_data.get_player_current_time();
                let out_sec = app_data.get_out_point_seconds();
                let dur = app_data.get_player_duration();
                if out_sec >= 0.0 && cur >= out_sec {
                    let new_out = (cur + 1.0).min(dur);
                    if new_out > cur {
                        app_data.set_out_point_seconds(new_out);
                        app_data
                            .set_out_point(SharedString::from(format_duration_hms(new_out as f64)));
                    } else {
                        app_data.set_out_point_seconds(-1.0);
                        app_data.set_out_point(SharedString::from(""));
                    }
                }
                app_data.set_in_point_seconds(cur);
                app_data.set_in_point(SharedString::from(format_duration_hms(cur as f64)));
                update_trim_indicators(&app_data);
            }
        });
    }

    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_set_out_point_current(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let cur = app_data.get_player_current_time();
                let in_sec = app_data.get_in_point_seconds();
                if in_sec >= 0.0 && cur <= in_sec {
                    let new_in = (cur - 1.0).max(0.0);
                    if new_in < cur {
                        app_data.set_in_point_seconds(new_in);
                        app_data
                            .set_in_point(SharedString::from(format_duration_hms(new_in as f64)));
                    } else {
                        app_data.set_in_point_seconds(-1.0);
                        app_data.set_in_point(SharedString::from(""));
                    }
                }
                app_data.set_out_point_seconds(cur);
                app_data.set_out_point(SharedString::from(format_duration_hms(cur as f64)));
                update_trim_indicators(&app_data);
            }
        });
    }

    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>()
            .on_set_in_point_from_timeline(move |val| {
                if let Some(ui) = ui_handle.upgrade() {
                    let app_data = ui.global::<AppData>();
                    if app_data.get_player_is_playing() {
                        ctx.stop_active_playback(&app_data);
                    }

                    let dur = app_data.get_player_duration();
                    let out_sec = app_data.get_out_point_seconds();
                    let max_in = if out_sec >= 0.0 {
                        (out_sec - 0.05).max(0.0)
                    } else {
                        dur.max(0.0)
                    };
                    let clamped = val.clamp(0.0, max_in);
                    app_data.set_in_point_seconds(clamped);
                    app_data.set_in_point(SharedString::from(format_duration_hms(clamped as f64)));
                    update_trim_indicators(&app_data);

                    app_data.set_player_current_time(clamped);
                    app_data.set_player_time_text(SharedString::from(format_duration_hms(
                        clamped as f64,
                    )));
                    if app_data.get_selected_is_video() {
                        request_current_preview_frame(
                            &app_data,
                            &ctx.preview_tx,
                            &ctx.latest_preview_id,
                        );
                    }
                }
            });
    }

    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>()
            .on_set_out_point_from_timeline(move |val| {
                if let Some(ui) = ui_handle.upgrade() {
                    let app_data = ui.global::<AppData>();
                    if app_data.get_player_is_playing() {
                        ctx.stop_active_playback(&app_data);
                    }

                    let dur = app_data.get_player_duration();
                    let in_sec = app_data.get_in_point_seconds();
                    let min_out = if in_sec >= 0.0 {
                        (in_sec + 0.05).min(dur)
                    } else {
                        0.0
                    };
                    let clamped = val.clamp(min_out, dur.max(0.0));
                    app_data.set_out_point_seconds(clamped);
                    app_data.set_out_point(SharedString::from(format_duration_hms(clamped as f64)));
                    update_trim_indicators(&app_data);

                    app_data.set_player_current_time(clamped);
                    app_data.set_player_time_text(SharedString::from(format_duration_hms(
                        clamped as f64,
                    )));
                    if app_data.get_selected_is_video() {
                        request_current_preview_frame(
                            &app_data,
                            &ctx.preview_tx,
                            &ctx.latest_preview_id,
                        );
                    }
                }
            });
    }

    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_jump_to_in_point(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    ctx.stop_active_playback(&app_data);
                }
                let in_sec = app_data.get_in_point_seconds();
                if in_sec >= 0.0 {
                    app_data.set_player_current_time(in_sec);
                    app_data.set_player_time_text(SharedString::from(format_duration_hms(
                        in_sec as f64,
                    )));
                    if app_data.get_selected_is_video() {
                        request_current_preview_frame(
                            &app_data,
                            &ctx.preview_tx,
                            &ctx.latest_preview_id,
                        );
                    }
                }
            }
        });
    }

    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_jump_to_out_point(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    ctx.stop_active_playback(&app_data);
                }
                let out_sec = app_data.get_out_point_seconds();
                if out_sec >= 0.0 {
                    app_data.set_player_current_time(out_sec);
                    app_data.set_player_time_text(SharedString::from(format_duration_hms(
                        out_sec as f64,
                    )));
                    if app_data.get_selected_is_video() {
                        request_current_preview_frame(
                            &app_data,
                            &ctx.preview_tx,
                            &ctx.latest_preview_id,
                        );
                    }
                }
            }
        });
    }

    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_clear_trim_points(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_in_point(SharedString::from(""));
                app_data.set_out_point(SharedString::from(""));
                app_data.set_in_point_seconds(-1.0);
                app_data.set_out_point_seconds(-1.0);
                reset_audio_fades(&app_data);
                update_trim_indicators(&app_data);
                ctx.restart_audio_preview_immediate(&app_data);
            }
        });
    }

    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>()
            .on_in_point_text_changed(move |txt| {
                if let Some(ui) = ui_handle.upgrade() {
                    let app_data = ui.global::<AppData>();
                    if let Some(sec) = parse_timecode_to_seconds(&txt) {
                        app_data.set_in_point_seconds(sec as f32);
                    } else if txt.trim().is_empty() {
                        app_data.set_in_point_seconds(-1.0);
                    }
                    update_trim_indicators(&app_data);
                }
            });
    }

    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>()
            .on_out_point_text_changed(move |txt| {
                if let Some(ui) = ui_handle.upgrade() {
                    let app_data = ui.global::<AppData>();
                    if let Some(sec) = parse_timecode_to_seconds(&txt) {
                        app_data.set_out_point_seconds(sec as f32);
                    } else if txt.trim().is_empty() {
                        app_data.set_out_point_seconds(-1.0);
                    }
                    update_trim_indicators(&app_data);
                }
            });
    }

    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_set_audio_fade_in(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let (_, _, trim_len) = effective_trim_span(&app_data);
                let cur_fade_out = app_data.get_audio_fade_out_seconds().max(0.0);
                let max_in = (trim_len - cur_fade_out).max(0.0);
                let final_val = clamp_and_round_fade(val, max_in);

                app_data.set_audio_fade_in_seconds(final_val);
                app_data.set_audio_fade_in_text(format_fade_seconds(final_val));
                update_audio_edit_indicators(&app_data);
                ctx.restart_audio_preview(&app_data);
            }
        });
    }

    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_set_audio_fade_out(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let (_, _, trim_len) = effective_trim_span(&app_data);
                let cur_fade_in = app_data.get_audio_fade_in_seconds().max(0.0);
                let max_out = (trim_len - cur_fade_in).max(0.0);
                let final_val = clamp_and_round_fade(val, max_out);

                app_data.set_audio_fade_out_seconds(final_val);
                app_data.set_audio_fade_out_text(format_fade_seconds(final_val));
                update_audio_edit_indicators(&app_data);
                ctx.restart_audio_preview(&app_data);
            }
        });
    }

    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_audio_fade_settled(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                ctx.restart_audio_preview_immediate(&app_data);
            }
        });
    }
}
