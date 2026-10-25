use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};

use another_encoder::ffmpeg::{extract_frame_rgba_transformed, extract_waveform_rgba};
use crate::context::{PreviewTask, WaveformTask};

/// Background worker that extracts video frames for preview scrubbing.
pub fn spawn_preview_worker(
    ffmpeg_bin: PathBuf,
    preview_rx: std::sync::mpsc::Receiver<PreviewTask>,
    latest_id: Arc<AtomicU64>,
    ui_weak: slint::Weak<crate::MainWindow>,
) {
    thread::spawn(move || {
        while let Ok(task) = preview_rx.recv() {
            // Drain pending tasks to keep up with scrubbing
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
                        ui.global::<crate::AppData>().set_preview_frame(img);
                        ui.global::<crate::AppData>().set_preview_frame_loading(false);
                    }
                });
            } else {
                let ui_w = ui_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = ui_w.upgrade() {
                        ui.global::<crate::AppData>().set_preview_frame_loading(false);
                    }
                });
            }
        }
    });
}

/// Background worker that renders audio waveform images.
pub fn spawn_waveform_worker(
    ffmpeg_bin: PathBuf,
    waveform_rx: std::sync::mpsc::Receiver<WaveformTask>,
    latest_id: Arc<AtomicU64>,
    ui_weak: slint::Weak<crate::MainWindow>,
) {
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
                (8192, 68)
            } else {
                (8192, 180)
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
                                let data = ui.global::<crate::AppData>();
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
                                ui.global::<crate::AppData>().set_has_waveform(false);
                            }
                        });
                    }
                }
            }
        }
    });
}
