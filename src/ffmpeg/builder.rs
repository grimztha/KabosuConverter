use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use serde::{Deserialize, Serialize};

static DETECTED_HW: AtomicU8 = AtomicU8::new(1); // Default: HwAccel::Cpu (1)

pub fn set_detected_hardware_accel(accel: HwAccel) {
    let code = match accel {
        HwAccel::Auto => 0,
        HwAccel::Cpu => 1,
        HwAccel::Nvenc => 2,
        HwAccel::Qsv => 3,
        HwAccel::Amf => 4,
        HwAccel::VideoToolbox => 5,
    };
    DETECTED_HW.store(code, Ordering::SeqCst);
}

pub fn get_detected_hardware_accel() -> HwAccel {
    match DETECTED_HW.load(Ordering::SeqCst) {
        0 => HwAccel::Auto,
        2 => HwAccel::Nvenc,
        3 => HwAccel::Qsv,
        4 => HwAccel::Amf,
        5 => HwAccel::VideoToolbox,
        _ => HwAccel::Cpu,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EncoderFunction {
    #[default]
    Transcode,
    Rewrap,
    LosslessCut,
    ExtractVideo,
    ExtractAudio,
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
    Copy,
}

impl VideoCodec {
    /// Maps the UI function/codec dropdown index to the corresponding video codec.
    pub fn from_function_index(idx: i32) -> Self {
        match idx {
            0 => Self::H264,
            1 => Self::H265,
            2 => Self::AV1,
            3 => Self::AppleProRes,
            4 => Self::AvidDNxHD,
            5 => Self::VP9,
            6 | 7 | 11 => Self::Copy,
            _ => Self::H264,
        }
    }

    pub fn to_function_index(self) -> i32 {
        match self {
            Self::H264 => 0,
            Self::H265 => 1,
            Self::AV1 => 2,
            Self::AppleProRes => 3,
            Self::AvidDNxHD => 4,
            Self::VP9 => 5,
            Self::Copy => 6,
        }
    }
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

impl HwAccel {
    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::Auto,
            1 => Self::Nvenc,
            2 => Self::Qsv,
            3 => Self::Amf,
            4 => Self::VideoToolbox,
            5 => Self::Cpu,
            _ => Self::Auto,
        }
    }

    pub fn to_index(self) -> i32 {
        match self {
            Self::Auto => 0,
            Self::Nvenc => 1,
            Self::Qsv => 2,
            Self::Amf => 3,
            Self::VideoToolbox => 4,
            Self::Cpu => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RateControlMode {
    #[default]
    CRF,
    CBR,
    VBR,
}

impl RateControlMode {
    pub fn from_index(idx: i32) -> Self {
        match idx {
            1 => Self::CBR,
            2 => Self::VBR,
            _ => Self::CRF,
        }
    }
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

impl AudioCodecChoice {
    pub fn from_index(idx: i32) -> Self {
        match idx {
            1 => Self::MP3,
            2 => Self::Opus,
            3 => Self::FLAC,
            4 => Self::WavPCM24,
            5 => Self::Copy,
            6 => Self::AC3,
            7 => Self::Copy,
            _ => Self::AAC,
        }
    }

    pub fn to_index(self) -> i32 {
        match self {
            Self::AAC => 0,
            Self::MP3 => 1,
            Self::Opus => 2,
            Self::FLAC => 3,
            Self::WavPCM24 | Self::WavPCM16 => 4,
            Self::Copy => 5,
            Self::AC3 => 6,
        }
    }

    /// Maps the UI audio bitrate dropdown index to kbps.
    pub fn bitrate_from_index(idx: i32) -> u32 {
        match idx {
            0 => 320,
            1 => 256,
            2 => 192,
            3 => 128,
            4 => 96,
            _ => 64,
        }
    }

    /// Maps an audio bitrate in kbps back to its UI dropdown index.
    pub fn bitrate_to_index(kbps: u32) -> i32 {
        match kbps {
            320 => 0,
            256 => 1,
            192 => 2,
            128 => 3,
            96 => 4,
            _ => 5,
        }
    }
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
    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::Original,
            1 => Self::UHD4K,
            2 => Self::QHD1440p,
            3 => Self::FHD1080p,
            4 => Self::HD720p,
            5 => Self::SD480p,
            _ => Self::Original,
        }
    }

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

impl DeinterlaceMode {
    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::None,
            1 => Self::Yadif,
            2 => Self::Bwdif,
            _ => Self::None,
        }
    }
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

/// Tracks an active handle drag operation with immutable starting anchors.
///
/// Prevents non-selected edges from shifting or drifting due to parity rounding
/// or micro-movements during interactive resize.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropHandleDragSession {
    pub handle_id: i32,
    pub start_x: i32,
    pub start_y: i32,
    pub start_w: i32,
    pub start_h: i32,
    pub fixed_right: i32,
    pub fixed_bottom: i32,
    pub start_norm_x: f32,
    pub start_norm_y: f32,
}

impl CropHandleDragSession {
    pub fn new(
        handle_id: i32,
        start_rect: CropRect,
        start_norm_x: f32,
        start_norm_y: f32,
        eff_w: u32,
        eff_h: u32,
    ) -> Self {
        let clean = start_rect.sanitize(eff_w, eff_h);
        let start_x = clean.x as i32;
        let start_y = clean.y as i32;
        let start_w = clean.width as i32;
        let start_h = clean.height as i32;
        let fixed_right = start_x + start_w;
        let fixed_bottom = start_y + start_h;

        Self {
            handle_id,
            start_x,
            start_y,
            start_w,
            start_h,
            fixed_right,
            fixed_bottom,
            start_norm_x,
            start_norm_y,
        }
    }

    /// Computes the updated `CropRect` for the current normalized pointer coordinates
    /// without letting opposite edges drift from rounding.
    pub fn compute_rect(
        &self,
        norm_x: f32,
        norm_y: f32,
        eff_w: u32,
        eff_h: u32,
        aspect: CropAspectRatio,
    ) -> CropRect {
        let eff_w_i = (eff_w.max(32) & !1) as i32;
        let eff_h_i = (eff_h.max(32) & !1) as i32;
        let delta_px = ((norm_x - self.start_norm_x) * eff_w_i as f32).round() as i32;
        let delta_py = ((norm_y - self.start_norm_y) * eff_h_i as f32).round() as i32;

        let (new_x, new_y, new_w, new_h) = if let Some((num, den)) = aspect.ratio() {
            let num_i = num as i64;
            let den_i = den as i64;

            // Fits width-driven resize inside (max_w, max_h) while keeping even dimensions.
            let fit_from_w = |raw_w: i32, max_w: i32, max_h: i32| -> (i32, i32) {
                let mut w = raw_w.clamp(16, max_w);
                let mut h = (w as i64 * den_i / num_i) as i32;
                if h > max_h {
                    h = max_h;
                    w = (h as i64 * num_i / den_i) as i32;
                }
                ((w & !1).clamp(16, max_w & !1), (h & !1).clamp(16, max_h & !1))
            };

            // Fits height-driven resize inside (max_w, max_h) while keeping even dimensions.
            let fit_from_h = |raw_h: i32, max_w: i32, max_h: i32| -> (i32, i32) {
                let mut h = raw_h.clamp(16, max_h);
                let mut w = (h as i64 * num_i / den_i) as i32;
                if w > max_w {
                    w = max_w;
                    h = (w as i64 * den_i / num_i) as i32;
                }
                ((w & !1).clamp(16, max_w & !1), (h & !1).clamp(16, max_h & !1))
            };

            match self.handle_id {
                0 => {
                    let (w, h) = fit_from_w(self.start_w - delta_px, self.fixed_right, self.fixed_bottom);
                    (self.fixed_right - w, self.fixed_bottom - h, w, h)
                }
                1 => {
                    let (w, h) = fit_from_w(self.start_w + delta_px, eff_w_i - self.start_x, self.fixed_bottom);
                    (self.start_x, self.fixed_bottom - h, w, h)
                }
                2 => {
                    let (w, h) = fit_from_w(self.start_w - delta_px, self.fixed_right, eff_h_i - self.start_y);
                    (self.fixed_right - w, self.start_y, w, h)
                }
                3 => {
                    let (w, h) = fit_from_w(self.start_w + delta_px, eff_w_i - self.start_x, eff_h_i - self.start_y);
                    (self.start_x, self.start_y, w, h)
                }
                4 => {
                    let (w, h) = fit_from_h(self.start_h - delta_py, eff_w_i, self.fixed_bottom);
                    let center_x = self.start_x + self.start_w / 2;
                    let x = (center_x - w / 2).clamp(0, eff_w_i - w) & !1;
                    (x, self.fixed_bottom - h, w, h)
                }
                5 => {
                    let (w, h) = fit_from_h(self.start_h + delta_py, eff_w_i, eff_h_i - self.start_y);
                    let center_x = self.start_x + self.start_w / 2;
                    let x = (center_x - w / 2).clamp(0, eff_w_i - w) & !1;
                    (x, self.start_y, w, h)
                }
                6 => {
                    let (w, h) = fit_from_w(self.start_w - delta_px, self.fixed_right, eff_h_i);
                    let center_y = self.start_y + self.start_h / 2;
                    let y = (center_y - h / 2).clamp(0, eff_h_i - h) & !1;
                    (self.fixed_right - w, y, w, h)
                }
                _ => {
                    let (w, h) = fit_from_w(self.start_w + delta_px, eff_w_i - self.start_x, eff_h_i);
                    let center_y = self.start_y + self.start_h / 2;
                    let y = (center_y - h / 2).clamp(0, eff_h_i - h) & !1;
                    (self.start_x, y, w, h)
                }
            }
        } else {
            // Free aspect ratio: only the dragged edge(s) move.
            match self.handle_id {
                0 => {
                    let new_x = (self.start_x + delta_px).clamp(0, self.fixed_right - 16) & !1;
                    let new_y = (self.start_y + delta_py).clamp(0, self.fixed_bottom - 16) & !1;
                    (new_x, new_y, self.fixed_right - new_x, self.fixed_bottom - new_y)
                }
                1 => {
                    let new_right = (self.fixed_right + delta_px).clamp(self.start_x + 16, eff_w_i) & !1;
                    let new_y = (self.start_y + delta_py).clamp(0, self.fixed_bottom - 16) & !1;
                    (self.start_x, new_y, new_right - self.start_x, self.fixed_bottom - new_y)
                }
                2 => {
                    let new_x = (self.start_x + delta_px).clamp(0, self.fixed_right - 16) & !1;
                    let new_bottom = (self.fixed_bottom + delta_py).clamp(self.start_y + 16, eff_h_i) & !1;
                    (new_x, self.start_y, self.fixed_right - new_x, new_bottom - self.start_y)
                }
                3 => {
                    let new_right = (self.fixed_right + delta_px).clamp(self.start_x + 16, eff_w_i) & !1;
                    let new_bottom = (self.fixed_bottom + delta_py).clamp(self.start_y + 16, eff_h_i) & !1;
                    (self.start_x, self.start_y, new_right - self.start_x, new_bottom - self.start_y)
                }
                4 => {
                    let new_y = (self.start_y + delta_py).clamp(0, self.fixed_bottom - 16) & !1;
                    (self.start_x, new_y, self.start_w, self.fixed_bottom - new_y)
                }
                5 => {
                    let new_bottom = (self.fixed_bottom + delta_py).clamp(self.start_y + 16, eff_h_i) & !1;
                    (self.start_x, self.start_y, self.start_w, new_bottom - self.start_y)
                }
                6 => {
                    let new_x = (self.start_x + delta_px).clamp(0, self.fixed_right - 16) & !1;
                    (new_x, self.start_y, self.fixed_right - new_x, self.start_h)
                }
                _ => {
                    let new_right = (self.fixed_right + delta_px).clamp(self.start_x + 16, eff_w_i) & !1;
                    (self.start_x, self.start_y, new_right - self.start_x, self.start_h)
                }
            }
        };

        CropRect {
            x: new_x.max(0) as u32,
            y: new_y.max(0) as u32,
            width: new_w.max(16) as u32,
            height: new_h.max(16) as u32,
        }
        .sanitize(eff_w, eff_h)
    }
}

/// Tracks moving the entire crop box without changing its dimensions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropBoxDragSession {
    pub start_x: i32,
    pub start_y: i32,
    pub width: u32,
    pub height: u32,
    pub start_norm_x: f32,
    pub start_norm_y: f32,
}

impl CropBoxDragSession {
    pub fn new(
        start_rect: CropRect,
        start_norm_x: f32,
        start_norm_y: f32,
        eff_w: u32,
        eff_h: u32,
    ) -> Self {
        let clean = start_rect.sanitize(eff_w, eff_h);
        Self {
            start_x: clean.x as i32,
            start_y: clean.y as i32,
            width: clean.width,
            height: clean.height,
            start_norm_x,
            start_norm_y,
        }
    }

    pub fn compute_rect(&self, norm_x: f32, norm_y: f32, eff_w: u32, eff_h: u32) -> CropRect {
        let eff_w_i = (eff_w.max(32) & !1) as i32;
        let eff_h_i = (eff_h.max(32) & !1) as i32;
        let delta_px = ((norm_x - self.start_norm_x) * eff_w_i as f32).round() as i32;
        let delta_py = ((norm_y - self.start_norm_y) * eff_h_i as f32).round() as i32;

        let max_x = (eff_w.saturating_sub(self.width) & !1) as i32;
        let max_y = (eff_h.saturating_sub(self.height) & !1) as i32;

        let target_x = (self.start_x + delta_px).clamp(0, max_x.max(0));
        let target_y = (self.start_y + delta_py).clamp(0, max_y.max(0));

        CropRect {
            x: (target_x & !1) as u32,
            y: (target_y & !1) as u32,
            width: self.width,
            height: self.height,
        }
        .sanitize(eff_w, eff_h)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodeJobConfig {
    pub function: EncoderFunction,
    pub input_path: String,
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
    /// Validates job inputs and builds the complete FFmpeg argument list.
    pub fn try_build(config: &EncodeJobConfig, ffmpeg_bin: &Path) -> anyhow::Result<Vec<String>> {
        crate::sanitize::validate_path(&config.input_path)
            .map_err(|e| anyhow::anyhow!("Invalid input path: {}", e))?;
        crate::sanitize::validate_path(&config.output_path)
            .map_err(|e| anyhow::anyhow!("Invalid output path: {}", e))?;

        if let Some(ref in_pt) = config.in_point {
            crate::sanitize::validate_timecode(in_pt)
                .map_err(|e| anyhow::anyhow!("Invalid in-point timecode: {}", e))?;
        }
        if let Some(ref out_pt) = config.out_point {
            crate::sanitize::validate_timecode(out_pt)
                .map_err(|e| anyhow::anyhow!("Invalid out-point timecode: {}", e))?;
        }

        let mut args = vec![
            ffmpeg_bin.to_string_lossy().to_string(),
            "-y".to_string(),
            "-hide_banner".to_string(),
        ];

        match config.function {
            EncoderFunction::Rewrap => Self::build_rewrap(config, &mut args),
            EncoderFunction::LosslessCut => Self::build_cut(config, &mut args),
            EncoderFunction::ExtractVideo => Self::build_extract_video(config, &mut args),
            EncoderFunction::ExtractAudio => Self::build_extract_audio(config, &mut args),
            EncoderFunction::AudioNormalize => Self::build_normalize(config, &mut args),
            EncoderFunction::ExtractSubtitles => Self::build_extract_subtitles(config, &mut args),
            EncoderFunction::Transcode => Self::build_transcode(config, &mut args),
        }

        Ok(args)
    }

    /// Panicking convenience wrapper around [`Self::try_build`] for validated configs and tests.
    pub fn build(config: &EncodeJobConfig, ffmpeg_bin: &Path) -> Vec<String> {
        Self::try_build(config, ffmpeg_bin).expect("Invalid FFmpeg job configuration")
    }

    fn is_mp4_like_container(output_path: &str) -> bool {
        let out_lower = output_path.to_lowercase();
        out_lower.ends_with(".mp4") || out_lower.ends_with(".mov") || out_lower.ends_with(".m4v")
    }

    fn push_pair(args: &mut Vec<String>, flag: &str, value: impl Into<String>) {
        args.push(flag.to_string());
        args.push(value.into());
    }

    fn push_nvenc_args(config: &EncodeJobConfig, args: &mut Vec<String>, hvc1_tag: bool) {
        Self::push_pair(args, "-preset", "p5");
        if config.rate_control == RateControlMode::CRF {
            Self::push_pair(args, "-cq", config.crf.to_string());
        } else {
            Self::push_pair(args, "-b:v", format!("{}k", config.video_bitrate_kbps));
        }
        if hvc1_tag {
            Self::push_pair(args, "-tag:v", "hvc1");
        }
        Self::push_pair(args, "-pix_fmt", "yuv420p");
    }

    fn push_qsv_args(config: &EncodeJobConfig, args: &mut Vec<String>, with_preset: bool, hvc1_tag: bool) {
        if with_preset {
            Self::push_pair(args, "-preset", "medium");
        }
        if config.rate_control == RateControlMode::CRF {
            Self::push_pair(args, "-global_quality", config.crf.to_string());
        } else {
            Self::push_pair(args, "-b:v", format!("{}k", config.video_bitrate_kbps));
        }
        if hvc1_tag {
            Self::push_pair(args, "-tag:v", "hvc1");
        }
        Self::push_pair(args, "-pix_fmt", "nv12");
    }

    fn push_amf_args(config: &EncodeJobConfig, args: &mut Vec<String>, with_quality: bool, hvc1_tag: bool) {
        if with_quality {
            Self::push_pair(args, "-quality", "balanced");
        }
        if config.rate_control == RateControlMode::CRF {
            Self::push_pair(args, "-rc", "cqp");
            Self::push_pair(args, "-qp_i", config.crf.to_string());
            Self::push_pair(args, "-qp_p", config.crf.to_string());
        } else {
            Self::push_pair(args, "-b:v", format!("{}k", config.video_bitrate_kbps));
        }
        if hvc1_tag {
            Self::push_pair(args, "-tag:v", "hvc1");
        }
        Self::push_pair(args, "-pix_fmt", "yuv420p");
    }

    fn push_videotoolbox_args(config: &EncodeJobConfig, args: &mut Vec<String>, hvc1_tag: bool) {
        if config.rate_control == RateControlMode::CRF {
            // Saturating arithmetic prevents underflow when CRF is 51 (51 * 2 > 100).
            let q = 100u32
                .saturating_sub(config.crf.saturating_mul(2))
                .clamp(10, 100);
            Self::push_pair(args, "-q:v", q.to_string());
        } else {
            Self::push_pair(args, "-b:v", format!("{}k", config.video_bitrate_kbps));
        }
        if hvc1_tag {
            Self::push_pair(args, "-tag:v", "hvc1");
        }
        Self::push_pair(args, "-pix_fmt", "yuv420p");
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

        // Determine effective hardware acceleration (auto-routing to detected hardware or VideoToolbox on macOS)
        let effective_hw_accel = if config.hw_accel == HwAccel::Auto {
            if cfg!(target_os = "macos") {
                HwAccel::VideoToolbox
            } else {
                get_detected_hardware_accel()
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
        if let Some(fps) = config.fps.filter(|v| v.is_finite() && *v > 0.0) {
            vf_filters.push(format!("fps=fps={}", fps));
        }

        // Video filters require re-encoding; promote Copy to H264 if any filters are active.
        let effective_video_codec = if !vf_filters.is_empty() && config.video_codec == VideoCodec::Copy {
            VideoCodec::H264
        } else {
            config.video_codec
        };

        if effective_video_codec == VideoCodec::Copy {
            vf_filters.clear();
        }

        if !vf_filters.is_empty() {
            Self::push_pair(args, "-vf", vf_filters.join(","));
        }

        let preset = if config.preset.is_empty() {
            "medium"
        } else {
            config.preset.as_str()
        };

        match effective_video_codec {
            VideoCodec::H264 => {
                let encoder = match effective_hw_accel {
                    HwAccel::Nvenc => "h264_nvenc",
                    HwAccel::Qsv => "h264_qsv",
                    HwAccel::Amf => "h264_amf",
                    HwAccel::VideoToolbox => "h264_videotoolbox",
                    _ => "libx264",
                };
                Self::push_pair(args, "-c:v", encoder);

                match encoder {
                    "libx264" => {
                        Self::push_pair(args, "-preset", preset);
                        match config.rate_control {
                            RateControlMode::CRF => Self::push_pair(args, "-crf", config.crf.to_string()),
                            RateControlMode::CBR => {
                                Self::push_pair(args, "-b:v", format!("{}k", config.video_bitrate_kbps));
                                Self::push_pair(args, "-maxrate", format!("{}k", config.video_bitrate_kbps));
                                Self::push_pair(args, "-bufsize", format!("{}k", config.video_bitrate_kbps * 2));
                            }
                            RateControlMode::VBR => {
                                Self::push_pair(args, "-b:v", format!("{}k", config.video_bitrate_kbps));
                                Self::push_pair(args, "-maxrate", format!("{}k", config.max_bitrate_kbps));
                                Self::push_pair(args, "-bufsize", format!("{}k", config.max_bitrate_kbps * 2));
                            }
                        }
                        Self::push_pair(args, "-pix_fmt", "yuv420p");
                    }
                    "h264_nvenc" => Self::push_nvenc_args(config, args, false),
                    "h264_qsv" => Self::push_qsv_args(config, args, true, false),
                    "h264_amf" => Self::push_amf_args(config, args, true, false),
                    "h264_videotoolbox" => Self::push_videotoolbox_args(config, args, false),
                    _ => {}
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
                Self::push_pair(args, "-c:v", encoder);

                match encoder {
                    "libx265" => {
                        Self::push_pair(args, "-preset", preset);
                        if config.rate_control == RateControlMode::CRF {
                            Self::push_pair(args, "-crf", config.crf.to_string());
                        } else {
                            Self::push_pair(args, "-b:v", format!("{}k", config.video_bitrate_kbps));
                        }
                        // Tag HEVC as hvc1 for Apple/QuickTime compatibility.
                        Self::push_pair(args, "-tag:v", "hvc1");
                        Self::push_pair(args, "-pix_fmt", "yuv420p");
                    }
                    "hevc_nvenc" => Self::push_nvenc_args(config, args, true),
                    "hevc_qsv" => Self::push_qsv_args(config, args, true, true),
                    "hevc_amf" => Self::push_amf_args(config, args, true, true),
                    "hevc_videotoolbox" => Self::push_videotoolbox_args(config, args, true),
                    _ => {}
                }
            }
            VideoCodec::AV1 => {
                let encoder = match effective_hw_accel {
                    HwAccel::Nvenc => "av1_nvenc",
                    HwAccel::Qsv => "av1_qsv",
                    HwAccel::Amf => "av1_amf",
                    _ => "libsvtav1",
                };
                Self::push_pair(args, "-c:v", encoder);

                match encoder {
                    "libsvtav1" => {
                        if config.rate_control == RateControlMode::CRF {
                            Self::push_pair(args, "-crf", config.crf.to_string());
                        } else {
                            Self::push_pair(args, "-b:v", format!("{}k", config.video_bitrate_kbps));
                        }
                        Self::push_pair(args, "-pix_fmt", "yuv420p10le");
                    }
                    "av1_nvenc" => Self::push_nvenc_args(config, args, false),
                    "av1_qsv" => Self::push_qsv_args(config, args, false, false),
                    "av1_amf" => Self::push_amf_args(config, args, false, false),
                    _ => Self::push_pair(args, "-cq", config.crf.to_string()),
                }
            }
            VideoCodec::VP9 => {
                Self::push_pair(args, "-c:v", "libvpx-vp9");
                Self::push_pair(args, "-crf", config.crf.to_string());
                Self::push_pair(args, "-b:v", "0");
                Self::push_pair(args, "-row-mt", "1");
                Self::push_pair(args, "-pix_fmt", "yuv420p");
            }
            VideoCodec::AppleProRes => {
                let encoder = match effective_hw_accel {
                    HwAccel::VideoToolbox => "prores_videotoolbox",
                    _ => "prores_ks",
                };
                Self::push_pair(args, "-c:v", encoder);
                Self::push_pair(args, "-profile:v", (config.prores_profile as u8).to_string());
                let pix_fmt = if matches!(
                    config.prores_profile,
                    ProResProfile::FourFourFourFour | ProResProfile::FourFourFourFourXQ
                ) {
                    "yuva444p10le"
                } else {
                    "yuv422p10le"
                };
                Self::push_pair(args, "-pix_fmt", pix_fmt);
            }
            VideoCodec::AvidDNxHD => {
                Self::push_pair(args, "-c:v", "dnxhd");
                Self::push_pair(args, "-profile:v", "dnxhr_hq");
                Self::push_pair(args, "-pix_fmt", "yuv422p");
            }
            VideoCodec::Copy => {
                Self::push_pair(args, "-c:v", "copy");
            }
        }

        if config.faststart && Self::is_mp4_like_container(&config.output_path) {
            Self::push_pair(args, "-movflags", "+faststart");
        }

        let has_af = Self::append_audio_filters(config, None, args);
        Self::apply_audio_encoder(config, has_af, args);

        args.push(config.output_path.clone());
    }

    /// Appends `-af` filters when gain or loudness normalization is active; returns true if added.
    fn append_audio_filters(config: &EncodeJobConfig, fallback_lufs: Option<f32>, args: &mut Vec<String>) -> bool {
        let mut af_filters: Vec<String> = Vec::new();
        if let Some(gain) = config.audio_gain_db.filter(|g| g.is_finite() && g.abs() > 0.05) {
            af_filters.push(format!("volume={:.1}dB", gain.clamp(-60.0, 60.0)));
        }
        if let Some(lufs) = config.normalize_lufs.or(fallback_lufs).filter(|v| v.is_finite()) {
            af_filters.push(format!("loudnorm=I={:.1}:TP=-1.0:LRA=7.0", lufs.clamp(-70.0, -5.0)));
        }

        if !af_filters.is_empty() {
            Self::push_pair(args, "-af", af_filters.join(","));
            true
        } else {
            false
        }
    }

    fn apply_audio_encoder(config: &EncodeJobConfig, has_af: bool, args: &mut Vec<String>) {
        let is_mp4_like = Self::is_mp4_like_container(&config.output_path);

        let effective_codec = if has_af && config.audio_codec == AudioCodecChoice::Copy {
            AudioCodecChoice::AAC
        } else if is_mp4_like && matches!(config.audio_codec, AudioCodecChoice::MP3 | AudioCodecChoice::Opus) {
            // MP3/Opus in MP4/MOV play silently on iOS, Android, and messenger apps; use AAC instead.
            AudioCodecChoice::AAC
        } else {
            config.audio_codec
        };

        match effective_codec {
            AudioCodecChoice::Copy => Self::push_pair(args, "-c:a", "copy"),
            AudioCodecChoice::AAC => {
                Self::push_pair(args, "-c:a", "aac");
                Self::push_pair(args, "-b:a", format!("{}k", config.audio_bitrate_kbps));
            }
            AudioCodecChoice::MP3 => {
                Self::push_pair(args, "-c:a", "libmp3lame");
                Self::push_pair(args, "-b:a", format!("{}k", config.audio_bitrate_kbps));
            }
            AudioCodecChoice::Opus => {
                Self::push_pair(args, "-c:a", "libopus");
                Self::push_pair(args, "-b:a", format!("{}k", config.audio_bitrate_kbps));
            }
            AudioCodecChoice::FLAC => Self::push_pair(args, "-c:a", "flac"),
            AudioCodecChoice::WavPCM16 => Self::push_pair(args, "-c:a", "pcm_s16le"),
            AudioCodecChoice::WavPCM24 => Self::push_pair(args, "-c:a", "pcm_s24le"),
            AudioCodecChoice::AC3 => {
                Self::push_pair(args, "-c:a", "ac3");
                Self::push_pair(args, "-b:a", format!("{}k", config.audio_bitrate_kbps.max(192)));
            }
        }

        if effective_codec != AudioCodecChoice::Copy {
            if let Some(sr) = config.audio_sample_rate {
                Self::push_pair(args, "-ar", sr.to_string());
            }
            if let Some(ch) = config.audio_channels {
                Self::push_pair(args, "-ac", ch.to_string());
            } else if is_mp4_like {
                // Default to stereo for MP4/MOV so surround tracks don't drop dialogue on phone speakers.
                Self::push_pair(args, "-ac", "2");
            }
        }
    }

    fn build_rewrap(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        Self::push_pair(args, "-i", config.input_path.clone());
        Self::push_pair(args, "-c", "copy");
        Self::push_pair(args, "-map", "0");
        args.push("-ignore_unknown".to_string());

        if Self::is_mp4_like_container(&config.output_path) {
            Self::push_pair(args, "-movflags", "+faststart");
        }

        args.push(config.output_path.clone());
    }

    fn build_cut(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        Self::push_pair(args, "-i", config.input_path.clone());
        Self::push_pair(args, "-c", "copy");
        Self::push_pair(args, "-avoid_negative_ts", "make_zero");
        Self::push_pair(args, "-map", "0");
        args.push(config.output_path.clone());
    }

    fn build_extract_video(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        Self::push_pair(args, "-i", config.input_path.clone());
        args.push("-an".to_string());
        args.push("-sn".to_string());
        Self::push_pair(args, "-c:v", "copy");
        Self::push_pair(args, "-map", "0:v:0");
        args.push(config.output_path.clone());
    }

    fn build_extract_audio(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        Self::push_pair(args, "-i", config.input_path.clone());
        args.push("-vn".to_string());
        args.push("-sn".to_string());

        let has_af = Self::append_audio_filters(config, None, args);
        Self::apply_audio_encoder(config, has_af, args);

        Self::push_pair(args, "-map", "0:a:0");
        args.push(config.output_path.clone());
    }

    fn build_extract_subtitles(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        Self::push_pair(args, "-i", config.input_path.clone());
        args.push("-vn".to_string());
        args.push("-an".to_string());
        Self::push_pair(args, "-c:s", "copy");
        Self::push_pair(args, "-map", "0:s:0?");
        args.push(config.output_path.clone());
    }

    fn build_normalize(config: &EncodeJobConfig, args: &mut Vec<String>) {
        Self::apply_trim(config, args);
        Self::push_pair(args, "-i", config.input_path.clone());
        Self::push_pair(args, "-c:v", "copy");

        let has_af = Self::append_audio_filters(config, Some(-23.0), args);
        Self::apply_audio_encoder(config, has_af, args);

        args.push(config.output_path.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_videotoolbox_crf_51_does_not_underflow() {
        let config = EncodeJobConfig {
            function: EncoderFunction::Transcode,
            input_path: "in.mp4".to_string(),
            output_path: "out.mp4".to_string(),
            video_codec: VideoCodec::H264,
            hw_accel: HwAccel::VideoToolbox,
            rate_control: RateControlMode::CRF,
            crf: 51,
            ..Default::default()
        };
        let cmd = FFmpegCommandBuilder::try_build(&config, Path::new("ffmpeg")).unwrap();
        let q_idx = cmd.iter().position(|a| a == "-q:v").unwrap();
        assert_eq!(cmd[q_idx + 1], "10");
    }

    #[test]
    fn test_audio_normalize_with_copy_promotes_to_aac() {
        let config = EncodeJobConfig {
            function: EncoderFunction::AudioNormalize,
            input_path: "in.mp4".to_string(),
            output_path: "out.mp4".to_string(),
            normalize_lufs: None,
            audio_codec: AudioCodecChoice::Copy,
            ..Default::default()
        };
        let cmd = FFmpegCommandBuilder::try_build(&config, Path::new("ffmpeg")).unwrap();
        assert!(cmd.contains(&"-af".to_string()));
        let ca_idx = cmd.iter().position(|a| a == "-c:a").unwrap();
        assert_eq!(cmd[ca_idx + 1], "aac");
    }

    #[test]
    fn test_try_build_rejects_invalid_timecode_without_panicking() {
        let config = EncodeJobConfig {
            input_path: "in.mp4".to_string(),
            output_path: "out.mp4".to_string(),
            in_point: Some("not-a-timecode".to_string()),
            ..Default::default()
        };
        assert!(FFmpegCommandBuilder::try_build(&config, Path::new("ffmpeg")).is_err());
    }
}
