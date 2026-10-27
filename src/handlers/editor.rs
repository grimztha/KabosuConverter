use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use slint::{ComponentHandle, SharedString};

use another_encoder::ffmpeg::{
    compute_letterbox_viewport, CropAspectRatio, CropBoxDragSession, CropHandleDragSession,
    CropRect, VideoRotation,
};
use crate::context::{AppContext, PreviewTask};
use crate::{AppData, AppLogic, MainWindow};

pub fn rotation_from_index(idx: i32) -> VideoRotation {
    match idx {
        1 => VideoRotation::Rotate90,
        2 => VideoRotation::RotateNeg90,
        3 => VideoRotation::Rotate180,
        _ => VideoRotation::None,
    }
}

pub fn update_video_edit_indicators(app_data: &AppData) {
    let rot_idx = app_data.get_video_rotation_index();
    let flip_h = app_data.get_video_flip_horizontal();
    let flip_v = app_data.get_video_flip_vertical();
    let crop_en = app_data.get_crop_enabled();
    app_data.set_has_active_video_edits(rot_idx != 0 || flip_h || flip_v || crop_en);
}

pub fn sync_crop_rect_to_ui(app_data: &AppData, rect: CropRect, update_text_fields: bool) {
    let eff_w = (app_data.get_effective_video_width().max(2) as u32) & !1;
    let eff_h = (app_data.get_effective_video_height().max(2) as u32) & !1;
    let clean = rect.sanitize(eff_w, eff_h);

    app_data.set_crop_x(clean.x as i32);
    app_data.set_crop_y(clean.y as i32);
    app_data.set_crop_width(clean.width as i32);
    app_data.set_crop_height(clean.height as i32);

    if update_text_fields {
        app_data.set_crop_x_text(SharedString::from(clean.x.to_string()));
        app_data.set_crop_y_text(SharedString::from(clean.y.to_string()));
        app_data.set_crop_width_text(SharedString::from(clean.width.to_string()));
        app_data.set_crop_height_text(SharedString::from(clean.height.to_string()));
    }

    let nx = (clean.x as f32 / eff_w as f32).clamp(0.0, 1.0);
    let ny = (clean.y as f32 / eff_h as f32).clamp(0.0, 1.0);
    let nw = (clean.width as f32 / eff_w as f32).clamp(0.01, 1.0 - nx);
    let nh = (clean.height as f32 / eff_h as f32).clamp(0.01, 1.0 - ny);

    app_data.set_crop_norm_x(nx);
    app_data.set_crop_norm_y(ny);
    app_data.set_crop_norm_w(nw);
    app_data.set_crop_norm_h(nh);
    update_video_edit_indicators(app_data);
}

pub fn sync_effective_dimensions_and_viewport(app_data: &AppData, reset_crop_rect: bool) {
    let src_w = app_data.get_selected_video_width().max(2) as u32;
    let src_h = app_data.get_selected_video_height().max(2) as u32;
    let rot = rotation_from_index(app_data.get_video_rotation_index());
    let (eff_w, eff_h) = rot.effective_dimensions(src_w, src_h);
    let eff_w = (eff_w & !1).max(2);
    let eff_h = (eff_h & !1).max(2);

    app_data.set_effective_video_width(eff_w as i32);
    app_data.set_effective_video_height(eff_h as i32);

    let (vx, vy, vw, vh) = compute_letterbox_viewport(eff_w, eff_h);
    app_data.set_preview_video_norm_x(vx);
    app_data.set_preview_video_norm_y(vy);
    app_data.set_preview_video_norm_w(vw);
    app_data.set_preview_video_norm_h(vh);

    if reset_crop_rect {
        let aspect = CropAspectRatio::from_index(app_data.get_crop_aspect_index());
        let rect = aspect.compute_centered_rect(eff_w, eff_h);
        sync_crop_rect_to_ui(app_data, rect, true);
    } else {
        let cur = CropRect {
            x: app_data.get_crop_x().max(0) as u32,
            y: app_data.get_crop_y().max(0) as u32,
            width: app_data.get_crop_width().max(2) as u32,
            height: app_data.get_crop_height().max(2) as u32,
        };
        let aspect = CropAspectRatio::from_index(app_data.get_crop_aspect_index());
        let adjusted = if aspect != CropAspectRatio::Free {
            aspect.compute_centered_rect(eff_w, eff_h)
        } else {
            cur.sanitize(eff_w, eff_h)
        };
        sync_crop_rect_to_ui(app_data, adjusted, true);
    }
}

pub fn request_current_preview_frame(
    app_data: &AppData,
    preview_tx: &Sender<PreviewTask>,
    latest_id: &AtomicU64,
) {
    if !app_data.get_selected_is_video() || !app_data.get_player_has_media() {
        return;
    }
    let path = app_data.get_selected_file_path().to_string();
    if path.is_empty() {
        return;
    }
    app_data.set_preview_frame_loading(true);
    let id = latest_id.fetch_add(1, Ordering::SeqCst) + 1;
    let time = app_data.get_player_current_time() as f64;
    let rotation = rotation_from_index(app_data.get_video_rotation_index());
    let flip_h = app_data.get_video_flip_horizontal();
    let flip_v = app_data.get_video_flip_vertical();
    let _ = preview_tx.send(PreviewTask {
        media_path: path,
        time,
        id,
        rotation,
        flip_h,
        flip_v,
    });
}

pub fn register(ui: &MainWindow, ctx: &AppContext) {
    // Handle Toggle Edit Panel
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_toggle_edit_panel(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_edit_panel_expanded(!app_data.get_edit_panel_expanded());
            }
        });
    }

    // Handle Set Video Rotation (0: 0°, 1: 90°, 2: -90°, 3: 180°)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_set_video_rotation(move |rot_idx| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    ctx.stop_active_playback(&app_data);
                }

                app_data.set_video_rotation_index(rot_idx.clamp(0, 3));
                sync_effective_dimensions_and_viewport(&app_data, false);
                update_video_edit_indicators(&app_data);
                request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
            }
        });
    }

    // Handle Toggle Flip Horizontal
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_toggle_flip_horizontal(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    ctx.stop_active_playback(&app_data);
                }

                app_data.set_video_flip_horizontal(!app_data.get_video_flip_horizontal());
                update_video_edit_indicators(&app_data);
                request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
            }
        });
    }

    // Handle Toggle Flip Vertical
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_toggle_flip_vertical(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    ctx.stop_active_playback(&app_data);
                }

                app_data.set_video_flip_vertical(!app_data.get_video_flip_vertical());
                update_video_edit_indicators(&app_data);
                request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
            }
        });
    }

    // Handle Toggle Crop Enabled
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_toggle_crop_enabled(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let next = !app_data.get_crop_enabled();
                app_data.set_crop_enabled(next);
                if next {
                    let eff_w = app_data.get_effective_video_width().max(2) as u32;
                    let eff_h = app_data.get_effective_video_height().max(2) as u32;
                    let rect = CropRect {
                        x: app_data.get_crop_x().max(0) as u32,
                        y: app_data.get_crop_y().max(0) as u32,
                        width: app_data.get_crop_width().max(2) as u32,
                        height: app_data.get_crop_height().max(2) as u32,
                    }
                    .sanitize(eff_w, eff_h);
                    sync_crop_rect_to_ui(&app_data, rect, true);
                } else {
                    update_video_edit_indicators(&app_data);
                }
            }
        });
    }

    // Handle Set Crop Aspect Preset
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_set_crop_aspect_preset(move |preset_idx| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_crop_aspect_index(preset_idx.max(0));
                app_data.set_crop_enabled(true);

                let eff_w = app_data.get_effective_video_width().max(2) as u32;
                let eff_h = app_data.get_effective_video_height().max(2) as u32;
                let aspect = CropAspectRatio::from_index(preset_idx);
                let rect = aspect.compute_centered_rect(eff_w, eff_h);
                sync_crop_rect_to_ui(&app_data, rect, true);
            }
        });
    }

    // Handle Manual Crop Width Edited
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_crop_width_edited(move |txt| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if let Ok(parsed_w) = txt.trim().parse::<u32>() {
                    app_data.set_crop_enabled(true);
                    let eff_w = app_data.get_effective_video_width().max(2) as u32;
                    let eff_h = app_data.get_effective_video_height().max(2) as u32;
                    let w = parsed_w.clamp(16, eff_w);
                    let aspect = CropAspectRatio::from_index(app_data.get_crop_aspect_index());
                    let (final_w, final_h, update_texts) = if let Some((num, den)) = aspect.ratio() {
                        let mut h = ((w as u64 * den as u64) / num as u64) as u32;
                        let mut adj_w = w;
                        if h > eff_h {
                            h = eff_h;
                            adj_w = ((h as u64 * num as u64) / den as u64) as u32;
                        }
                        (adj_w, h.max(16), true)
                    } else {
                        (w, app_data.get_crop_height().max(16) as u32, false)
                    };

                    let rect = CropRect {
                        x: app_data.get_crop_x().max(0) as u32,
                        y: app_data.get_crop_y().max(0) as u32,
                        width: final_w,
                        height: final_h,
                    }
                    .sanitize(eff_w, eff_h);
                    sync_crop_rect_to_ui(&app_data, rect, update_texts);
                }
            }
        });
    }

    // Handle Manual Crop Height Edited
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_crop_height_edited(move |txt| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if let Ok(parsed_h) = txt.trim().parse::<u32>() {
                    app_data.set_crop_enabled(true);
                    let eff_w = app_data.get_effective_video_width().max(2) as u32;
                    let eff_h = app_data.get_effective_video_height().max(2) as u32;
                    let h = parsed_h.clamp(16, eff_h);
                    let aspect = CropAspectRatio::from_index(app_data.get_crop_aspect_index());
                    let (final_w, final_h, update_texts) = if let Some((num, den)) = aspect.ratio() {
                        let mut w = ((h as u64 * num as u64) / den as u64) as u32;
                        let mut adj_h = h;
                        if w > eff_w {
                            w = eff_w;
                            adj_h = ((w as u64 * den as u64) / num as u64) as u32;
                        }
                        (w.max(16), adj_h, true)
                    } else {
                        (app_data.get_crop_width().max(16) as u32, h, false)
                    };

                    let rect = CropRect {
                        x: app_data.get_crop_x().max(0) as u32,
                        y: app_data.get_crop_y().max(0) as u32,
                        width: final_w,
                        height: final_h,
                    }
                    .sanitize(eff_w, eff_h);
                    sync_crop_rect_to_ui(&app_data, rect, update_texts);
                }
            }
        });
    }

    // Handle Manual Crop X Edited
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_crop_x_edited(move |txt| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if let Ok(parsed_x) = txt.trim().parse::<u32>() {
                    app_data.set_crop_enabled(true);
                    let eff_w = app_data.get_effective_video_width().max(2) as u32;
                    let eff_h = app_data.get_effective_video_height().max(2) as u32;
                    let rect = CropRect {
                        x: parsed_x,
                        y: app_data.get_crop_y().max(0) as u32,
                        width: app_data.get_crop_width().max(16) as u32,
                        height: app_data.get_crop_height().max(16) as u32,
                    }
                    .sanitize(eff_w, eff_h);
                    sync_crop_rect_to_ui(&app_data, rect, false);
                }
            }
        });
    }

    // Handle Manual Crop Y Edited
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_crop_y_edited(move |txt| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if let Ok(parsed_y) = txt.trim().parse::<u32>() {
                    app_data.set_crop_enabled(true);
                    let eff_w = app_data.get_effective_video_width().max(2) as u32;
                    let eff_h = app_data.get_effective_video_height().max(2) as u32;
                    let rect = CropRect {
                        x: app_data.get_crop_x().max(0) as u32,
                        y: parsed_y,
                        width: app_data.get_crop_width().max(16) as u32,
                        height: app_data.get_crop_height().max(16) as u32,
                    }
                    .sanitize(eff_w, eff_h);
                    sync_crop_rect_to_ui(&app_data, rect, false);
                }
            }
        });
    }

    let box_drag_session: Rc<RefCell<Option<CropBoxDragSession>>> = Rc::new(RefCell::new(None));
    let handle_drag_session: Rc<RefCell<Option<CropHandleDragSession>>> = Rc::new(RefCell::new(None));

    // Handle Visual Crop Box Drag (Start, Move, End)
    {
        let ui_handle = ui.as_weak();
        let session_cell = Rc::clone(&box_drag_session);
        ui.global::<AppLogic>().on_crop_box_drag_start(move |norm_x, norm_y| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_crop_enabled(true);
                let eff_w = app_data.get_effective_video_width().max(2) as u32;
                let eff_h = app_data.get_effective_video_height().max(2) as u32;
                let cur = CropRect {
                    x: app_data.get_crop_x().max(0) as u32,
                    y: app_data.get_crop_y().max(0) as u32,
                    width: app_data.get_crop_width().max(16) as u32,
                    height: app_data.get_crop_height().max(16) as u32,
                };
                *session_cell.borrow_mut() = Some(CropBoxDragSession::new(cur, norm_x, norm_y, eff_w, eff_h));
            }
        });
    }
    {
        let ui_handle = ui.as_weak();
        let session_cell = Rc::clone(&box_drag_session);
        ui.global::<AppLogic>().on_crop_box_drag_move(move |norm_x, norm_y| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let eff_w = app_data.get_effective_video_width().max(2) as u32;
                let eff_h = app_data.get_effective_video_height().max(2) as u32;
                let session = {
                    let mut b = session_cell.borrow_mut();
                    match *b {
                        Some(ref s) => *s,
                        None => {
                            let cur = CropRect {
                                x: app_data.get_crop_x().max(0) as u32,
                                y: app_data.get_crop_y().max(0) as u32,
                                width: app_data.get_crop_width().max(16) as u32,
                                height: app_data.get_crop_height().max(16) as u32,
                            };
                            let s = CropBoxDragSession::new(cur, norm_x, norm_y, eff_w, eff_h);
                            *b = Some(s);
                            s
                        }
                    }
                };
                let rect = session.compute_rect(norm_x, norm_y, eff_w, eff_h);
                sync_crop_rect_to_ui(&app_data, rect, true);
            }
        });
    }
    {
        let session_cell = Rc::clone(&box_drag_session);
        ui.global::<AppLogic>().on_crop_box_drag_end(move || {
            *session_cell.borrow_mut() = None;
        });
    }

    // Handle Visual Crop Resize Handles (0..7)
    {
        let ui_handle = ui.as_weak();
        let session_cell = Rc::clone(&handle_drag_session);
        ui.global::<AppLogic>().on_crop_handle_drag_start(move |handle_id, norm_x, norm_y| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_crop_enabled(true);
                let eff_w = app_data.get_effective_video_width().max(32) as u32;
                let eff_h = app_data.get_effective_video_height().max(32) as u32;
                let cur = CropRect {
                    x: app_data.get_crop_x().max(0) as u32,
                    y: app_data.get_crop_y().max(0) as u32,
                    width: app_data.get_crop_width().max(16) as u32,
                    height: app_data.get_crop_height().max(16) as u32,
                };
                *session_cell.borrow_mut() = Some(CropHandleDragSession::new(
                    handle_id, cur, norm_x, norm_y, eff_w, eff_h,
                ));
            }
        });
    }
    {
        let ui_handle = ui.as_weak();
        let session_cell = Rc::clone(&handle_drag_session);
        ui.global::<AppLogic>().on_crop_handle_drag(move |handle_id, norm_x, norm_y| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_crop_enabled(true);
                let eff_w = app_data.get_effective_video_width().max(32) as u32;
                let eff_h = app_data.get_effective_video_height().max(32) as u32;
                let session = {
                    let mut b = session_cell.borrow_mut();
                    match *b {
                        Some(ref s) if s.handle_id == handle_id => *s,
                        _ => {
                            let cur = CropRect {
                                x: app_data.get_crop_x().max(0) as u32,
                                y: app_data.get_crop_y().max(0) as u32,
                                width: app_data.get_crop_width().max(16) as u32,
                                height: app_data.get_crop_height().max(16) as u32,
                            };
                            let s = CropHandleDragSession::new(
                                handle_id, cur, norm_x, norm_y, eff_w, eff_h,
                            );
                            *b = Some(s);
                            s
                        }
                    }
                };
                let aspect = CropAspectRatio::from_index(app_data.get_crop_aspect_index());
                let rect = session.compute_rect(norm_x, norm_y, eff_w, eff_h, aspect);
                sync_crop_rect_to_ui(&app_data, rect, true);
            }
        });
    }
    {
        let session_cell = Rc::clone(&handle_drag_session);
        ui.global::<AppLogic>().on_crop_handle_drag_end(move || {
            *session_cell.borrow_mut() = None;
        });
    }

    // Handle Crop Nudge (Arrow keys & panel directional buttons)
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_crop_nudge(move |dx, dy| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if !app_data.get_crop_enabled() {
                    return;
                }
                let eff_w = app_data.get_effective_video_width().max(2) as u32;
                let eff_h = app_data.get_effective_video_height().max(2) as u32;
                let rect = CropRect {
                    x: app_data.get_crop_x().max(0) as u32,
                    y: app_data.get_crop_y().max(0) as u32,
                    width: app_data.get_crop_width().max(16) as u32,
                    height: app_data.get_crop_height().max(16) as u32,
                }
                .nudge(dx, dy, eff_w, eff_h);
                sync_crop_rect_to_ui(&app_data, rect, true);
            }
        });
    }

    // Handle Center Crop Box
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_center_crop_box(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let eff_w = app_data.get_effective_video_width().max(2) as u32;
                let eff_h = app_data.get_effective_video_height().max(2) as u32;
                let w = (app_data.get_crop_width().max(16) as u32).min(eff_w);
                let h = (app_data.get_crop_height().max(16) as u32).min(eff_h);
                let x = (eff_w.saturating_sub(w) / 2) & !1;
                let y = (eff_h.saturating_sub(h) / 2) & !1;
                let rect = CropRect {
                    x,
                    y,
                    width: w,
                    height: h,
                }
                .sanitize(eff_w, eff_h);
                sync_crop_rect_to_ui(&app_data, rect, true);
            }
        });
    }

    // Handle Reset Crop
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_reset_crop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_crop_aspect_index(0);
                app_data.set_crop_enabled(false);
                let eff_w = app_data.get_effective_video_width().max(2) as u32;
                let eff_h = app_data.get_effective_video_height().max(2) as u32;
                let rect = CropRect::new(0, 0, eff_w, eff_h).sanitize(eff_w, eff_h);
                sync_crop_rect_to_ui(&app_data, rect, true);
            }
        });
    }

    // Handle Reset All Video Edits
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_reset_all_video_edits(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    ctx.stop_active_playback(&app_data);
                }

                app_data.set_video_rotation_index(0);
                app_data.set_video_flip_horizontal(false);
                app_data.set_video_flip_vertical(false);
                app_data.set_crop_enabled(false);
                app_data.set_crop_aspect_index(0);
                sync_effective_dimensions_and_viewport(&app_data, true);
                update_video_edit_indicators(&app_data);
                request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
            }
        });
    }

    // Handle Reset All Edits (Video + Audio)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_reset_all_edits(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    ctx.stop_active_playback(&app_data);
                }

                // Reset video edits
                app_data.set_video_rotation_index(0);
                app_data.set_video_flip_horizontal(false);
                app_data.set_video_flip_vertical(false);
                app_data.set_crop_enabled(false);
                app_data.set_crop_aspect_index(0);
                sync_effective_dimensions_and_viewport(&app_data, true);
                update_video_edit_indicators(&app_data);

                // Reset audio edits
                app_data.set_audio_gain_db(0.0);
                app_data.set_audio_gain_text(SharedString::from("0.0 dB"));
                app_data.set_audio_normalize_enabled(false);
                app_data.set_audio_normalize_lufs(-16.0);
                app_data.set_audio_normalize_preset_index(0);
                app_data.set_has_active_audio_edits(false);

                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &ctx.preview_tx, &ctx.latest_preview_id);
                }
            }
        });
    }

    // Handle Set Audio Gain (dB)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_set_audio_gain_db(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let clamped = (val * 2.0).round() / 2.0; // round to 0.5 dB
                let clamped = clamped.clamp(-24.0, 24.0);
                app_data.set_audio_gain_db(clamped);
                let text = if clamped.abs() < 0.05 {
                    "0.0 dB".to_string()
                } else {
                    format!("{:+.1} dB", clamped)
                };
                app_data.set_audio_gain_text(SharedString::from(text));
                let is_active = clamped.abs() > 0.05 || app_data.get_audio_normalize_enabled();
                app_data.set_has_active_audio_edits(is_active);

                ctx.restart_audio_preview(&app_data);
            }
        });
    }

    // Handle Step Audio Gain (dB)
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_step_audio_gain_db(move |delta| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let cur = app_data.get_audio_gain_db();
                let next = (cur + delta).clamp(-24.0, 24.0);
                app_data.set_audio_gain_db(next);
                let text = if next.abs() < 0.05 {
                    "0.0 dB".to_string()
                } else {
                    format!("{:+.1} dB", next)
                };
                app_data.set_audio_gain_text(SharedString::from(text));
                let is_active = next.abs() > 0.05 || app_data.get_audio_normalize_enabled();
                app_data.set_has_active_audio_edits(is_active);

                ctx.restart_audio_preview(&app_data);
            }
        });
    }

    // Handle Toggle Audio Normalization
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_toggle_audio_normalize(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let next = !app_data.get_audio_normalize_enabled();
                app_data.set_audio_normalize_enabled(next);
                let is_active = app_data.get_audio_gain_db().abs() > 0.05 || next;
                app_data.set_has_active_audio_edits(is_active);

                ctx.restart_audio_preview(&app_data);
            }
        });
    }

    // Handle Set Audio Normalize Preset
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_set_audio_normalize_preset(move |idx| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let lufs = match idx {
                    0 => -16.0,
                    1 => -14.0,
                    2 => -23.0,
                    3 => -12.0,
                    _ => -16.0,
                };
                app_data.set_audio_normalize_preset_index(idx);
                app_data.set_audio_normalize_lufs(lufs);
                app_data.set_audio_normalize_enabled(true);
                app_data.set_has_active_audio_edits(true);

                ctx.restart_audio_preview(&app_data);
            }
        });
    }

    // Handle Reset Audio Edits
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        ui.global::<AppLogic>().on_reset_audio_edits(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_audio_gain_db(0.0);
                app_data.set_audio_gain_text(SharedString::from("0.0 dB"));
                app_data.set_audio_normalize_enabled(false);
                app_data.set_audio_normalize_lufs(-16.0);
                app_data.set_audio_normalize_preset_index(0);
                app_data.set_has_active_audio_edits(false);

                ctx.restart_audio_preview(&app_data);
            }
        });
    }
}
