use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use another_encoder::ffmpeg::{spawn_audio_playback, AudioPlaybackSession, FFmpegBinaries, VideoRotation};
use another_encoder::ytdlp;

#[derive(Clone, Debug)]
pub struct PreviewTask {
    pub media_path: String,
    pub time: f64,
    pub id: u64,
    pub rotation: VideoRotation,
    pub flip_h: bool,
    pub flip_v: bool,
}

#[derive(Clone, Debug)]
pub struct WaveformTask {
    pub media_path: String,
    pub id: u64,
    pub is_video: bool,
}

/// Handles passed to background threads for probing and frame extraction.
#[derive(Clone)]
pub struct TaskContext {
    pub binaries: Arc<FFmpegBinaries>,
    pub preview_tx: std::sync::mpsc::Sender<PreviewTask>,
    pub latest_preview_id: Arc<AtomicU64>,
    pub waveform_tx: std::sync::mpsc::Sender<WaveformTask>,
    pub latest_waveform_id: Arc<AtomicU64>,
}

/// Shared runtime context holding process handles, worker channels, and playback state.
#[derive(Clone)]
pub struct AppContext {
    pub binaries: Arc<FFmpegBinaries>,
    pub cancel_flag: Arc<AtomicBool>,
    pub active_player_child: Arc<Mutex<Option<AudioPlaybackSession>>>,
    pub active_playback_child: Arc<Mutex<Option<std::process::Child>>>,
    pub playback_session_id: Arc<AtomicU64>,
    pub preview_tx: std::sync::mpsc::Sender<PreviewTask>,
    pub latest_preview_id: Arc<AtomicU64>,
    pub waveform_tx: std::sync::mpsc::Sender<WaveformTask>,
    pub latest_waveform_id: Arc<AtomicU64>,
    pub ytdlp_binary: Arc<Option<PathBuf>>,
    pub ytdlp_discovered_options: Arc<Mutex<Vec<ytdlp::DownloadFormatOption>>>,
    pub active_ytdlp_child: Arc<Mutex<Option<std::process::Child>>>,
    pub active_ytdlp_cancel: Arc<AtomicBool>,
    pub playback_timer: Rc<RefCell<slint::Timer>>,
    pub meter_timer: Rc<RefCell<slint::Timer>>,
    pub last_audio_restart: Arc<Mutex<std::time::Instant>>,
}

impl AppContext {
    pub fn task_context(&self) -> TaskContext {
        TaskContext {
            binaries: self.binaries.clone(),
            preview_tx: self.preview_tx.clone(),
            latest_preview_id: self.latest_preview_id.clone(),
            waveform_tx: self.waveform_tx.clone(),
            latest_waveform_id: self.latest_waveform_id.clone(),
        }
    }

    /// Stop any running preview playback stream and native ffplay session.
    pub fn stop_active_playback(&self, app_data: &crate::AppData) {
        self.playback_session_id.fetch_add(1, Ordering::SeqCst);
        self.playback_timer.borrow().stop();
        let mut stream_lock = self.active_playback_child.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut child) = stream_lock.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let mut player_lock = self.active_player_child.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(mut session) = player_lock.take() {
            let _ = session.kill();
        }
        app_data.set_player_is_playing(false);
    }

    /// Restart live audio playback preview with updated audio filters (gain, normalization).
    pub fn restart_audio_preview(&self, app_data: &crate::AppData) {
        if !app_data.get_player_is_playing() || !app_data.get_selected_is_audio() {
            return;
        }
        if let Ok(mut t) = self.last_audio_restart.lock() {
            if t.elapsed() < std::time::Duration::from_millis(50) {
                return;
            }
            *t = std::time::Instant::now();
        }

        let vol_float = if app_data.get_player_is_muted() {
            0.0
        } else {
            app_data.get_player_volume()
        };
        let vol_int = (vol_float * 100.0).round().clamp(0.0, 100.0) as i32;

        if let Some(ref ffplay_bin) = self.binaries.ffplay {
            let cur_time = app_data.get_player_current_time() as f64;
            let out_sec = app_data.get_out_point_seconds() as f64;
            let dur_opt = if out_sec > cur_time {
                Some(out_sec - cur_time)
            } else {
                None
            };
            let path = app_data.get_selected_file_path().to_string();
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

            let mut lock = self.active_player_child.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(mut old) = lock.take() {
                let _ = old.kill();
            }
            if let Ok(session) = spawn_audio_playback(
                ffplay_bin,
                &path,
                cur_time,
                dur_opt,
                vol_int,
                edit_gain_opt,
                edit_norm_opt,
            ) {
                *lock = Some(session);
            }
        }
    }
}
