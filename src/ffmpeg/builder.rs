use std::path::Path;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EncoderFunction {
    #[default]
    Transcode,
    Rewrap,
    LosslessCut,
    ExtractVideo,
    ExtractAudio,
    ReplaceAudio,
    Merge,
    AudioNormalize,
    ExtractSubtitles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VideoCodec {
    #[default]
    H264,
    H265,
    AV1,
    VP9,
    AppleProRes,
    AvidDNxHD,
    MPEG2,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HwAccel {
    #[default]
    Auto,
    Cpu,
    Nvenc,
    Qsv,
    Amf,
    VideoToolbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RateControlMode {
    #[default]
    CRF,
    CBR,
    VBR,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AudioCodecChoice {
    #[default]
    AAC,
    MP3,
    Opus,
    FLAC,
    WavPCM16,
    WavPCM24,
    AC3,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ResolutionPreset {
    #[default]
    Original,
    UHD4K,
    QHD1440p,
    FHD1080p,
    HD720p,
    SD480p,
}

impl ResolutionPreset {
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        match self {
            Self::Original => None,
            Self::UHD4K => Some((3840, 2160)),
            Self::QHD1440p => Some((2560, 1440)),
            Self::FHD1080p => Some((1920, 1080)),
            Self::HD720p => Some((1280, 720)),
            Self::SD480p => Some((854, 480)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DeinterlaceMode {
    #[default]
    None,
    Yadif,
    Bwdif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ProResProfile {
    Proxy = 0,
    LT = 1,
    Standard = 2,
    #[default]
    HQ = 3,
    FourFourFourFour = 4,
    FourFourFourFourXQ = 5,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VideoRotation {
    #[default]
    None,
    Rotate90,
    RotateNeg90,
    Rotate180,
}

impl VideoRotation {
    pub fn from_degrees(deg: i32) -> Self {
        match deg {
            90 => Self::Rotate90,
            -90 | 270 => Self::RotateNeg90,
            180 | -180 => Self::Rotate180,
            _ => Self::None,
        }
    }

    pub fn degrees(&self) -> i32 {
        match self {
            Self::None => 0,
            Self::Rotate90 => 90,
            Self::RotateNeg90 => -90,
            Self::Rotate180 => 180,
        }
    }

    pub fn swaps_dimensions(&self) -> bool {
        matches!(self, Self::Rotate90 | Self::RotateNeg90)
    }

    pub fn effective_dimensions(&self, width: u32, height: u32) -> (u32, u32) {
        if self.swaps_dimensions() {
            (height, width)
        } else {
            (width, height)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CropAspectRatio {
    #[default]
    Free,
    Ratio16x9,
    Ratio9x16,
    Ratio4x3,
    Ratio3x4,
    Ratio1x1,
    Ratio2x3,
    Ratio3x2,
    Ratio21x9,
    Ratio4x5,
}

impl CropAspectRatio {
    pub fn from_index(index: i32) -> Self {
        match index {
            1 => Self::Ratio16x9,
            2 => Self::Ratio9x16,
            3 => Self::Ratio4x3,
            4 => Self::Ratio3x4,
            5 => Self::Ratio1x1,
            6 => Self::Ratio2x3,
            7 => Self::Ratio3x2,
            8 => Self::Ratio21x9,
            9 => Self::Ratio4x5,
            _ => Self::Free,
        }
    }

    pub fn ratio(&self) -> Option<(u32, u32)> {
        match self {
            Self::Free => None,
            Self::Ratio16x9 => Some((16, 9)),
            Self::Ratio9x16 => Some((9, 16)),
            Self::Ratio4x3 => Some((4, 3)),
            Self::Ratio3x4 => Some((3, 4)),
            Self::Ratio1x1 => Some((1, 1)),
            Self::Ratio2x3 => Some((2, 3)),
            Self::Ratio3x2 => Some((3, 2)),
            Self::Ratio21x9 => Some((21, 9)),
            Self::Ratio4x5 => Some((4, 5)),
        }
    }

    /// Computes a centered CropRect within `(max_w, max_h)` that fits this aspect ratio.
    pub fn compute_centered_rect(&self, max_w: u32, max_h: u32) -> CropRect {
        let max_w = (max_w & !1).max(2);
        let max_h = (max_h & !1).max(2);
        if let Some((num, den)) = self.ratio() {
            let target_r = num as f64 / den as f64;
            let frame_r = max_w as f64 / max_h as f64;
            let (mut w, mut h) = if frame_r > target_r {
                let h = max_h;
                let w = ((h as f64 * target_r).round() as u32).min(max_w);
                (w, h)
            } else {
                let w = max_w;
                let h = ((w as f64 / target_r).round() as u32).min(max_h);
                (w, h)
            };
            w = (w & !1).max(2);
            h = (h & !1).max(2);
            let x = ((max_w.saturating_sub(w)) / 2) & !1;
            let y = ((max_h.saturating_sub(h)) / 2) & !1;
            CropRect { x, y, width: w, height: h }
        } else {
            CropRect {
                x: 0,
                y: 0,
                width: max_w,
                height: max_h,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CropRect {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// Ensures even dimensions and bounds within `(max_w, max_h)`.
    pub fn sanitize(&self, max_w: u32, max_h: u32) -> Self {
        let max_w = (max_w & !1).max(2);
        let max_h = (max_h & !1).max(2);
        let w = (self.width & !1).clamp(2, max_w);
        let h = (self.height & !1).clamp(2, max_h);
        let x = (self.x & !1).min(max_w.saturating_sub(w));
        let y = (self.y & !1).min(max_h.saturating_sub(h));
        Self { x, y, width: w, height: h }
    }

    /// Moves the crop rectangle by `(dx, dy)` pixels while clamping inside `(max_w, max_h)`.
    pub fn nudge(&self, dx: i32, dy: i32, max_w: u32, max_h: u32) -> Self {
        let max_w = (max_w & !1).max(2);
        let max_h = (max_h & !1).max(2);
        let w = (self.width & !1).clamp(2, max_w);
        let h = (self.height & !1).clamp(2, max_h);
        let max_x = max_w.saturating_sub(w) as i32;
        let max_y = max_h.saturating_sub(h) as i32;
        let new_x = (self.x as i32 + dx).clamp(0, max_x) as u32;
        let new_y = (self.y as i32 + dy).clamp(0, max_y) as u32;
        Self {
            x: new_x & !1,
            y: new_y & !1,
            width: w,
            height: h,
        }
    }

    /// Formats as an FFmpeg crop filter string: `crop=w:h:x:y`
    pub fn to_ffmpeg_filter(&self) -> String {
        let w = (self.width & !1).max(2);
        let h = (self.height & !1).max(2);
        let x = self.x & !1;
        let y = self.y & !1;
        format!("crop={}:{}:{}:{}", w, h, x, y)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodeJobConfig {
    pub function: EncoderFunction,
    pub input_path: String,
    pub secondary_input_path: Option<String>, // for replace audio or merge
    pub output_path: String,

    // Video Options
    pub video_codec: VideoCodec,
    pub hw_accel: HwAccel,
    pub rate_control: RateControlMode,
    pub crf: u32,
    pub video_bitrate_kbps: u32,
    pub max_bitrate_kbps: u32,
    pub preset: String, // e.g. "medium", "slow", "fast", "p4"
    pub resolution: ResolutionPreset,
    pub custom_resolution: Option<(u32, u32)>,
    pub fps: Option<f64>,
    pub deinterlace: DeinterlaceMode,
    pub prores_profile: ProResProfile,
    pub faststart: bool,

    // Audio Options
    pub audio_codec: AudioCodecChoice,
    pub audio_bitrate_kbps: u32,
    pub audio_sample_rate: Option<u32>,
    pub audio_channels: Option<u32>, // 1 = mono, 2 = stereo, 6 = 5.1
    pub audio_gain_db: Option<f32>,
    pub normalize_lufs: Option<f32>, // e.g. -23.0 (EBU R128) or -14.0 (web)

    // Trimming / Cutting
    pub in_point: Option<String>,  // "00:00:10.000" or seconds
    pub out_point: Option<String>, // "00:01:25.500"

    // Video Editing / Transform & Crop
    pub rotation: VideoRotation,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
    pub crop: Option<CropRect>,
}

impl Default for EncodeJobConfig {
    fn default() -> Self {
        Self {
            function: EncoderFunction::Transcode,
            input_path: String::new(),
            secondary_input_path: None,
            output_path: String::new(),

            video_codec: VideoCodec::H264,
            hw_accel: HwAccel::Auto,
            rate_control: RateControlMode::CRF,
            crf: 23,
            video_bitrate_kbps: 6000,
            max_bitrate_kbps: 12000,
            preset: "medium".to_string(),
            resolution: ResolutionPreset::Original,
            custom_resolution: None,
            fps: None,
            deinterlace: DeinterlaceMode::None,
            prores_profile: ProResProfile::Standard,
            faststart: true,

            audio_codec: AudioCodecChoice::AAC,
            audio_bitrate_kbps: 192,
            audio_sample_rate: Some(48000),
            audio_channels: Some(2),
            audio_gain_db: None,
            normalize_lufs: None,

            in_point: None,
            out_point: None,

            rotation: VideoRotation::None,
            flip_horizontal: false,
            flip_vertical: false,
            crop: None,
        }
    }
}

impl EncodeJobConfig {
    /// Returns true if any video transform (rotation, flip) or crop is active.
    pub fn has_video_edits(&self) -> bool {
        self.rotation != VideoRotation::None
            || self.flip_horizontal
            || self.flip_vertical
            || self.crop.is_some()
    }
}

pub struct FFmpegCommandBuilder;

impl FFmpegCommandBuilder {
    pub fn build(config: &EncodeJobConfig, ffmpeg_bin: &Path) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        args.push(ffmpeg_bin.to_string_lossy().to_string());
        args.push("-y".to_string()); // overwrite output without prompt
        args.push("-hide_banner".to_string());

        match config.function {
            EncoderFunction::Rewrap => {
                Self::build_rewrap(config, &mut args);
            }
            EncoderFunction::LosslessCut => {
                Self::build_cut(config, &mut args);
            }
            EncoderFunction::ExtractVideo => {
                Self::build_extract_video(config, &mut args);
            }
            EncoderFunction::ExtractAudio => {
                Self::build_extract_audio(config, &mut args);
            }
            EncoderFunction::ReplaceAudio => {
                Self::build_replace_audio(config, &mut args);
            }
            EncoderFunction::Merge => {
                Self::build_merge(config, &mut args);
            }
            EncoderFunction::AudioNormalize => {
                Self::build_normalize(config, &mut args);
            }
            EncoderFunction::ExtractSubtitles => {
                Self::build_extract_subtitles(config, &mut args);
            }
            EncoderFunction::Transcode => {
                Self::build_transcode(config, &mut args);
            }
        }

        args
    }

    fn apply_trim(config: &EncodeJobConfig, args: &mut Vec<String>) {
        if let Some(in_pt) = &config.in_point {
            if !in_pt.trim().is_empty() {
                args.push("-ss".to_string());
                args.push(in_pt.clone());
            }
        }
        if let Some(out_pt) = &config.out_point {
            if !out_pt.trim().is_empty() {
                args.push("-to".to_string());
                args.push(out_pt.clone());
            }
        }
    }

    /// Builds the FFmpeg video filter list for rotation, horizontal/vertical mirror flip, and crop.
    pub fn build_transform_and_crop_filters(
        rotation: VideoRotation,
        flip_horizontal: bool,
        flip_vertical: bool,
        crop: Option<CropRect>,
    ) -> Vec<String> {
        let mut filters = Vec::new();

        // 1. Rotation
        match rotation {
            VideoRotation::None => {}
            VideoRotation::Rotate90 => filters.push("transpose=1".to_string()),
            VideoRotation::RotateNeg90 => filters.push("transpose=2".to_string()),
            VideoRotation::Rotate180 => {
                filters.push("hflip".to_string());
                filters.push("vflip".to_string());
            }
        }

        // 2. Mirror / Flip (Horizontal & Vertical)
        if flip_horizontal {
            filters.push("hflip".to_string());
        }
        if flip_vertical {
            filters.push("vflip".to_string());
        }

        // 3. Crop (in post-rotation/flip space so it matches the visual preview)
        if let Some(c) = crop {
            if c.width >= 2 && c.height >= 2 {
                filters.push(c.to_ffmpeg_filter());
            }
        }

        filters
    }

    fn build_transcode(config: &EncodeJobConfig, args: &mut Vec<String>) {
        // In & Out points (fast seek before input)
        Self::apply_trim(config, args);

        // Determine effective hardware acceleration (auto-routing to VideoToolbox on macOS)
        let effective_hw_accel = if config.hw_accel == HwAccel::Auto {
            if cfg!(target_os = "macos") {
                HwAccel::VideoToolbox
            } else {
                HwAccel::Auto
            }
        } else {
            config.hw_accel
        };

        // Hardware accel decoding if requested
        if effective_hw_accel == HwAccel::Nvenc {
            args.push("-hwaccel".to_string());
            args.push("cuda".to_string());
        } else if effective_hw_accel == HwAccel::Qsv {
            args.push("-hwaccel".to_string());
            args.push("qsv".to_string());
        } else if effective_hw_accel == HwAccel::VideoToolbox {
            args.push("-hwaccel".to_string());
            args.push("videotoolbox".to_string());
        }

        // Input
        args.push("-i".to_string());
        args.push(config.input_path.clone());

        // Video filters
        let mut vf_filters: Vec<String> = Vec::new();

        // Deinterlace
        match config.deinterlace {
            DeinterlaceMode::Yadif => vf_filters.push("yadif=0:-1:0".to_string()),
            DeinterlaceMode::Bwdif => vf_filters.push("bwdif=mode=0".to_string()),
            DeinterlaceMode::None => {}
        }

        // Rotation, Flip & Crop
        vf_filters.extend(Self::build_transform_and_crop_filters(
            config.rotation,
            config.flip_horizontal,
            config.flip_vertical,
            config.crop,
        ));

        // Scaling
        let target_dims = config.custom_resolution.or_else(|| config.resolution.dimensions());
        if let Some((w, h)) = target_dims {
            // Keep aspect ratio within bounds and ensure even dimensions for encoder
            vf_filters.push(format!(
                "scale=w={}:h={}:force_original_aspect_ratio=decrease,scale=trunc(iw/2)*2:trunc(ih/2)*2:flags=lanczos",
                w, h
            ));
        }

        // Framerate
        if let Some(fps) = config.fps {
            vf_filters.push(format!("fps=fps={}", fps));
        }

        // Filters require re-encoding; promote Copy to H264 if any filters are active
        let effective_video_codec = if !vf_filters.is_empty() && config.video_codec == VideoCodec::Copy {
            VideoCodec::H264
        } else {
            config.video_codec
        };

        if effective_video_codec == VideoCodec::Copy {
            vf_filters.clear();
        }

        if !vf_filters.is_empty() {
            args.push("-vf".to_string());
            args.push(vf_filters.join(","));
        }

        // Video Encoder
        match effective_video_codec {
            VideoCodec::H264 => {
                let encoder = match effective_hw_accel {
                    HwAccel::Nvenc => "h264_nvenc",
                    HwAccel::Qsv => "h264_qsv",
                    HwAccel::Amf => "h264_amf",
                    HwAccel::VideoToolbox => "h264_videotoolbox",
                    _ => "libx264",
                };
                args.push("-c:v".to_string());
                args.push(encoder.to_string());

                if encoder == "libx264" {
                    args.push("-preset".to_string());
                    args.push(if config.preset.is_empty() { "medium".to_string() } else { config.preset.clone() });
                    match config.rate_control {
                        RateControlMode::CRF => {
                            args.push("-crf".to_string());
                            args.push(config.crf.to_string());
                        }
                        RateControlMode::CBR => {
                            args.push("-b:v".to_string());
                            args.push(format!("{}k", config.video_bitrate_kbps));
                            args.push("-maxrate".to_string());
                            args.push(format!("{}k", config.video_bitrate_kbps));
                            args.push("-bufsize".to_string());
                            args.push(format!("{}k", config.video_bitrate_kbps * 2));
                        }
                        RateControlMode::VBR => {
                            args.push("-b:v".to_string());
                            args.push(format!("{}k", config.video_bitrate_kbps));
                            args.push("-maxrate".to_string());
                            args.push(format!("{}k", config.max_bitrate_kbps));
                            args.push("-bufsize".to_string());
                            args.push(format!("{}k", config.max_bitrate_kbps * 2));
                        }
                    }
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p".to_string());
                } else if encoder == "h264_nvenc" {
                    args.push("-preset".to_string());
                    args.push("p5".to_string());
                    if config.rate_control == RateControlMode::CRF {
                        args.push("-cq".to_string());
                        args.push(config.crf.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p".to_string());
                } else if encoder == "h264_qsv" {
                    args.push("-preset".to_string());
                    args.push("medium".to_string());
                    if config.rate_control == RateControlMode::CRF {
                        args.push("-global_quality".to_string());
                        args.push(config.crf.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-pix_fmt".to_string());
                    args.push("nv12".to_string());
                } else if encoder == "h264_amf" {
                    args.push("-quality".to_string());
                    args.push("balanced".to_string());
                    if config.rate_control == RateControlMode::CRF {
                        args.push("-rc".to_string());
                        args.push("cqp".to_string());
                        args.push("-qp_i".to_string());
                        args.push(config.crf.to_string());
                        args.push("-qp_p".to_string());
                        args.push(config.crf.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p".to_string());
                } else if encoder == "h264_videotoolbox" {
                    if config.rate_control == RateControlMode::CRF {
                        let q = (100 - (config.crf * 2)).clamp(10, 100);
                        args.push("-q:v".to_string());
                        args.push(q.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p".to_string());
                }
            }
            VideoCodec::H265 => {
                let encoder = match effective_hw_accel {
                    HwAccel::Nvenc => "hevc_nvenc",
                    HwAccel::Qsv => "hevc_qsv",
                    HwAccel::Amf => "hevc_amf",
                    HwAccel::VideoToolbox => "hevc_videotoolbox",
                    _ => "libx265",
                };
                args.push("-c:v".to_string());
                args.push(encoder.to_string());

                if encoder == "libx265" {
                    args.push("-preset".to_string());
                    args.push(if config.preset.is_empty() { "medium".to_string() } else { config.preset.clone() });
                    if config.rate_control == RateControlMode::CRF {
                        args.push("-crf".to_string());
                        args.push(config.crf.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-tag:v".to_string());
                    args.push("hvc1".to_string()); // Apple/QuickTime compatibility
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p".to_string());
                } else if encoder == "hevc_nvenc" {
                    args.push("-preset".to_string());
                    args.push("p5".to_string());
                    if config.rate_control == RateControlMode::CRF {
                        args.push("-cq".to_string());
                        args.push(config.crf.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-tag:v".to_string());
                    args.push("hvc1".to_string());
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p".to_string());
                } else if encoder == "hevc_qsv" {
                    args.push("-preset".to_string());
                    args.push("medium".to_string());
                    if config.rate_control == RateControlMode::CRF {
                        args.push("-global_quality".to_string());
                        args.push(config.crf.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-tag:v".to_string());
                    args.push("hvc1".to_string());
                    args.push("-pix_fmt".to_string());
                    args.push("nv12".to_string());
                } else if encoder == "hevc_amf" {
                    args.push("-quality".to_string());
                    args.push("balanced".to_string());
                    if config.rate_control == RateControlMode::CRF {
                        args.push("-rc".to_string());
                        args.push("cqp".to_string());
                        args.push("-qp_i".to_string());
                        args.push(config.crf.to_string());
                        args.push("-qp_p".to_string());
                        args.push(config.crf.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-tag:v".to_string());
                    args.push("hvc1".to_string());
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p".to_string());
                } else if encoder == "hevc_videotoolbox" {
                    if config.rate_control == RateControlMode::CRF {
                        let q = (100 - (config.crf * 2)).clamp(10, 100);
                        args.push("-q:v".to_string());
                        args.push(q.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-tag:v".to_string());
                    args.push("hvc1".to_string());
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p".to_string());
                }
            }
            VideoCodec::AV1 => {
                let encoder = match effective_hw_accel {
                    HwAccel::Nvenc => "av1_nvenc",
                    HwAccel::Qsv => "av1_qsv",
                    HwAccel::Amf => "av1_amf",
                    _ => "libsvtav1",
                };
                args.push("-c:v".to_string());
                args.push(encoder.to_string());

                if encoder == "libsvtav1" {
                    args.push("-crf".to_string());
                    args.push(config.crf.to_string());
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p10le".to_string());
                } else if encoder == "av1_qsv" {
                    if config.rate_control == RateControlMode::CRF {
                        args.push("-global_quality".to_string());
                        args.push(config.crf.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-pix_fmt".to_string());
                    args.push("nv12".to_string());
                } else if encoder == "av1_amf" {
                    if config.rate_control == RateControlMode::CRF {
                        args.push("-rc".to_string());
                        args.push("cqp".to_string());
                        args.push("-qp_i".to_string());
                        args.push(config.crf.to_string());
                        args.push("-qp_p".to_string());
                        args.push(config.crf.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p".to_string());
                } else if encoder == "av1_nvenc" {
                    args.push("-preset".to_string());
                    args.push("p5".to_string());
                    if config.rate_control == RateControlMode::CRF {
                        args.push("-cq".to_string());
                        args.push(config.crf.to_string());
                    } else {
                        args.push("-b:v".to_string());
                        args.push(format!("{}k", config.video_bitrate_kbps));
                    }
                    args.push("-pix_fmt".to_string());
                    args.push("yuv420p".to_string());
                } else {
                    args.push("-cq".to_string());
                    args.push(config.crf.to_string());
                }
            }
            VideoCodec::VP9 => {
                args.push("-c:v".to_string());
                args.push("libvpx-vp9".to_string());
                args.push("-crf".to_string());
                args.push(config.crf.to_string());
                args.push("-b:v".to_string());
                args.push("0".to_string());
                args.push("-row-mt".to_string());
                args.push("1".to_string());
                args.push("-pix_fmt".to_string());
                args.push("yuv420p".to_string());
            }
            VideoCodec::AppleProRes => {
                let encoder = match effective_hw_accel {
                    HwAccel::VideoToolbox => "prores_videotoolbox",
                    _ => "prores_ks",
                };
                args.push("-c:v".to_string());
                args.push(encoder.to_string());
                args.push("-profile:v".to_string());
                args.push((config.prores_profile as u8).to_string());
                if config.prores_profile == ProResProfile::FourFourFourFour || config.prores_profile == ProResProfile::FourFourFourFourXQ {
                    args.push("-pix_fmt".to_string());
                    args.push("yuva444p10le".to_string());
                } else {
                    args.push("-pix_fmt".to_string());
                    args.push("yuv422p10le".to_string());
                }
            }
            VideoCodec::AvidDNxHD => {
                args.push("-c:v".to_string());
                args.push("dnxhd".to_string());
                args.push("-profile:v".to_string());
                args.push("dnxhr_hq".to_string());
                args.push("-pix_fmt".to_string());
                args.push("yuv422p".to_string());
            }
            VideoCodec::MPEG2 => {
                args.push("-c:v".to_string());
                args.push("mpeg2video".to_string());
                args.push("-b:v".to_string());
                args.push(format!("{}k", config.video_bitrate_kbps.max(25000)));
            }
            VideoCodec::Copy => {
                args.push("-c:v".to_string());
                args.push("copy".to_string());
            }
        }

        // Faststart for streaming web containers
        if config.faststart && (config.output_path.ends_with(".mp4") || config.output_path.ends_with(".mov") || config.output_path.ends_with(".m4v")) {
            args.push("-movflags".to_string());
            args.push("+faststart".to_string());
        }

        // Audio filters
        let mut af_filters: Vec<String> = Vec::new();
        if let Some(gain) = config.audio_gain_db {
            if gain.abs() > 0.05 {
                af_filters.push(format!("volume={:.1}dB", gain));
            }
        }
        if let Some(lufs) = config.normalize_lufs {
            af_filters.push(format!("loudnorm=I={:.1}:TP=-1.0:LRA=7.0", lufs));
        }

        if !af_filters.is_empty() {
            args.push("-af".to_string());
            args.push(af_filters.join(","));
        }

        // Audio Encoder
        Self::apply_audio_encoder(config, args);

        // Output file
        args.push(config.output_path.clone());
    }

    fn apply_audio_encoder(config: &EncodeJobConfig, args: &mut Vec<String>) {
        let has_af = config.audio_gain_db.is_some_and(|g| g.abs() > 0.05) || config.normalize_lufs.is_some();
        let effective_codec = if has_af && config.audio_codec == AudioCodecChoice::Copy {
            AudioCodecChoice::AAC
        } else {
            config.audio_codec
        };

        match effective_codec {
            AudioCodecChoice::Copy => {
                args.push("-c:a".to_string());
                args.push("copy".to_string());
            }
            AudioCodecChoice::AAC => {
                args.push("-c:a".to_string());
                args.push("aac".to_string());
                args.push("-b:a".to_string());
                args.push(format!("{}k", config.audio_bitrate_kbps));
            }
            AudioCodecChoice::MP3 => {
                args.push("-c:a".to_string());
                args.push("libmp3lame".to_string());
                args.push("-b:a".to_string());
                args.push(format!("{}k", config.audio_bitrate_kbps));
            }
            AudioCodecChoice::Opus => {
                args.push("-c:a".to_string());
                args.push("libopus".to_string());
                args.push("-b:a".to_string());
                args.push(format!("{}k", config.audio_bitrate_kbps));
            }
            AudioCodecChoice::FLAC => {
                args.push("-c:a".to_string());
                args.push("flac".to_string());
            }
            AudioCodecChoice::WavPCM16 => {
                args.push("-c:a".to_string());
                args.push("pcm_s16le".to_string());
            }
            AudioCodecChoice::WavPCM24 => {
                args.push("-c:a".to_string());
                args.push("pcm_s24le".to_string());
            }
            AudioCodecChoice::AC3 => {
                args.push("-c:a".to_string());
                args.push("ac3".to_string());
                args.push("-b:a".to_string());
                args.push(format!("{}k", config.audio_bitrate_kbps.max(192)));
            }
        }

        if config.audio_codec != AudioCodecChoice::Copy {
            if let Some(sr) = config.audio_sample_rate {
                args.push("-ar".to_string());
                args.push(sr.to_string());
            }
            if let Some(ch) = config.audio_channels {
                args.push("-ac".to_string());
                args.push(ch.to_string());
            }
        }
    }

    fn build_rewrap(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        args.push("-i".to_string());
        args.push(config.input_path.clone());

        args.push("-c".to_string());
        args.push("copy".to_string());
        args.push("-map".to_string());
        args.push("0".to_string());
        args.push("-ignore_unknown".to_string());

        if config.output_path.ends_with(".mp4") || config.output_path.ends_with(".mov") {
            args.push("-movflags".to_string());
            args.push("+faststart".to_string());
        }

        args.push(config.output_path.clone());
    }

    fn build_cut(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);

        args.push("-i".to_string());
        args.push(config.input_path.clone());

        args.push("-c".to_string());
        args.push("copy".to_string());
        args.push("-avoid_negative_ts".to_string());
        args.push("make_zero".to_string());
        args.push("-map".to_string());
        args.push("0".to_string());

        args.push(config.output_path.clone());
    }

    fn build_extract_video(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        args.push("-i".to_string());
        args.push(config.input_path.clone());
        args.push("-an".to_string());
        args.push("-sn".to_string());
        args.push("-c:v".to_string());
        args.push("copy".to_string());
        args.push("-map".to_string());
        args.push("0:v:0".to_string());
        args.push(config.output_path.clone());
    }

    fn build_extract_audio(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        args.push("-i".to_string());
        args.push(config.input_path.clone());
        args.push("-vn".to_string());
        args.push("-sn".to_string());

        // Audio filters
        let mut af_filters: Vec<String> = Vec::new();
        if let Some(gain) = config.audio_gain_db {
            if gain.abs() > 0.05 {
                af_filters.push(format!("volume={:.1}dB", gain));
            }
        }
        if let Some(lufs) = config.normalize_lufs {
            af_filters.push(format!("loudnorm=I={:.1}:TP=-1.0:LRA=7.0", lufs));
        }

        if !af_filters.is_empty() {
            args.push("-af".to_string());
            args.push(af_filters.join(","));
        }

        Self::apply_audio_encoder(config, args);

        args.push("-map".to_string());
        args.push("0:a:0".to_string());
        args.push(config.output_path.clone());
    }

    fn build_extract_subtitles(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        args.push("-i".to_string());
        args.push(config.input_path.clone());
        args.push("-vn".to_string());
        args.push("-an".to_string());
        args.push("-c:s".to_string());
        args.push("copy".to_string());
        args.push("-map".to_string());
        args.push("0:s:0?".to_string());
        args.push(config.output_path.clone());
    }

    fn build_replace_audio(config: &EncodeJobConfig, args: &mut Vec<String>) {
        args.push("-i".to_string());
        args.push(config.input_path.clone()); // Video source

        if let Some(audio_in) = &config.secondary_input_path {
            args.push("-i".to_string());
            args.push(audio_in.clone()); // Audio source
        }

        args.push("-c:v".to_string());
        args.push("copy".to_string());

        Self::apply_audio_encoder(config, args);

        args.push("-map".to_string());
        args.push("0:v:0".to_string());
        args.push("-map".to_string());
        args.push("1:a:0".to_string());

        args.push("-shortest".to_string());
        args.push(config.output_path.clone());
    }

    fn build_merge(config: &EncodeJobConfig, args: &mut Vec<String>) {
        // Concat demuxer
        args.push("-f".to_string());
        args.push("concat".to_string());
        args.push("-safe".to_string());
        args.push("0".to_string());
        args.push("-i".to_string());
        args.push(config.input_path.clone()); // Points to generated concat text file

        args.push("-c".to_string());
        args.push("copy".to_string());
        args.push("-map".to_string());
        args.push("0".to_string());

        args.push(config.output_path.clone());
    }

    fn build_normalize(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        args.push("-i".to_string());
        args.push(config.input_path.clone());

        // Copy video without touching it
        args.push("-c:v".to_string());
        args.push("copy".to_string());

        let mut af_filters: Vec<String> = Vec::new();
        if let Some(gain) = config.audio_gain_db {
            if gain.abs() > 0.05 {
                af_filters.push(format!("volume={:.1}dB", gain));
            }
        }
        let lufs = config.normalize_lufs.unwrap_or(-23.0);
        af_filters.push(format!("loudnorm=I={:.1}:TP=-1.0:LRA=7.0", lufs));

        args.push("-af".to_string());
        args.push(af_filters.join(","));

        Self::apply_audio_encoder(config, args);

        args.push(config.output_path.clone());
    }
}
