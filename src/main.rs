#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use slint::{ComponentHandle, ModelRc, VecModel};

use another_encoder::ffmpeg::{AudioPlaybackSession, FFmpegBinaries};
use another_encoder::settings;
use another_encoder::ytdlp;

mod context;
mod handlers;
mod utils;
mod workers;

#[cfg(windows)]
mod win_drag_drop;

use context::{AppContext, PreviewTask, WaveformTask};

fn main() -> Result<()> {
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
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            Arc::new(FFmpegBinaries {
                ffmpeg: cwd.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }),
                ffprobe: cwd.join(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }),
                ffplay: None,
            })
        }
    };

    let ui = MainWindow::new().map_err(|e| anyhow::anyhow!("Failed to create window: {:?}", e))?;
    let app_settings = settings::load_settings();
    handlers::settings::apply_settings_to_app_data(&ui.global::<AppData>(), &app_settings);

    // Dynamic engine version badge
    let (_full_ver, badge_text) = binaries.detect_version();
    ui.global::<AppData>().set_ffmpeg_version_badge(badge_text.into());

    let (hw_accel, has_gpu, gpu_name) = binaries.detect_hardware_accel();
    another_encoder::ffmpeg::set_detected_hardware_accel(hw_accel);
    ui.global::<AppData>().set_has_gpu_hardware(has_gpu);
    ui.global::<AppData>().set_detected_gpu_name(gpu_name.into());

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

    let ytdlp_binary = Arc::new(ytdlp::find_ytdlp_binary());
    #[cfg(debug_assertions)]
    if let Some(ref p) = *ytdlp_binary {
        println!("yt-dlp engine found at: {:?}", p);
    } else {
        eprintln!("Warning: yt-dlp binary not found in project root path");
    }

    // Dynamic yt-dlp engine version badge
    let (_ytdlp_ver, ytdlp_badge) = match ytdlp_binary.as_ref() {
        Some(ref p) => ytdlp::detect_version(p),
        None => ("2026.10.26".to_string(), "yt-dlp 2026.10.26 (Local Engine)".to_string()),
    };
    ui.global::<AppData>().set_ytdlp_version_badge(ytdlp_badge.into());

    // If external dependencies are missing, initiate background auto-download into app-local storage
    let needs_download = !binaries.ffmpeg.is_file() || ytdlp_binary.is_none();
    if needs_download {
        let ui_weak = ui.as_weak();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            if let Ok(rt) = rt {
                rt.block_on(async move {
                    if let Ok(dm) = another_encoder::binaries::DependencyManager::new(another_encoder::settings::APP_NAME) {
                        let ui_prog = ui_weak.clone();
                        let result = dm.ensure_dependencies(move |name, prog| {
                            let status = match prog {
                                another_encoder::binaries::DownloadProgress::Started { total_bytes } => {
                                    format!("Downloading {} ({:?} bytes)...", name, total_bytes)
                                }
                                another_encoder::binaries::DownloadProgress::Transferred { total_downloaded, .. } => {
                                    format!("Downloading {}: {:.1} MB", name, total_downloaded as f64 / 1_048_576.0)
                                }
                                another_encoder::binaries::DownloadProgress::VerifyingHash => {
                                    format!("Verifying {} SHA-256 integrity...", name)
                                }
                                another_encoder::binaries::DownloadProgress::Extracting => {
                                    format!("Extracting {} into app-local storage...", name)
                                }
                                another_encoder::binaries::DownloadProgress::Complete => {
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

                        if let Ok(paths) = result {
                            let loc = FFmpegBinaries {
                                ffmpeg: paths.ffmpeg,
                                ffprobe: paths.ffprobe,
                                ffplay: paths.ffplay,
                            };
                            let (_full_ver, badge_text) = loc.detect_version();
                            let ui_cb = ui_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_cb.upgrade() {
                                    ui.global::<AppData>().set_ffmpeg_version_badge(badge_text.into());
                                    let (hw_accel, has_gpu, gpu_name) = loc.detect_hardware_accel();
                                    another_encoder::ffmpeg::set_detected_hardware_accel(hw_accel);
                                    ui.global::<AppData>().set_has_gpu_hardware(has_gpu);
                                    ui.global::<AppData>().set_detected_gpu_name(gpu_name.into());
                                    let (_y_ver, y_badge) = ytdlp::detect_version(&paths.yt_dlp);
                                    ui.global::<AppData>().set_ytdlp_version_badge(y_badge.into());
                                    ui.global::<AppData>().set_status_summary("Dependencies installed and verified.".into());
                                }
                            });
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
    };

    // Register all event callbacks onto the UI
    handlers::register_all(&ui, &ctx);

    // Register custom window control callbacks (minimize, maximize, close)
    let ui_min = ui.as_weak();
    let ui_max = ui.as_weak();
    let ui_close = ui.as_weak();

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
        }
    });

    ui.global::<AppLogic>().on_window_close(move || {
        if let Some(ui) = ui_close.upgrade() {
            handlers::settings::save_settings_from_app_data(&ui.global::<AppData>());
            let _ = ui.window().hide();
        }
        let _ = slint::quit_event_loop();
    });

    let ui_close_req = ui.as_weak();
    ui.window().on_close_requested(move || {
        if let Some(ui) = ui_close_req.upgrade() {
            handlers::settings::save_settings_from_app_data(&ui.global::<AppData>());
        }
        slint::CloseRequestResponse::HideWindow
    });

    ui.show()
        .map_err(|e| anyhow::anyhow!("Failed to show window: {:?}", e))?;

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

    handlers::settings::save_settings_from_app_data(&ui.global::<AppData>());

    run_res
}
