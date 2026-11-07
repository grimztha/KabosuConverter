# Kabosu Converter

<p align="center">
  <img src="assets/logo.png" alt="Kabosu Converter Logo" width="160" />
</p>

<p align="center">
  <strong>High-performance, modern video & audio transcoding studio and universal stream downloader.</strong><br>
  Built from scratch in <strong>Rust</strong> and <strong>Slint</strong>, powered by local <strong>FFmpeg 8+</strong> and <strong>yt-dlp</strong> binaries.
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPLv3-green.svg" alt="License: GPLv3"></a>
  <a href="https://github.com/grimztha/KabosuConverter/actions"><img src="https://img.shields.io/github/actions/workflow/status/grimztha/KabosuConverter/ci.yml?branch=main&label=CI" alt="CI Status"></a>
  <a href="https://github.com/grimztha/KabosuConverter/releases"><img src="https://img.shields.io/github/v/release/grimztha/KabosuConverter?color=brightgreen&label=Release" alt="Latest Release"></a>
  <a href="https://slint.dev"><img src="https://img.shields.io/badge/GUI-Slint%201.18-blueviolet.svg" alt="Slint GUI"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/Rust-2021%20Edition-orange.svg" alt="Rust 2021"></a>
</p>

---

## Overview

**Kabosu Converter** is a desktop application engineered in native Rust to replace bloated, electron-based, or JVM-dependent transcoders with an ultra-responsive, memory-safe, hardware-accelerated transcoding studio. 

Featuring an intuitive **Studio Dark** aesthetic with vibrant lime-green accents, Kabosu Converter pairs real-time visual scrubbing, interactive video cropping, audio normalization, and frame-accurate timeline trimming with professional intermediate codecs (ProRes, DNxHR), lossless container remuxing, and a universal web stream downloader.

---

## Highlights & Key Features

### 🎬 Media Player & Timeline Studio
- **Dual Visual Monitor**:
  - **Video Screen**: Real-time debounced frame extraction at the playhead position with timecode display, playback status, and aspect-ratio preservation.
  - **Audio Screen**: Multi-band dynamic equalizer visualizer with glowing amplitude bars and large timecode indicator.
- **Interactive Timeline Scrubber**:
  - Precision timeline scrubber with real-time needle cursor.
  - Glowing visual range highlighting between **In Point** and **Out Point**.
  - Multi-tier timecode display: Playhead position, Trimmed Span duration (`In → Out`), and Total Duration.
- **Dedicated Trimming Controls**:
  - `[ Set In ]` and `[ Set Out ]` cut boundaries with single-click precision.
  - `[ ◀ Jump In ]` and `[ Jump Out ▶ ]` playhead navigation.
  - `[ ✂ Preview Trim ]` to audition the exact trimmed cut.
  - Frame-accurate micro-stepping (`-5s`, `-1s`, `-0.1s`, `+0.1s`, `+1s`, `+5s`).
  - Two-way timecode synchronization: manual timestamp editing immediately updates timeline markers.
- **Instant Lossless Export**:
  - `[ ✂ Lossless Cut Now ]`: Exports selected range without re-encoding in milliseconds.
  - `[ ⚙ Codec Settings → ]`: Transfers the trimmed clip to the Transcoder tab with active in/out points automatically applied.
- **Native Player Integration**: Launches full-speed GPU accelerated preview with native audio playback via bundled `ffplay`.

### 📐 Interactive Video Crop & Geometry Studio
- **On-Canvas Interactive Crop Box**:
  - 8 draggable resize handles for fluid, responsive crop boundary adjustment.
  - Fully draggable crop zone with real-time coordinate and dimension updates.
  - Keyboard arrow nudge mode with Center alignment button.
- **Aspect Ratio Lock Presets**:
  - `Free (Unlocked)`, `16:9`, `9:16`, `4:3`, `3:4`, `1:1`, `2:3`, `3:2`, `21:9`, `4:5`.
- **Rotation & Mirroring**:
  - 90°, -90°, and 180° instant video rotation.
  - Horizontal flip and vertical mirror controls.

### 🔊 Studio Audio Editing & Broadcast Normalization
- **Gain & Attenuation**: Adjustable volume control from `-24 dB` to `+24 dB` with stepped quick-presets (`-12dB`, `-6dB`, `0dB`, `+3dB`, `+6dB`, `+12dB`, `±1.0dB`).
- **Broadcast Loudness Normalization (EBU R128 / LUFS)**: Integrated multi-pass `loudnorm` filter with broadcast-standard profiles:
  - **EBU R128** (-23 LUFS)
  - **YouTube / Spotify** (-14 LUFS)
  - **Apple Podcasts** (-16 LUFS)
  - **Loud Web** (-12 LUFS)
- **Live Stereo VU Peak Meters**: Dual-channel (L/R) dynamic peak level visualization calibrated from `-60 dB` to `0 dB`.

### ⚡ Professional Transcoding & Lossless Remuxing
- **Modern Distribution Codecs**:
  - **H.264 (AVC)**: Multi-threaded `libx264`, NVIDIA NVENC (`h264_nvenc`), Intel QSV, AMD AMF with CRF, CBR, and VBR rate controls.
  - **H.265 (HEVC)**: `libx265` and `hevc_nvenc` with Apple QuickTime compatibility tagging (`-tag:v hvc1`).
  - **AV1**: `libsvtav1` and `av1_nvenc` with 10-bit color depth (`yuv420p10le`).
  - **VP9**: `libvpx-vp9` with row-based multi-threading for web delivery.
- **Post-Production & Intermediate Workflows**:
  - **Apple ProRes**: Full profile suite via `prores_ks` (Proxy, LT, Standard 422, HQ, 4444 with alpha, 4444 XQ).
  - **Avid DNxHD & DNxHR**: High-bitrate broadcast editing formats (`dnxhr_hq`, `dnxhd`).
- **Lossless Stream Operations (Zero Quality Loss)**:
  - **Rewrap / Remux**: Instantly swap containers between `.mp4`, `.mkv`, `.mov`, `.webm`, `.avi`, and `.ts` without re-encoding (`-c copy`).
  - **Stream Demuxing**: Isolate video streams, extract raw or converted audio tracks, or demux subtitles.
  - **Audio Track Replacement**: Swap audio tracks into an existing video without touching video streams.

### 🌐 Universal Media Downloader (`yt-dlp`)
- **Direct Stream Inspection**: Paste stream URLs to discover all available formats, video resolutions, video/audio codecs, and standalone audio tracks.
- **Supported Platforms**: YouTube, Twitch, Vimeo, TikTok, Bilibili, SoundCloud, X / Twitter, direct HLS / M3U8 livestream playlists.
- **Format Selection**: Best Video + Audio muxing, 4K/1080p MP4/MKV presets, or standalone high-quality audio extraction (MP3, WAV, FLAC, Opus).
- **Integrated Workflow**: One-click download transfer directly to the Player/Trimmer or the Queue for instant editing and transcoding.

### 📦 Self-Contained Binary Management
- **Zero-Config Deployment**: The application runs portably without requiring manual installation of external dependencies.
- **Three-Tier Binary Discovery**:
  1. Checks App-Local directory:
     - **Windows**: `%LOCALAPPDATA%\Kabosu\bin\`
     - **macOS**: `~/Library/Application Support/Kabosu/bin/`
     - **Linux**: `~/.local/share/kabosu/bin/`
  2. Fallback to system `$PATH` for custom installations.
  3. Automatic streaming background download strictly into the app-local folder if binaries are missing.
- **SHA-256 Integrity Verification**: Every binary download is verified against official checksum manifests before extraction.
- **Clean Uninstallation**: All settings, caches, and downloaded tools reside inside the local app data folder; deleting the Kabosu folder leaves your system completely untouched.

---

## Tech Stack

| Component | Technology | Description |
|:---|:---|:---|
| **Core Language** | [Rust](https://www.rust-lang.org/) (2021 Edition) | Memory-safe, concurrency-driven, zero-overhead systems language |
| **GUI Framework** | [Slint](https://slint.dev/) (v1.18+) | Native declarative GUI compiled directly to native code with GPU rendering |
| **Async Runtime** | [Tokio](https://tokio.rs/) (v1.46+) | Multi-threaded async engine powering non-blocking I/O, process management, and streaming |
| **Media Processing** | [FFmpeg](https://ffmpeg.org/) (8+) | High-performance encoding, decoding, filtering, and stream extraction engine |
| **Media Inspection** | [FFprobe](https://ffmpeg.org/ffprobe.html) | JSON-based deep container and stream metadata introspection |
| **Media Preview** | [FFplay](https://ffmpeg.org/ffplay.html) | Synchronized hardware-accelerated playback and audio preview |
| **Stream Downloader** | [yt-dlp](https://github.com/yt-dlp/yt-dlp) | Industry-standard web video extraction and format discovery tool |
| **Native Windows API** | `windows-sys` (v0.61) | Native shell integration, IDropTarget OLE drag-and-drop, and window styling |
| **Binary Compression** | `zip`, `tar`, `xz2` | Multi-format archive extraction for automated binary provisioning |
| **CI/CD Automation** | GitHub Actions | Automated multi-platform testing and tagged release binary packaging |

---

## Project Structure

```
KabosuConverter/
├── .github/workflows/        # CI testing & multi-platform release automation
│   ├── ci.yml               # Automated tests on Windows, macOS, Linux
│   └── release.yml          # Tagged release packaging (.zip, .tar.gz, SHA-256)
├── assets/                  # High-resolution logos, ICO icons, and favicons
├── src/
│   ├── binaries/            # Automated binary download, verification, extraction
│   │   ├── extract.rs       # Archive decompression (.zip, .tar.xz)
│   │   ├── fetcher.rs       # Streaming HTTP client with SHA-256 validation
│   │   ├── manifest.rs      # Dependency state tracking and checksum manifests
│   │   └── platform.rs     # OS and CPU architecture detection
│   ├── ffmpeg/              # Type-safe FFmpeg process runner & command builder
│   │   ├── builder.rs       # CLI flag generator for all encoding modes
│   │   ├── locator.rs       # Multi-tier binary resolution and smoke testing
│   │   ├── preview.rs       # Debounced frame extraction and audio playback
│   │   ├── probe.rs         # Structured media metadata parsing
│   │   └── runner.rs        # Asynchronous process runner and progress parser
│   ├── handlers/            # UI event handlers and bridge controllers
│   │   ├── downloader.rs    # yt-dlp format fetching and download execution
│   │   ├── editor.rs        # Video crop, rotation, flip, and geometry handlers
│   │   ├── encoder.rs       # Transcoding queue coordinator and execution
│   │   ├── playback.rs      # Playhead tracking, scrub, and volume controls
│   │   ├── queue.rs         # Batch file queue management and ordering
│   │   └── timeline.rs      # In/out trimming points and range synchronization
│   ├── ui/                  # Declarative Slint UI layout and components
│   │   ├── components/      # Reusable controls (buttons, meters, inputs, switches)
│   │   ├── icons/           # Studio vector SVG icons and app branding
│   │   ├── timeline/        # Waveform visualizer, grid, and timeline scrubbers
│   │   ├── views/           # Player tab, Transcoder tab, Downloader tab, Crop overlay
│   │   ├── main.slint       # Top-level window assembly and layout
│   │   └── theme.slint      # Studio Dark palette and typography tokens
│   ├── context.rs           # Shared application state
│   ├── dialogs.rs           # Native OS file and directory picker dialogs
│   ├── lib.rs               # Library root and component exports
│   ├── main.rs              # Application entry point and callback registrations
│   ├── settings.rs          # Persistent JSON user preferences
│   ├── win_drag_drop.rs     # Windows native OLE drag-and-drop integration
│   └── ytdlp.rs             # Safe wrapper for yt-dlp queries and downloads
├── tests/                   # Integration tests (FFmpeg, Downloader, UI, Settings)
├── build.rs                 # Slint compilation and Windows PE resource embedding
├── Cargo.toml               # Package dependencies, metadata, and optimization profiles
└── LICENSE                  # GNU General Public License v3.0
```

---

## Building from Source

### Prerequisites

- **Rust** (1.80+ or latest stable): [Install Rust](https://rustup.rs/)
- **C/C++ Build Tools**:
  - Windows: Visual Studio C++ Build Tools or MSVC toolchain
  - Linux: `libfontconfig1-dev`, `libfreetype6-dev`, `libx11-dev`, `libxkbcommon-dev`, `libasound2-dev`
  - macOS: Xcode Command Line Tools (`xcode-select --install`)

### Development Build

```bash
# Clone the repository
git clone https://github.com/grimztha/KabosuConverter.git
cd KabosuConverter

# Run in debug mode
cargo run
```

### Running Tests

```bash
cargo test
```

### Optimized Production Release Build

```bash
cargo build --release
```

The optimized standalone binary will be generated at:
- **Windows**: `target/release/kabosu-converter.exe`
- **macOS / Linux**: `target/release/kabosu-converter`

---

## Prebuilt Binaries

Precompiled standalone packages are automatically built and published for every release via GitHub Actions:
- **Windows x64** (`.zip`)
- **macOS Apple Silicon & Intel** (`.tar.gz`)
- **Linux x64** (`.tar.gz`)

Check the [Releases](https://github.com/grimztha/KabosuConverter/releases) page for the latest version.

---

## License

This project is open-source software licensed under the **[GNU General Public License v3.0](LICENSE)**.
