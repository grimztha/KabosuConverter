use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result};

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
            if h < 0.0 || m < 0.0 || m >= 60.0 || sec < 0.0 || sec >= 60.0 {
                return None;
            }
            Some(h * 3600.0 + m * 60.0 + sec)
        }
        2 => {
            let m: f64 = parts[0].trim().parse().ok()?;
            let sec: f64 = parts[1].trim().parse().ok()?;
            if m < 0.0 || sec < 0.0 || sec >= 60.0 {
                return None;
            }
            Some(m * 60.0 + sec)
        }
        1 => {
            let sec: f64 = parts[0].trim().parse().ok()?;
            if sec < 0.0 {
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

    let child = cmd.spawn().context("Failed to spawn ffplay preview player")?;
    Ok(child)
}

/// Extracts a single preview video frame directly into an in-memory RGBA byte buffer (640x360x4).
/// Uses two-stage seeking with `-copyts` to provide fast, sub-frame accurate positioning on timeline scrubs.
pub fn extract_frame_rgba<B: AsRef<Path>>(
    ffmpeg_bin: B,
    media_path: &str,
    time_seconds: f64,
) -> Result<Vec<u8>> {
    let mut cmd = Command::new(ffmpeg_bin.as_ref());
    let target = time_seconds.max(0.0);
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
            "scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2:black",
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
            "scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2:black",
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

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());

    let output = crate::ffmpeg::spawn_with_output(cmd, "ffmpeg")?;
    let expected = 640 * 360 * 4;
    if output.stdout.len() < expected {
        anyhow::bail!("Incomplete raw frame: got {} vs expected {}", output.stdout.len(), expected);
    }

    Ok(output.stdout[..expected].to_vec())
}

/// Spawns a persistent continuous ffmpeg decode stream that pipes raw RGBA frames (640x360x4) at 24fps.
/// Uses fast-seek with -copyts to begin streaming exactly at `start_seconds` without frame drift.
pub fn spawn_playback_stream<B: AsRef<Path>>(
    ffmpeg_bin: B,
    media_path: &str,
    start_seconds: f64,
) -> Result<std::process::Child> {
    let mut cmd = Command::new(ffmpeg_bin.as_ref());
    let target = start_seconds.max(0.0);
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
            "scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2:black",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-r",
            "24",
            "-an",
            "-sn",
            "pipe:1",
        ]);
    } else {
        cmd.args([
            "-i",
            media_path,
            "-ss",
            &format!("{:.3}", target),
            "-vf",
            "scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2:black",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-r",
            "24",
            "-an",
            "-sn",
            "pipe:1",
        ]);
    }

    crate::set_no_window(&mut cmd);

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());

    let child = cmd.spawn().context("Failed to spawn ffmpeg playback stream")?;
    Ok(child)
}

/// Spawns audio playback via ffplay in headless mode (-nodisp) directly to the system default audio device.
pub fn spawn_audio_playback<B: AsRef<Path>>(
    ffplay_bin: B,
    media_path: &str,
    start_seconds: f64,
    duration: Option<f64>,
    volume: i32,
) -> Result<std::process::Child> {
    let mut cmd = Command::new(ffplay_bin.as_ref());
    cmd.arg("-nodisp");
    cmd.arg("-autoexit");
    let vol_clamped = volume.clamp(0, 100);
    cmd.arg("-volume").arg(vol_clamped.to_string());
    if start_seconds > 0.05 {
        cmd.arg("-ss").arg(format!("{:.3}", start_seconds));
    }
    if let Some(dur) = duration {
        if dur > 0.0 {
            cmd.arg("-t").arg(format!("{:.3}", dur));
        }
    }
    cmd.arg("--").arg(media_path);

    crate::set_no_window(&mut cmd);

    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

    let child = cmd.spawn().context("Failed to spawn audio playback via ffplay")?;
    Ok(child)
}

/// Extracts an audio waveform image into an RGBA byte buffer (width x height x 4).
pub fn extract_waveform_rgba<B: AsRef<Path>>(
    ffmpeg_bin: B,
    media_path: &str,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let mut cmd = Command::new(ffmpeg_bin.as_ref());
    let filter = format!(
        "aresample=8000,aformat=sample_fmts=s16:channel_layouts=mono,showwavespic=s={}x{}:colors=0x38bdf8|0x10b981",
        width, height
    );
    cmd.args([
        "-threads",
        "2",
        "-vn",
        "-sn",
        "-dn",
        "-t",
        "600",
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

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());

    let output = crate::ffmpeg::spawn_with_output(cmd, "ffmpeg")?;
    let expected = (width * height * 4) as usize;
    if output.stdout.len() < expected {
        anyhow::bail!("Incomplete waveform data: got {} vs expected {}", output.stdout.len(), expected);
    }

    Ok(output.stdout[..expected].to_vec())
}
