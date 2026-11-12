#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use slint::{ComponentHandle, ModelRc, VecModel};

use kabosu_converter::ffmpeg::{AudioPlaybackSession, FFmpegBinaries};
use kabosu_converter::settings;
use kabosu_converter::ytdlp;

mod context;
mod dialogs;
mod handlers;
mod utils;
mod workers;

#[cfg(windows)]
mod win_drag_drop;

use context::{AppContext, PreviewTask, WaveformTask};

fn main() -> Result<()> {
    let default_bin_dir = settings::get_app_dir().join("bin");
    let binaries = match FFmpegBinaries::locate() {
        Ok(b) => {
            #[cfg(debug_assertions)]
            {
                println!("FFmpeg found at: {:?}", b.ffmpeg);
                println!("FFprobe found at: {:?}", b.ffprobe);
                if let Some(ref p) = b.ffplay {
                    println!("FFplay found at: {:?}", p);
                }
            }
            Arc::new(b)
        }
        Err(_e) => {
            #[cfg(debug_assertions)]
            eprintln!("Warning: FFmpeg binaries not found: {}", _e);
            Arc::new(FFmpegBinaries {
                ffmpeg: default_bin_dir.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }),
                ffprobe: default_bin_dir.join(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }),
                ffplay: Some(default_bin_dir.join(if cfg!(windows) { "ffplay.exe" } else { "ffplay" })),
            })
        }
    };

    let ui = MainWindow::new().map_err(|e| anyhow::anyhow!("Failed to create window: {:?}", e))?;
    let app_settings = settings::load_settings();
    handlers::settings::apply_settings_to_app_data(&ui.global::<AppData>(), &app_settings);

    // Populate initial engine version and hardware acceleration badges if ffmpeg is already present
    if binaries.ffmpeg.is_file() {
        let (_full_ver, badge_text) = binaries.detect_version();
        ui.global::<AppData>().set_ffmpeg_version_badge(badge_text.into());

        let (hw_accel, has_gpu, gpu_name) = binaries.detect_hardware_accel();
        kabosu_converter::ffmpeg::set_detected_hardware_accel(hw_accel);
        ui.global::<AppData>().set_has_gpu_hardware(has_gpu);
        ui.global::<AppData>().set_detected_gpu_name(gpu_name.into());
    }

    ui.global::<AppData>().set_app_version(env!("CARGO_PKG_VERSION").into());
    ui.global::<AppData>().set_update_status_text("Checking for updates...".into());

    ui.global::<AppData>()
        .set_queue(ModelRc::new(VecModel::from(Vec::<QueueFile>::new())));

    let cancel_flag = Arc::new(AtomicBool::new(false));

    let active_player_child: Arc<Mutex<Option<AudioPlaybackSession>>> = Arc::new(Mutex::new(None));
    let active_playback_child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
    let playback_session_id = Arc::new(AtomicU64::new(0));

    let (preview_tx, preview_rx) = std::sync::mpsc::channel::<PreviewTask>();
    let latest_preview_id = Arc::new(AtomicU64::new(0));

    let (waveform_tx, waveform_rx) = std::sync::mpsc::channel::<WaveformTask>();
    let latest_waveform_id = Arc::new(AtomicU64::new(0));

    let found_ytdlp = ytdlp::find_ytdlp_binary();
    #[cfg(debug_assertions)]
    if let Some(ref p) = found_ytdlp {
        println!("yt-dlp engine found at: {:?}", p);
    } else {
        eprintln!("Warning: yt-dlp binary not found in project root path");
    }

    let ytdlp_binary = Arc::new(Some(found_ytdlp.unwrap_or_else(|| {
        default_bin_dir.join(if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" })
    })));

    // Populate initial yt-dlp engine version badge if already installed
    if let Some(ref p) = *ytdlp_binary {
        if p.is_file() {
            let (_ytdlp_ver, ytdlp_badge) = ytdlp::detect_version(p);
            ui.global::<AppData>().set_ytdlp_version_badge(ytdlp_badge.into());
        }
    }

    // Download and verify any missing external binaries in the background on first launch
    let needs_download = !binaries.ffmpeg.is_file()
        || !binaries.ffprobe.is_file()
        || ytdlp_binary.as_ref().as_ref().is_none_or(|p| !p.is_file());
    if needs_download {
        let ui_weak = ui.as_weak();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(rt) = rt {
                rt.block_on(async move {
                    if let Ok(dm) = kabosu_converter::binaries::DependencyManager::new(kabosu_converter::settings::APP_NAME) {
                        let ui_prog = ui_weak.clone();
                        let result = dm.ensure_dependencies(move |name, prog| {
                            let status = match prog {
                                kabosu_converter::binaries::DownloadProgress::Started { total_bytes } => {
                                    match total_bytes {
                                        Some(total) => format!("Downloading {} ({:.1} MB)...", name, total as f64 / 1_048_576.0),
                                        None => format!("Downloading {}...", name),
                                    }
                                }
                                kabosu_converter::binaries::DownloadProgress::Transferred { total_downloaded, .. } => {
                                    format!("Downloading {}: {:.1} MB", name, total_downloaded as f64 / 1_048_576.0)
                                }
                                kabosu_converter::binaries::DownloadProgress::VerifyingHash => {
                                    format!("Verifying {} SHA-256 integrity...", name)
                                }
                                kabosu_converter::binaries::DownloadProgress::Extracting => {
                                    format!("Extracting {} into app-local storage...", name)
                                }
                                kabosu_converter::binaries::DownloadProgress::Complete => {
                                    format!("{} installed successfully.", name)
                                }
                            };
                            let ui_cb = ui_prog.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_cb.upgrade() {
                                    ui.global::<AppData>().set_status_summary(status.into());
                                }
                            });
                        }).await;

                        match result {
                            Ok(paths) => {
                                let loc = FFmpegBinaries {
                                    ffmpeg: paths.ffmpeg,
                                    ffprobe: paths.ffprobe,
                                    ffplay: paths.ffplay,
                                };
                                let (_full_ver, badge_text) = loc.detect_version();
                                let (hw_accel, has_gpu, gpu_name) = loc.detect_hardware_accel();
                                kabosu_converter::ffmpeg::set_detected_hardware_accel(hw_accel);
                                let (_y_ver, y_badge) = ytdlp::detect_version(&paths.yt_dlp);

                                let ui_cb = ui_weak.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_cb.upgrade() {
                                        let data = ui.global::<AppData>();
                                        data.set_ffmpeg_version_badge(badge_text.into());
                                        data.set_has_gpu_hardware(has_gpu);
                                        data.set_detected_gpu_name(gpu_name.into());
                                        data.set_ytdlp_version_badge(y_badge.into());
                                        data.set_status_summary("Dependencies installed and verified.".into());
                                    }
                                });
                            }
                            Err(e) => {
                                let msg = format!("Dependency setup failed: {}", e);
                                let ui_cb = ui_weak.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_cb.upgrade() {
                                        ui.global::<AppData>().set_status_summary(msg.into());
                                    }
                                });
                            }
                        }
                    }
                });
            }
        });
    }

    let ytdlp_discovered_options = Arc::new(Mutex::new(Vec::new()));
    let active_ytdlp_child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
    let active_ytdlp_cancel = Arc::new(AtomicBool::new(false));
    let playback_timer = Rc::new(RefCell::new(slint::Timer::default()));
    let meter_timer = Rc::new(RefCell::new(slint::Timer::default()));
    let last_audio_restart = Arc::new(Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(1)));

    // Spawn preview frame background worker
    workers::spawn_preview_worker(
        binaries.ffmpeg.clone(),
        preview_rx,
        latest_preview_id.clone(),
        ui.as_weak(),
    );

    // Spawn audio waveform background worker
    workers::spawn_waveform_worker(
        binaries.ffmpeg.clone(),
        waveform_rx,
        latest_waveform_id.clone(),
        ui.as_weak(),
    );

    let ctx = AppContext {
        binaries: binaries.clone(),
        cancel_flag,
        active_player_child,
        active_playback_child,
        playback_session_id,
        preview_tx,
        latest_preview_id,
        waveform_tx,
        latest_waveform_id,
        ytdlp_binary,
        ytdlp_discovered_options,
        active_ytdlp_child,
        active_ytdlp_cancel,
        playback_timer,
        meter_timer,
        last_audio_restart,
        was_playing_before_scrub: Arc::new(AtomicBool::new(false)),
        latest_release: Arc::new(Mutex::new(None)),
    };

    // Register all event callbacks onto the UI
    handlers::register_all(&ui, &ctx);

    // Register custom window control callbacks (minimize, maximize, close)
    let ui_min = ui.as_weak();
    let ui_max = ui.as_weak();
    let ui_close = ui.as_weak();
    let ctx_close = ctx.clone();

    ui.global::<AppLogic>().on_window_minimize(move || {
        if let Some(ui) = ui_min.upgrade() {
            ui.window().set_minimized(true);
        }
    });

    ui.global::<AppLogic>().on_window_maximize(move || {
        if let Some(ui) = ui_max.upgrade() {
            let is_max = ui.window().is_maximized();
            ui.window().set_maximized(!is_max);
            ui.global::<AppData>().set_is_window_maximized(!is_max);
            #[cfg(windows)]
            win_drag_drop::clean_drag_state();
        }
    });

    ui.global::<AppLogic>().on_window_close(move || {
        if let Some(ui) = ui_close.upgrade() {
            let app_data = ui.global::<AppData>();
            ctx_close.shutdown_all_processes(&app_data);
            handlers::settings::save_settings_from_app_data(&app_data);
            let _ = ui.window().hide();
        }
        let _ = slint::quit_event_loop();
    });

    let ui_close_req = ui.as_weak();
    let ctx_close_req = ctx.clone();
    ui.window().on_close_requested(move || {
        if let Some(ui) = ui_close_req.upgrade() {
            let app_data = ui.global::<AppData>();
            ctx_close_req.shutdown_all_processes(&app_data);
            handlers::settings::save_settings_from_app_data(&app_data);
        }
        slint::CloseRequestResponse::HideWindow
    });

    ui.show()
        .map_err(|e| anyhow::anyhow!("Failed to show window: {:?}", e))?;

    // Automatically check for version updates on application launch
    ui.global::<AppLogic>().invoke_check_for_updates();

    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let native_hwnd = ui
            .window()
            .window_handle()
            .window_handle()
            .ok()
            .and_then(|handle| {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    Some(win32.hwnd.get() as *mut std::ffi::c_void)
                } else {
                    None
                }
            })
            .or_else(win_drag_drop::find_app_window);

        let ui_weak = ui.as_weak();
        let task_ctx_drop = ctx.task_context();

        let initial_hwnd = native_hwnd.unwrap_or(std::ptr::null_mut());
        if !initial_hwnd.is_null() {
            unsafe {
                win_drag_drop::apply_window_rounded_corners(initial_hwnd);
            }
        }
        win_drag_drop::setup_drop_target(initial_hwnd, move |dropped_paths| {
            #[cfg(debug_assertions)]
            println!("Dropped {} file(s) into queue", dropped_paths.len());
            handlers::queue::process_incoming_paths(
                dropped_paths,
                ui_weak.clone(),
                task_ctx_drop.clone(),
            );
        });

        let ui_double = ui.as_weak();
        win_drag_drop::set_titlebar_double_click_handler(move || {
            let ui_weak = ui_double.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    let target_max = win_drag_drop::is_window_maximized_os()
                        .unwrap_or_else(|| !ui.window().is_maximized());
                    if ui.window().is_maximized() != target_max {
                        ui.window().set_maximized(target_max);
                    }
                    if ui.global::<AppData>().get_is_window_maximized() != target_max {
                        ui.global::<AppData>().set_is_window_maximized(target_max);
                    }
                }
            });
        });

        let ui_hit = ui.as_weak();
        win_drag_drop::set_titlebar_hit_test(move |x_logical| {
            if let Some(ui) = ui_hit.upgrade() {
                let mut tabs_start = ui.get_tabs_hit_start();
                let mut tabs_end = ui.get_tabs_hit_end();
                if tabs_end <= tabs_start {
                    let expanded = ui.global::<AppData>().get_queue_panel_expanded();
                    tabs_start = if expanded { 320.0 } else { 58.0 };
                    tabs_end = tabs_start + 340.0;
                }
                // If inside tabs pill, return false so tab switching is not blocked
                if x_logical >= tabs_start && x_logical <= tabs_end {
                    return false;
                }
                true
            } else {
                true
            }
        });

        // Ensure the timer stops once attached or after reaching the attempt limit
        let check_timer = Rc::new(RefCell::new(slint::Timer::default()));
        let check_timer_clone = check_timer.clone();
        let mut attempts = 0;
        check_timer.borrow_mut().start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(150),
            move || {
                attempts += 1;
                win_drag_drop::reapply_drop_target();
                if (win_drag_drop::is_attached() && attempts >= 5) || attempts >= 20 {
                    check_timer_clone.borrow().stop();
                }
            },
        );
    }

    let run_res = ui.run()
        .map_err(|e| anyhow::anyhow!("Platform error: {:?}", e));

    let app_data = ui.global::<AppData>();
    ctx.shutdown_all_processes(&app_data);
    handlers::settings::save_settings_from_app_data(&app_data);

    run_res
}
