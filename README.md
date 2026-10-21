# Another Encoder

A high-performance, safe, modern video and audio transcoding & processing desktop application built from scratch in **Rust** and **Slint**, powered by local **FFmpeg 7+ / 8+** binaries.

Inspired by the comprehensive capabilities of Shutter Encoder, **Another Encoder** is engineered in native Rust to eliminate Java runtime dependencies, garbage collection stalls, and dated UI paradigms, providing an ultra-responsive Studio Dark interface with real-time hardware encoding, metadata inspection, and lossless stream manipulations.

---

## Key Features

### 1. High-Performance Output Codecs
- **H.264 (AVC)**: Multi-threaded `libx264`, NVIDIA NVENC (`h264_nvenc`), Intel QSV, AMD AMF with CRF, CBR, and VBR rate controls.
- **H.265 (HEVC)**: `libx265` and `hevc_nvenc` with Apple QuickTime compatibility tag (`-tag:v hvc1`).
- **AV1**: `libsvtav1` and `av1_nvenc` with 10-bit color depth (`yuv420p10le`).
- **VP9**: `libvpx-vp9` with row-based multi-threading for web distribution.

### 2. Professional Editing & Intermediate Codecs
- **Apple ProRes**: Full profile support via `prores_ks` (Proxy, LT, Standard 422, HQ, 4444 with alpha, 4444 XQ).
- **Avid DNxHD & DNxHR**: High-bitrate broadcast intermediate workflows (`dnxhr_hq`, `dnxhd`).

### 3. Lossless Stream Remuxing & Cutting (Instant, Zero Quality Loss)
- **Rewrap / Remux**: Swap containers between `.mp4`, `.mkv`, `.mov`, `.webm`, `.avi`, `.ts` in milliseconds without re-encoding (`-c copy`).
- **Lossless Cut / Trimming**: Frame-accurate keyframe trimming with `-ss` and `-to` using `-avoid_negative_ts make_zero`.
- **Stream Extraction**: Isolate video streams without audio/subs, extract raw or converted audio tracks, or demux subtitles.
- **Audio Track Replacement**: Replace audio track in a video with an external audio file without re-encoding video.

### 4. Studio Audio Processing & Broadcast Normalization
- **Audio Codecs**: AAC, MP3 (`libmp3lame`), Opus, FLAC (Lossless), WAV (PCM 24-bit / 16-bit), AC3.
- **Loudness Normalization (EBU R128 / LUFS)**: Built-in `loudnorm` filter supporting EBU R128 (-23 LUFS), YouTube / Spotify (-14 LUFS), Apple Podcasts (-16 LUFS), and Loud Web (-12 LUFS).

### 5. Dedicated Media Player & Timeline Trimming Studio
- **Workspace Navigation Tabs**:
  - **`[ 🎬 Media Player & Trimmer ]`**: Primary studio workspace dedicated to previewing, scrubbing, frame stepping, and trimming media.
  - **`[ ⚙️ Transcoder & Codec Settings ]`**: Detailed encoder controls for hardware acceleration, bitrates, CRF, filters, and container formats with automatic active trim reflection.
- **Dual Audio & Video Player Monitor**:
  - **Video Screen**: Real-time debounced frame extraction at the playhead position with timecode watermark, playback status pill, and aspect-ratio preservation.
  - **Audio Screen**: Multi-band dynamic equalizer visualizer with glowing amplitude bars, track information, and large timecode display.
- **Interactive Timeline Scrubber & Range Highlighting**:
  - Precision timeline scrubber with real-time playhead cursor needle.
  - Visual glowing amber/emerald range highlight between In Point and Out Point showing the exact trimmed region.
  - Multi-tier timecode display: Playhead position, Trimmed Span duration (`In → Out`), and Total Media duration.
- **Dedicated Trimming Controls**:
  - **`[ Set In (Start) ]` & `[ Set Out (End) ]`**: Mark cut boundaries at the playhead position with a single click.
  - **`[ ◀ Jump In ]` & `[ Jump Out ▶ ]`**: Instantly jump playhead to In or Out points.
  - **`[ ✂ Preview Trim ]`**: Audition only the selected trimmed segment.
  - **`[ ↺ Reset ]`**: Clear trim points back to full media duration.
  - **Precision Stepping**: Frame-accurate stepping (`-5s`, `-1s`, `-0.1s`, `+0.1s`, `+1s`, `+5s`).
  - **Two-Way Sync**: Direct text input in timecode fields (`HH:MM:SS.mmm`) instantly synchronizes with the visual timeline markers.
- **Instant Lossless Export**:
  - **`[ ✂ Lossless Cut Now ]`**: Immediately trims and saves the selected segment without re-encoding in milliseconds.
  - **`[ ⚙ Codec Settings → ]`**: Sends the trimmed media to the Transcoder tab for full transcoding with active trim applied.
- **Native FFplay Integration**: One-click launch to open full-speed GPU accelerated playback with native audio output via bundled `ffplay.exe`.

### 6. Media Intelligence & Real-Time Monitoring
- **Deep Metadata Inspection**: Automatically inspects width, height, framerate, duration, video codec, audio codec, channels, sample rate, and bitrates via bundled `ffprobe.exe`.
- **Live Performance Feedback**: Percentage progress, elapsed time / total duration, instant encoding speed multiplier (e.g. `3.2x`), encoding FPS, and instantaneous bitrate.
- **Log Stream Viewer**: Collapsible live console displaying exact generated FFmpeg commands and real-time stderr output.

---

## Architecture

- **`src/ffmpeg/locator.rs`**: Automatic detection and validation of bundled or system `ffmpeg.exe` and `ffprobe.exe`.
- **`src/ffmpeg/probe.rs`**: JSON-based media probe engine.
- **`src/ffmpeg/builder.rs`**: Type-safe FFmpeg CLI generator for all workflow modes.
- **`src/ffmpeg/runner.rs`**: Asynchronous child process runner with regex-based progress stream parsing and thread-safe cancellation.
- **`src/ui/main.slint`**: Declarative Slint UI styled with a modern dark theme, smooth sliders, custom badges, and responsive queue views.
- **`src/settings.rs`**: Configuration persistence to user app data.

---

## Building & Running

### Requirements
- Rust (1.80+ or latest stable)
- Slint 1.18+
- Windows x64 (or compatible platform)
- `ffmpeg.exe` and `ffprobe.exe` (included in project root)

### Development Run
```bash
cargo run
```

### Run Tests
```bash
cargo test
```

### Optimized Production Release Build
```bash
cargo build --release
```
The optimized standalone executable will be located at:
`target/release/another-encoder.exe`
