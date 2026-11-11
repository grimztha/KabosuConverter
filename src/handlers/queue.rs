use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use kabosu_converter::ffmpeg::probe_file;
use crate::context::{AppContext, TaskContext, WaveformTask};
use crate::handlers::editor::{
    request_current_preview_frame, sync_effective_dimensions_and_viewport,
    update_video_edit_indicators,
};
use crate::handlers::timeline::update_trim_indicators;
use crate::utils::{collect_media_files_from_dir, format_bytes_fast};
use crate::{AppData, AppLogic, MainWindow, QueueFile};

pub fn load_selected_file(app_data: &AppData, item: &QueueFile, ctx: &TaskContext) {
    app_data.set_selected_file_name(item.file_name.clone());
    app_data.set_selected_file_path(item.file_path.clone());
    app_data.set_selected_is_video(item.is_video);
    app_data.set_selected_is_audio(item.is_audio);
    app_data.set_selected_resolution(item.resolution.clone());
    app_data.set_selected_audio_desc(item.codecs.clone());
    app_data.set_player_duration(item.duration_seconds);
    app_data.set_player_duration_text(item.duration.clone());
    app_data.set_player_current_time(0.0);
    app_data.set_player_time_text(SharedString::from("00:00.000"));
    app_data.set_player_is_playing(false);
    app_data.set_player_has_media(true);
    app_data.set_active_tab(1);

    // Reset trim points for newly selected file
    app_data.set_in_point(SharedString::from(""));
    app_data.set_out_point(SharedString::from(""));
    app_data.set_in_point_seconds(-1.0);
    app_data.set_out_point_seconds(-1.0);
    update_trim_indicators(app_data);

    // Initialize video dimensions and reset video editing state for newly selected file
    let vw = if item.video_width > 0 { item.video_width } else { 1920 };
    let vh = if item.video_height > 0 { item.video_height } else { 1080 };
    app_data.set_selected_video_width(vw);
    app_data.set_selected_video_height(vh);
    app_data.set_video_rotation_index(0);
    app_data.set_video_flip_horizontal(false);
    app_data.set_video_flip_vertical(false);
    app_data.set_crop_enabled(false);
    app_data.set_crop_aspect_index(0);
    sync_effective_dimensions_and_viewport(app_data, true);
    update_video_edit_indicators(app_data);

    // Reset audio edits for newly selected file
    app_data.set_audio_gain_db(0.0);
    app_data.set_audio_gain_text(SharedString::from("0.0 dB"));
    app_data.set_audio_normalize_enabled(false);
    app_data.set_audio_normalize_lufs(-16.0);
    app_data.set_audio_normalize_preset_index(0);
    app_data.set_has_active_audio_edits(false);

    // Reset timeline zoom to fit 1.0x on file load
    app_data.set_timeline_zoom(1.0);

    // Request audio waveform visualization for the timeline only when audio stream is present
    app_data.set_has_waveform(false);
    if item.is_audio {
        let wf_id = ctx.latest_waveform_id.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = ctx.waveform_tx.send(WaveformTask {
            media_path: item.file_path.to_string(),
            id: wf_id,
            is_video: item.is_video,
        });
    }

    // Request initial preview frame if video
    if item.is_video {
        request_current_preview_frame(app_data, &ctx.preview_tx, &ctx.latest_preview_id);
    }
}

pub fn process_incoming_paths(
    paths: Vec<PathBuf>,
    ui_weak: slint::Weak<MainWindow>,
    ctx: TaskContext,
) {
    const MEDIA_EXTS: &[&str] = &[
        "mp4", "mkv", "mov", "avi", "webm", "ts", "m4v", "flv", "wmv",
        "mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "m4b",
    ];
    const AUDIO_ONLY_EXTS: &[&str] = &[
        "mp3", "wav", "flac", "aac", "m4a", "ogg", "opus", "m4b",
    ];

    thread::spawn(move || {
        let mut expanded = Vec::new();
        for path in paths {
            if path.is_dir() {
                collect_media_files_from_dir(&path, 0, &mut expanded, MEDIA_EXTS);
            } else if path.is_file() {
                expanded.push(path);
            }
        }

        if expanded.is_empty() {
            return;
        }

        crate::dialogs::set_last_opened_dir(&expanded[0]);

        // Create queue placeholders immediately before probing
        let mut initial_items = Vec::with_capacity(expanded.len());
        for path in expanded {
            let path_str = path.to_string_lossy().to_string();
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            let is_audio_only = AUDIO_ONLY_EXTS.iter().any(|&a| a.eq_ignore_ascii_case(ext));

            let placeholder = QueueFile {
                id: 0,
                file_name: SharedString::from(file_name),
                file_path: SharedString::from(path_str),
                file_size_formatted: SharedString::from(format_bytes_fast(size_bytes)),
                resolution: SharedString::from(if is_audio_only { "Audio Only" } else { "Probing..." }),
                duration: SharedString::from("--:--"),
                codecs: SharedString::from("Probing..."),
                status: SharedString::from("Probing..."),
                progress: 0.0,
                duration_seconds: 0.0,
                is_video: !is_audio_only,
                is_audio: true,
                video_width: 1920,
                video_height: 1080,
            };
            initial_items.push((path, placeholder));
        }

        // Batch-insert placeholders on UI thread immediately
        let ui_w_init = ui_weak.clone();
        let placeholders_for_ui: Vec<QueueFile> =
            initial_items.iter().map(|(_, item)| item.clone()).collect();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_w_init.upgrade() {
                let app_data = ui.global::<AppData>();
                let mut list: Vec<QueueFile> = app_data.get_queue().iter().collect();
                let mut first_added: Option<(usize, QueueFile)> = None;

                for mut item in placeholders_for_ui {
                    let row_idx = list.len();
                    item.id = row_idx as i32;
                    if first_added.is_none() {
                        first_added = Some((row_idx, item.clone()));
                    }
                    list.push(item);
                }

                let count = list.len();
                app_data.set_queue(ModelRc::new(VecModel::from(list)));
                app_data.set_status_summary(SharedString::from(format!(
                    "{} file(s) in queue",
                    count
                )));

                if app_data.get_selected_file_index() == -1 {
                    if let Some((first_idx, first_item)) = first_added {
                        app_data.set_selected_file_index(first_idx as i32);
                        app_data.set_selected_file_name(first_item.file_name.clone());
                        app_data.set_selected_file_path(first_item.file_path.clone());
                        app_data.set_selected_is_video(first_item.is_video);
                        app_data.set_selected_is_audio(first_item.is_audio);
                        app_data.set_selected_resolution(first_item.resolution.clone());
                        app_data.set_selected_audio_desc(first_item.codecs.clone());
                        app_data.set_player_has_media(true);
                    }
                }
            }
        });

        // Probe files concurrently (up to 4 threads)
        let to_probe = Arc::new(initial_items);
        let worker_count = to_probe.len().clamp(1, 4);
        let next_idx = Arc::new(AtomicUsize::new(0));

        for _ in 0..worker_count {
            let queue_arc = to_probe.clone();
            let idx_counter = next_idx.clone();
            let bins = ctx.binaries.clone();
            let ui_w = ui_weak.clone();
            let ctx_sub = ctx.clone();

            thread::spawn(move || {
                loop {
                    let i = idx_counter.fetch_add(1, Ordering::SeqCst);
                    if i >= queue_arc.len() {
                        break;
                    }
                    let (ref path, ref base_item) = queue_arc[i];
                    let mut updated = base_item.clone();

                    match probe_file(path, &bins.ffprobe) {
                        Ok(meta) => {
                            let codecs = meta.codecs_summary();
                            let is_video = meta.primary_video().is_some();
                            let is_audio = meta.primary_audio().is_some();
                            let (vw, vh) = meta
                                .primary_video()
                                .map(|v| (v.width as i32, v.height as i32))
                                .unwrap_or((0, 0));
                            updated.file_size_formatted = SharedString::from(meta.file_size_formatted());
                            updated.resolution = SharedString::from(meta.resolution_str());
                            updated.duration = SharedString::from(meta.duration_formatted);
                            updated.codecs = SharedString::from(codecs);
                            updated.status = SharedString::from("Ready");
                            updated.duration_seconds = meta.duration_seconds as f32;
                            updated.is_video = is_video;
                            updated.is_audio = is_audio;
                            updated.video_width = vw;
                            updated.video_height = vh;
                        }
                        Err(_) => {
                            updated.resolution = SharedString::from("Unknown");
                            updated.duration = SharedString::from("00:00.000");
                            updated.codecs = SharedString::from("Unknown");
                            updated.status = SharedString::from("Ready");
                        }
                    }

                    let ui_w_row = ui_w.clone();
                    let ctx_row = ctx_sub.clone();

                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w_row.upgrade() {
                            let app_data = ui.global::<AppData>();
                            let model = app_data.get_queue();
                            let row_count = model.row_count();
                            let matched_row = (0..row_count)
                                .find(|&r| {
                                    model.row_data(r).is_some_and(|item| {
                                        item.file_path == updated.file_path && item.status == "Probing..."
                                    })
                                })
                                .or_else(|| {
                                    (0..row_count).find(|&r| {
                                        model.row_data(r).is_some_and(|item| item.file_path == updated.file_path)
                                    })
                                });

                            if let Some(r) = matched_row {
                                updated.id = r as i32;
                                model.set_row_data(r, updated.clone());
                                if app_data.get_selected_file_path() == updated.file_path {
                                    let prev_tab = app_data.get_active_tab();
                                    load_selected_file(&app_data, &updated, &ctx_row);
                                    if prev_tab == 0 {
                                        app_data.set_active_tab(0);
                                    }
                                }
                            }
                        }
                    });
                }
            });
        }
    });
}

pub fn register(ui: &MainWindow, ctx: &AppContext) {
    // Handle Add Files
    {
        let ui_handle = ui.as_weak();
        let task_ctx = ctx.task_context();

        ui.global::<AppLogic>().on_add_files(move || {
            let (parent_hwnd, suggested_dir) = if let Some(ui) = ui_handle.upgrade() {
                let hwnd = crate::dialogs::get_window_hwnd(ui.window());
                let app_data = ui.global::<AppData>();
                let sel_path = app_data.get_selected_file_path().to_string();
                let dir = if !sel_path.is_empty() {
                    let p = PathBuf::from(sel_path);
                    p.parent().map(|d| d.to_path_buf())
                } else {
                    let queue: Vec<QueueFile> = app_data.get_queue().iter().collect();
                    queue.last().and_then(|item| {
                        let p = item.file_path.to_string();
                        if !p.is_empty() {
                            PathBuf::from(p).parent().map(|d| d.to_path_buf())
                        } else {
                            None
                        }
                    }).or_else(crate::dialogs::get_last_opened_dir)
                };
                (hwnd, dir)
            } else {
                (std::ptr::null_mut(), crate::dialogs::get_last_opened_dir())
            };

            let ui_weak = ui_handle.clone();
            let task_ctx_c = task_ctx.clone();

            crate::dialogs::pick_media_files_async(parent_hwnd, suggested_dir, move |paths| {
                process_incoming_paths(paths, ui_weak, task_ctx_c);
            });
        });
    }

    // Handle Select File
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        let task_ctx = ctx.task_context();

        ui.global::<AppLogic>().on_select_file(move |index| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let list: Vec<QueueFile> = app_data.get_queue().iter().collect();
                if index >= 0 && (index as usize) < list.len() {
                    ctx.stop_active_playback(&app_data);

                    app_data.set_selected_file_index(index);
                    load_selected_file(&app_data, &list[index as usize], &task_ctx);
                }
            }
        });
    }

    // Handle Remove File
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        let task_ctx = ctx.task_context();

        ui.global::<AppLogic>().on_remove_file(move |index| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let mut list: Vec<QueueFile> = app_data.get_queue().iter().collect();
                if index >= 0 && (index as usize) < list.len() {
                    list.remove(index as usize);
                    for (i, item) in list.iter_mut().enumerate() {
                        item.id = i as i32;
                    }
                    let count = list.len();
                    app_data.set_queue(ModelRc::new(VecModel::from(list.clone())));
                    app_data.set_status_summary(SharedString::from(format!(
                        "{} file(s) in queue",
                        count
                    )));

                    let cur_sel = app_data.get_selected_file_index();
                    if cur_sel == index {
                        ctx.stop_active_playback(&app_data);

                        if list.is_empty() {
                            app_data.set_selected_file_index(-1);
                            app_data.set_player_has_media(false);
                            app_data.set_in_point(SharedString::from(""));
                            app_data.set_out_point(SharedString::from(""));
                            app_data.set_in_point_seconds(-1.0);
                            app_data.set_out_point_seconds(-1.0);
                            update_trim_indicators(&app_data);
                        } else {
                            let new_sel = (index as usize).min(list.len() - 1) as i32;
                            app_data.set_selected_file_index(new_sel);
                            load_selected_file(&app_data, &list[new_sel as usize], &task_ctx);
                        }
                    } else if cur_sel > index {
                        app_data.set_selected_file_index(cur_sel - 1);
                    }
                }
            }
        });
    }

    // Handle Clear Queue
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();

        ui.global::<AppLogic>().on_clear_queue(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                ctx.stop_active_playback(&app_data);

                app_data.set_queue(ModelRc::new(VecModel::from(Vec::<QueueFile>::new())));
                app_data.set_status_summary(SharedString::from("Queue cleared"));
                app_data.set_progress_percent(0.0);
                app_data.set_progress_text(SharedString::from("Ready"));
                app_data.set_selected_file_index(-1);
                app_data.set_player_has_media(false);
                app_data.set_in_point(SharedString::from(""));
                app_data.set_out_point(SharedString::from(""));
                app_data.set_in_point_seconds(-1.0);
                app_data.set_out_point_seconds(-1.0);
                update_trim_indicators(&app_data);
            }
        });
    }

    // Handle Toggle Queue Panel
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_toggle_queue_panel(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_queue_panel_expanded(!app_data.get_queue_panel_expanded());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::ComponentHandle;
    use slint::platform::{Key, WindowEvent};

    #[test]
    fn test_del_key_removes_selected_file_from_queue() {
        super::super::init_test_backend();
        let ui = match MainWindow::new() {
            Ok(u) => u,
            Err(_) => return, // Slint event loop cannot be re-initialized on concurrent test threads
        };
        if ui.show().is_err() {
            return;
        }
        let app_data = ui.global::<AppData>();

        // Set up dummy queue with 3 files
        let f1 = QueueFile {
            id: 0,
            file_name: "file1.mp4".into(),
            file_path: "/path/file1.mp4".into(),
            file_size_formatted: "10 MB".into(),
            resolution: "1920x1080".into(),
            duration: "01:00".into(),
            codecs: "h264/aac".into(),
            status: "Ready".into(),
            progress: 0.0,
            duration_seconds: 60.0,
            is_video: true,
            is_audio: true,
            video_width: 1920,
            video_height: 1080,
        };
        let f2 = QueueFile {
            id: 1,
            file_name: "file2.mp4".into(),
            file_path: "/path/file2.mp4".into(),
            file_size_formatted: "20 MB".into(),
            resolution: "1920x1080".into(),
            duration: "02:00".into(),
            codecs: "h264/aac".into(),
            status: "Ready".into(),
            progress: 0.0,
            duration_seconds: 120.0,
            is_video: true,
            is_audio: true,
            video_width: 1920,
            video_height: 1080,
        };
        let f3 = QueueFile {
            id: 2,
            file_name: "file3.mp4".into(),
            file_path: "/path/file3.mp4".into(),
            file_size_formatted: "30 MB".into(),
            resolution: "1920x1080".into(),
            duration: "03:00".into(),
            codecs: "h264/aac".into(),
            status: "Ready".into(),
            progress: 0.0,
            duration_seconds: 180.0,
            is_video: true,
            is_audio: true,
            video_width: 1920,
            video_height: 1080,
        };

        app_data.set_queue(ModelRc::new(VecModel::from(vec![f1, f2, f3])));
        app_data.set_selected_file_index(1); // Select f2

        // Register dummy on_remove_file that removes the index
        let ui_weak = ui.as_weak();
        ui.global::<AppLogic>().on_remove_file(move |index| {
            if let Some(ui) = ui_weak.upgrade() {
                let data = ui.global::<AppData>();
                let mut list: Vec<QueueFile> = data.get_queue().iter().collect();
                if index >= 0 && (index as usize) < list.len() {
                    list.remove(index as usize);
                    for (i, item) in list.iter_mut().enumerate() {
                        item.id = i as i32;
                    }
                    data.set_queue(ModelRc::new(VecModel::from(list.clone())));
                    if list.is_empty() {
                        data.set_selected_file_index(-1);
                    } else {
                        let new_sel = (index as usize).min(list.len() - 1) as i32;
                        data.set_selected_file_index(new_sel);
                    }
                }
            }
        });

        // Dispatch DEL key event to MainWindow
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Delete.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Delete.into(),
        });

        // Verify f2 (index 1) was removed, leaving 2 files (f1 and f3)
        let q: Vec<QueueFile> = app_data.get_queue().iter().collect();
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].file_name.as_str(), "file1.mp4");
        assert_eq!(q[1].file_name.as_str(), "file3.mp4");
        assert_eq!(app_data.get_selected_file_index(), 1); // f3 is now at index 1 and selected

        // Press DEL again - should remove f3
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Delete.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Delete.into(),
        });

        let q2: Vec<QueueFile> = app_data.get_queue().iter().collect();
        assert_eq!(q2.len(), 1);
        assert_eq!(q2[0].file_name.as_str(), "file1.mp4");
        assert_eq!(app_data.get_selected_file_index(), 0);

        // Press DEL again - should remove f1
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Delete.into(),
        });
        ui.window().dispatch_event(WindowEvent::KeyReleased {
            text: Key::Delete.into(),
        });

        let q3: Vec<QueueFile> = app_data.get_queue().iter().collect();
        assert_eq!(q3.len(), 0);
        assert_eq!(app_data.get_selected_file_index(), -1);

        // Press DEL again when empty - should be a safe no-op
        ui.window().dispatch_event(WindowEvent::KeyPressed {
            text: Key::Delete.into(),
        });
        assert_eq!(app_data.get_selected_file_index(), -1);
    }
}
