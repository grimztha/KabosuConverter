# Implementation Plan: Another Encoder

## Overview
"Another Encoder" is a professional video & audio transcode, remux, cut, merge, and analysis desktop application built from scratch in Rust, Slint, and FFmpeg. It replicates the core capabilities and workflows of Shutter Encoder with the performance, safety, and modern desktop feel of a native Rust/Slint application on Windows.

## Architecture & Technology Stack
- **Language & Runtime**: Rust (edition 2021) with Tokio for async child process execution.
- **GUI Framework**: Slint 1.18+ with a custom sleek dark theme tailored for pro media workflows.
- **Media Engine**: Local `ffmpeg.exe` and `ffprobe.exe` binaries with real-time stderr parsing.
- **Configuration**: Serde JSON configuration stored in `%APPDATA%/another-encoder/settings.json`.
- **File Dialogs**: Native Windows file and folder pickers via `rfd`.

## Key Modules
1. `src/ffmpeg/locator.rs`: Detection and validation of local or system `ffmpeg.exe`, `ffprobe.exe`, `ffplay.exe`.
2. `src/ffmpeg/probe.rs`: Deep inspection of video/audio containers, codecs, resolution, duration, framerate, channels.
3. `src/ffmpeg/builder.rs`: Deterministic FFmpeg CLI generator for:
   - Output Codecs: H.264 (x264/nvenc/qsv), H.265 (x265/hevc_nvenc), AV1 (svtav1/av1_nvenc), VP9.
   - Editing Codecs: Apple ProRes (Proxy, LT, Standard, HQ, 4444), Avid DNxHD/DNxHR.
   - Audio Codecs: AAC, MP3, Opus, FLAC, AC3, WAV (PCM 16/24-bit).
   - Direct Stream / Lossless: Rewrap / Remux container, Lossless Cut, Extract Streams, Replace Audio, Merge / Concat.
   - Filters & Enhancements: Resolution scaling, conform framerate, deinterlace, audio gain, EBU R128 loudness normalization.
4. `src/ffmpeg/runner.rs`: Process lifecycle, non-blocking stderr line reading, timecode/progress calculation, speed/fps tracking, cancel/kill tokens.
5. `src/settings.rs`: Persistent application and encoder preferences.
6. `src/ui/main.slint`: High-aesthetic Slint UI with file queue, function categories, codec parameters, audio settings, progress bar, and log drawer.
7. `src/main.rs`: Slint UI event wiring, async runner dispatch, state synchronization.

## Verification Checkpoints
- Unit tests for command builder, timecode parsing, probe data extraction, and regex progress parsing.
- Slint UI compilation verification (`cargo check`).
- Live transcode verification using test media generation with local `ffmpeg.exe`.
- End-to-end integration and user interface verification.
