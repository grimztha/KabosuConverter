use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use kabosu_converter::ffmpeg::{
    spawn_audio_playback_with_fade, AudioFadePreview, AudioPlaybackSession, FFmpegBinaries,
    VideoRotation,
};
use kabosu_converter::ytdlp;

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
    pub was_playing_before_scrub: Arc<AtomicBool>,
    pub latest_release: Arc<Mutex<Option<kabosu_converter::updater::GithubRelease>>>,
}

/// Audio filter and volume snapshot extracted from UI state for `ffplay` preview.
#[derive(Clone, Copy, Debug)]
pub struct AudioPreviewParams {
    pub volume_percent: i32,
    pub gain_db: Option<f32>,
    pub normalize_lufs: Option<f32>,
    pub fade: Option<AudioFadePreview>,
}

impl AudioPreviewParams {
    pub fn from_app_data(app_data: &crate::AppData) -> Self {
        let vol_float = if app_data.get_player_is_muted() {
            0.0
        } else {
            app_data.get_player_volume()
        };
        let volume_percent = (vol_float * 100.0).round().clamp(0.0, 100.0) as i32;
        let raw_gain = app_data.get_audio_gain_db();
        let gain_db = (raw_gain.is_finite() && raw_gain.abs() > 0.05).then_some(raw_gain);
        let normalize_lufs = app_data
            .get_audio_normalize_enabled()
            .then(|| app_data.get_audio_normalize_lufs());
        let fade = AppContext::build_audio_fade_preview(app_data);

        Self {
            volume_percent,
            gain_db,
            normalize_lufs,
            fade,
        }
    }
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

    /// Stops any active preview stream and native `ffplay` session.
    pub fn stop_active_playback(&self, app_data: &crate::AppData) {
        self.playback_session_id.fetch_add(1, Ordering::SeqCst);
        self.playback_timer.borrow().stop();
        let stream_child = self
            .active_playback_child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(mut child) = stream_child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let player_session = self
            .active_player_child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(mut session) = player_session {
            let _ = session.kill();
        }
        app_data.set_player_is_playing(false);
    }

    /// Stops all active playback, encoding, and downloader subprocesses on exit.
    pub fn shutdown_all_processes(&self, app_data: &crate::AppData) {
        self.cancel_flag.store(true, Ordering::SeqCst);
        self.active_ytdlp_cancel.store(true, Ordering::SeqCst);
        self.meter_timer.borrow().stop();
        self.stop_active_playback(app_data);
        let ytdlp_child = self
            .active_ytdlp_child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(mut child) = ytdlp_child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Restarts live audio preview with updated filters (throttled to 80ms).
    pub fn restart_audio_preview(&self, app_data: &crate::AppData) {
        if !app_data.get_player_is_playing() || !app_data.get_selected_is_audio() {
            return;
        }
        if let Ok(mut t) = self.last_audio_restart.lock() {
            if t.elapsed() < std::time::Duration::from_millis(80) {
                return;
            }
            *t = std::time::Instant::now();
        }
        self.do_restart_audio_preview(app_data);
    }

    /// Restarts live audio preview immediately, bypassing the throttle.
    pub fn restart_audio_preview_immediate(&self, app_data: &crate::AppData) {
        if !app_data.get_player_is_playing() || !app_data.get_selected_is_audio() {
            return;
        }
        if let Ok(mut t) = self.last_audio_restart.lock() {
            *t = std::time::Instant::now();
        }
        self.do_restart_audio_preview(app_data);
    }

    /// Builds live audio fade preview settings from UI state, or `None` when inactive.
    pub fn build_audio_fade_preview(app_data: &crate::AppData) -> Option<AudioFadePreview> {
        let fade_in = app_data.get_audio_fade_in_seconds() as f64;
        let fade_out = app_data.get_audio_fade_out_seconds() as f64;
        if fade_in <= 0.01 && fade_out <= 0.01 {
            return None;
        }
        let dur = (app_data.get_player_duration() as f64).max(0.0);
        let in_sec = app_data.get_in_point_seconds() as f64;
        let out_sec = app_data.get_out_point_seconds() as f64;
        let trim_start_sec = if in_sec >= 0.0 { in_sec.min(dur) } else { 0.0 };
        let trim_end_sec = if out_sec >= 0.0 {
            out_sec.clamp(trim_start_sec, dur)
        } else {
            dur
        };
        if trim_end_sec <= trim_start_sec {
            return None;
        }
        Some(AudioFadePreview {
            trim_start_sec,
            trim_end_sec,
            fade_in_sec: fade_in.max(0.0),
            fade_out_sec: fade_out.max(0.0),
        })
    }

    fn do_restart_audio_preview(&self, app_data: &crate::AppData) {
        let Some(ref ffplay_bin) = self.binaries.ffplay else {
            return;
        };

        let cur_time = app_data.get_player_current_time() as f64;
        let out_sec = app_data.get_out_point_seconds() as f64;
        let dur_opt = (out_sec > cur_time).then_some(out_sec - cur_time);
        let path = app_data.get_selected_file_path().to_string();
        let params = AudioPreviewParams::from_app_data(app_data);

        let old_session = self
            .active_player_child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(mut old) = old_session {
            let _ = old.kill();
        }

        if let Ok(session) = spawn_audio_playback_with_fade(
            ffplay_bin,
            &path,
            cur_time,
            dur_opt,
            params.volume_percent,
            params.gain_db,
            params.normalize_lufs,
            params.fade,
        ) {
            let mut lock = self
                .active_player_child
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *lock = Some(session);
        }
    }
}
