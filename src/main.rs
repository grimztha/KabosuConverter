slint::include_modules!();

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use another_encoder::ffmpeg::{
    AudioCodecChoice, AudioPlaybackSession, FFmpegBinaries, HwAccel, VideoCodec,
};
use another_encoder::settings::{self, AppSettings};
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
    let app_settings = settings::load_settings();
    apply_settings_to_ui(&ui, &app_settings);

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

    if app_settings.default_output_dir.is_none() {
        let default_dl_dir = dirs::download_dir()
            .or_else(dirs::video_dir)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        ui.global::<AppData>().set_ytdlp_output_dir(SharedString::from(default_dl_dir.to_string_lossy().as_ref()));
    }

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

fn apply_settings_to_ui(ui: &MainWindow, settings: &AppSettings) {
    let data = ui.global::<AppData>();

    if let Some(ref dir) = settings.default_output_dir {
        if !dir.is_empty() && dir != "Same as source" {
            data.set_output_directory(SharedString::from(dir));
            data.set_ytdlp_output_dir(SharedString::from(dir));
        }
    }

    let func_idx = match settings.default_video_codec {
        VideoCodec::H264 => 0,
        VideoCodec::H265 => 1,
        VideoCodec::AV1 => 2,
        VideoCodec::AppleProRes => 3,
        VideoCodec::AvidDNxHD => 4,
        VideoCodec::VP9 => 5,
        VideoCodec::Copy => 6,
        _ => 0,
    };
    data.set_function_index(func_idx);

    let hw_idx = match settings.default_hw_accel {
        HwAccel::Auto => 0,
        HwAccel::Nvenc => 1,
        HwAccel::Qsv => 2,
        HwAccel::Amf => 3,
        HwAccel::Cpu => 4,
    };
    data.set_hw_accel_index(hw_idx);

    data.set_crf_value(settings.default_crf as f32);

    let audio_codec_idx = match settings.default_audio_codec {
        AudioCodecChoice::AAC => 0,
        AudioCodecChoice::MP3 => 1,
        AudioCodecChoice::Opus => 2,
        AudioCodecChoice::FLAC => 3,
        AudioCodecChoice::WavPCM24 => 4,
        AudioCodecChoice::WavPCM16 => 5,
        AudioCodecChoice::AC3 => 6,
        AudioCodecChoice::Copy => 7,
    };
    data.set_audio_codec_index(audio_codec_idx);

    let audio_br_idx = match settings.default_audio_bitrate_kbps {
        320 => 0,
        256 => 1,
        192 => 2,
        128 => 3,
        96 => 4,
        _ => 5,
    };
    data.set_audio_bitrate_index(audio_br_idx);
}
