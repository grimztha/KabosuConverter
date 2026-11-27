use anyhow::Context;

pub mod builder;
pub mod locator;
pub mod preview;
pub mod probe;
pub mod runner;

pub use builder::{
    get_detected_hardware_accel, set_detected_hardware_accel, AudioCodecChoice, CropAspectRatio,
    CropBoxDragSession, CropHandleDragSession, CropRect, DeinterlaceMode, EncodeJobConfig,
    EncoderFunction, FFmpegCommandBuilder, HwAccel, ProResProfile, RateControlMode,
    ResolutionPreset, VideoCodec, VideoRotation,
};
pub use locator::{parse_ffmpeg_version, FFmpegBinaries};
pub use preview::{
    build_preview_afade_filters, compute_letterbox_viewport, extract_frame, extract_frame_rgba,
    extract_frame_rgba_transformed, extract_waveform_rgba, parse_astats_level, parse_ffplay_pts,
    parse_timecode_to_seconds, read_ffplay_meter, spawn_audio_playback,
    spawn_audio_playback_with_fade, spawn_ffplay, spawn_playback_stream,
    spawn_playback_stream_transformed, spawn_playback_stream_transformed_with_duration,
    AudioFadePreview, AudioPlaybackSession,
};
pub use probe::{
    format_bytes, format_duration_hms, probe_file, AudioStreamInfo, MediaMetadata, VideoStreamInfo,
};
pub use runner::{FFmpegRunner, ProgressUpdate};

/// Spawn an FFmpeg-related process (ffprobe, ffmpeg, ffplay) with proper setup.
/// Automatically applies platform-specific flags (CREATE_NO_WINDOW on Windows).
/// Returns the output on success, or an anyhow error with context on failure.
pub fn spawn_with_output(
    mut cmd: std::process::Command,
    operation: &str,
) -> anyhow::Result<std::process::Output> {
    crate::set_no_window(&mut cmd);
    cmd.stdin(std::process::Stdio::null());
    cmd.output()
        .with_context(|| format!("Failed to run {} process", operation))
}
