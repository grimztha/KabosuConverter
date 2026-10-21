# Tasks: Another Encoder

## Phase 1: FFmpeg Engine & Media Intelligence
- [x] Task 1.1: Binary locator (`src/ffmpeg/locator.rs`) to detect local & system `ffmpeg.exe`, `ffprobe.exe`.
- [x] Task 1.2: Media metadata probe engine (`src/ffmpeg/probe.rs`) parsing video/audio codecs, resolution, duration, bitrate, frame rate.
- [x] Task 1.3: Type-safe FFmpeg CLI command builder (`src/ffmpeg/builder.rs`) supporting all modes: Transcode (H.264, H.265, AV1, ProRes, DNxHD, VP9), Rewrap, Cut, Extract, Audio Normalization, Merge, Replace Audio.
- [x] Task 1.4: Real-time asynchronous process runner (`src/ffmpeg/runner.rs`) with progress percentage, FPS, bitrate, speed parsing and job cancellation.
- [x] Task 1.5: Unit tests for locator, probe parsing, command builder, and progress parsing (`cargo test` - 11/11 passed).

## Phase 2: Configuration & State Management
- [x] Task 2.1: Rich settings schema (`src/settings.rs`) for encoder presets, hardware acceleration, default output folder, audio normalization defaults.
- [x] Task 2.2: Queue management logic integrated in UI model for multiple input files, statuses, and batch progression.

## Phase 3: Slint UI Design & Pro Aesthetics
- [x] Task 3.1: Design system in Slint (`src/ui/main.slint`) with modern dark theme palette, typography, badges, custom buttons, and smooth sliders.
- [x] Task 3.2: Input Files queue component with multi-file list, remove item, clear, file details (resolution, duration, codec, file size).
- [x] Task 3.3: Function & Preset selector with categorized functions (Output Codecs, Editing Codecs, Audio Conversion, Lossless Remux, Cut, Extract, Merge, Normalize).
- [x] Task 3.4: Video settings panel (Codec, Hardware Accel, Rate Control CRF/Bitrate, Preset, Scaling, Framerate, Deinterlace).
- [x] Task 3.5: Audio settings panel (Audio Codec, Bitrate, Channels, Sample Rate, Normalization LUFS).
- [x] Task 3.6: Cut & Trim controls (In Point, Out Point) and Output destination selection.
- [x] Task 3.7: Progress bar, stats indicators (speed, ETA, elapsed), log drawer, Start and Cancel actions.

## Phase 4: Integration & Wire-Up
- [x] Task 4.1: Connect Slint UI callbacks in `src/main.rs` with native file pickers (`rfd`), media probe, and async queue runner.
- [x] Task 4.2: Real-time UI updates via Slint `slint::invoke_from_event_loop`.
- [x] Task 4.3: Open destination folder / play output file.

## Phase 5: Verification, Testing & Polish
- [x] Task 5.1: Run all unit tests and verify they pass (`cargo test`).
- [x] Task 5.2: Test actual encoding with synthetic media created with `ffmpeg.exe`.
- [x] Task 5.3: Build release binary and verify startup and performance (`target/release/another-encoder.exe`).

## Phase 6: Media Player & Timeline Trimming Studio
- [x] Task 6.1: Dual Video/Audio Player screen in Slint (`src/ui/main.slint`) with dynamic frame canvas, audio equalizer visualizer, and timecode overlays.
- [x] Task 6.2: Interactive timeline scrubber with visual trimmed range highlight, in-point/out-point markers, and step buttons (`-5s`, `-1s`, `-0.1s`, `+0.1s`, `+1s`, `+5s`).
- [x] Task 6.3: Asynchronous frame preview extraction engine (`src/ffmpeg/preview.rs`) using background worker with request deduplication and temporary file cleanup.
- [x] Task 6.4: Two-way synchronization between visual timeline markers, playhead time, and text inputs for In & Out points.
- [x] Task 6.5: Native FFplay integration for high-performance external playback and previewing trimmed ranges.
- [x] Task 6.6: Integration and unit tests for timecode parsing and preview frame extraction (13/13 tests passing).
