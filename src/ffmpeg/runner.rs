use std::io::{BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use anyhow::{Context, Result};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub percentage: f32,
    pub current_seconds: f64,
    pub total_seconds: f64,
    pub fps: f32,
    pub speed: f32,
    pub bitrate_kbps: f32,
    pub raw_line: String,
}

fn progress_regexes() -> &'static (Regex, Regex, Regex, Regex) {
    static REGEXES: OnceLock<(Regex, Regex, Regex, Regex)> = OnceLock::new();
    REGEXES.get_or_init(|| {
        (
            Regex::new(r"time=(\d{2}):(\d{2}):(\d{2})\.(\d+)").unwrap(),
            Regex::new(r"fps=\s*([\d\.]+)").unwrap(),
            Regex::new(r"speed=\s*([\d\.]+)x").unwrap(),
            Regex::new(r"bitrate=\s*([\d\.]+)kbits/s").unwrap(),
        )
    })
}

pub struct FFmpegRunner;

impl FFmpegRunner {
    /// Executes an FFmpeg command with real-time stderr progress parsing and cancellation support.
    ///
    /// * `cmd_args`: Full command with executable as first element
    /// * `total_duration`: Expected total duration in seconds for percentage calculation
    /// * `cancel_flag`: Atomic flag to request child process kill
    /// * `progress_cb`: Callback invoked on each progress update
    /// * `log_cb`: Callback invoked on each stderr log line
    pub fn run_with_progress<F, L>(
        cmd_args: &[String],
        total_duration: f64,
        cancel_flag: Arc<AtomicBool>,
        mut progress_cb: F,
        mut log_cb: L,
    ) -> Result<()>
    where
        F: FnMut(ProgressUpdate),
        L: FnMut(String),
    {
        if cmd_args.is_empty() {
            anyhow::bail!("Command arguments list cannot be empty");
        }

        let exe = &cmd_args[0];
        let args = &cmd_args[1..];

        let mut cmd = Command::new(exe);
        cmd.args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        crate::set_no_window(&mut cmd);

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn ffmpeg executable: {}", exe))?;

        let stderr = child.stderr.take().context("Failed to capture stderr")?;
        let mut reader = BufReader::new(stderr);
        let (time_regex, fps_regex, speed_regex, bitrate_regex) = progress_regexes();

        let mut buf = [0u8; 4096];
        let mut line_bytes: Vec<u8> = Vec::with_capacity(256);

        let mut handle_line = |raw_bytes: &[u8]| {
            let line = String::from_utf8_lossy(raw_bytes).trim().to_string();
            if line.is_empty() {
                return;
            }

            log_cb(line.clone());

            let mut current_sec = 0.0;
            let mut has_time = false;

            if let Some(caps) = time_regex.captures(&line) {
                let h: f64 = caps[1].parse().unwrap_or(0.0);
                let m: f64 = caps[2].parse().unwrap_or(0.0);
                let s: f64 = caps[3].parse().unwrap_or(0.0);
                let frac: f64 = caps[4].parse().unwrap_or(0.0);
                let frac_div = 10f64.powi(caps[4].len() as i32);
                current_sec = h * 3600.0 + m * 60.0 + s + (frac / frac_div);
                has_time = true;
            }

            if has_time {
                let fps: f32 = fps_regex
                    .captures(&line)
                    .and_then(|c| c[1].parse().ok())
                    .unwrap_or(0.0);

                let speed: f32 = speed_regex
                    .captures(&line)
                    .and_then(|c| c[1].parse().ok())
                    .unwrap_or(1.0);

                let bitrate: f32 = bitrate_regex
                    .captures(&line)
                    .and_then(|c| c[1].parse().ok())
                    .unwrap_or(0.0);

                let percentage = if total_duration > 0.0 {
                    ((current_sec / total_duration) as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                progress_cb(ProgressUpdate {
                    percentage,
                    current_seconds: current_sec,
                    total_seconds: total_duration,
                    fps,
                    speed,
                    bitrate_kbps: bitrate,
                    raw_line: line,
                });
            }
        };

        loop {
            // Check cancellation request
            if cancel_flag.load(Ordering::SeqCst) {
                let _ = child.kill();
                anyhow::bail!("Process cancelled by user");
            }

            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };

            for &b in &buf[..n] {
                if b == b'\r' || b == b'\n' {
                    if !line_bytes.is_empty() {
                        handle_line(&line_bytes);
                        line_bytes.clear();
                    }
                } else {
                    line_bytes.push(b);
                }
            }
        }

        if !line_bytes.is_empty() {
            handle_line(&line_bytes);
        }

        let status = child.wait().context("Failed to wait on ffmpeg process")?;

        if cancel_flag.load(Ordering::SeqCst) {
            anyhow::bail!("Process cancelled by user");
        }

        if !status.success() {
            anyhow::bail!("FFmpeg finished with non-zero exit code: {:?}", status.code());
        }

        // Final 100% completion event
        progress_cb(ProgressUpdate {
            percentage: 1.0,
            current_seconds: total_duration,
            total_seconds: total_duration,
            fps: 0.0,
            speed: 1.0,
            bitrate_kbps: 0.0,
            raw_line: "Done".to_string(),
        });

        Ok(())
    }
}
