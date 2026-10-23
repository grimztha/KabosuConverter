use anyhow::Context;

pub mod locator;
pub mod probe;
pub mod builder;
pub mod runner;
pub mod preview;

pub use locator::FFmpegBinaries;
pub use probe::{probe_file, format_duration_hms, MediaMetadata, VideoStreamInfo, AudioStreamInfo};
pub use builder::{
    FFmpegCommandBuilder, EncodeJobConfig, EncoderFunction, VideoCodec, HwAccel,
    RateControlMode, AudioCodecChoice, ResolutionPreset, DeinterlaceMode, ProResProfile
};
pub use runner::{FFmpegRunner, ProgressUpdate};
pub use preview::{parse_timecode_to_seconds, extract_frame, extract_frame_rgba, spawn_playback_stream, spawn_ffplay, spawn_audio_playback, extract_waveform_rgba};

/// Spawn an FFmpeg-related process (ffprobe, ffmpeg, ffplay) with proper setup.
/// Automatically applies platform-specific flags (CREATE_NO_WINDOW on Windows).
/// Returns the output on success, or an anyhow error with context on failure.
pub fn spawn_with_output(mut cmd: std::process::Command, operation: &str) -> anyhow::Result<std::process::Output> {
    crate::set_no_window(&mut cmd);
    cmd.output()
        .with_context(|| format!("Failed to run {} process", operation))
}
