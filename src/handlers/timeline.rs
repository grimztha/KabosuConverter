use slint::{ComponentHandle, SharedString};

use another_encoder::ffmpeg::{format_duration_hms, parse_timecode_to_seconds};
use crate::context::AppContext;
use crate::handlers::editor::request_current_preview_frame;
use crate::{AppData, AppLogic, MainWindow};

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

    if has_trim {
        let length_str = format_duration_hms((end - start).max(0.0) as f64);
        app_data.set_trim_duration_text(SharedString::from(length_str));
    } else {
        app_data.set_trim_duration_text(SharedString::from(format!(
            "Full ({})",
            app_data.get_player_duration_text()
        )));
    }
}

pub fn register(ui: &MainWindow, ctx: &AppContext) {
    // Handle Set In Point Current
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
                        app_data.set_out_point(SharedString::from(format_duration_hms(new_out as f64)));
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

    // Handle Set Out Point Current
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
                        app_data.set_in_point(SharedString::from(format_duration_hms(new_in as f64)));
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

    // Handle Set In Point from Timeline Drag
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_set_in_point_from_timeline(move |val| {
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
                app_data.set_player_time_text(SharedString::from(format_duration_hms(clamped as f64)));
                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
                }
            }
        });
    }

    // Handle Set Out Point from Timeline Drag
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_set_out_point_from_timeline(move |val| {
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
                app_data.set_player_time_text(SharedString::from(format_duration_hms(clamped as f64)));
                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
                }
            }
        });
    }

    // Handle Jump to In Point
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
                    app_data.set_player_time_text(SharedString::from(format_duration_hms(in_sec as f64)));
                    if app_data.get_selected_is_video() {
                        request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
                    }
                }
            }
        });
    }

    // Handle Jump to Out Point
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
                    app_data.set_player_time_text(SharedString::from(format_duration_hms(out_sec as f64)));
                    if app_data.get_selected_is_video() {
                        request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
                    }
                }
            }
        });
    }

    // Handle Clear Trim Points
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_clear_trim_points(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_in_point(SharedString::from(""));
                app_data.set_out_point(SharedString::from(""));
                app_data.set_in_point_seconds(-1.0);
                app_data.set_out_point_seconds(-1.0);
                update_trim_indicators(&app_data);
            }
        });
    }

    // Handle In Point Text Input Edited
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_in_point_text_changed(move |txt| {
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

    // Handle Out Point Text Input Edited
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_out_point_text_changed(move |txt| {
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
}
