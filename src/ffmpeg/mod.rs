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
