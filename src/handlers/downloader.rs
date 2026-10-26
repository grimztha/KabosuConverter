use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::thread;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use another_encoder::ffmpeg::{format_duration_hms, probe_file};
use another_encoder::ytdlp;
use crate::context::AppContext;
use crate::handlers::queue::{load_selected_file, process_incoming_paths};
use crate::utils::{get_clipboard_string, normalize_path_for_explorer, open_folder_in_explorer};
use crate::{AppData, AppLogic, MainWindow, QueueFile};

pub fn register(ui: &MainWindow, ctx: &AppContext) {
    // Handle yt-dlp URL Changed
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_ytdlp_url_changed(move |new_url| {
            if let Some(ui) = ui_handle.upgrade() {
                ui.global::<AppData>().set_ytdlp_url(new_url);
            }
        });
    }

    // Handle yt-dlp Clear URL
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_ytdlp_clear_url(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let data = ui.global::<AppData>();
                data.set_ytdlp_url(SharedString::from(""));
                data.set_ytdlp_has_discovered(false);
                data.set_ytdlp_custom_filename(SharedString::from(""));
                data.set_ytdlp_title(SharedString::from(""));
                data.set_ytdlp_has_error(false);
                data.set_ytdlp_error_message(SharedString::from(""));
                data.set_ytdlp_status_message(SharedString::from(
                    "Ready. Paste video or stream URL to discover formats.",
                ));
            }
        });
    }

    // Handle yt-dlp Paste Clipboard
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_ytdlp_paste_clipboard(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if let Some(clip) = get_clipboard_string() {
                    ui.global::<AppData>().set_ytdlp_url(SharedString::from(clip));
                }
            }
        });
    }

    // Handle yt-dlp Select Output Directory
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_ytdlp_select_output_dir(move || {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                if let Some(ui) = ui_handle.upgrade() {
                    let folder_str = folder.to_string_lossy().to_string();
                    ui.global::<AppData>()
                        .set_ytdlp_output_dir(SharedString::from(&folder_str));
                    let mut s = another_encoder::settings::load_settings();
                    s.default_output_dir = Some(folder_str);
                    let _ = another_encoder::settings::save_settings(&s);
                }
            }
        });
    }

    // Handle yt-dlp Open Output Directory
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_ytdlp_open_output_dir(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let dir = app_data.get_ytdlp_output_dir().trim().to_string();
                let last_file = app_data.get_ytdlp_last_downloaded_file().trim().to_string();

                let mut target_folder: Option<PathBuf> = None;

                if !last_file.is_empty() {
                    let file_path = Path::new(&last_file);
                    let resolved_file = if file_path.is_absolute() {
                        file_path.to_path_buf()
                    } else if !dir.is_empty() {
                        PathBuf::from(&dir).join(file_path)
                    } else {
                        std::env::current_dir()
                            .unwrap_or_else(|_| PathBuf::from("."))
                            .join(file_path)
                    };
                    if let Some(parent) = resolved_file.parent() {
                        if !parent.as_os_str().is_empty() {
                            target_folder = Some(parent.to_path_buf());
                        }
                    }
                }

                let folder = target_folder.unwrap_or_else(|| {
                    if !dir.is_empty() {
                        PathBuf::from(&dir)
                    } else {
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                    }
                });

                open_folder_in_explorer(&folder);
            }
        });
    }

    // Handle yt-dlp Discover
    {
        let ui_handle = ui.as_weak();
        let ytdlp_bin_arc = ctx.ytdlp_binary.clone();
        let options_store = ctx.ytdlp_discovered_options.clone();

        ui.global::<AppLogic>().on_ytdlp_discover(move || {
            let ui = match ui_handle.upgrade() {
                Some(u) => u,
                None => return,
            };
            let data = ui.global::<AppData>();
            let url = data.get_ytdlp_url().trim().to_string();
            if url.is_empty() {
                return;
            }

            let bin_path = match ytdlp_bin_arc.as_ref().clone().or_else(ytdlp::find_ytdlp_binary) {
                Some(p) => p,
                None => {
                    data.set_ytdlp_has_error(true);
                    data.set_ytdlp_error_message(SharedString::from(
                        "yt-dlp binary not found in app-local storage or system PATH",
                    ));
                    return;
                }
            };

            data.set_ytdlp_is_discovering(true);
            data.set_ytdlp_has_error(false);
            data.set_ytdlp_error_message(SharedString::from(""));
            data.set_ytdlp_status_message(SharedString::from(
                "Detecting stream formats and metadata via yt-dlp...",
            ));

            let ui_w = ui_handle.clone();
            let opts_store = options_store.clone();

            thread::spawn(move || {
                let result = ytdlp::discover_media(&bin_path, &url);

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_w.upgrade() {
                        let data = ui.global::<AppData>();
                        data.set_ytdlp_is_discovering(false);

                        match result {
                            Ok((info, options)) => {
                                data.set_ytdlp_has_discovered(true);
                                let title_str = info.title.clone().unwrap_or_else(|| "Unknown Title".to_string());
                                data.set_ytdlp_title(SharedString::from(&title_str));
                                let clean_stem = ytdlp::sanitize_filename_stem(&title_str);
                                data.set_ytdlp_custom_filename(SharedString::from(clean_stem));
                                data.set_ytdlp_uploader(SharedString::from(
                                    info.uploader.or(info.channel).unwrap_or_default(),
                                ));
                                data.set_ytdlp_duration(SharedString::from(
                                    info.duration_string.unwrap_or_else(|| {
                                        info.duration
                                            .map(format_duration_hms)
                                            .unwrap_or_default()
                                    }),
                                ));
                                data.set_ytdlp_discovered_id(SharedString::from(
                                    info.id.unwrap_or_default(),
                                ));
                                data.set_ytdlp_status_message(SharedString::from(
                                    "Stream discovered! Select format & click Start Download.",
                                ));

                                let labels: Vec<SharedString> = options
                                    .iter()
                                    .map(|o| SharedString::from(&o.display_label))
                                    .collect();
                                data.set_ytdlp_format_options(ModelRc::new(VecModel::from(labels)));
                                data.set_ytdlp_selected_format_index(0);

                                if let Ok(mut store) = opts_store.lock() {
                                    *store = options;
                                }
                            }
                            Err(e) => {
                                data.set_ytdlp_has_error(true);
                                data.set_ytdlp_error_message(SharedString::from(format!("{}", e)));
                                data.set_ytdlp_status_message(SharedString::from(
                                    "Discovery failed. Check URL and try again.",
                                ));
                            }
                        }
                    }
                });
            });
        });
    }

    // Handle yt-dlp Start Download
    {
        let ui_handle = ui.as_weak();
        let ytdlp_bin_arc = ctx.ytdlp_binary.clone();
        let ffmpeg_dir_arc = ctx.binaries.ffmpeg.parent().map(|p| p.to_path_buf());
        let options_store = ctx.ytdlp_discovered_options.clone();
        let child_store = ctx.active_ytdlp_child.clone();
        let cancel_store = ctx.active_ytdlp_cancel.clone();

        ui.global::<AppLogic>().on_ytdlp_start_download(move || {
            let ui = match ui_handle.upgrade() {
                Some(u) => u,
                None => return,
            };
            let data = ui.global::<AppData>();
            let url = data.get_ytdlp_url().trim().to_string();
            if url.is_empty() {
                return;
            }

            let bin_path = match ytdlp_bin_arc.as_ref().clone().or_else(ytdlp::find_ytdlp_binary) {
                Some(p) => p,
                None => {
                    data.set_ytdlp_has_error(true);
                    data.set_ytdlp_error_message(SharedString::from(
                        "yt-dlp binary not found in app-local storage or system PATH",
                    ));
                    return;
                }
            };

            let selected_idx = data.get_ytdlp_selected_format_index() as usize;
            let format_opt = {
                let store = options_store.lock().unwrap_or_else(|e| e.into_inner());
                if selected_idx < store.len() {
                    store[selected_idx].clone()
                } else if !store.is_empty() {
                    store[0].clone()
                } else {
                    ytdlp::DownloadFormatOption {
                        display_label: "[Video] Best Quality".to_string(),
                        format_spec: "bv*+ba/b".to_string(),
                        is_audio_only: false,
                        audio_ext: None,
                        container_ext: "mp4".to_string(),
                    }
                }
            };

            let dest_dir_str = data.get_ytdlp_output_dir().trim().to_string();
            let raw_dest_dir = if dest_dir_str.is_empty() {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            } else {
                PathBuf::from(dest_dir_str)
            };
            let _ = std::fs::create_dir_all(&raw_dest_dir);
            let dest_dir = normalize_path_for_explorer(&raw_dest_dir);

            cancel_store.store(false, Ordering::SeqCst);
            data.set_ytdlp_is_downloading(true);
            data.set_ytdlp_download_finished(false);
            data.set_ytdlp_progress(0.0);
            data.set_ytdlp_progress_text(SharedString::from("0.0%"));
            data.set_ytdlp_speed_text(SharedString::from("Connecting..."));
            data.set_ytdlp_eta_text(SharedString::from("--:--"));
            data.set_ytdlp_size_text(SharedString::from("-- / --"));
            data.set_ytdlp_status_message(SharedString::from("Initializing stream download..."));

            let ui_progress = ui_handle.clone();
            let ui_log = ui_handle.clone();
            let ui_complete = ui_handle.clone();
            let child_store_inner = child_store.clone();
            let cancel_clone = cancel_store.clone();
            let f_dir = ffmpeg_dir_arc.clone();

            let custom_fname = data.get_ytdlp_custom_filename().trim().to_string();
            let custom_fname_opt = if custom_fname.is_empty() {
                None
            } else {
                Some(custom_fname.as_str())
            };

            let job = ytdlp::start_download(
                &bin_path,
                f_dir.as_deref(),
                &url,
                &format_opt,
                &dest_dir,
                custom_fname_opt,
                child_store_inner.clone(),
                cancel_clone,
                move |prog| {
                    let ui_w = ui_progress.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            let data = ui.global::<AppData>();
                            data.set_ytdlp_progress(prog.percent);
                            data.set_ytdlp_progress_text(SharedString::from(format!(
                                "{:.1}%",
                                prog.percent
                            )));
                            if !prog.speed.is_empty() {
                                data.set_ytdlp_speed_text(SharedString::from(prog.speed));
                            }
                            if !prog.eta.is_empty() {
                                data.set_ytdlp_eta_text(SharedString::from(prog.eta));
                            }
                            if !prog.size.is_empty() {
                                data.set_ytdlp_size_text(SharedString::from(prog.size));
                            }
                            if !prog.status.is_empty() {
                                data.set_ytdlp_status_message(SharedString::from(prog.status));
                            }
                        }
                    });
                },
                move |log_line| {
                    let ui_w = ui_log.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            let cur = ui.global::<AppData>().get_log_preview().to_string();
                            let mut lines: Vec<&str> = cur.lines().collect();
                            if lines.len() > 100 {
                                lines.remove(0);
                            }
                            lines.push(&log_line);
                            ui.global::<AppData>()
                                .set_log_preview(SharedString::from(lines.join("\n")));
                        }
                    });
                },
                move |res| {
                    let ui_w = ui_complete.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            let data = ui.global::<AppData>();
                            data.set_ytdlp_is_downloading(false);
                            match res {
                                Ok(file_path) => {
                                    let clean_file_path = normalize_path_for_explorer(&file_path);
                                    data.set_ytdlp_has_error(false);
                                    data.set_ytdlp_error_message(SharedString::from(""));
                                    data.set_ytdlp_download_finished(true);
                                    data.set_ytdlp_progress(100.0);
                                    data.set_ytdlp_progress_text(SharedString::from("100%"));
                                    data.set_ytdlp_status_message(SharedString::from(
                                        "Download completed successfully!",
                                    ));
                                    data.set_ytdlp_last_downloaded_file(SharedString::from(
                                        clean_file_path.to_string_lossy().as_ref(),
                                    ));
                                }
                                Err(e) => {
                                    data.set_ytdlp_has_error(true);
                                    data.set_ytdlp_error_message(SharedString::from(format!("{}", e)));
                                    data.set_ytdlp_status_message(SharedString::from(format!(
                                        "Download failed: {}",
                                        e
                                    )));
                                }
                            }
                        }
                    });
                },
            );

            if let Err(e) = job {
                data.set_ytdlp_is_downloading(false);
                data.set_ytdlp_has_error(true);
                data.set_ytdlp_error_message(SharedString::from(format!(
                    "Failed to start download: {}",
                    e
                )));
            }
        });
    }

    // Handle yt-dlp Cancel Download
    {
        let ui_handle = ui.as_weak();
        let child_store = ctx.active_ytdlp_child.clone();
        let cancel_store = ctx.active_ytdlp_cancel.clone();

        ui.global::<AppLogic>().on_ytdlp_cancel_download(move || {
            cancel_store.store(true, Ordering::SeqCst);
            let mut lock = child_store.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(mut child) = lock.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
            if let Some(ui) = ui_handle.upgrade() {
                let data = ui.global::<AppData>();
                data.set_ytdlp_is_downloading(false);
                data.set_ytdlp_status_message(SharedString::from("Download cancelled by user."));
            }
        });
    }

    // Handle yt-dlp Load in Player
    {
        let ui_handle = ui.as_weak();
        let ctx = ctx.clone();
        let bins = ctx.binaries.clone();
        let task_ctx = ctx.task_context();

        ui.global::<AppLogic>().on_ytdlp_load_in_player(move || {
            let ui = match ui_handle.upgrade() {
                Some(u) => u,
                None => return,
            };
            let app_data = ui.global::<AppData>();
            let file_path_str = app_data
                .get_ytdlp_last_downloaded_file()
                .to_string();
            if file_path_str.is_empty() {
                return;
            }
            let path = PathBuf::from(&file_path_str);
            if !path.exists() {
                return;
            }

            ctx.stop_active_playback(&app_data);
            app_data.set_active_tab(0);
            let ui_w = ui_handle.clone();
            let bins_c = bins.clone();
            let task_ctx_c = task_ctx.clone();

            thread::spawn(move || {
                let file_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let (file_size_formatted, resolution, duration, codecs, duration_seconds, is_video, is_audio, vw, vh) =
                    match probe_file(&path, &bins_c.ffprobe) {
                        Ok(meta) => {
                            let codecs = meta.codecs_summary();
                            let is_video = meta.primary_video().is_some();
                            let is_audio = meta.primary_audio().is_some();
                            let (vw, vh) = meta
                                .primary_video()
                                .map(|v| (v.width as i32, v.height as i32))
                                .unwrap_or((0, 0));
                            (
                                meta.file_size_formatted(),
                                meta.resolution_str(),
                                meta.duration_formatted,
                                codecs,
                                meta.duration_seconds as f32,
                                is_video,
                                is_audio,
                                vw,
                                vh,
                            )
                        }
                        Err(_) => (
                            "Unknown size".to_string(),
                            "Unknown".to_string(),
                            "00:00.000".to_string(),
                            "Unknown".to_string(),
                            0.0,
                            false,
                            false,
                            0,
                            0,
                        ),
                    };

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_w.upgrade() {
                        let app_data = ui.global::<AppData>();
                        let mut list: Vec<QueueFile> = app_data.get_queue().iter().collect();
                        let item = QueueFile {
                            id: list.len() as i32,
                            file_name: SharedString::from(file_name),
                            file_path: SharedString::from(file_path_str),
                            file_size_formatted: SharedString::from(file_size_formatted),
                            resolution: SharedString::from(resolution),
                            duration: SharedString::from(duration),
                            codecs: SharedString::from(codecs),
                            status: SharedString::from("Ready"),
                            progress: 0.0,
                            duration_seconds,
                            is_video,
                            is_audio,
                            video_width: vw,
                            video_height: vh,
                        };
                        list.push(item.clone());
                        let idx = (list.len() - 1) as i32;
                        app_data.set_queue(ModelRc::new(VecModel::from(list)));
                        app_data.set_selected_file_index(idx);
                        load_selected_file(&app_data, &item, &task_ctx_c);
                    }
                });
            });
        });
    }

    // Handle yt-dlp Send to Queue
    {
        let ui_handle = ui.as_weak();
        let task_ctx = ctx.task_context();

        ui.global::<AppLogic>().on_ytdlp_send_to_queue(move || {
            let ui = match ui_handle.upgrade() {
                Some(u) => u,
                None => return,
            };
            let app_data = ui.global::<AppData>();
            let file_path_str = app_data
                .get_ytdlp_last_downloaded_file()
                .to_string();
            if file_path_str.is_empty() {
                return;
            }
            let path = PathBuf::from(&file_path_str);
            if !path.exists() {
                return;
            }

            app_data.set_active_tab(2);
            process_incoming_paths(vec![path], ui_handle.clone(), task_ctx.clone());
        });
    }
}
