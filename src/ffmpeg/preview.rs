use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use anyhow::{Context, Result};
use crate::ffmpeg::{FFmpegCommandBuilder, VideoRotation};
use crate::sanitize::validate_path;

/// Computes the normalized `(vx, vy, vw, vh)` active video sub-rectangle inside a `640x360`
/// preview buffer when `force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2` is used.
pub fn compute_letterbox_viewport(eff_width: u32, eff_height: u32) -> (f32, f32, f32, f32) {
    if eff_width == 0 || eff_height == 0 {
        return (0.0, 0.0, 1.0, 1.0);
    }
    let target_w = 640.0f32;
    let target_h = 360.0f32;
    let src_r = eff_width as f32 / eff_height as f32;
    let dst_r = target_w / target_h;

    let (scaled_w, scaled_h) = if src_r > dst_r {
        let w = target_w;
        let h = (target_w / src_r).round().clamp(1.0, target_h);
        (w, h)
    } else {
        let h = target_h;
        let w = (target_h * src_r).round().clamp(1.0, target_w);
        (w, h)
    };

    let pad_x = (target_w - scaled_w) * 0.5;
    let pad_y = (target_h - scaled_h) * 0.5;
    (
        (pad_x / target_w).clamp(0.0, 1.0),
        (pad_y / target_h).clamp(0.0, 1.0),
        (scaled_w / target_w).clamp(0.01, 1.0),
        (scaled_h / target_h).clamp(0.01, 1.0),
    )
}

fn build_preview_vf_chain(rotation: VideoRotation, flip_h: bool, flip_v: bool) -> String {
    let mut filters = FFmpegCommandBuilder::build_transform_and_crop_filters(
        rotation,
        flip_h,
        flip_v,
        None,
    );
    filters.push("scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2:black".to_string());
    filters.join(",")
}

/// Parses various timestamp formats into seconds (f64).
/// Supported:
/// - "HH:MM:SS.mmm" or "HH:MM:SS" (e.g. "01:23:45.500")
/// - "MM:SS.mmm" or "MM:SS" (e.g. "02:30.250")
/// - "SS.mmm" or "SS" (e.g. "45.0" or "45")
pub fn parse_timecode_to_seconds(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        3 => {
            let h: f64 = parts[0].trim().parse().ok()?;
            let m: f64 = parts[1].trim().parse().ok()?;
            let sec: f64 = parts[2].trim().parse().ok()?;
            if !h.is_finite() || !m.is_finite() || !sec.is_finite() {
                return None;
            }
            if h < 0.0 || m < 0.0 || m >= 60.0 || sec < 0.0 || sec >= 60.0 {
                return None;
            }
            Some(h * 3600.0 + m * 60.0 + sec)
        }
        2 => {
            let m: f64 = parts[0].trim().parse().ok()?;
            let sec: f64 = parts[1].trim().parse().ok()?;
            if !m.is_finite() || !sec.is_finite() {
                return None;
            }
            if m < 0.0 || sec < 0.0 || sec >= 60.0 {
                return None;
            }
            Some(m * 60.0 + sec)
        }
        1 => {
            let sec: f64 = parts[0].trim().parse().ok()?;
            if !sec.is_finite() || sec < 0.0 {
                return None;
            }
            Some(sec)
        }
        _ => None,
    }
}

/// Extracts a single preview video frame at a specific timestamp to the given image file.
pub fn extract_frame<P: AsRef<Path>, B: AsRef<Path>>(
    ffmpeg_bin: B,
    media_path: &str,
    time_seconds: f64,
    out_img_path: P,
) -> Result<()> {
    validate_path(media_path)?;
    validate_path(out_img_path.as_ref())?;

    let mut cmd = Command::new(ffmpeg_bin.as_ref());
    let target = time_seconds.max(0.0);
    if target > 2.0 {
        let coarse = target - 2.0;
        cmd.args([
            "-ss",
            &format!("{:.3}", coarse),
            "-copyts",
            "-i",
            media_path,
            "-ss",
            &format!("{:.3}", target),
            "-frames:v",
            "1",
            "-q:v",
            "2",
            "-vf",
            "scale='min(640,iw)':-2",
            "-update",
            "1",
            "-y",
        ]);
    } else {
        cmd.args([
            "-i",
            media_path,
            "-ss",
            &format!("{:.3}", target),
            "-frames:v",
            "1",
            "-q:v",
            "2",
            "-vf",
            "scale='min(640,iw)':-2",
            "-update",
            "1",
            "-y",
        ]);
    }
    cmd.arg(out_img_path.as_ref());

    let output = crate::ffmpeg::spawn_with_output(cmd, "ffmpeg")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffmpeg frame extraction failed: {}", err);
    }

    Ok(())
}

/// Launches native ffplay for smooth full-fps audio/video preview.
pub fn spawn_ffplay<B: AsRef<Path>>(
    ffplay_bin: B,
    media_path: &str,
    start_seconds: f64,
    duration: Option<f64>,
    audio_only: bool,
    volume: Option<i32>,
) -> Result<std::process::Child> {
    validate_path(media_path)?;

    let mut cmd = Command::new(ffplay_bin.as_ref());
    if let Some(vol) = volume {
        cmd.arg("-volume").arg(vol.clamp(0, 100).to_string());
    }
    if start_seconds > 0.05 {
        cmd.arg("-ss").arg(format!("{:.3}", start_seconds));
    }

    if let Some(dur) = duration {
        if dur > 0.0 {
            cmd.arg("-t").arg(format!("{:.3}", dur)).arg("-autoexit");
        }
    }

    let file_name = Path::new(media_path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    cmd.arg("-window_title").arg(format!("Preview — {}", file_name));

    if audio_only {
        // Show visualizer window for audio
        cmd.arg("-showmode").arg("waves");
    }

    cmd.arg("--").arg(media_path);
    cmd.stdin(std::process::Stdio::null());

    crate::set_no_window(&mut cmd);

    let child = cmd.spawn().context("Failed to spawn ffplay preview player")?;
    Ok(child)
}

/// Extracts a preview video frame into an RGBA byte buffer (640x360x4).
pub fn extract_frame_rgba<B: AsRef<Path>>(
    ffmpeg_bin: B,
    media_path: &str,
    time_seconds: f64,
) -> Result<Vec<u8>> {
    extract_frame_rgba_transformed(
        ffmpeg_bin,
        media_path,
        time_seconds,
        VideoRotation::None,
        false,
        false,
    )
}

/// Extracts a preview video frame with rotation and flip applied.
pub fn extract_frame_rgba_transformed<B: AsRef<Path>>(
    ffmpeg_bin: B,
    media_path: &str,
    time_seconds: f64,
    rotation: VideoRotation,
    flip_h: bool,
    flip_v: bool,
) -> Result<Vec<u8>> {
    validate_path(media_path)?;

    let mut cmd = Command::new(ffmpeg_bin.as_ref());
    let target = time_seconds.max(0.0);
    let vf_chain = build_preview_vf_chain(rotation, flip_h, flip_v);
    if target > 2.0 {
        let coarse = target - 2.0;
        cmd.args([
            "-threads",
            "2",
            "-ss",
            &format!("{:.3}", coarse),
            "-copyts",
            "-i",
            media_path,
            "-ss",
            &format!("{:.3}", target),
            "-vf",
            &vf_chain,
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-an",
            "-sn",
            "-dn",
            "pipe:1",
        ]);
    } else {
        cmd.args([
            "-threads",
            "2",
            "-i",
            media_path,
            "-ss",
            &format!("{:.3}", target),
            "-vf",
            &vf_chain,
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-an",
            "-sn",
            "-dn",
            "pipe:1",
        ]);
    }

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());

    let output = crate::ffmpeg::spawn_with_output(cmd, "ffmpeg")?;
    let expected = 640 * 360 * 4;
    if output.stdout.len() < expected {
        anyhow::bail!("Incomplete raw frame: got {} vs expected {}", output.stdout.len(), expected);
    }

    Ok(output.stdout[..expected].to_vec())
}

/// Starts an ffmpeg stream piping raw RGBA frames (640x360x4) at 24fps.
pub fn spawn_playback_stream<B: AsRef<Path>>(
    ffmpeg_bin: B,
    media_path: &str,
    start_seconds: f64,
) -> Result<std::process::Child> {
    spawn_playback_stream_transformed(
        ffmpeg_bin,
        media_path,
        start_seconds,
        VideoRotation::None,
        false,
        false,
    )
}

/// Starts an ffmpeg stream with rotation and flip applied.
pub fn spawn_playback_stream_transformed<B: AsRef<Path>>(
    ffmpeg_bin: B,
    media_path: &str,
    start_seconds: f64,
    rotation: VideoRotation,
    flip_h: bool,
    flip_v: bool,
) -> Result<std::process::Child> {
    spawn_playback_stream_transformed_with_duration(
        ffmpeg_bin,
        media_path,
        start_seconds,
        None,
        rotation,
        flip_h,
        flip_v,
    )
}

/// Starts an ffmpeg stream with rotation, flip, and optional duration applied.
pub fn spawn_playback_stream_transformed_with_duration<B: AsRef<Path>>(
    ffmpeg_bin: B,
    media_path: &str,
    start_seconds: f64,
    duration: Option<f64>,
    rotation: VideoRotation,
    flip_h: bool,
    flip_v: bool,
) -> Result<std::process::Child> {
    validate_path(media_path)?;

    let mut cmd = Command::new(ffmpeg_bin.as_ref());
    let target = start_seconds.max(0.0);
    let vf_chain = build_preview_vf_chain(rotation, flip_h, flip_v);
    if target > 2.0 {
        let coarse = target - 2.0;
        cmd.args([
            "-ss",
            &format!("{:.3}", coarse),
            "-copyts",
            "-i",
            media_path,
            "-ss",
            &format!("{:.3}", target),
            "-vf",
            &vf_chain,
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-r",
            "24",
            "-an",
            "-sn",
        ]);
        if let Some(dur) = duration {
            if dur > 0.0 {
                cmd.arg("-t").arg(format!("{:.3}", dur));
            }
        }
        cmd.arg("pipe:1");
    } else {
        cmd.args([
            "-i",
            media_path,
            "-ss",
            &format!("{:.3}", target),
            "-vf",
            &vf_chain,
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-r",
            "24",
            "-an",
            "-sn",
        ]);
        if let Some(dur) = duration {
            if dur > 0.0 {
                cmd.arg("-t").arg(format!("{:.3}", dur));
            }
        }
        cmd.arg("pipe:1");
    }

    crate::set_no_window(&mut cmd);

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());

    let child = cmd.spawn().context("Failed to spawn ffmpeg playback stream")?;
    Ok(child)
}

/// An active `ffplay` audio session with PTS tracked from ffplay's output and real-time audio meter levels.
pub struct AudioPlaybackSession {
    pub child: std::process::Child,
    pub master_clock: Arc<AtomicU64>,
    pub meter_levels: Arc<Mutex<(f32, f32)>>,
    pub intentional_exit: Arc<AtomicBool>,
}

impl AudioPlaybackSession {
    /// Wraps a child process without audio clock tracking (e.g. native popup ffplay).
    pub fn from_child(child: std::process::Child) -> Self {
        Self {
            child,
            master_clock: Arc::new(AtomicU64::new(u64::MAX)),
            meter_levels: Arc::new(Mutex::new((-60.0, -60.0))),
            intentional_exit: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Kills the child process immediately, marking it as intentionally stopped.
    pub fn kill(&mut self) -> std::io::Result<()> {
        self.intentional_exit.store(true, Ordering::Release);
        let res = self.child.kill();
        let _ = self.child.wait();
        res
    }

    /// Current audio timestamp in seconds, or None if not started yet.
    pub fn get_pts(&self) -> Option<f64> {
        let val = self.master_clock.load(Ordering::Acquire);
        if val == u64::MAX {
            None
        } else {
            Some((val as f64) / 1_000_000.0)
        }
    }

    /// Current real-time stereo meter levels (L_dB, R_dB) in [-60.0, 6.0].
    pub fn get_meter_levels(&self) -> (f32, f32) {
        if let Ok(lock) = self.meter_levels.lock() {
            *lock
        } else {
            (-60.0, -60.0)
        }
    }

    /// Checks if the process has naturally exited (i.e. not intentionally killed by restart/stop).
    pub fn is_finished(&mut self) -> bool {
        if self.intentional_exit.load(Ordering::Acquire) {
            return false;
        }
        matches!(self.child.try_wait(), Ok(Some(_status)))
    }
}

impl Drop for AudioPlaybackSession {
    fn drop(&mut self) {
        self.intentional_exit.store(true, Ordering::Release);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Parses per-channel peak level in dB from `lavfi.astats.<ch>.Peak_level=<val>` lines.
/// Returns `Some((channel, level_db))` where `channel` is 1 (Left) or 2 (Right),
/// and `level_db` is clamped between `-60.0` and `6.0`.
pub fn parse_astats_level(line: &str) -> Option<(usize, f32)> {
    let trimmed = line.trim();
    const PREFIX: &str = "lavfi.astats.";
    let pos = trimmed.find(PREFIX)?;
    let rest = &trimmed[pos + PREFIX.len()..];
    let dot_pos = rest.find('.')?;
    let ch_str = &rest[..dot_pos];
    let ch: usize = ch_str.parse().ok()?;

    let after_dot = &rest[dot_pos + 1..];
    const KEY: &str = "Peak_level=";
    if !after_dot.starts_with(KEY) {
        return None;
    }
    let val_str = after_dot[KEY.len()..].trim();
    let val = if val_str.contains("-inf") || val_str.contains("-nan") || val_str.contains("nan") {
        -60.0f32
    } else if let Ok(parsed) = val_str.parse::<f32>() {
        if parsed.is_finite() {
            parsed.clamp(-60.0, 6.0)
        } else {
            -60.0
        }
    } else {
        -60.0
    };

    Some((ch, val))
}

/// Reads metadata lines from `ffplay`'s stdout stream printed by `ametadata=mode=print:file=-`
/// and continuously updates the shared atomic/mutex meter levels.
pub fn read_ffplay_meter<R: std::io::Read>(reader: R, meter: Arc<Mutex<(f32, f32)>>) {
    use std::io::BufRead;
    let mut buf_reader = std::io::BufReader::with_capacity(4096, reader);
    let mut line_buf = String::with_capacity(128);

    while let Ok(n) = buf_reader.read_line(&mut line_buf) {
        if n == 0 {
            break;
        }
        if let Some((ch, lvl)) = parse_astats_level(&line_buf) {
            if let Ok(mut lock) = meter.lock() {
                if ch == 1 {
                    lock.0 = lvl;
                } else if ch == 2 {
                    lock.1 = lvl;
                }
            }
        }
        line_buf.clear();
        if line_buf.capacity() > 4096 {
            line_buf.shrink_to(512);
        }
    }
}

/// Parses the audio PTS from an `ffplay -nodisp` status line.
pub fn parse_ffplay_pts(line: &str) -> Option<f64> {
    let pos = line.find("M-A:")?;
    let prefix = line[..pos].trim();
    let token = prefix.split_whitespace().last()?;
    if let Ok(val) = token.parse::<f64>() {
        if val.is_finite() {
            return Some(val.max(0.0));
        }
    }
    None
}

/// Reads status lines from `ffplay`'s stderr stream delimited by `\r` (or `\n`)
/// and continuously updates the shared atomic master clock.
pub fn read_ffplay_clock<R: std::io::Read>(reader: R, clock: Arc<AtomicU64>) {
    use std::io::BufRead;
    let mut buf_reader = std::io::BufReader::with_capacity(4096, reader);
    let mut line_buf = Vec::with_capacity(256);

    while let Ok(n) = buf_reader.read_until(b'\r', &mut line_buf) {
        if n == 0 {
            break;
        }
        let text = String::from_utf8_lossy(&line_buf);
        if let Some(pts) = parse_ffplay_pts(&text) {
            let clamped_micros = (pts.max(0.0) * 1_000_000.0).min((u64::MAX - 1) as f64) as u64;
            clock.store(clamped_micros, Ordering::Release);
        }
        line_buf.clear();
        if line_buf.capacity() > 16384 {
            line_buf.shrink_to(4096);
        }
    }
}

/// Spawns audio playback via ffplay in headless mode (-nodisp) directly to the system default audio device.
/// Optionally applies real-time audio filters such as volume gain in dB and loudness normalization.
/// Attaches audio level meter filter chain measuring L and R peak dB independently.
/// Returns an `AudioPlaybackSession` with atomic master clock tracking and real-time audio meter levels.
pub fn spawn_audio_playback<B: AsRef<Path>>(
    ffplay_bin: B,
    media_path: &str,
    start_seconds: f64,
    duration: Option<f64>,
    volume: i32,
    gain_db: Option<f32>,
    normalize: Option<f32>,
) -> Result<AudioPlaybackSession> {
    validate_path(media_path)?;

    let mut cmd = Command::new(ffplay_bin.as_ref());
    cmd.arg("-nodisp");
    cmd.arg("-autoexit");
    cmd.arg("-seek2any").arg("1");
    let vol_clamped = volume.clamp(0, 100);
    cmd.arg("-volume").arg(vol_clamped.to_string());

    let mut af_filters = Vec::new();
    if let Some(lufs) = normalize {
        af_filters.push(format!("loudnorm=I={:.1}:TP=-1.5:LRA=11", lufs));
    }
    if let Some(gain) = gain_db {
        if gain.abs() > 0.05 {
            af_filters.push(format!("volume={:.1}dB", gain));
        }
    }

    // Force stereo layout so both L and R channels exist, window into 2048 samples (~46ms),
    // measure peak level per channel, and stream to stdout via ametadata
    af_filters.push("aformat=channel_layouts=stereo".to_string());
    af_filters.push("asetnsamples=n=2048".to_string());
    af_filters.push("astats=metadata=1:reset=1:measure_perchannel=Peak_level:measure_overall=none".to_string());
    af_filters.push("ametadata=mode=print:file=-:direct=1".to_string());

    cmd.arg("-af").arg(af_filters.join(","));

    let target = start_seconds.max(0.0);
    if target > 0.001 {
        cmd.arg("-ss").arg(format!("{:.3}", target));
    }
    if let Some(dur) = duration {
        if dur > 0.0 {
            cmd.arg("-t").arg(format!("{:.3}", dur));
        }
    }
    cmd.arg("--").arg(media_path);

    crate::set_no_window(&mut cmd);

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().context("Failed to spawn audio playback via ffplay")?;
    let master_clock = Arc::new(AtomicU64::new(u64::MAX));
    let meter_levels = Arc::new(Mutex::new((-60.0f32, -60.0f32)));

    if let Some(stdout) = child.stdout.take() {
        let meter = meter_levels.clone();
        std::thread::Builder::new()
            .name("ffplay-meter-reader".to_string())
            .spawn(move || {
                read_ffplay_meter(stdout, meter);
            })
            .ok();
    }

    if let Some(stderr) = child.stderr.take() {
        let clock = master_clock.clone();
        std::thread::Builder::new()
            .name("ffplay-clock-reader".to_string())
            .spawn(move || {
                read_ffplay_clock(stderr, clock);
            })
            .ok();
    }

    Ok(AudioPlaybackSession {
        child,
        master_clock,
        meter_levels,
        intentional_exit: Arc::new(AtomicBool::new(false)),
    })
}

/// Extracts an audio waveform image into an RGBA byte buffer (width x height x 4).
pub fn extract_waveform_rgba<B: AsRef<Path>>(
    ffmpeg_bin: B,
    media_path: &str,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    validate_path(media_path)?;

    let mut cmd = Command::new(ffmpeg_bin.as_ref());
    let filter = if height >= 100 {
        format!(
            "aformat=channel_layouts=stereo,showwavespic=s={}x{}:split_channels=1:colors=0x38bdf8|0xa855f7:filter=peak:scale=sqrt",
            width, height
        )
    } else {
        format!(
            "aformat=sample_fmts=s16:channel_layouts=mono,showwavespic=s={}x{}:colors=0x38bdf8:filter=peak:scale=sqrt",
            width, height
        )
    };
    cmd.args([
        "-threads",
        "2",
        "-vn",
        "-sn",
        "-dn",
        "-i",
        media_path,
        "-filter_complex",
        &filter,
        "-frames:v",
        "1",
        "-f",
        "rawvideo",
        "-pix_fmt",
        "rgba",
        "pipe:1",
    ]);

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());

    let output = crate::ffmpeg::spawn_with_output(cmd, "ffmpeg")?;
    let expected = (width * height * 4) as usize;
    if output.stdout.len() < expected {
        anyhow::bail!("Incomplete waveform data: got {} vs expected {}", output.stdout.len(), expected);
    }

    Ok(output.stdout[..expected].to_vec())
}
