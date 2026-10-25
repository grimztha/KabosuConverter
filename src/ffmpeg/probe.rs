use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VideoStreamInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub frame_rate: f64,
    pub frame_rate_str: String,
    pub pixel_format: String,
    pub aspect_ratio: String,
    pub bitrate_kbps: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AudioStreamInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel_layout: String,
    pub bitrate_kbps: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaMetadata {
    pub path: String,
    pub file_name: String,
    pub file_size_bytes: u64,
    pub duration_seconds: f64,
    pub duration_formatted: String,
    pub container_format: String,
    pub overall_bitrate_kbps: u64,
    pub video_streams: Vec<VideoStreamInfo>,
    pub audio_streams: Vec<AudioStreamInfo>,
    pub subtitle_count: usize,
}

impl MediaMetadata {
    pub fn primary_video(&self) -> Option<&VideoStreamInfo> {
        self.video_streams.first()
    }

    pub fn primary_audio(&self) -> Option<&AudioStreamInfo> {
        self.audio_streams.first()
    }

    pub fn resolution_str(&self) -> String {
        if let Some(v) = self.primary_video() {
            format!("{}x{}", v.width, v.height)
        } else {
            "Audio Only".to_string()
        }
    }

    pub fn codecs_summary(&self) -> String {
        let v_codec = self.primary_video().map(|v| v.codec.as_str()).unwrap_or("none");
        let a_codec = self.primary_audio().map(|a| a.codec.as_str()).unwrap_or("none");
        format!("V: {}, A: {}", v_codec, a_codec)
    }

    pub fn file_size_formatted(&self) -> String {
        let bytes = self.file_size_bytes as f64;
        if bytes >= 1024.0 * 1024.0 * 1024.0 {
            format!("{:.2} GB", bytes / (1024.0 * 1024.0 * 1024.0))
        } else if bytes >= 1024.0 * 1024.0 {
            format!("{:.1} MB", bytes / (1024.0 * 1024.0))
        } else if bytes >= 1024.0 {
            format!("{:.0} KB", bytes / 1024.0)
        } else {
            format!("{} B", self.file_size_bytes)
        }
    }
}

pub fn format_duration_hms(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "00:00.000".to_string();
    }
    let total_millis = (seconds * 1000.0).round() as u64;
    let hours = total_millis / 3_600_000;
    let rem = total_millis % 3_600_000;
    let minutes = rem / 60_000;
    let rem = rem % 60_000;
    let secs = rem / 1000;
    let millis = rem % 1000;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, secs, millis)
    } else {
        format!("{:02}:{:02}.{:03}", minutes, secs, millis)
    }
}

fn parse_fraction(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() == 2 {
        let num: f64 = parts[0].trim().parse().ok()?;
        let den: f64 = parts[1].trim().parse().ok()?;
        if den != 0.0 && num.is_finite() && den.is_finite() {
            return Some(num / den);
        }
    } else if parts.len() == 1 {
        let val: f64 = parts[0].trim().parse().ok()?;
        if val.is_finite() {
            return Some(val);
        }
    }
    None
}

/// Probes media file metadata using ffprobe.
pub fn probe_file<P: AsRef<Path>, B: AsRef<Path>>(file_path: P, ffprobe_bin: B) -> Result<MediaMetadata> {
    let path_ref = file_path.as_ref();
    let file_path_str = path_ref.to_string_lossy().to_string();
    let file_name = path_ref.file_name().unwrap_or_default().to_string_lossy().to_string();

    let file_size_bytes = std::fs::metadata(path_ref)
        .map(|m| m.len())
        .unwrap_or(0);

    let mut cmd = Command::new(ffprobe_bin.as_ref());
    cmd.args([
        "-v", "quiet",
        "-print_format", "json",
        "-show_format",
        "-show_streams",
        "--",
        &file_path_str,
    ]);

    let output = crate::ffmpeg::spawn_with_output(cmd, "ffprobe")?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("ffprobe failed for {}: {}", file_path_str, err_msg);
    }

    let json_text = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&json_text)
        .context("Failed to parse ffprobe json output")?;

    let format_obj = &parsed["format"];
    let mut duration_seconds: f64 = format_obj["duration"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| format_obj["duration"].as_f64())
        .unwrap_or(0.0);

    let container_format = format_obj["format_name"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    let overall_bitrate_kbps: u64 = format_obj["bit_rate"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| format_obj["bit_rate"].as_u64())
        .map(|b| b / 1000)
        .unwrap_or(0);

    let mut video_streams = Vec::new();
    let mut audio_streams = Vec::new();
    let mut subtitle_count = 0;

    if let Some(streams) = parsed["streams"].as_array() {
        for stream in streams {
            let codec_type = stream["codec_type"].as_str().unwrap_or("");
            let codec_name = stream["codec_name"].as_str().unwrap_or("unknown").to_string();

            match codec_type {
                "video" => {
                    // Ignore attached images / album art
                    let disposition = &stream["disposition"];
                    let is_attached_pic = disposition["attached_pic"].as_i64().unwrap_or(0) == 1;
                    if !is_attached_pic {
                        let width = stream["width"].as_u64().unwrap_or(0) as u32;
                        let height = stream["height"].as_u64().unwrap_or(0) as u32;
                        let r_fps_str = stream["r_frame_rate"].as_str().unwrap_or("0/0");
                        let avg_fps_str = stream["avg_frame_rate"].as_str().unwrap_or("0/0");
                        let fps = parse_fraction(avg_fps_str)
                            .or_else(|| parse_fraction(r_fps_str))
                            .unwrap_or(0.0);
                        let pix_fmt = stream["pix_fmt"].as_str().unwrap_or("").to_string();
                        let dar = stream["display_aspect_ratio"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| {
                                if width > 0 && height > 0 {
                                    format!("{}:{}", width, height)
                                } else {
                                    String::new()
                                }
                            });
                        let bitrate_kbps = stream["bit_rate"]
                            .as_str()
                            .and_then(|s| s.parse::<u64>().ok())
                            .or_else(|| stream["bit_rate"].as_u64())
                            .map(|b| b / 1000);

                        video_streams.push(VideoStreamInfo {
                            codec: codec_name,
                            width,
                            height,
                            frame_rate: fps,
                            frame_rate_str: format!("{:.2} fps", fps),
                            pixel_format: pix_fmt,
                            aspect_ratio: dar,
                            bitrate_kbps,
                        });
                    }
                }
                "audio" => {
                    let sample_rate = stream["sample_rate"]
                        .as_str()
                        .and_then(|s| s.parse().ok())
                        .or_else(|| stream["sample_rate"].as_u64())
                        .unwrap_or(48000) as u32;

                    let channels = stream["channels"].as_u64().unwrap_or(2) as u32;
                    let channel_layout = stream["channel_layout"].as_str().unwrap_or("stereo").to_string();
                    let bitrate_kbps = stream["bit_rate"]
                        .as_str()
                        .and_then(|s| s.parse::<u64>().ok())
                        .or_else(|| stream["bit_rate"].as_u64())
                        .map(|b| b / 1000);

                    audio_streams.push(AudioStreamInfo {
                        codec: codec_name,
                        sample_rate,
                        channels,
                        channel_layout,
                        bitrate_kbps,
                    });
                }
                "subtitle" => {
                    subtitle_count += 1;
                }
                _ => {}
            }
        }
    }

    if duration_seconds <= 0.0 {
        if let Some(streams) = parsed["streams"].as_array() {
            for stream in streams {
                let stream_dur = stream["duration"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| stream["duration"].as_f64())
                    .unwrap_or(0.0);
                if stream_dur > duration_seconds {
                    duration_seconds = stream_dur;
                }
            }
        }
    }

    Ok(MediaMetadata {
        path: file_path_str,
        file_name,
        file_size_bytes,
        duration_seconds,
        duration_formatted: format_duration_hms(duration_seconds),
        container_format,
        overall_bitrate_kbps,
        video_streams,
        audio_streams,
        subtitle_count,
    })
}
