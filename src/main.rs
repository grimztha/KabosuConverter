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
    extract_frame_rgba, extract_waveform_rgba, format_duration_hms, parse_timecode_to_seconds,
    probe_file, spawn_audio_playback, spawn_ffplay, spawn_playback_stream, AudioCodecChoice,
    DeinterlaceMode, EncodeJobConfig, EncoderFunction, FFmpegBinaries, FFmpegCommandBuilder,
    FFmpegRunner, HwAccel, ProResProfile, RateControlMode, ResolutionPreset, VideoCodec,
};
use another_encoder::settings;
use another_encoder::ytdlp;

fn get_clipboard_string() -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-Command", "Get-Clipboard"]);
        cmd.creation_flags(0x08000000);
        if let Ok(output) = cmd.output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !text.is_empty() {
                    return Some(text);
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
}

struct WaveformTask {
    media_path: String,
    id: u64,
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

    // Request audio waveform visualization for the timeline
    app_data.set_has_waveform(false);
    let wf_id = latest_wf_id.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = waveform_tx.send(WaveformTask {
        media_path: item.file_path.to_string(),
        id: wf_id,
    });

    // Request initial preview frame if video
    if item.is_video {
        app_data.set_preview_frame_loading(true);
        let id = latest_id.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = preview_tx.send(PreviewTask {
            media_path: item.file_path.to_string(),
            time: 0.0f64,
            id,
        });
    }
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

                let res = extract_frame_rgba(&ffmpeg_bin, &current_task.media_path, current_task.time);
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

                match extract_waveform_rgba(&ffmpeg_bin, &current_task.media_path, 800, 44) {
                    Ok(rgba) => {
                        if current_task.id == latest_id.load(Ordering::SeqCst) {
                            let ui_w = ui_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(800, 44);
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
                let ui_weak = ui_handle.clone();
                let bins = binaries.clone();
                let p_tx = preview_tx.clone();
                let l_id = latest_id.clone();
                let wf_tx_th = wf_tx.clone();
                let l_wf_th = l_wf_id.clone();

                thread::spawn(move || {
                    for path in paths {
                        let path_str = path.to_string_lossy().to_string();
                        let file_name = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();

                        let (file_size_formatted, resolution, duration, codecs, duration_seconds, is_video, is_audio) =
                            match probe_file(&path, &bins.ffprobe) {
                                Ok(meta) => {
                                    let codecs = meta.codecs_summary();
                                    let is_video = meta.primary_video().is_some();
                                    let is_audio = meta.primary_audio().is_some();
                                    (
                                        meta.file_size_formatted(),
                                        meta.resolution_str(),
                                        meta.duration_formatted,
                                        codecs,
                                        meta.duration_seconds as f32,
                                        is_video,
                                        is_audio,
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
                                ),
                            };

                        let ui_w = ui_weak.clone();
                        let p_tx_sub = p_tx.clone();
                        let l_id_sub = l_id.clone();
                        let wf_tx_sub = wf_tx_th.clone();
                        let l_wf_sub = l_wf_th.clone();
                        let item = QueueFile {
                            id: 0,
                            file_name: SharedString::from(file_name),
                            file_path: SharedString::from(path_str),
                            file_size_formatted: SharedString::from(file_size_formatted),
                            resolution: SharedString::from(resolution),
                            duration: SharedString::from(duration),
                            codecs: SharedString::from(codecs),
                            status: SharedString::from("Ready"),
                            progress: 0.0,
                            duration_seconds,
                            is_video,
                            is_audio,
                        };

                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_w.upgrade() {
                                let app_data = ui.global::<AppData>();
                                let mut list: Vec<QueueFile> =
                                    app_data.get_queue().iter().collect();
                                let mut new_item = item;
                                new_item.id = list.len() as i32;
                                list.push(new_item.clone());
                                let count = list.len();
                                app_data.set_queue(ModelRc::new(VecModel::from(list)));
                                app_data.set_status_summary(SharedString::from(format!(
                                    "{} file(s) in queue",
                                    count
                                )));

                                // Automatically select the first file if none selected
                                if app_data.get_selected_file_index() == -1 {
                                    app_data.set_selected_file_index(0);
                                    load_selected_file(&app_data, &new_item, &p_tx_sub, &l_id_sub, &wf_tx_sub, &l_wf_sub);
                                }
                            }
                        });
                    }
                });
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
                    app_data.set_preview_frame_loading(true);
                    let id = latest_id.fetch_add(1, Ordering::SeqCst) + 1;
                    let path = app_data.get_selected_file_path().to_string();
                    let _ = preview_tx.send(PreviewTask {
                        media_path: path,
                        time: clamped as f64,
                        id,
                    });
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
                    app_data.set_preview_frame_loading(true);
                    let id = latest_id.fetch_add(1, Ordering::SeqCst) + 1;
                    let path = app_data.get_selected_file_path().to_string();
                    let _ = preview_tx.send(PreviewTask {
                        media_path: path,
                        time: new_val as f64,
                        id,
                    });
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
                    app_data.set_preview_frame_loading(true);
                    let id = latest_id.fetch_add(1, Ordering::SeqCst) + 1;
                    let path = app_data.get_selected_file_path().to_string();
                    let _ = preview_tx.send(PreviewTask {
                        media_path: path,
                        time: clamped as f64,
                        id,
                    });
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
                    app_data.set_preview_frame_loading(true);
                    let id = latest_id.fetch_add(1, Ordering::SeqCst) + 1;
                    let path = app_data.get_selected_file_path().to_string();
                    let _ = preview_tx.send(PreviewTask {
                        media_path: path,
                        time: clamped as f64,
                        id,
                    });
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
                        app_data.set_preview_frame_loading(true);
                        let id = latest_id.fetch_add(1, Ordering::SeqCst) + 1;
                        let path = app_data.get_selected_file_path().to_string();
                        let _ = preview_tx.send(PreviewTask {
                            media_path: path,
                            time: in_sec as f64,
                            id,
                        });
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
                        app_data.set_preview_frame_loading(true);
                        let id = latest_id.fetch_add(1, Ordering::SeqCst) + 1;
                        let path = app_data.get_selected_file_path().to_string();
                        let _ = preview_tx.send(PreviewTask {
                            media_path: path,
                            time: out_sec as f64,
                            id,
                        });
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

                        // Launch audio playback in parallel via ffplay in headless mode (-nodisp)
                        if let Some(ref ffplay_bin) = binaries.ffplay {
                            let dur_opt = if out_sec > start_sec {
                                Some(out_sec - start_sec)
                            } else {
                                None
                            };
                            if let Ok(ff_child) = spawn_audio_playback(ffplay_bin, &path, start_sec, dur_opt) {
                                if let Ok(mut lock) = active_ffplay_clone.lock() {
                                    *lock = Some(ff_child);
                                }
                            }
                        }

                        match spawn_playback_stream(&ffmpeg_bin, &path, start_sec) {
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
                                    let mut frame_count: u64 = 0;
                                    let frame_interval = std::time::Duration::from_millis(41); // ~24fps

                                    loop {
                                        if session_check.load(Ordering::SeqCst) != my_session {
                                            break;
                                        }

                                        let loop_start = std::time::Instant::now();

                                        if stdout.read_exact(&mut buf).is_err() {
                                            let ui_w2 = ui_w.clone();
                                            let _ = slint::invoke_from_event_loop(move || {
                                                if let Some(ui) = ui_w2.upgrade() {
                                                    ui.global::<AppData>().set_player_is_playing(false);
                                                }
                                            });
                                            break;
                                        }

                                        frame_count += 1;
                                        let cur_time = start_sec + (frame_count as f64) / 24.0;

                                        if (out_sec >= 0.0 && cur_time >= out_sec) || cur_time >= dur_sec {
                                            let ui_w2 = ui_w.clone();
                                            let _ = slint::invoke_from_event_loop(move || {
                                                if let Some(ui) = ui_w2.upgrade() {
                                                    ui.global::<AppData>().set_player_is_playing(false);
                                                }
                                            });
                                            break;
                                        }

                                        let mut pixel_buffer = SharedPixelBuffer::<Rgba8Pixel>::new(640, 360);
                                        pixel_buffer.make_mut_bytes().copy_from_slice(&buf);

                                        let ui_w2 = ui_w.clone();
                                        let _ = slint::invoke_from_event_loop(move || {
                                            if let Some(ui) = ui_w2.upgrade() {
                                                let app_data = ui.global::<AppData>();
                                                if app_data.get_player_is_playing() {
                                                    let img = slint::Image::from_rgba8(pixel_buffer);
                                                    app_data.set_preview_frame(img);
                                                    app_data.set_player_current_time(cur_time as f32);
                                                    app_data.set_player_time_text(SharedString::from(format_duration_hms(cur_time)));
                                                }
                                            }
                                        });

                                        if let Some(rem) = frame_interval.checked_sub(loop_start.elapsed()) {
                                            thread::sleep(rem);
                                        }
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

                        if let Some(ref ffplay_bin) = binaries.ffplay {
                            let dur_opt = if out_sec > start_sec {
                                Some(out_sec - start_sec)
                            } else {
                                None
                            };
                            if let Ok(ff_child) = spawn_audio_playback(ffplay_bin, &path, start_sec, dur_opt) {
                                if let Ok(mut lock) = active_ffplay_clone.lock() {
                                    *lock = Some(ff_child);
                                }
                            }
                        }

                        let ui_w = ui_handle.clone();
                        let timer_ref = play_timer_clone.clone();
                        let ffplay_child_ref = active_ffplay_clone.clone();
                        let my_session = session_id_clone.fetch_add(1, Ordering::SeqCst) + 1;
                        let session_check = session_id_clone.clone();

                        play_timer_clone.borrow().start(
                            slint::TimerMode::Repeated,
                            std::time::Duration::from_millis(50),
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

                                    let cur = app_data.get_player_current_time() + 0.05;
                                    let dur = app_data.get_player_duration();
                                    let out_sec = app_data.get_out_point_seconds();

                                    if (out_sec >= 0.0 && cur >= out_sec) || cur >= dur {
                                        app_data.set_player_is_playing(false);
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
                    let id = latest_id.fetch_add(1, Ordering::SeqCst) + 1;
                    let path = app_data.get_selected_file_path().to_string();
                    let _ = preview_tx.send(PreviewTask {
                        media_path: path,
                        time: reset_pos as f64,
                        id,
                    });
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
                    let id = latest_id.fetch_add(1, Ordering::SeqCst) + 1;
                    let path = app_data.get_selected_file_path().to_string();
                    let _ = preview_tx.send(PreviewTask {
                        media_path: path,
                        time: start as f64,
                        id,
                    });
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

                if let Ok(mut lock) = active_child_clone.lock() {
                    if let Some(mut old) = lock.take() {
                        let _ = old.kill();
                    }
                    if let Ok(child) = spawn_ffplay(&ffplay_bin, &media_path, cur, duration, !is_video) {
                        *lock = Some(child);
                    }
                }
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
                let dir_str = ui.global::<AppData>().get_output_directory().to_string();
                let target = if dir_str == "Same as source" || dir_str.is_empty() {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                } else {
                    PathBuf::from(dir_str)
                };

                #[cfg(windows)]
                {
                    let _ = std::process::Command::new("explorer").arg(target).spawn();
                }
                #[cfg(not(windows))]
                {
                    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
                }
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
                ui.global::<AppLogic>().invoke_start_processing();
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

    // Handle Start Processing
    {
        let ui_handle = ui.as_weak();
        let binaries = binaries.clone();
        let cancel_flag = cancel_flag.clone();

        ui.global::<AppLogic>().on_start_processing(move || {
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
                let crf = ui.global::<AppData>().get_crf_value().round() as u32;
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

                        // Determine function
                        let function = match func_idx {
                            0..=5 => EncoderFunction::Transcode,
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
                            audio_gain_db: None,
                            normalize_lufs,

                            in_point,
                            out_point,
                        };

                        // Probe total duration of file
                        let total_duration = match probe_file(&input_path, &bins.ffprobe) {
                            Ok(m) => m.duration_seconds,
                            Err(_) => 0.0,
                        };

                        // Build FFmpeg command
                        let cmd_args = FFmpegCommandBuilder::build(&config, &bins.ffmpeg);
                        let cmd_str = cmd_args.join(" ");

                        // Update current file status to Encoding
                        {
                            let ui_w = ui_weak.clone();
                            let idx = item_idx;
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_w.upgrade() {
                                    let app_data = ui.global::<AppData>();
                                    let mut list: Vec<QueueFile> =
                                        app_data.get_queue().iter().collect();
                                    if idx < list.len() {
                                        list[idx].status = SharedString::from("Encoding...");
                                        app_data.set_queue(ModelRc::new(VecModel::from(list)));
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

                        let progress_callback =
                            move |p: another_encoder::ffmpeg::ProgressUpdate| {
                                let ui_w = ui_w_progress.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_w.upgrade() {
                                        let app_data = ui.global::<AppData>();
                                        let mut list: Vec<QueueFile> =
                                            app_data.get_queue().iter().collect();
                                        if idx < list.len() {
                                            list[idx].progress = p.percentage;
                                            list[idx].status = SharedString::from(format!(
                                                "{:.1}%",
                                                p.percentage * 100.0
                                            ));
                                            app_data.set_queue(ModelRc::new(VecModel::from(list)));
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
                                        cur = cur[cur.len() - 3000..].to_string();
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
                            total_duration,
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
                                let mut list: Vec<QueueFile> =
                                    app_data.get_queue().iter().collect();
                                if idx < list.len() {
                                    list[idx].status = SharedString::from(status_str);
                                    if is_ok {
                                        list[idx].progress = 1.0;
                                    }
                                    app_data.set_queue(ModelRc::new(VecModel::from(list)));
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
                let dir = ui.global::<AppData>().get_ytdlp_output_dir().to_string();
                let last_file = ui.global::<AppData>().get_ytdlp_last_downloaded_file().to_string();
                if !last_file.is_empty() && Path::new(&last_file).exists() {
                    #[cfg(windows)]
                    {
                        let _ = std::process::Command::new("explorer")
                            .arg(format!("/select,{}", last_file))
                            .spawn();
                    }
                } else if !dir.is_empty() {
                    let path = PathBuf::from(&dir);
                    let _ = std::fs::create_dir_all(&path);
                    #[cfg(windows)]
                    {
                        let _ = std::process::Command::new("explorer").arg(&path).spawn();
                    }
                }
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

            let dest_dir_str = data.get_ytdlp_output_dir().to_string();
            let dest_dir = if dest_dir_str.is_empty() {
                PathBuf::from(".")
            } else {
                PathBuf::from(dest_dir_str)
            };

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
                                    data.set_ytdlp_has_error(false);
                                    data.set_ytdlp_error_message(SharedString::from(""));
                                    data.set_ytdlp_download_finished(true);
                                    data.set_ytdlp_progress(100.0);
                                    data.set_ytdlp_progress_text(SharedString::from("100%"));
                                    data.set_ytdlp_status_message(SharedString::from(
                                        "Download completed successfully!",
                                    ));
                                    data.set_ytdlp_last_downloaded_file(SharedString::from(
                                        file_path.to_string_lossy().as_ref(),
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
            let file_path_str = ui
                .global::<AppData>()
                .get_ytdlp_last_downloaded_file()
                .to_string();
            if file_path_str.is_empty() {
                return;
            }
            let path = PathBuf::from(&file_path_str);
            if !path.exists() {
                return;
            }

            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let (file_size_formatted, resolution, duration, codecs, duration_seconds, is_video, is_audio) =
                match probe_file(&path, &bins.ffprobe) {
                    Ok(meta) => {
                        let codecs = meta.codecs_summary();
                        let is_video = meta.primary_video().is_some();
                        let is_audio = meta.primary_audio().is_some();
                        (
                            meta.file_size_formatted(),
                            meta.resolution_str(),
                            meta.duration_formatted,
                            codecs,
                            meta.duration_seconds as f32,
                            is_video,
                            is_audio,
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
                    ),
                };

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
            };
            list.push(item.clone());
            let idx = (list.len() - 1) as i32;
            app_data.set_queue(ModelRc::new(VecModel::from(list)));
            app_data.set_selected_file_index(idx);
            load_selected_file(&app_data, &item, &p_tx, &l_id, &wf_tx, &l_wf);
            app_data.set_active_tab(0);
        });
    }

    // Handle yt-dlp Send to Queue
    {
        let ui_handle = ui.as_weak();
        let bins = binaries.clone();

        ui.global::<AppLogic>().on_ytdlp_send_to_queue(move || {
            let ui = match ui_handle.upgrade() {
                Some(u) => u,
                None => return,
            };
            let file_path_str = ui
                .global::<AppData>()
                .get_ytdlp_last_downloaded_file()
                .to_string();
            if file_path_str.is_empty() {
                return;
            }
            let path = PathBuf::from(&file_path_str);
            if !path.exists() {
                return;
            }

            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let (file_size_formatted, resolution, duration, codecs, duration_seconds, is_video, is_audio) =
                match probe_file(&path, &bins.ffprobe) {
                    Ok(meta) => {
                        let codecs = meta.codecs_summary();
                        let is_video = meta.primary_video().is_some();
                        let is_audio = meta.primary_audio().is_some();
                        (
                            meta.file_size_formatted(),
                            meta.resolution_str(),
                            meta.duration_formatted,
                            codecs,
                            meta.duration_seconds as f32,
                            is_video,
                            is_audio,
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
                    ),
                };

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
            };
            list.push(item);
            let count = list.len();
            app_data.set_queue(ModelRc::new(VecModel::from(list)));
            app_data.set_status_summary(SharedString::from(format!(
                "{} file(s) in queue",
                count
            )));
            app_data.set_active_tab(2);
        });
    }

    ui.run()
        .map_err(|e| anyhow::anyhow!("Platform error: {:?}", e))
}
