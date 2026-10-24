slint::include_modules!();

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::Result;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel, SharedPixelBuffer, Rgba8Pixel};

use another_encoder::ffmpeg::{
    compute_letterbox_viewport, extract_frame_rgba_transformed, extract_waveform_rgba,
    format_duration_hms, parse_timecode_to_seconds, probe_file, spawn_audio_playback,
    spawn_ffplay, spawn_playback_stream_transformed, AudioCodecChoice, CropAspectRatio, CropRect,
    DeinterlaceMode, EncodeJobConfig, EncoderFunction, FFmpegBinaries, FFmpegCommandBuilder,
    FFmpegRunner, HwAccel, ProResProfile, RateControlMode, ResolutionPreset, VideoCodec,
    VideoRotation,
};
use another_encoder::settings;
use another_encoder::ytdlp;

#[cfg(windows)]
mod win_drag_drop;

fn get_clipboard_string() -> Option<String> {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        extern "system" {
            fn OpenClipboard(hWndNewOwner: *mut std::ffi::c_void) -> i32;
            fn CloseClipboard() -> i32;
            fn GetClipboardData(uFormat: u32) -> *mut std::ffi::c_void;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalLock(hMem: *mut std::ffi::c_void) -> *const u16;
            fn GlobalUnlock(hMem: *mut std::ffi::c_void) -> i32;
        }
        const CF_UNICODETEXT: u32 = 13;

        unsafe {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                let h_mem = GetClipboardData(CF_UNICODETEXT);
                let mut result = None;
                if !h_mem.is_null() {
                    let ptr = GlobalLock(h_mem);
                    if !ptr.is_null() {
                        let mut len = 0usize;
                        while *ptr.add(len) != 0 && len < 32_768 {
                            len += 1;
                        }
                        let slice = std::slice::from_raw_parts(ptr, len);
                        let text = String::from_utf16_lossy(slice).trim().to_string();
                        GlobalUnlock(h_mem);
                        if !text.is_empty() {
                            result = Some(text);
                        }
                    }
                }
                CloseClipboard();
                if result.is_some() {
                    return result;
                }
            }
        }
    }
    None
}

fn stop_active_playback(
    active_stream_child: &Arc<Mutex<Option<std::process::Child>>>,
    active_ffplay_child: &Arc<Mutex<Option<std::process::Child>>>,
    session_id: &Arc<AtomicU64>,
    timer: &Rc<RefCell<slint::Timer>>,
    app_data: &AppData,
) {
    session_id.fetch_add(1, Ordering::SeqCst);
    timer.borrow().stop();
    if let Ok(mut lock) = active_stream_child.lock() {
        if let Some(mut child) = lock.take() {
            let _ = child.kill();
        }
    }
    if let Ok(mut lock) = active_ffplay_child.lock() {
        if let Some(mut child) = lock.take() {
            let _ = child.kill();
        }
    }
    app_data.set_player_is_playing(false);
}

struct PreviewTask {
    media_path: String,
    time: f64,
    id: u64,
    rotation: VideoRotation,
    flip_h: bool,
    flip_v: bool,
}

struct WaveformTask {
    media_path: String,
    id: u64,
    is_video: bool,
}

fn rotation_from_index(idx: i32) -> VideoRotation {
    match idx {
        1 => VideoRotation::Rotate90,
        2 => VideoRotation::RotateNeg90,
        3 => VideoRotation::Rotate180,
        _ => VideoRotation::None,
    }
}

fn update_video_edit_indicators(app_data: &AppData) {
    let rot_idx = app_data.get_video_rotation_index();
    let flip_h = app_data.get_video_flip_horizontal();
    let flip_v = app_data.get_video_flip_vertical();
    let crop_en = app_data.get_crop_enabled();
    app_data.set_has_active_video_edits(rot_idx != 0 || flip_h || flip_v || crop_en);
}

fn sync_crop_rect_to_ui(app_data: &AppData, rect: CropRect, update_text_fields: bool) {
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

fn sync_effective_dimensions_and_viewport(app_data: &AppData, reset_crop_rect: bool) {
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

fn request_current_preview_frame(
    app_data: &AppData,
    preview_tx: &std::sync::mpsc::Sender<PreviewTask>,
    latest_id: &Arc<AtomicU64>,
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

fn update_trim_indicators(app_data: &AppData) {
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

fn load_selected_file(
    app_data: &AppData,
    item: &QueueFile,
    preview_tx: &std::sync::mpsc::Sender<PreviewTask>,
    latest_id: &Arc<AtomicU64>,
    waveform_tx: &std::sync::mpsc::Sender<WaveformTask>,
    latest_wf_id: &Arc<AtomicU64>,
) {
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
    app_data.set_active_tab(0);

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
        let wf_id = latest_wf_id.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = waveform_tx.send(WaveformTask {
            media_path: item.file_path.to_string(),
            id: wf_id,
            is_video: item.is_video,
        });
    }

    // Request initial preview frame if video
    if item.is_video {
        request_current_preview_frame(app_data, preview_tx, latest_id);
    }
}

fn format_bytes_fast(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else if bytes > 0 {
        format!("{} B", bytes)
    } else {
        "Unknown size".to_string()
    }
}

fn collect_media_files_from_dir(dir: &Path, depth: usize, out: &mut Vec<PathBuf>, exts: &[&str]) {
    if depth > 4 {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let child = entry.path();
            if child.is_dir() {
                collect_media_files_from_dir(&child, depth + 1, out, exts);
            } else if child.is_file() {
                if let Some(ext) = child.extension().and_then(|e| e.to_str()) {
                    if exts.iter().any(|&m| m.eq_ignore_ascii_case(ext)) {
                        out.push(child);
                    }
                }
            }
        }
    }
}

fn normalize_path_for_explorer(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let resolved = std::fs::canonicalize(&abs).unwrap_or(abs);
    let s = resolved.to_string_lossy();
    let stripped = s.strip_prefix(r"\\?\").unwrap_or(&s);
    let mut clean = stripped.replace('/', r"\");
    while clean.len() > 3 && clean.ends_with('\\') {
        clean.pop();
    }
    PathBuf::from(clean)
}

fn open_folder_in_explorer(folder: &Path) {
    let _ = std::fs::create_dir_all(folder);
    let clean_dir = normalize_path_for_explorer(folder);
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg(&clean_dir)
            .spawn();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(&clean_dir)
            .spawn();
    }
}

fn process_incoming_paths(
    paths: Vec<PathBuf>,
    ui_weak: slint::Weak<MainWindow>,
    binaries: Arc<FFmpegBinaries>,
    preview_tx: std::sync::mpsc::Sender<PreviewTask>,
    latest_id: Arc<AtomicU64>,
    wf_tx: std::sync::mpsc::Sender<WaveformTask>,
    l_wf_id: Arc<AtomicU64>,
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

        // Build lightweight placeholder items immediately (no external process spawns)
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

        // Probe metadata concurrently using a bounded worker pool (up to 4 threads)
        let to_probe = Arc::new(initial_items);
        let worker_count = to_probe.len().clamp(1, 4);
        let next_idx = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        for _ in 0..worker_count {
            let queue_arc = to_probe.clone();
            let idx_counter = next_idx.clone();
            let bins = binaries.clone();
            let ui_w = ui_weak.clone();
            let p_tx_sub = preview_tx.clone();
            let l_id_sub = latest_id.clone();
            let wf_tx_sub = wf_tx.clone();
            let l_wf_sub = l_wf_id.clone();

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
                    let p_tx_row = p_tx_sub.clone();
                    let l_id_row = l_id_sub.clone();
                    let wf_tx_row = wf_tx_sub.clone();
                    let l_wf_row = l_wf_sub.clone();

                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w_row.upgrade() {
                            let app_data = ui.global::<AppData>();
                            let mut list: Vec<QueueFile> = app_data.get_queue().iter().collect();
                            let mut matched = false;
                            for (r, item) in list.iter_mut().enumerate() {
                                if item.file_path == updated.file_path && item.status == "Probing..." {
                                    updated.id = r as i32;
                                    *item = updated.clone();
                                    matched = true;
                                    break;
                                }
                            }
                            if !matched {
                                for (r, item) in list.iter_mut().enumerate() {
                                    if item.file_path == updated.file_path {
                                        updated.id = r as i32;
                                        *item = updated.clone();
                                        matched = true;
                                        break;
                                    }
                                }
                            }
                            if matched {
                                app_data.set_queue(ModelRc::new(VecModel::from(list)));
                                if app_data.get_selected_file_path() == updated.file_path {
                                    load_selected_file(
                                        &app_data,
                                        &updated,
                                        &p_tx_row,
                                        &l_id_row,
                                        &wf_tx_row,
                                        &l_wf_row,
                                    );
                                }
                            }
                        }
                    });
                }
            });
        }
    });
}

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

    // Active ffplay child process for native playback preview
    let active_player_child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
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

    let ytdlp_discovered_options: Arc<Mutex<Vec<ytdlp::DownloadFormatOption>>> =
        Arc::new(Mutex::new(Vec::new()));
    let active_ytdlp_child: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));
    let active_ytdlp_cancel = Arc::new(AtomicBool::new(false));

    {
        let ffmpeg_bin = binaries.ffmpeg.clone();
        let ui_weak = ui.as_weak();
        let latest_id = latest_preview_id.clone();

        thread::spawn(move || {
            while let Ok(task) = preview_rx.recv() {
                // Drain newer tasks if user is scrubbing fast
                let mut current_task = task;
                while let Ok(newer) = preview_rx.try_recv() {
                    current_task = newer;
                }

                if current_task.id < latest_id.load(Ordering::SeqCst) {
                    continue;
                }

                let res = extract_frame_rgba_transformed(
                    &ffmpeg_bin,
                    &current_task.media_path,
                    current_task.time,
                    current_task.rotation,
                    current_task.flip_h,
                    current_task.flip_v,
                );
                if let Ok(rgba_bytes) = res {
                    if current_task.id < latest_id.load(Ordering::SeqCst) {
                        continue;
                    }

                    let ui_w = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(640, 360);
                            pixel_buffer.make_mut_bytes().copy_from_slice(&rgba_bytes);
                            let img = slint::Image::from_rgba8(pixel_buffer);
                            ui.global::<AppData>().set_preview_frame(img);
                            ui.global::<AppData>().set_preview_frame_loading(false);
                        }
                    });
                } else {
                    let ui_w = ui_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            ui.global::<AppData>().set_preview_frame_loading(false);
                        }
                    });
                }
            }
        });
    }

    {
        let ffmpeg_bin = binaries.ffmpeg.clone();
        let ui_weak = ui.as_weak();
        let latest_id = latest_waveform_id.clone();

        thread::spawn(move || {
            while let Ok(task) = waveform_rx.recv() {
                let mut current_task = task;
                while let Ok(newer) = waveform_rx.try_recv() {
                    current_task = newer;
                }

                if current_task.id < latest_id.load(Ordering::SeqCst) {
                    continue;
                }

                let (wf_w, wf_h) = if current_task.is_video {
                    (2400, 68)
                } else {
                    (2400, 180)
                };

                match extract_waveform_rgba(&ffmpeg_bin, &current_task.media_path, wf_w, wf_h) {
                    Ok(rgba) => {
                        if current_task.id == latest_id.load(Ordering::SeqCst) {
                            let ui_w = ui_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(wf_w, wf_h);
                                    pixel_buffer.make_mut_bytes().copy_from_slice(&rgba);
                                    let img = slint::Image::from_rgba8(pixel_buffer);
                                    let data = ui.global::<AppData>();
                                    data.set_timeline_waveform(img);
                                    data.set_has_waveform(true);
                                }
                            });
                        }
                    }
                    Err(_) => {
                        if current_task.id == latest_id.load(Ordering::SeqCst) {
                            let ui_w = ui_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    ui.global::<AppData>().set_has_waveform(false);
                                }
                            });
                        }
                    }
                }
            }
        });
    }


    // Playback timer (runs on Slint UI thread)
    let playback_timer = Rc::new(RefCell::new(slint::Timer::default()));

    // Handle Add Files
    {
        let ui_handle = ui.as_weak();
        let binaries = binaries.clone();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let wf_tx = waveform_tx.clone();
        let l_wf_id = latest_waveform_id.clone();

        ui.global::<AppLogic>().on_add_files(move || {
            let files = rfd::FileDialog::new()
                .set_title("Select Media Files")
                .add_filter(
                    "Video & Audio",
                    &[
                        "mp4", "mkv", "mov", "avi", "webm", "ts", "m4v", "flv", "wmv", "mp3",
                        "wav", "flac", "aac", "m4a", "ogg", "opus",
                    ],
                )
                .pick_files();

            if let Some(paths) = files {
                process_incoming_paths(
                    paths,
                    ui_handle.clone(),
                    binaries.clone(),
                    preview_tx.clone(),
                    latest_id.clone(),
                    wf_tx.clone(),
                    l_wf_id.clone(),
                );
            }
        });
    }

    // Handle Select File
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let wf_tx = waveform_tx.clone();
        let l_wf_id = latest_waveform_id.clone();
        let play_timer_clone = playback_timer.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();

        ui.global::<AppLogic>().on_select_file(move |index| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let list: Vec<QueueFile> = app_data.get_queue().iter().collect();
                if index >= 0 && (index as usize) < list.len() {
                    // Stop any ongoing playback
                    stop_active_playback(&active_stream_clone, &active_ffplay_clone, &session_id_clone, &play_timer_clone, &app_data);

                    app_data.set_selected_file_index(index);
                    load_selected_file(&app_data, &list[index as usize], &preview_tx, &latest_id, &wf_tx, &l_wf_id);
                }
            }
        });
    }

    // Handle Remove File
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let wf_tx = waveform_tx.clone();
        let l_wf_id = latest_waveform_id.clone();
        let play_timer_clone = playback_timer.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();

        ui.global::<AppLogic>().on_remove_file(move |index| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let mut list: Vec<QueueFile> = app_data.get_queue().iter().collect();
                if index >= 0 && (index as usize) < list.len() {
                    list.remove(index as usize);
                    let count = list.len();
                    app_data.set_queue(ModelRc::new(VecModel::from(list.clone())));
                    app_data.set_status_summary(SharedString::from(format!(
                        "{} file(s) in queue",
                        count
                    )));

                    let cur_sel = app_data.get_selected_file_index();
                    if cur_sel == index {
                        stop_active_playback(&active_stream_clone, &active_ffplay_clone, &session_id_clone, &play_timer_clone, &app_data);

                        if list.is_empty() {
                            app_data.set_selected_file_index(-1);
                            app_data.set_player_has_media(false);
                            app_data.set_in_point(SharedString::from(""));
                            app_data.set_out_point(SharedString::from(""));
                            app_data.set_in_point_seconds(-1.0);
                            app_data.set_out_point_seconds(-1.0);
                            update_trim_indicators(&app_data);
                        } else {
                            let new_sel = 0.min(list.len() as i32 - 1);
                            app_data.set_selected_file_index(new_sel);
                            load_selected_file(&app_data, &list[new_sel as usize], &preview_tx, &latest_id, &wf_tx, &l_wf_id);
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
        let play_timer_clone = playback_timer.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();

        ui.global::<AppLogic>().on_clear_queue(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                stop_active_playback(&active_stream_clone, &active_ffplay_clone, &session_id_clone, &play_timer_clone, &app_data);

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

    // Handle Player Seek (Scrubber slider)
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let play_timer_clone = playback_timer.clone();

        ui.global::<AppLogic>().on_player_seek(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    stop_active_playback(&active_stream_clone, &active_ffplay_clone, &session_id_clone, &play_timer_clone, &app_data);
                }

                let dur = app_data.get_player_duration();
                let clamped = val.clamp(0.0, dur.max(0.0));
                app_data.set_player_current_time(clamped);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(clamped as f64)));

                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &preview_tx, &latest_id);
                }
            }
        });
    }

    // Handle Player Step (Relative stepping: -1s, -0.1s, +0.1s, +1s)
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let play_timer_clone = playback_timer.clone();

        ui.global::<AppLogic>().on_player_step(move |delta| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    stop_active_playback(&active_stream_clone, &active_ffplay_clone, &session_id_clone, &play_timer_clone, &app_data);
                }

                let cur = app_data.get_player_current_time();
                let dur = app_data.get_player_duration();
                let new_val = (cur + delta).clamp(0.0, dur.max(0.0));
                app_data.set_player_current_time(new_val);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(new_val as f64)));

                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &preview_tx, &latest_id);
                }
            }
        });
    }

    // Handle Set In Point Current
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_set_in_point_current(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let cur = app_data.get_player_current_time();
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
                app_data.set_out_point_seconds(cur);
                app_data.set_out_point(SharedString::from(format_duration_hms(cur as f64)));
                update_trim_indicators(&app_data);
            }
        });
    }

    // Handle Set In Point from Timeline Drag
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let play_timer_clone = playback_timer.clone();

        ui.global::<AppLogic>().on_set_in_point_from_timeline(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    stop_active_playback(&active_stream_clone, &active_ffplay_clone, &session_id_clone, &play_timer_clone, &app_data);
                }

                let dur = app_data.get_player_duration();
                let clamped = val.clamp(0.0, dur.max(0.0));
                app_data.set_in_point_seconds(clamped);
                app_data.set_in_point(SharedString::from(format_duration_hms(clamped as f64)));
                update_trim_indicators(&app_data);

                app_data.set_player_current_time(clamped);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(clamped as f64)));
                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &preview_tx, &latest_id);
                }
            }
        });
    }

    // Handle Set Out Point from Timeline Drag
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let play_timer_clone = playback_timer.clone();

        ui.global::<AppLogic>().on_set_out_point_from_timeline(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    stop_active_playback(&active_stream_clone, &active_ffplay_clone, &session_id_clone, &play_timer_clone, &app_data);
                }

                let dur = app_data.get_player_duration();
                let clamped = val.clamp(0.0, dur.max(0.0));
                app_data.set_out_point_seconds(clamped);
                app_data.set_out_point(SharedString::from(format_duration_hms(clamped as f64)));
                update_trim_indicators(&app_data);

                app_data.set_player_current_time(clamped);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(clamped as f64)));
                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &preview_tx, &latest_id);
                }
            }
        });
    }

    // Handle Jump to In Point
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();

        ui.global::<AppLogic>().on_jump_to_in_point(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let in_sec = app_data.get_in_point_seconds();
                if in_sec >= 0.0 {
                    app_data.set_player_current_time(in_sec);
                    app_data.set_player_time_text(SharedString::from(format_duration_hms(in_sec as f64)));
                    if app_data.get_selected_is_video() {
                        request_current_preview_frame(&app_data, &preview_tx, &latest_id);
                    }
                }
            }
        });
    }

    // Handle Jump to Out Point
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();

        ui.global::<AppLogic>().on_jump_to_out_point(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let out_sec = app_data.get_out_point_seconds();
                if out_sec >= 0.0 {
                    app_data.set_player_current_time(out_sec);
                    app_data.set_player_time_text(SharedString::from(format_duration_hms(out_sec as f64)));
                    if app_data.get_selected_is_video() {
                        request_current_preview_frame(&app_data, &preview_tx, &latest_id);
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

    // Handle Player Play/Pause
    {
        let ui_handle = ui.as_weak();
        let play_timer_clone = playback_timer.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let binaries = binaries.clone();

        ui.global::<AppLogic>().on_player_play_pause(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if !app_data.get_player_has_media() {
                    return;
                }

                let is_playing = app_data.get_player_is_playing();
                if is_playing {
                    // Pause
                    stop_active_playback(
                        &active_stream_clone,
                        &active_ffplay_clone,
                        &session_id_clone,
                        &play_timer_clone,
                        &app_data,
                    );
                } else {
                    // Play: first stop any previous playback
                    stop_active_playback(
                        &active_stream_clone,
                        &active_ffplay_clone,
                        &session_id_clone,
                        &play_timer_clone,
                        &app_data,
                    );

                    let cur = app_data.get_player_current_time();
                    let dur = app_data.get_player_duration();
                    let start_at = if cur >= dur {
                        let in_pos = app_data.get_in_point_seconds().max(0.0);
                        app_data.set_player_current_time(in_pos);
                        app_data.set_player_time_text(SharedString::from(format_duration_hms(in_pos as f64)));
                        in_pos
                    } else {
                        cur
                    };

                    app_data.set_player_is_playing(true);

                    if app_data.get_selected_is_video() {
                        let path = app_data.get_selected_file_path().to_string();
                        let start_sec = start_at as f64;
                        let out_sec = app_data.get_out_point_seconds() as f64;
                        let dur_sec = app_data.get_player_duration() as f64;
                        let ffmpeg_bin = binaries.ffmpeg.clone();

                        let vol_float = if app_data.get_player_is_muted() {
                            0.0
                        } else {
                            app_data.get_player_volume()
                        };
                        let vol_int = (vol_float * 100.0).round().clamp(0.0, 100.0) as i32;

                        // Launch audio playback in parallel via ffplay in headless mode (-nodisp)
                        if vol_int > 0 {
                            if let Some(ref ffplay_bin) = binaries.ffplay {
                                let dur_opt = if out_sec > start_sec {
                                    Some(out_sec - start_sec)
                                } else {
                                    None
                                };
                                let edit_gain_db = app_data.get_audio_gain_db();
                                let edit_gain_opt = if edit_gain_db.abs() > 0.05 { Some(edit_gain_db) } else { None };
                                let edit_norm_opt = if app_data.get_audio_normalize_enabled() { Some(app_data.get_audio_normalize_lufs()) } else { None };
                                if let Ok(ff_child) = spawn_audio_playback(ffplay_bin, &path, start_sec, dur_opt, vol_int, edit_gain_opt, edit_norm_opt) {
                                    if let Ok(mut lock) = active_ffplay_clone.lock() {
                                        *lock = Some(ff_child);
                                    }
                                }
                            }
                        }

                        let rot = rotation_from_index(app_data.get_video_rotation_index());
                        let flip_h = app_data.get_video_flip_horizontal();
                        let flip_v = app_data.get_video_flip_vertical();

                        match spawn_playback_stream_transformed(&ffmpeg_bin, &path, start_sec, rot, flip_h, flip_v) {
                            Ok(mut child) => {
                                let my_session = session_id_clone.fetch_add(1, Ordering::SeqCst) + 1;
                                let session_check = session_id_clone.clone();
                                let stream_child_ref = active_stream_clone.clone();
                                let ffplay_child_ref = active_ffplay_clone.clone();
                                let ui_w = ui_handle.clone();
                                let mut stdout = child.stdout.take();

                                if let Ok(mut lock) = active_stream_clone.lock() {
                                    *lock = Some(child);
                                }

                                thread::spawn(move || {
                                    use std::io::Read;
                                    let mut stdout = match stdout.take() {
                                        Some(s) => s,
                                        None => return,
                                    };

                                    let expected = 640 * 360 * 4;
                                    let mut buf = vec![0u8; expected];
                                    let mut frame_index: u64 = 0;
                                    let frame_dur = 1.0 / 24.0;
                                    let mut playback_start: Option<std::time::Instant> = None;

                                    loop {
                                        if session_check.load(Ordering::SeqCst) != my_session {
                                            break;
                                        }

                                        if stdout.read_exact(&mut buf).is_err() {
                                            let ui_w2 = ui_w.clone();
                                            let _ = slint::invoke_from_event_loop(move || {
                                                if let Some(ui) = ui_w2.upgrade() {
                                                    ui.global::<AppData>().set_player_is_playing(false);
                                                }
                                            });
                                            break;
                                        }

                                        let start_instant = match playback_start {
                                            Some(inst) => inst,
                                            None => {
                                                let inst = std::time::Instant::now();
                                                playback_start = Some(inst);
                                                inst
                                            }
                                        };

                                        let frame_target_sec = (frame_index as f64) * frame_dur;
                                        let elapsed_sec = start_instant.elapsed().as_secs_f64();
                                        let diff = frame_target_sec - elapsed_sec;

                                        let master_current_time = start_sec + elapsed_sec;
                                        if (out_sec >= 0.0 && master_current_time >= out_sec) || master_current_time >= dur_sec {
                                            let final_time = if out_sec >= 0.0 && master_current_time >= out_sec {
                                                out_sec
                                            } else {
                                                dur_sec
                                            };
                                            let ui_w2 = ui_w.clone();
                                            let _ = slint::invoke_from_event_loop(move || {
                                                if let Some(ui) = ui_w2.upgrade() {
                                                    let app_data = ui.global::<AppData>();
                                                    app_data.set_player_is_playing(false);
                                                    app_data.set_player_current_time(final_time as f32);
                                                    app_data.set_player_time_text(SharedString::from(format_duration_hms(final_time)));
                                                }
                                            });
                                            break;
                                        }

                                        frame_index += 1;

                                        // If this frame is significantly behind wall clock (> 40ms / 1 frame late),
                                        // drop it so video catches up with audio immediately!
                                        if diff < -0.040 {
                                            continue;
                                        }

                                        // If this frame is early, sleep until its exact presentation time
                                        if diff > 0.002 {
                                            thread::sleep(std::time::Duration::from_secs_f64(diff));
                                        }

                                        if session_check.load(Ordering::SeqCst) != my_session {
                                            break;
                                        }

                                        let current_render_time = start_sec + start_instant.elapsed().as_secs_f64();
                                        let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(640, 360);
                                        pixel_buffer.make_mut_bytes().copy_from_slice(&buf);

                                        let ui_w2 = ui_w.clone();
                                        let _ = slint::invoke_from_event_loop(move || {
                                            if let Some(ui) = ui_w2.upgrade() {
                                                let app_data = ui.global::<AppData>();
                                                if app_data.get_player_is_playing() {
                                                    let img = slint::Image::from_rgba8(pixel_buffer);
                                                    app_data.set_preview_frame(img);
                                                    app_data.set_player_current_time(current_render_time as f32);
                                                    app_data.set_player_time_text(SharedString::from(format_duration_hms(current_render_time)));
                                                }
                                            }
                                        });
                                    }

                                    if let Ok(mut lock) = stream_child_ref.lock() {
                                        if let Some(mut c) = lock.take() {
                                            let _ = c.kill();
                                        }
                                    }
                                    if let Ok(mut lock) = ffplay_child_ref.lock() {
                                        if let Some(mut c) = lock.take() {
                                            let _ = c.kill();
                                        }
                                    }
                                });
                            }
                            Err(e) => {
                                eprintln!("Failed to spawn playback stream: {}", e);
                                app_data.set_player_is_playing(false);
                            }
                        }
                    } else {
                        // Audio-only playback with ffplay
                        let path = app_data.get_selected_file_path().to_string();
                        let start_sec = start_at as f64;
                        let out_sec = app_data.get_out_point_seconds() as f64;

                        let vol_float = if app_data.get_player_is_muted() {
                            0.0
                        } else {
                            app_data.get_player_volume()
                        };
                        let vol_int = (vol_float * 100.0).round().clamp(0.0, 100.0) as i32;

                        if vol_int > 0 {
                            if let Some(ref ffplay_bin) = binaries.ffplay {
                                let dur_opt = if out_sec > start_sec {
                                    Some(out_sec - start_sec)
                                } else {
                                    None
                                };
                                let edit_gain_db = app_data.get_audio_gain_db();
                                let edit_gain_opt = if edit_gain_db.abs() > 0.05 { Some(edit_gain_db) } else { None };
                                let edit_norm_opt = if app_data.get_audio_normalize_enabled() { Some(app_data.get_audio_normalize_lufs()) } else { None };
                                if let Ok(ff_child) = spawn_audio_playback(ffplay_bin, &path, start_sec, dur_opt, vol_int, edit_gain_opt, edit_norm_opt) {
                                    if let Ok(mut lock) = active_ffplay_clone.lock() {
                                        *lock = Some(ff_child);
                                    }
                                }
                            }
                        }

                        let ui_w = ui_handle.clone();
                        let timer_ref = play_timer_clone.clone();
                        let ffplay_child_ref = active_ffplay_clone.clone();
                        let my_session = session_id_clone.fetch_add(1, Ordering::SeqCst) + 1;
                        let session_check = session_id_clone.clone();
                        let audio_start_instant = std::time::Instant::now();

                        play_timer_clone.borrow().start(
                            slint::TimerMode::Repeated,
                            std::time::Duration::from_millis(40),
                            move || {
                                if session_check.load(Ordering::SeqCst) != my_session {
                                    timer_ref.borrow().stop();
                                    return;
                                }

                                if let Some(ui) = ui_w.upgrade() {
                                    let app_data = ui.global::<AppData>();
                                    if !app_data.get_player_is_playing() {
                                        timer_ref.borrow().stop();
                                        return;
                                    }

                                    let elapsed = audio_start_instant.elapsed().as_secs_f64();
                                    let cur = (start_sec + elapsed) as f32;
                                    let dur = app_data.get_player_duration();
                                    let out_sec = app_data.get_out_point_seconds();

                                    if (out_sec >= 0.0 && cur >= out_sec) || cur >= dur {
                                        let final_time = if out_sec >= 0.0 && cur >= out_sec {
                                            out_sec
                                        } else {
                                            dur
                                        };
                                        app_data.set_player_is_playing(false);
                                        app_data.set_player_current_time(final_time);
                                        app_data.set_player_time_text(SharedString::from(format_duration_hms(final_time as f64)));
                                        timer_ref.borrow().stop();
                                        if let Ok(mut lock) = ffplay_child_ref.lock() {
                                            if let Some(mut c) = lock.take() {
                                                let _ = c.kill();
                                            }
                                        }
                                    } else {
                                        app_data.set_player_current_time(cur);
                                        app_data.set_player_time_text(SharedString::from(format_duration_hms(cur as f64)));
                                    }
                                }
                            },
                        );
                    }
                }
            }
        });
    }

    // Handle Player Stop
    {
        let ui_handle = ui.as_weak();
        let play_timer_clone = playback_timer.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();

        ui.global::<AppLogic>().on_player_stop(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                stop_active_playback(
                    &active_stream_clone,
                    &active_ffplay_clone,
                    &session_id_clone,
                    &play_timer_clone,
                    &app_data,
                );

                let reset_pos = app_data.get_in_point_seconds().max(0.0);
                app_data.set_player_current_time(reset_pos);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(reset_pos as f64)));

                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &preview_tx, &latest_id);
                }
            }
        });
    }

    // Handle Play Trimmed Range
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();

        ui.global::<AppLogic>().on_play_trimmed_range(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if !app_data.get_player_has_media() {
                    return;
                }

                let start = app_data.get_in_point_seconds().max(0.0);
                app_data.set_player_current_time(start);
                app_data.set_player_time_text(SharedString::from(format_duration_hms(start as f64)));

                if app_data.get_selected_is_video() {
                    request_current_preview_frame(&app_data, &preview_tx, &latest_id);
                }

                // If currently playing, stop first then restart at trimmed start
                if app_data.get_player_is_playing() {
                    ui.global::<AppLogic>().invoke_player_play_pause();
                }
                ui.global::<AppLogic>().invoke_player_play_pause();
            }
        });
    }

    // Handle Launch FFplay (Native full-fps audio/video preview)
    {
        let ui_handle = ui.as_weak();
        let binaries = binaries.clone();
        let active_child_clone = active_player_child.clone();

        ui.global::<AppLogic>().on_launch_ffplay(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if !app_data.get_player_has_media() {
                    return;
                }

                let media_path = app_data.get_selected_file_path().to_string();
                let cur = app_data.get_player_current_time() as f64;
                let is_video = app_data.get_selected_is_video();
                let out_sec = app_data.get_out_point_seconds();
                let duration = if out_sec >= 0.0 && (out_sec as f64) > cur {
                    Some(out_sec as f64 - cur)
                } else {
                    None
                };

                let ffplay_bin = binaries.ffplay.clone().unwrap_or_else(|| {
                    binaries
                        .ffmpeg
                        .with_file_name(if cfg!(windows) { "ffplay.exe" } else { "ffplay" })
                });

                let vol_float = if app_data.get_player_is_muted() {
                    0.0
                } else {
                    app_data.get_player_volume()
                };
                let vol_int = (vol_float * 100.0).round().clamp(0.0, 100.0) as i32;

                if let Ok(mut lock) = active_child_clone.lock() {
                    if let Some(mut old) = lock.take() {
                        let _ = old.kill();
                    }
                    if let Ok(child) = spawn_ffplay(&ffplay_bin, &media_path, cur, duration, !is_video, Some(vol_int)) {
                        *lock = Some(child);
                    }
                }
            }
        });
    }

    // Handle Player Volume Changed
    {
        let ui_handle = ui.as_weak();
        let binaries = binaries.clone();
        let active_ffplay_clone = active_player_child.clone();
        let last_restart = Arc::new(Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(1)));

        ui.global::<AppLogic>().on_player_volume_changed(move |val| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let clamped = val.clamp(0.0, 1.0);
                app_data.set_player_volume(clamped);

                if clamped <= 0.001 {
                    app_data.set_player_is_muted(true);
                } else if app_data.get_player_is_muted() {
                    app_data.set_player_is_muted(false);
                }

                if app_data.get_player_is_playing() {
                    let mut lr = last_restart.lock().unwrap();
                    let now = std::time::Instant::now();
                    if now.duration_since(*lr) >= std::time::Duration::from_millis(120) {
                        *lr = now;
                        let vol_int = if app_data.get_player_is_muted() {
                            0
                        } else {
                            (clamped * 100.0).round().clamp(0.0, 100.0) as i32
                        };

                        if let Ok(mut lock) = active_ffplay_clone.lock() {
                            if let Some(mut old) = lock.take() {
                                let _ = old.kill();
                            }
                            if vol_int > 0 {
                                if let Some(ref ffplay_bin) = binaries.ffplay {
                                    let cur_time = app_data.get_player_current_time() as f64;
                                    let out_sec = app_data.get_out_point_seconds() as f64;
                                    let dur_opt = if out_sec > cur_time {
                                        Some(out_sec - cur_time)
                                    } else {
                                        None
                                    };
                                    let path = app_data.get_selected_file_path().to_string();
                                    let edit_gain_db = app_data.get_audio_gain_db();
                                    let edit_gain_opt = if edit_gain_db.abs() > 0.05 { Some(edit_gain_db) } else { None };
                                    let edit_norm_opt = if app_data.get_audio_normalize_enabled() { Some(app_data.get_audio_normalize_lufs()) } else { None };
                                    if let Ok(child) = spawn_audio_playback(ffplay_bin, &path, cur_time, dur_opt, vol_int, edit_gain_opt, edit_norm_opt) {
                                        *lock = Some(child);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    // Handle Player Toggle Mute
    {
        let ui_handle = ui.as_weak();
        let binaries = binaries.clone();
        let active_ffplay_clone = active_player_child.clone();

        ui.global::<AppLogic>().on_player_toggle_mute(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let is_muted = app_data.get_player_is_muted();
                if is_muted {
                    // Unmute
                    let mut restore_vol = app_data.get_player_pre_mute_volume();
                    if restore_vol < 0.05 {
                        restore_vol = 0.8;
                    }
                    app_data.set_player_volume(restore_vol);
                    app_data.set_player_is_muted(false);

                    if app_data.get_player_is_playing() {
                        let cur_time = app_data.get_player_current_time() as f64;
                        let out_sec = app_data.get_out_point_seconds() as f64;
                        let dur_opt = if out_sec > cur_time {
                            Some(out_sec - cur_time)
                        } else {
                            None
                        };
                        let path = app_data.get_selected_file_path().to_string();
                        let vol_int = (restore_vol * 100.0).round().clamp(0.0, 100.0) as i32;

                        if let Ok(mut lock) = active_ffplay_clone.lock() {
                            if let Some(mut old) = lock.take() {
                                let _ = old.kill();
                            }
                            if vol_int > 0 {
                                if let Some(ref ffplay_bin) = binaries.ffplay {
                                    let edit_gain_db = app_data.get_audio_gain_db();
                                    let edit_gain_opt = if edit_gain_db.abs() > 0.05 { Some(edit_gain_db) } else { None };
                                    let edit_norm_opt = if app_data.get_audio_normalize_enabled() { Some(app_data.get_audio_normalize_lufs()) } else { None };
                                    if let Ok(child) = spawn_audio_playback(ffplay_bin, &path, cur_time, dur_opt, vol_int, edit_gain_opt, edit_norm_opt) {
                                        *lock = Some(child);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    // Mute
                    let cur_vol = app_data.get_player_volume();
                    if cur_vol > 0.01 {
                        app_data.set_player_pre_mute_volume(cur_vol);
                    }
                    app_data.set_player_is_muted(true);

                    // Silence audio immediately
                    if let Ok(mut lock) = active_ffplay_clone.lock() {
                        if let Some(mut old) = lock.take() {
                            let _ = old.kill();
                        }
                    }
                }
            }
        });
    }

    // =========================================================================
    // VIDEO EDITING STUDIO CALLBACKS (ROTATE, FLIP, CROP)
    // =========================================================================

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
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let play_timer_clone = playback_timer.clone();

        ui.global::<AppLogic>().on_set_video_rotation(move |rot_idx| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    stop_active_playback(
                        &active_stream_clone,
                        &active_ffplay_clone,
                        &session_id_clone,
                        &play_timer_clone,
                        &app_data,
                    );
                }

                app_data.set_video_rotation_index(rot_idx.clamp(0, 3));
                sync_effective_dimensions_and_viewport(&app_data, false);
                update_video_edit_indicators(&app_data);
                request_current_preview_frame(&app_data, &preview_tx, &latest_id);
            }
        });
    }

    // Handle Toggle Flip Horizontal
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let play_timer_clone = playback_timer.clone();

        ui.global::<AppLogic>().on_toggle_flip_horizontal(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    stop_active_playback(
                        &active_stream_clone,
                        &active_ffplay_clone,
                        &session_id_clone,
                        &play_timer_clone,
                        &app_data,
                    );
                }

                app_data.set_video_flip_horizontal(!app_data.get_video_flip_horizontal());
                update_video_edit_indicators(&app_data);
                request_current_preview_frame(&app_data, &preview_tx, &latest_id);
            }
        });
    }

    // Handle Toggle Flip Vertical
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let play_timer_clone = playback_timer.clone();

        ui.global::<AppLogic>().on_toggle_flip_vertical(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    stop_active_playback(
                        &active_stream_clone,
                        &active_ffplay_clone,
                        &session_id_clone,
                        &play_timer_clone,
                        &app_data,
                    );
                }

                app_data.set_video_flip_vertical(!app_data.get_video_flip_vertical());
                update_video_edit_indicators(&app_data);
                request_current_preview_frame(&app_data, &preview_tx, &latest_id);
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

    // Handle Visual Crop Box Drag Start & Move
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_crop_box_drag_start(move || {
            if let Some(ui) = ui_handle.upgrade() {
                ui.global::<AppData>().set_crop_enabled(true);
            }
        });
    }
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_crop_box_drag_move(move |norm_dx, norm_dy| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let eff_w = app_data.get_effective_video_width().max(2);
                let eff_h = app_data.get_effective_video_height().max(2);
                let dx = (norm_dx * eff_w as f32).round() as i32;
                let dy = (norm_dy * eff_h as f32).round() as i32;
                if dx == 0 && dy == 0 {
                    return;
                }
                let rect = CropRect {
                    x: app_data.get_crop_x().max(0) as u32,
                    y: app_data.get_crop_y().max(0) as u32,
                    width: app_data.get_crop_width().max(16) as u32,
                    height: app_data.get_crop_height().max(16) as u32,
                }
                .nudge(dx, dy, eff_w as u32, eff_h as u32);
                sync_crop_rect_to_ui(&app_data, rect, true);
            }
        });
    }

    // Handle Visual Crop Resize Handles (0..7)
    {
        let ui_handle = ui.as_weak();
        ui.global::<AppLogic>().on_crop_handle_drag(move |handle_id, norm_x, norm_y| {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_crop_enabled(true);
                let eff_w = app_data.get_effective_video_width().max(32);
                let eff_h = app_data.get_effective_video_height().max(32);
                let cur_x = app_data.get_crop_x().clamp(0, eff_w - 16);
                let cur_y = app_data.get_crop_y().clamp(0, eff_h - 16);
                let cur_w = app_data.get_crop_width().clamp(16, eff_w - cur_x);
                let cur_h = app_data.get_crop_height().clamp(16, eff_h - cur_y);
                let right = cur_x + cur_w;
                let bottom = cur_y + cur_h;

                let px = (norm_x.clamp(0.0, 1.0) * eff_w as f32).round() as i32;
                let py = (norm_y.clamp(0.0, 1.0) * eff_h as f32).round() as i32;
                let aspect = CropAspectRatio::from_index(app_data.get_crop_aspect_index());

                let (mut new_x, mut new_y, mut new_w, mut new_h) = match handle_id {
                    0 => {
                        // Top-Left corner (anchor: right, bottom)
                        let w = (right - px).clamp(16, right);
                        let h = (bottom - py).clamp(16, bottom);
                        (right - w, bottom - h, w, h)
                    }
                    1 => {
                        // Top-Right corner (anchor: cur_x, bottom)
                        let w = (px - cur_x).clamp(16, eff_w - cur_x);
                        let h = (bottom - py).clamp(16, bottom);
                        (cur_x, bottom - h, w, h)
                    }
                    2 => {
                        // Bottom-Left corner (anchor: right, cur_y)
                        let w = (right - px).clamp(16, right);
                        let h = (py - cur_y).clamp(16, eff_h - cur_y);
                        (right - w, cur_y, w, h)
                    }
                    3 => {
                        // Bottom-Right corner (anchor: cur_x, cur_y)
                        let w = (px - cur_x).clamp(16, eff_w - cur_x);
                        let h = (py - cur_y).clamp(16, eff_h - cur_y);
                        (cur_x, cur_y, w, h)
                    }
                    4 => {
                        // Top edge (anchor: bottom)
                        let h = (bottom - py).clamp(16, bottom);
                        (cur_x, bottom - h, cur_w, h)
                    }
                    5 => {
                        // Bottom edge (anchor: cur_y)
                        let h = (py - cur_y).clamp(16, eff_h - cur_y);
                        (cur_x, cur_y, cur_w, h)
                    }
                    6 => {
                        // Left edge (anchor: right)
                        let w = (right - px).clamp(16, right);
                        (right - w, cur_y, w, cur_h)
                    }
                    _ => {
                        // Right edge (anchor: cur_x)
                        let w = (px - cur_x).clamp(16, eff_w - cur_x);
                        (cur_x, cur_y, w, cur_h)
                    }
                };

                // Enforce aspect ratio lock if active
                if let Some((num, den)) = aspect.ratio() {
                    let num_i = num as i64;
                    let den_i = den as i64;
                    match handle_id {
                        4 | 5 => {
                            // Driven primarily by height
                            let max_h = if handle_id == 4 { bottom } else { eff_h - cur_y };
                            let mut h = new_h.clamp(16, max_h);
                            let mut w = ((h as i64 * num_i) / den_i) as i32;
                            if w > eff_w {
                                w = eff_w;
                                h = ((w as i64 * den_i) / num_i) as i32;
                            }
                            new_w = w.max(16);
                            new_h = h.max(16);
                            new_x = (cur_x + (cur_w - new_w) / 2).clamp(0, (eff_w - new_w).max(0));
                            new_y = if handle_id == 4 { bottom - new_h } else { cur_y };
                        }
                        _ => {
                            // Driven primarily by width
                            let max_w = if handle_id == 0 || handle_id == 2 || handle_id == 6 {
                                right
                            } else {
                                eff_w - cur_x
                            };
                            let max_h = if handle_id == 0 || handle_id == 1 {
                                bottom
                            } else if handle_id == 2 || handle_id == 3 {
                                eff_h - cur_y
                            } else {
                                eff_h
                            };
                            let mut w = new_w.clamp(16, max_w);
                            let mut h = ((w as i64 * den_i) / num_i) as i32;
                            if h > max_h {
                                h = max_h;
                                w = ((h as i64 * num_i) / den_i) as i32;
                            }
                            new_w = w.max(16);
                            new_h = h.max(16);
                            new_x = if handle_id == 0 || handle_id == 2 || handle_id == 6 {
                                (right - new_w).max(0)
                            } else {
                                cur_x
                            };
                            new_y = if handle_id == 0 || handle_id == 1 {
                                (bottom - new_h).max(0)
                            } else if handle_id == 2 || handle_id == 3 {
                                cur_y
                            } else {
                                (cur_y + (cur_h - new_h) / 2).clamp(0, (eff_h - new_h).max(0))
                            };
                        }
                    }
                }

                let rect = CropRect {
                    x: new_x.max(0) as u32,
                    y: new_y.max(0) as u32,
                    width: new_w.max(16) as u32,
                    height: new_h.max(16) as u32,
                }
                .sanitize(eff_w as u32, eff_h as u32);
                sync_crop_rect_to_ui(&app_data, rect, true);
            }
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
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let play_timer_clone = playback_timer.clone();

        ui.global::<AppLogic>().on_reset_all_video_edits(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    stop_active_playback(
                        &active_stream_clone,
                        &active_ffplay_clone,
                        &session_id_clone,
                        &play_timer_clone,
                        &app_data,
                    );
                }

                app_data.set_video_rotation_index(0);
                app_data.set_video_flip_horizontal(false);
                app_data.set_video_flip_vertical(false);
                app_data.set_crop_enabled(false);
                app_data.set_crop_aspect_index(0);
                sync_effective_dimensions_and_viewport(&app_data, true);
                update_video_edit_indicators(&app_data);
                request_current_preview_frame(&app_data, &preview_tx, &latest_id);
            }
        });
    }

    // Helper to restart live audio playback preview with updated audio filters
    let restart_audio_preview = {
        let binaries = binaries.clone();
        let active_ffplay = active_player_child.clone();
        let last_restart = Arc::new(Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(1)));
        move |app_data: &AppData| {
            if !app_data.get_player_is_playing() {
                return;
            }
            if let Ok(mut t) = last_restart.lock() {
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
            if vol_int <= 0 {
                return;
            }

            if let Some(ref ffplay_bin) = binaries.ffplay {
                let cur_time = app_data.get_player_current_time() as f64;
                let out_sec = app_data.get_out_point_seconds() as f64;
                let dur_opt = if out_sec > cur_time {
                    Some(out_sec - cur_time)
                } else {
                    None
                };
                let path = app_data.get_selected_file_path().to_string();
                let edit_gain_db = app_data.get_audio_gain_db();
                let edit_gain_opt = if edit_gain_db.abs() > 0.05 { Some(edit_gain_db) } else { None };
                let edit_norm_opt = if app_data.get_audio_normalize_enabled() {
                    Some(app_data.get_audio_normalize_lufs())
                } else {
                    None
                };

                if let Ok(mut lock) = active_ffplay.lock() {
                    if let Some(mut old) = lock.take() {
                        let _ = old.kill();
                    }
                    if let Ok(child) = spawn_audio_playback(ffplay_bin, &path, cur_time, dur_opt, vol_int, edit_gain_opt, edit_norm_opt) {
                        *lock = Some(child);
                    }
                }
            }
        }
    };

    // Handle Reset All Edits (Video + Audio)
    {
        let ui_handle = ui.as_weak();
        let preview_tx = preview_tx.clone();
        let latest_id = latest_preview_id.clone();
        let active_stream_clone = active_playback_child.clone();
        let active_ffplay_clone = active_player_child.clone();
        let session_id_clone = playback_session_id.clone();
        let play_timer_clone = playback_timer.clone();

        ui.global::<AppLogic>().on_reset_all_edits(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if app_data.get_player_is_playing() {
                    stop_active_playback(
                        &active_stream_clone,
                        &active_ffplay_clone,
                        &session_id_clone,
                        &play_timer_clone,
                        &app_data,
                    );
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
                    request_current_preview_frame(&app_data, &preview_tx, &latest_id);
                }
            }
        });
    }

    // Handle Set Audio Gain (dB)
    {
        let ui_handle = ui.as_weak();
        let restart_preview = restart_audio_preview.clone();

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

                restart_preview(&app_data);
            }
        });
    }

    // Handle Step Audio Gain (dB)
    {
        let ui_handle = ui.as_weak();
        let restart_preview = restart_audio_preview.clone();

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

                restart_preview(&app_data);
            }
        });
    }

    // Handle Toggle Audio Normalization
    {
        let ui_handle = ui.as_weak();
        let restart_preview = restart_audio_preview.clone();

        ui.global::<AppLogic>().on_toggle_audio_normalize(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let next = !app_data.get_audio_normalize_enabled();
                app_data.set_audio_normalize_enabled(next);
                let is_active = app_data.get_audio_gain_db().abs() > 0.05 || next;
                app_data.set_has_active_audio_edits(is_active);

                restart_preview(&app_data);
            }
        });
    }

    // Handle Set Audio Normalize Preset
    {
        let ui_handle = ui.as_weak();
        let restart_preview = restart_audio_preview.clone();

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

                restart_preview(&app_data);
            }
        });
    }

    // Handle Reset Audio Edits
    {
        let ui_handle = ui.as_weak();
        let restart_preview = restart_audio_preview.clone();

        ui.global::<AppLogic>().on_reset_audio_edits(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                app_data.set_audio_gain_db(0.0);
                app_data.set_audio_gain_text(SharedString::from("0.0 dB"));
                app_data.set_audio_normalize_enabled(false);
                app_data.set_audio_normalize_lufs(-16.0);
                app_data.set_audio_normalize_preset_index(0);
                app_data.set_has_active_audio_edits(false);

                restart_preview(&app_data);
            }
        });
    }

    // Handle Select Output Directory
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_select_output_dir(move || {
            if let Some(folder) = rfd::FileDialog::new()
                .set_title("Select Output Directory")
                .pick_folder()
            {
                if let Some(ui) = ui_handle.upgrade() {
                    ui.global::<AppData>().set_output_directory(SharedString::from(
                        folder.to_string_lossy().to_string(),
                    ));
                }
            }
        });
    }

    // Handle Open Output Directory
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_open_output_dir(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                let dir_str = app_data.get_output_directory().trim().to_string();
                let target = if dir_str == "Same as source" || dir_str.is_empty() {
                    let sel_path = app_data.get_selected_file_path().to_string();
                    if !sel_path.is_empty() {
                        Path::new(&sel_path)
                            .parent()
                            .map(|p| p.to_path_buf())
                            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
                    } else {
                        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                    }
                } else {
                    PathBuf::from(dir_str)
                };

                open_folder_in_explorer(&target);
            }
        });
    }

    // Handle Toggle Logs
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_toggle_logs(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let cur = ui.global::<AppData>().get_show_logs();
                ui.global::<AppData>().set_show_logs(!cur);
            }
        });
    }

    // Handle Switch Tab
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_switch_tab(move |tab_idx| {
            if let Some(ui) = ui_handle.upgrade() {
                ui.global::<AppData>().set_active_tab(tab_idx);
            }
        });
    }

    // Handle Quick Lossless Cut
    {
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_quick_lossless_cut(move || {
            if let Some(ui) = ui_handle.upgrade() {
                let app_data = ui.global::<AppData>();
                if !app_data.get_has_active_trim() || !app_data.get_player_has_media() {
                    return;
                }
                // Function 7 corresponds to LosslessCut
                app_data.set_function_index(7);
                ui.global::<AppLogic>().invoke_run_task();
            }
        });
    }

    // Handle Cancel Processing
    {
        let cancel_flag = cancel_flag.clone();
        let ui_handle = ui.as_weak();

        ui.global::<AppLogic>().on_cancel_processing(move || {
            cancel_flag.store(true, Ordering::SeqCst);
            if let Some(ui) = ui_handle.upgrade() {
                ui.global::<AppData>()
                    .set_progress_text(SharedString::from("Cancelling..."));
            }
        });
    }

    // Handle Run Task (formerly Start Processing)
    {
        let ui_handle = ui.as_weak();
        let binaries = binaries.clone();
        let cancel_flag = cancel_flag.clone();

        ui.global::<AppLogic>().on_run_task(move || {
            if let Some(ui) = ui_handle.upgrade() {
                if ui.global::<AppData>().get_is_processing() {
                    return;
                }
                let items: Vec<QueueFile> =
                    ui.global::<AppData>().get_queue().iter().collect();
                if items.is_empty() {
                    return;
                }

                // Reset cancellation flag
                cancel_flag.store(false, Ordering::SeqCst);

                // Set processing state
                ui.global::<AppData>().set_is_processing(true);
                ui.global::<AppData>().set_progress_percent(0.0);
                ui.global::<AppData>()
                    .set_progress_text(SharedString::from("Starting..."));

                // Read all parameters from UI
                let func_idx = ui.global::<AppData>().get_function_index();
                let hw_accel_idx = ui.global::<AppData>().get_hw_accel_index();
                let rate_ctrl_idx = ui.global::<AppData>().get_rate_control_index();
                let crf_raw = ui.global::<AppData>().get_crf_value().round() as u32;
                let crf = crf_raw.min(51); // Clamp to valid range [0, 51]
                let video_br_idx = ui.global::<AppData>().get_video_bitrate_index();
                let preset_idx = ui.global::<AppData>().get_preset_index();
                let res_idx = ui.global::<AppData>().get_resolution_index();
                let fps_idx = ui.global::<AppData>().get_fps_index();
                let deinterlace_idx = ui.global::<AppData>().get_deinterlace_index();
                let prores_idx = ui.global::<AppData>().get_prores_profile_index();
                let faststart = ui.global::<AppData>().get_faststart();

                let audio_codec_idx = ui.global::<AppData>().get_audio_codec_index();
                let audio_br_idx = ui.global::<AppData>().get_audio_bitrate_index();
                let audio_sr_idx = ui.global::<AppData>().get_audio_sample_rate_index();
                let normalize_idx = ui.global::<AppData>().get_normalize_index();

                let in_point_str = ui.global::<AppData>().get_in_point().to_string();
                let out_point_str = ui.global::<AppData>().get_out_point().to_string();
                let container_idx = ui.global::<AppData>().get_container_index();
                let out_dir_str = ui.global::<AppData>().get_output_directory().to_string();

                let rot_idx = ui.global::<AppData>().get_video_rotation_index();
                let flip_h = ui.global::<AppData>().get_video_flip_horizontal();
                let flip_v = ui.global::<AppData>().get_video_flip_vertical();
                let crop_enabled = ui.global::<AppData>().get_crop_enabled();
                let crop_x = ui.global::<AppData>().get_crop_x();
                let crop_y = ui.global::<AppData>().get_crop_y();
                let crop_w = ui.global::<AppData>().get_crop_width();
                let crop_h = ui.global::<AppData>().get_crop_height();

                let edit_gain_val = ui.global::<AppData>().get_audio_gain_db();
                let edit_gain_opt = if edit_gain_val.abs() > 0.05 { Some(edit_gain_val) } else { None };
                let edit_norm_enabled = ui.global::<AppData>().get_audio_normalize_enabled();
                let edit_norm_lufs = ui.global::<AppData>().get_audio_normalize_lufs();

                let ui_weak = ui_handle.clone();
                let bins = binaries.clone();
                let cancel = cancel_flag.clone();

                // Spawn worker thread for batch queue processing
                thread::spawn(move || {
                    let total_items = items.len();

                    for (item_idx, item) in items.iter().enumerate() {
                        if cancel.load(Ordering::SeqCst) {
                            break;
                        }

                        let input_path = item.file_path.to_string();
                        let in_p = Path::new(&input_path);

                        // Determine destination directory and filename
                        let out_container = if func_idx == 12 {
                            ".srt"
                        } else {
                            match container_idx {
                                1 => ".mkv",
                                2 => ".mov",
                                3 => ".webm",
                                4 => ".avi",
                                5 => ".ts",
                                6 => ".mp3",
                                7 => ".wav",
                                8 => ".flac",
                                _ => ".mp4",
                            }
                        };

                        let parent_dir =
                            if out_dir_str != "Same as source" && !out_dir_str.is_empty() {
                                PathBuf::from(&out_dir_str)
                            } else {
                                in_p.parent()
                                    .map(|p| p.to_path_buf())
                                    .unwrap_or_else(|| PathBuf::from("."))
                            };

                        let stem = in_p.file_stem().unwrap_or_default().to_string_lossy();
                        let mut output_path =
                            parent_dir.join(format!("{}_encoded{}", stem, out_container));

                        // Avoid overwriting existing output by appending counter if needed
                        let mut counter = 1;
                        while output_path.exists() && output_path != in_p {
                            output_path = parent_dir
                                .join(format!("{}_encoded_{}{}", stem, counter, out_container));
                            counter += 1;
                        }

                        let rotation = rotation_from_index(rot_idx);
                        let crop = if crop_enabled && crop_w >= 2 && crop_h >= 2 {
                            Some(CropRect {
                                x: crop_x.max(0) as u32,
                                y: crop_y.max(0) as u32,
                                width: crop_w.max(2) as u32,
                                height: crop_h.max(2) as u32,
                            })
                        } else {
                            None
                        };
                        let has_audio_edits = edit_gain_opt.is_some() || edit_norm_enabled;
                        let has_video_edits =
                            rotation != VideoRotation::None || flip_h || flip_v || crop.is_some();

                        // Determine function (upgrade Rewrap/LosslessCut to Transcode if video or audio filters are active)
                        let function = match func_idx {
                            0..=5 => EncoderFunction::Transcode,
                            6 | 7 if has_video_edits || has_audio_edits => EncoderFunction::Transcode,
                            6 => EncoderFunction::Rewrap,
                            7 => EncoderFunction::LosslessCut,
                            8 => EncoderFunction::ExtractVideo,
                            9 => EncoderFunction::ExtractAudio,
                            10 => EncoderFunction::AudioNormalize,
                            11 => EncoderFunction::Transcode, // Audio conversion
                            12 => EncoderFunction::ExtractSubtitles,
                            _ => EncoderFunction::Transcode,
                        };

                        let video_codec = match func_idx {
                            0 => VideoCodec::H264,
                            1 => VideoCodec::H265,
                            2 => VideoCodec::AV1,
                            3 => VideoCodec::AppleProRes,
                            4 => VideoCodec::AvidDNxHD,
                            5 => VideoCodec::VP9,
                            11 => VideoCodec::Copy,
                            _ => VideoCodec::H264,
                        };

                        let hw_accel = match hw_accel_idx {
                            1 => HwAccel::Nvenc,
                            2 => HwAccel::Qsv,
                            3 => HwAccel::Amf,
                            4 => HwAccel::Cpu,
                            _ => HwAccel::Auto,
                        };

                        let rate_control = match rate_ctrl_idx {
                            1 => RateControlMode::CBR,
                            2 => RateControlMode::VBR,
                            _ => RateControlMode::CRF,
                        };

                        let video_bitrate_kbps = match video_br_idx {
                            0 => 15000,
                            1 => 10000,
                            2 => 6000,
                            3 => 4000,
                            4 => 2500,
                            _ => 1200,
                        };

                        let preset = match preset_idx {
                            0 => "ultrafast",
                            1 => "faster",
                            2 => "fast",
                            3 => "medium",
                            4 => "slow",
                            5 => "veryslow",
                            _ => "medium",
                        }
                        .to_string();

                        let resolution = match res_idx {
                            1 => ResolutionPreset::UHD4K,
                            2 => ResolutionPreset::QHD1440p,
                            3 => ResolutionPreset::FHD1080p,
                            4 => ResolutionPreset::HD720p,
                            5 => ResolutionPreset::SD480p,
                            _ => ResolutionPreset::Original,
                        };

                        let fps = match fps_idx {
                            1 => Some(23.976),
                            2 => Some(24.0),
                            3 => Some(25.0),
                            4 => Some(29.97),
                            5 => Some(30.0),
                            6 => Some(50.0),
                            7 => Some(59.94),
                            8 => Some(60.0),
                            _ => None,
                        };

                        let deinterlace = match deinterlace_idx {
                            1 => DeinterlaceMode::Yadif,
                            2 => DeinterlaceMode::Bwdif,
                            _ => DeinterlaceMode::None,
                        };

                        let prores_profile = match prores_idx {
                            0 => ProResProfile::Proxy,
                            1 => ProResProfile::LT,
                            2 => ProResProfile::Standard,
                            3 => ProResProfile::HQ,
                            4 => ProResProfile::FourFourFourFour,
                            5 => ProResProfile::FourFourFourFourXQ,
                            _ => ProResProfile::Standard,
                        };

                        let audio_codec = match audio_codec_idx {
                            1 => AudioCodecChoice::MP3,
                            2 => AudioCodecChoice::Opus,
                            3 => AudioCodecChoice::FLAC,
                            4 => AudioCodecChoice::WavPCM24,
                            5 => AudioCodecChoice::WavPCM16,
                            6 => AudioCodecChoice::AC3,
                            7 => AudioCodecChoice::Copy,
                            _ => AudioCodecChoice::AAC,
                        };

                        let audio_bitrate_kbps = match audio_br_idx {
                            0 => 320,
                            1 => 256,
                            2 => 192,
                            3 => 128,
                            4 => 96,
                            _ => 64,
                        };

                        let audio_sample_rate = match audio_sr_idx {
                            1 => Some(48000),
                            2 => Some(44100),
                            3 => Some(96000),
                            _ => None,
                        };

                        let normalize_lufs = match normalize_idx {
                            1 => Some(-23.0),
                            2 => Some(-14.0),
                            3 => Some(-16.0),
                            4 => Some(-12.0),
                            _ => {
                                if func_idx == 10 {
                                    Some(-23.0)
                                } else {
                                    None
                                }
                            }
                        };

                        let in_point = if in_point_str.trim().is_empty() {
                            None
                        } else {
                            Some(in_point_str.clone())
                        };

                        let out_point = if out_point_str.trim().is_empty() {
                            None
                        } else {
                            Some(out_point_str.clone())
                        };

                        // Reuse already-probed duration if available, otherwise probe on demand
                        let total_duration = if item.duration_seconds > 0.0 {
                            item.duration_seconds as f64
                        } else {
                            match probe_file(&input_path, &bins.ffprobe) {
                                Ok(m) => m.duration_seconds,
                                Err(_) => 0.0,
                            }
                        };

                        // Validate timecodes
                        let in_secs = in_point.as_ref().and_then(|s| parse_timecode_to_seconds(s));
                        let out_secs = out_point.as_ref().and_then(|s| parse_timecode_to_seconds(s));

                        if in_secs.is_some() && out_secs.is_some() && in_secs.unwrap() >= out_secs.unwrap() {
                            let ui_w = ui_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    ui.global::<AppData>().set_status_summary(SharedString::from("In point must be before out point"));
                                }
                            });
                            continue;
                        }

                        if let Some(in_s) = in_secs {
                            if total_duration > 0.0 && in_s >= total_duration {
                                let ui_w = ui_weak.clone();
                                let in_s_c = in_s;
                                let dur_c = total_duration;
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_w.upgrade() {
                                        ui.global::<AppData>().set_status_summary(SharedString::from(format!("In point ({:.1}s) is beyond video duration ({:.1}s)", in_s_c, dur_c)));
                                    }
                                });
                                continue;
                            }
                        }

                        if let Some(out_s) = out_secs {
                            if total_duration > 0.0 && out_s > total_duration {
                                let ui_w = ui_weak.clone();
                                let out_s_c = out_s;
                                let dur_c = total_duration;
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_w.upgrade() {
                                        ui.global::<AppData>().set_status_summary(SharedString::from(format!("Out point ({:.1}s) is beyond video duration ({:.1}s)", out_s_c, dur_c)));
                                    }
                                });
                                continue;
                            }
                        }

                        let start_sec = in_secs.unwrap_or(0.0).max(0.0);
                        let end_sec = out_secs.unwrap_or(total_duration);
                        let effective_duration = if end_sec > start_sec {
                            end_sec - start_sec
                        } else {
                            total_duration
                        };

                        let final_norm_opt = if edit_norm_enabled {
                            Some(edit_norm_lufs)
                        } else {
                            normalize_lufs
                        };

                        let config = EncodeJobConfig {
                            function,
                            input_path: input_path.clone(),
                            secondary_input_path: None,
                            output_path: output_path.to_string_lossy().to_string(),

                            video_codec,
                            hw_accel,
                            rate_control,
                            crf,
                            video_bitrate_kbps,
                            max_bitrate_kbps: video_bitrate_kbps * 2,
                            preset,
                            resolution,
                            custom_resolution: None,
                            fps,
                            deinterlace,
                            prores_profile,
                            faststart,

                            audio_codec,
                            audio_bitrate_kbps,
                            audio_sample_rate,
                            audio_channels: None,
                            audio_gain_db: edit_gain_opt,
                            normalize_lufs: final_norm_opt,

                            in_point,
                            out_point,

                            rotation,
                            flip_horizontal: flip_h,
                            flip_vertical: flip_v,
                            crop,
                        };

                        // Build FFmpeg command
                        let cmd_args = FFmpegCommandBuilder::build(&config, &bins.ffmpeg);
                        let cmd_str = cmd_args.join(" ");

                        // Update current file status to Encoding in-place
                        {
                            let ui_w = ui_weak.clone();
                            let idx = item_idx;
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    let app_data = ui.global::<AppData>();
                                    let model = app_data.get_queue();
                                    if let Some(mut row) = model.row_data(idx) {
                                        row.status = SharedString::from("Encoding...");
                                        model.set_row_data(idx, row);
                                    }
                                }
                            });
                        }

                        // Display command preview in UI
                        {
                            let ui_w = ui_weak.clone();
                            let c_str = cmd_str.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    ui.global::<AppData>().set_log_preview(SharedString::from(
                                        format!("$ {}\n", c_str),
                                    ));
                                }
                            });
                        }

                        let ui_w_progress = ui_weak.clone();
                        let idx = item_idx;
                        let mut last_ui_tick = std::time::Instant::now()
                            .checked_sub(std::time::Duration::from_millis(100))
                            .unwrap_or_else(std::time::Instant::now);

                        let progress_callback =
                            move |p: another_encoder::ffmpeg::ProgressUpdate| {
                                let now = std::time::Instant::now();
                                if p.percentage < 1.0
                                    && now.duration_since(last_ui_tick).as_millis() < 45
                                {
                                    return;
                                }
                                last_ui_tick = now;

                                let ui_w = ui_w_progress.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_w.upgrade() {
                                        let app_data = ui.global::<AppData>();
                                        let model = app_data.get_queue();
                                        if let Some(mut row) = model.row_data(idx) {
                                            row.progress = p.percentage;
                                            row.status = SharedString::from(format!(
                                                "{:.1}%",
                                                p.percentage * 100.0
                                            ));
                                            model.set_row_data(idx, row);
                                        }

                                        let overall =
                                            (idx as f32 + p.percentage) / (total_items as f32);
                                        app_data.set_progress_percent(overall);
                                        app_data.set_progress_text(SharedString::from(format!(
                                            "File {}/{} — {:.1}%",
                                            idx + 1,
                                            total_items,
                                            p.percentage * 100.0
                                        )));
                                        app_data.set_speed_text(SharedString::from(format!(
                                            "{:.2}x", p.speed
                                        )));
                                        app_data.set_fps_text(SharedString::from(format!(
                                            "{:.0} fps", p.fps
                                        )));
                                        app_data.set_time_text(SharedString::from(format!(
                                            "{} / {}",
                                            format_duration_hms(p.current_seconds),
                                            format_duration_hms(p.total_seconds)
                                        )));
                                    }
                                });
                            };

                        let ui_w_log = ui_weak.clone();
                        let log_callback = move |line: String| {
                            let ui_w = ui_w_log.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    let mut cur =
                                        ui.global::<AppData>().get_log_preview().to_string();
                                    if cur.len() > 4000 {
                                        let mut split_idx = cur.len() - 3000;
                                        while !cur.is_char_boundary(split_idx) {
                                            split_idx += 1;
                                        }
                                        cur = cur[split_idx..].to_string();
                                    }
                                    cur.push_str(&line);
                                    cur.push('\n');
                                    ui.global::<AppData>()
                                        .set_log_preview(SharedString::from(cur));
                                }
                            });
                        };

                        // Run execution
                        let result = FFmpegRunner::run_with_progress(
                            &cmd_args,
                            effective_duration,
                            cancel.clone(),
                            progress_callback,
                            log_callback,
                        );

                        let ui_w_done = ui_weak.clone();
                        let status_str = match &result {
                            Ok(_) => "Completed".to_string(),
                            Err(e) => {
                                if cancel.load(Ordering::SeqCst) {
                                    "Cancelled".to_string()
                                } else {
                                    format!("Failed: {}", e)
                                }
                            }
                        };
                        let is_ok = result.is_ok();

                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_w_done.upgrade() {
                                let app_data = ui.global::<AppData>();
                                let model = app_data.get_queue();
                                if let Some(mut row) = model.row_data(idx) {
                                    row.status = SharedString::from(status_str);
                                    if is_ok {
                                        row.progress = 1.0;
                                    }
                                    model.set_row_data(idx, row);
                                }
                            }
                        });
                    }

                    // Batch finished or cancelled
                    let ui_w = ui_weak.clone();
                    let was_cancelled = cancel.load(Ordering::SeqCst);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui_w.upgrade() {
                            ui.global::<AppData>().set_is_processing(false);
                            if was_cancelled {
                                ui.global::<AppData>()
                                    .set_progress_text(SharedString::from("Cancelled"));
                            } else {
                                ui.global::<AppData>().set_progress_percent(1.0);
                                ui.global::<AppData>().set_progress_text(SharedString::from(
                                    "All tasks finished successfully!",
                                ));
                            }
                        }
                    });
                });
            }
        });

        // Backward compatibility callback forwarding
        let ui_handle_fwd = ui.as_weak();
        ui.global::<AppLogic>().on_start_processing(move || {
            if let Some(ui) = ui_handle_fwd.upgrade() {
                ui.global::<AppLogic>().invoke_run_task();
            }
        });
    }

    // =========================================================================
    // YT-DLP WEB DOWNLOADER CALLBACKS
    // =========================================================================

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
                    ui.global::<AppData>()
                        .set_ytdlp_output_dir(SharedString::from(folder.to_string_lossy().as_ref()));
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
        let ytdlp_bin_arc = ytdlp_binary.clone();
        let options_store = ytdlp_discovered_options.clone();

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

            let bin_path = match ytdlp_bin_arc.as_ref() {
                Some(p) => p.clone(),
                None => {
                    data.set_ytdlp_has_error(true);
                    data.set_ytdlp_error_message(SharedString::from(
                        "yt-dlp binary not found in project root path",
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
        let ytdlp_bin_arc = ytdlp_binary.clone();
        let ffmpeg_dir_arc = binaries.ffmpeg.parent().map(|p| p.to_path_buf());
        let options_store = ytdlp_discovered_options.clone();
        let child_store = active_ytdlp_child.clone();
        let cancel_store = active_ytdlp_cancel.clone();

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

            let bin_path = match ytdlp_bin_arc.as_ref() {
                Some(p) => p.clone(),
                None => {
                    data.set_ytdlp_has_error(true);
                    data.set_ytdlp_error_message(SharedString::from(
                        "yt-dlp binary not found in project root path",
                    ));
                    return;
                }
            };

            let selected_idx = data.get_ytdlp_selected_format_index() as usize;
            let format_opt = {
                let store = options_store.lock().unwrap();
                if selected_idx < store.len() {
                    store[selected_idx].clone()
                } else if !store.is_empty() {
                    store[0].clone()
                } else {
                    ytdlp::DownloadFormatOption {
                        display_label: "Best Quality".to_string(),
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
        let child_store = active_ytdlp_child.clone();
        let cancel_store = active_ytdlp_cancel.clone();

        ui.global::<AppLogic>().on_ytdlp_cancel_download(move || {
            cancel_store.store(true, Ordering::SeqCst);
            if let Ok(mut lock) = child_store.lock() {
                if let Some(mut child) = lock.take() {
                    let _ = child.kill();
                }
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
        let bins = binaries.clone();
        let p_tx = preview_tx.clone();
        let l_id = latest_preview_id.clone();
        let wf_tx = waveform_tx.clone();
        let l_wf = latest_waveform_id.clone();

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

            app_data.set_active_tab(0);
            let ui_w = ui_handle.clone();
            let bins_c = bins.clone();
            let p_tx_c = p_tx.clone();
            let l_id_c = l_id.clone();
            let wf_tx_c = wf_tx.clone();
            let l_wf_c = l_wf.clone();

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
                        load_selected_file(&app_data, &item, &p_tx_c, &l_id_c, &wf_tx_c, &l_wf_c);
                    }
                });
            });
        });
    }

    // Handle yt-dlp Send to Queue
    {
        let ui_handle = ui.as_weak();
        let bins = binaries.clone();
        let p_tx = preview_tx.clone();
        let l_id = latest_preview_id.clone();
        let wf_tx = waveform_tx.clone();
        let l_wf = latest_waveform_id.clone();

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
            process_incoming_paths(
                vec![path],
                ui_handle.clone(),
                bins.clone(),
                p_tx.clone(),
                l_id.clone(),
                wf_tx.clone(),
                l_wf.clone(),
            );
        });
    }

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
        let bins = binaries.clone();
        let p_tx = preview_tx.clone();
        let l_id = latest_preview_id.clone();
        let wf_tx_th = waveform_tx.clone();
        let l_wf_th = latest_waveform_id.clone();

        let initial_hwnd = native_hwnd.unwrap_or(std::ptr::null_mut());
        win_drag_drop::setup_drop_target(initial_hwnd, move |dropped_paths| {
            println!("Dropped {} file(s) into queue", dropped_paths.len());
            process_incoming_paths(
                dropped_paths,
                ui_weak.clone(),
                bins.clone(),
                p_tx.clone(),
                l_id.clone(),
                wf_tx_th.clone(),
                l_wf_th.clone(),
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
