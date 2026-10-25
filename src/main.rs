slint::include_modules!();

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

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
    // Locate FFmpeg & FFprobe binaries
    let binaries = match FFmpegBinaries::locate() {
        Ok(b) => {
            println!("FFmpeg found at: {:?}", b.ffmpeg);
            println!("FFprobe found at: {:?}", b.ffprobe);
            if let Some(ref p) = b.ffplay {
                println!("FFplay found at: {:?}", p);
            }
            Arc::new(b)
        }
        Err(e) => {
            eprintln!("Warning: FFmpeg binaries not found in project root path: {}", e);
            // Default to local project root directory names
            Arc::new(FFmpegBinaries {
                ffmpeg: PathBuf::from(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }),
                ffprobe: PathBuf::from(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }),
                ffplay: None,
            })
        }
    };

    let ui = MainWindow::new().map_err(|e| anyhow::anyhow!("Failed to create window: {:?}", e))?;
    let _app_settings = settings::load_settings();

    // Initialize empty queue model in Slint
    ui.global::<AppData>()
        .set_queue(ModelRc::new(VecModel::from(Vec::<QueueFile>::new())));

    // Shared cancellation flag for child ffmpeg processes
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Active ffplay child process for native playback preview (with atomic audio master clock)
    let active_player_child: Arc<Mutex<Option<AudioPlaybackSession>>> = Arc::new(Mutex::new(None));
    // Active embedded 24fps stream child process
    let active_playback_child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
    let playback_session_id = Arc::new(AtomicU64::new(0));

    // Frame preview extraction channel and background worker thread (In-Memory, Zero Disk I/O)
    let (preview_tx, preview_rx) = std::sync::mpsc::channel::<PreviewTask>();
    let latest_preview_id = Arc::new(AtomicU64::new(0));

    // Audio waveform extraction channel and background worker thread
    let (waveform_tx, waveform_rx) = std::sync::mpsc::channel::<WaveformTask>();
    let latest_waveform_id = Arc::new(AtomicU64::new(0));

    // Locate yt-dlp binary
    let ytdlp_binary = Arc::new(ytdlp::find_ytdlp_binary());
    if let Some(ref p) = *ytdlp_binary {
        println!("yt-dlp engine found at: {:?}", p);
    } else {
        eprintln!("Warning: yt-dlp binary not found in project root path");
    }

    let default_dl_dir = _app_settings
        .default_output_dir
        .as_ref()
        .map(PathBuf::from)
        .or_else(dirs::download_dir)
        .or_else(dirs::video_dir)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    ui.global::<AppData>().set_ytdlp_output_dir(SharedString::from(default_dl_dir.to_string_lossy().as_ref()));

    let ytdlp_discovered_options = Arc::new(Mutex::new(Vec::new()));
    let active_ytdlp_child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
    let active_ytdlp_cancel = Arc::new(AtomicBool::new(false));
    let playback_timer = Rc::new(RefCell::new(slint::Timer::default()));
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
        last_audio_restart,
    };

    // Register all event callbacks onto the UI
    handlers::register_all(&ui, &ctx);

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
        win_drag_drop::setup_drop_target(initial_hwnd, move |dropped_paths| {
            println!("Dropped {} file(s) into queue", dropped_paths.len());
            handlers::queue::process_incoming_paths(
                dropped_paths,
                ui_weak.clone(),
                task_ctx_drop.clone(),
            );
        });

        // Poll during early window lifecycle to attach to the real application HWND as soon as it appears
        let check_timer = Rc::new(RefCell::new(slint::Timer::default()));
        let check_timer_clone = check_timer.clone();
        let mut attempts = 0;
        check_timer.borrow_mut().start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(150),
            move || {
                attempts += 1;
                win_drag_drop::reapply_drop_target();
                if win_drag_drop::is_attached() && attempts >= 15 {
                    check_timer_clone.borrow().stop();
                }
            },
        );
    }

    ui.run()
        .map_err(|e| anyhow::anyhow!("Platform error: {:?}", e))
}
