use std::path::Path;
use another_encoder::ffmpeg::{
    FFmpegCommandBuilder, EncodeJobConfig, EncoderFunction, VideoCodec,
    RateControlMode, AudioCodecChoice, ProResProfile, format_duration_hms, FFmpegBinaries,
    VideoRotation, ResolutionPreset
};

#[test]
fn test_duration_formatting() {
    assert_eq!(format_duration_hms(0.0), "00:00.000");
    assert_eq!(format_duration_hms(65.5), "01:05.500");
    assert_eq!(format_duration_hms(3665.123), "01:01:05.123");
}

#[test]
fn test_h264_command_generation() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mov".to_string(),
        output_path: "output.mp4".to_string(),
        video_codec: VideoCodec::H264,
        rate_control: RateControlMode::CRF,
        crf: 18,
        preset: "slow".to_string(),
        audio_codec: AudioCodecChoice::AAC,
        audio_bitrate_kbps: 256,
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"-c:v".to_string()));
    assert!(cmd.contains(&"libx264".to_string()));
    assert!(cmd.contains(&"-crf".to_string()));
    assert!(cmd.contains(&"18".to_string()));
    assert!(cmd.contains(&"-preset".to_string()));
    assert!(cmd.contains(&"slow".to_string()));
    assert!(cmd.contains(&"-c:a".to_string()));
    assert!(cmd.contains(&"aac".to_string()));
    assert!(cmd.contains(&"-b:a".to_string()));
    assert!(cmd.contains(&"256k".to_string()));
    assert!(cmd.contains(&"+faststart".to_string()));
}

#[test]
fn test_h265_apple_compat_tag() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mkv".to_string(),
        output_path: "output.mp4".to_string(),
        video_codec: VideoCodec::H265,
        crf: 22,
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"libx265".to_string()));
    assert!(cmd.contains(&"-tag:v".to_string()));
    assert!(cmd.contains(&"hvc1".to_string()));
}

#[test]
fn test_prores_profile_generation() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "raw.mov".to_string(),
        output_path: "master_hq.mov".to_string(),
        video_codec: VideoCodec::AppleProRes,
        prores_profile: ProResProfile::HQ,
        audio_codec: AudioCodecChoice::WavPCM24,
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"prores_ks".to_string()));
    assert!(cmd.contains(&"-profile:v".to_string()));
    assert!(cmd.contains(&"3".to_string())); // Profile 3 is ProRes HQ
    assert!(cmd.contains(&"yuv422p10le".to_string()));
    assert!(cmd.contains(&"pcm_s24le".to_string()));
}

#[test]
fn test_rewrap_command() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Rewrap,
        input_path: "movie.mkv".to_string(),
        output_path: "movie.mp4".to_string(),
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"-c".to_string()));
    assert!(cmd.contains(&"copy".to_string()));
    assert!(cmd.contains(&"-map".to_string()));
    assert!(cmd.contains(&"0".to_string()));
    assert!(cmd.contains(&"+faststart".to_string()));
}

#[test]
fn test_lossless_cut_command() {
    let config = EncodeJobConfig {
        function: EncoderFunction::LosslessCut,
        input_path: "gameplay.mp4".to_string(),
        output_path: "highlight.mp4".to_string(),
        in_point: Some("00:01:15.000".to_string()),
        out_point: Some("00:02:45.000".to_string()),
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"-ss".to_string()));
    assert!(cmd.contains(&"00:01:15.000".to_string()));
    assert!(cmd.contains(&"-to".to_string()));
    assert!(cmd.contains(&"00:02:45.000".to_string()));
    assert!(cmd.contains(&"-c".to_string()));
    assert!(cmd.contains(&"copy".to_string()));
    assert!(cmd.contains(&"make_zero".to_string()));
}

#[test]
fn test_extract_audio_command() {
    let config = EncodeJobConfig {
        function: EncoderFunction::ExtractAudio,
        input_path: "concert.mp4".to_string(),
        output_path: "audio.flac".to_string(),
        audio_codec: AudioCodecChoice::FLAC,
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"-vn".to_string()));
    assert!(cmd.contains(&"-c:a".to_string()));
    assert!(cmd.contains(&"flac".to_string()));
    assert!(cmd.contains(&"0:a:0".to_string()));
}

#[test]
fn test_audio_normalization_command() {
    let config = EncodeJobConfig {
        function: EncoderFunction::AudioNormalize,
        input_path: "interview.mp4".to_string(),
        output_path: "interview_normalized.mp4".to_string(),
        normalize_lufs: Some(-14.0),
        audio_codec: AudioCodecChoice::AAC,
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"-c:v".to_string()));
    assert!(cmd.contains(&"copy".to_string()));
    assert!(cmd.contains(&"-af".to_string()));
    let af_idx = cmd.iter().position(|r| r == "-af").unwrap();
    assert!(cmd[af_idx + 1].contains("loudnorm=I=-14.0:TP=-1.0:LRA=7.0"));
}

#[test]
fn test_binary_locator_workspace() {
    let binaries = FFmpegBinaries::locate().expect("Should locate ffmpeg & ffprobe in project root path");
    assert!(binaries.ffmpeg.exists(), "ffmpeg binary must exist in project root");
    assert!(binaries.ffprobe.exists(), "ffprobe binary must exist in project root");
    assert!(binaries.ffplay.is_some(), "ffplay binary must be found in project root");
    assert!(binaries.ffplay.as_ref().unwrap().exists(), "ffplay binary must exist");

    let ffmpeg_str = binaries.ffmpeg.to_string_lossy().to_lowercase();
    let ffprobe_str = binaries.ffprobe.to_string_lossy().to_lowercase();
    let ffplay_str = binaries.ffplay.as_ref().unwrap().to_string_lossy().to_lowercase();
    assert!(ffmpeg_str.contains("another_encoder"), "ffmpeg must be in another_encoder project root");
    assert!(ffprobe_str.contains("another_encoder"), "ffprobe must be in another_encoder project root");
    assert!(ffplay_str.contains("another_encoder"), "ffplay must be in another_encoder project root");
}

#[test]
fn test_live_probe_synthetic() {
    let binaries = FFmpegBinaries::locate().unwrap();
    let synth = Path::new("test_synthetic.mp4");
    if synth.exists() {
        let meta = another_encoder::ffmpeg::probe_file(synth, &binaries.ffprobe).unwrap();
        assert_eq!(meta.primary_video().unwrap().width, 1280);
        assert_eq!(meta.primary_video().unwrap().height, 720);
        assert_eq!(meta.primary_video().unwrap().codec, "h264");
        assert_eq!(meta.primary_audio().unwrap().codec, "aac");
        assert!(meta.duration_seconds >= 2.9);
    }
}

#[test]
fn test_live_transcode_synthetic_hevc() {
    let binaries = FFmpegBinaries::locate().unwrap();
    let synth = Path::new("test_synthetic.mp4");
    if synth.exists() {
        let out = Path::new("test_synthetic_hevc.mp4");
        if out.exists() {
            let _ = std::fs::remove_file(out);
        }

        let config = EncodeJobConfig {
            function: EncoderFunction::Transcode,
            input_path: synth.to_string_lossy().to_string(),
            output_path: out.to_string_lossy().to_string(),
            video_codec: VideoCodec::H265,
            crf: 28,
            preset: "ultrafast".to_string(),
            audio_codec: AudioCodecChoice::AAC,
            ..Default::default()
        };

        let cmd = FFmpegCommandBuilder::build(&config, &binaries.ffmpeg);
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let mut progress_count = 0;
        let res = another_encoder::ffmpeg::FFmpegRunner::run_with_progress(
            &cmd,
            3.0,
            cancel,
            |_p| {
                progress_count += 1;
            },
            |_l| {},
        );

        assert!(res.is_ok(), "Live transcode should succeed: {:?}", res);
        assert!(out.exists(), "Output file must be created");
        assert!(progress_count > 0, "Progress events must have fired");

        let _ = std::fs::remove_file(out);
    }
}

#[test]
fn test_parse_timecode() {
    use another_encoder::ffmpeg::parse_timecode_to_seconds;

    assert_eq!(parse_timecode_to_seconds("00:00:10.000"), Some(10.0));
    assert_eq!(parse_timecode_to_seconds("01:30.500"), Some(90.5));
    assert_eq!(parse_timecode_to_seconds("01:02:03"), Some(3723.0));
    assert_eq!(parse_timecode_to_seconds("45"), Some(45.0));
    assert_eq!(parse_timecode_to_seconds("   12.345 "), Some(12.345));
    assert_eq!(parse_timecode_to_seconds(""), None);
    assert_eq!(parse_timecode_to_seconds("invalid"), None);
}

#[test]
fn test_extract_frame_preview() {
    use another_encoder::ffmpeg::{extract_frame, FFmpegBinaries};

    let binaries = FFmpegBinaries::locate().unwrap();
    let synth = Path::new("test_synthetic.mp4");
    if synth.exists() {
        let out_img = Path::new("test_preview_thumb.jpg");
        if out_img.exists() {
            let _ = std::fs::remove_file(out_img);
        }

        let res = extract_frame(&binaries.ffmpeg, &synth.to_string_lossy(), 1.0, out_img);
        assert!(res.is_ok(), "Frame extraction should succeed: {:?}", res);
        assert!(out_img.exists(), "Extracted preview image should exist");
        assert!(out_img.metadata().unwrap().len() > 100, "Image should contain byte data");

        let _ = std::fs::remove_file(out_img);
    }
}

#[test]
fn test_waveform_rgba_extraction() {
    use another_encoder::ffmpeg::{extract_waveform_rgba, FFmpegBinaries};

    if let Ok(binaries) = FFmpegBinaries::locate() {
        let test_wav = Path::new("target/test_waveform_input.wav");
        let mut cmd = std::process::Command::new(&binaries.ffmpeg);
        cmd.args(["-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-y"])
            .arg(test_wav);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        if let Ok(status) = cmd.status() {
            if status.success() {
                let res = extract_waveform_rgba(&binaries.ffmpeg, &test_wav.to_string_lossy(), 400, 30);
                assert!(res.is_ok(), "Waveform extraction should succeed: {:?}", res.err());
                let bytes = res.unwrap();
                assert_eq!(bytes.len(), 400 * 30 * 4, "Waveform RGBA buffer must be width*height*4");
                let _ = std::fs::remove_file(test_wav);
            }
        }
    }
}

#[test]
fn test_audio_playback_spawning() {
    use another_encoder::ffmpeg::{spawn_audio_playback, FFmpegBinaries};

    if let Ok(binaries) = FFmpegBinaries::locate() {
        if let Some(ref ffplay) = binaries.ffplay {
            let test_wav = Path::new("target/test_playback_input.wav");
            let mut cmd = std::process::Command::new(&binaries.ffmpeg);
            cmd.args(["-f", "lavfi", "-i", "sine=frequency=440:duration=1", "-y"])
                .arg(test_wav);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            if let Ok(status) = cmd.status() {
                if status.success() {
                    let child = spawn_audio_playback(ffplay, &test_wav.to_string_lossy(), 0.0, Some(0.2), 80, Some(3.0), Some(-16.0));
                    assert!(child.is_ok(), "spawn_audio_playback with gain and normalization filters should succeed");
                    if let Ok(mut c) = child {
                        let _ = c.kill();
                    }
                    let _ = std::fs::remove_file(test_wav);
                }
            }
        }
    }
}

#[test]
fn test_parse_ffplay_pts() {
    use another_encoder::ffmpeg::parse_ffplay_pts;

    // Standard playback line
    let line1 = "   3.45 M-A:  0.000 fd=   0 aq=    9KB vq=    0KB sq=    0B";
    assert_eq!(parse_ffplay_pts(line1), Some(3.45));

    // Fast-forward / high-timestamp line
    let line2 = " 1234.56 M-A: -0.001 fd=   0 aq=    8KB vq=    0KB sq=    0B";
    assert_eq!(parse_ffplay_pts(line2), Some(1234.56));

    // Zero timestamp
    let line3 = "   0.00 M-A:  0.000 fd=   0 aq=    9KB vq=    0KB sq=    0B";
    assert_eq!(parse_ffplay_pts(line3), Some(0.0));

    // Negative initial AAC priming timestamp (clamped to 0.0)
    let line4 = "  -0.04 M-A:  0.000 fd=   0 aq=    9KB vq=    0KB sq=    0B";
    assert_eq!(parse_ffplay_pts(line4), Some(0.0));

    // Uninitialized nan output
    let line5 = " nan M-A:    nan fd=   0 aq=    7KB vq=    0KB sq=    0B";
    assert_eq!(parse_ffplay_pts(line5), None);

    // Stderr banner lines without M-A:
    assert_eq!(parse_ffplay_pts("ffplay version N-127696..."), None);
    assert_eq!(parse_ffplay_pts("Input #0, mov,mp4,m4a,3gp..."), None);

    // Line with leading carriage return and noise
    let line6 = "\r[mov,mp4 @ 0x123] demuxer skip\r   5.12 M-A:  0.000";
    assert_eq!(parse_ffplay_pts(line6), Some(5.12));
}

#[test]
fn test_ffplay_clock_reader_and_session() {
    use another_encoder::ffmpeg::preview::read_ffplay_clock;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::io::Cursor;

    let simulated_stderr = b"ffplay version ...\r\n\
                             nan M-A:    nan fd=   0\r\
                             -0.03 M-A:  0.000 fd=   0\r\
                              0.05 M-A:  0.000 fd=   0\r\
                              0.10 M-A:  0.000 fd=   0\r\
                              0.25 M-A:  0.000 fd=   0\r";

    let clock = Arc::new(AtomicU64::new(u64::MAX));
    read_ffplay_clock(Cursor::new(simulated_stderr), clock.clone());

    let final_val = clock.load(Ordering::Acquire);
    assert_ne!(final_val, u64::MAX);
    let pts = (final_val as f64) / 1_000_000.0;
    assert!((pts - 0.25).abs() < 1e-4, "Clock should have updated to 0.25s, got: {}", pts);
}

#[test]
fn test_live_audio_clock_tracking() {
    use another_encoder::ffmpeg::{spawn_audio_playback, FFmpegBinaries};

    if let Ok(binaries) = FFmpegBinaries::locate() {
        if let Some(ref ffplay) = binaries.ffplay {
            let test_wav = Path::new("target/test_live_audio_clock.wav");
            let mut cmd = std::process::Command::new(&binaries.ffmpeg);
            cmd.args(["-f", "lavfi", "-i", "sine=frequency=880:duration=2", "-y"])
                .arg(test_wav);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            if let Ok(status) = cmd.status() {
                if status.success() {
                    let mut session = spawn_audio_playback(ffplay, &test_wav.to_string_lossy(), 0.0, Some(0.6), 80, None, None)
                        .expect("spawn_audio_playback should succeed");

                    // Wait up to 600ms for ffplay to initialize and produce master clock PTS
                    let start = std::time::Instant::now();
                    let mut saw_pts = false;
                    while start.elapsed().as_millis() < 600 {
                        if let Some(pts) = session.get_pts() {
                            assert!(pts >= 0.0, "PTS must be non-negative");
                            saw_pts = true;
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    assert!(saw_pts, "Live ffplay audio session should update master clock with real-time PTS");

                    let _ = session.kill();
                    let _ = std::fs::remove_file(test_wav);
                }
            }
        }
    }
}

#[test]
fn test_av_sync_master_clock_playback() {
    use another_encoder::ffmpeg::{spawn_audio_playback, spawn_playback_stream_transformed, VideoRotation, FFmpegBinaries};
    use std::io::Read;

    if let Ok(binaries) = FFmpegBinaries::locate() {
        if let Some(ref ffplay) = binaries.ffplay {
            let test_media = Path::new("target/test_av_sync_media.mp4");
            let mut cmd = std::process::Command::new(&binaries.ffmpeg);
            cmd.args([
                "-f", "lavfi", "-i", "testsrc=duration=3:size=640x360:rate=24",
                "-f", "lavfi", "-i", "sine=frequency=440:duration=3",
                "-c:v", "libx264", "-pix_fmt", "yuv420p",
                "-c:a", "aac",
                "-y",
            ]).arg(test_media);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            if let Ok(status) = cmd.status() {
                if status.success() {
                    let media_str = test_media.to_string_lossy().to_string();
                    let start_sec = 1.0;

                    // 1. Spawn audio playback with master clock
                    let mut audio_session = spawn_audio_playback(ffplay, &media_str, start_sec, Some(1.0), 100, None, None)
                        .expect("Audio playback should spawn");

                    // 2. Spawn video decode stream
                    let mut video_child = spawn_playback_stream_transformed(
                        &binaries.ffmpeg,
                        &media_str,
                        start_sec,
                        VideoRotation::None,
                        false,
                        false,
                    ).expect("Video stream should spawn");

                    let mut stdout = video_child.stdout.take().expect("Should have stdout pipe");
                    let mut frame_buf = vec![0u8; 640 * 360 * 4];

                    // Read first video frame from pipe
                    let read_res = stdout.read_exact(&mut frame_buf);
                    assert!(read_res.is_ok(), "First decoded video frame should be read successfully");

                    // Verify audio clock advances around start_sec (1.0)
                    let start = std::time::Instant::now();
                    let mut got_audio_clock = false;
                    while start.elapsed().as_millis() < 800 {
                        if let Some(pts) = audio_session.get_pts() {
                            assert!((0.5..=2.5).contains(&pts), "Audio PTS should be near start_sec (1.0), got: {}", pts);
                            got_audio_clock = true;
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                    assert!(got_audio_clock, "Audio master clock should report PTS near seek target");

                    let _ = audio_session.kill();
                    let _ = video_child.kill();
                    let _ = video_child.wait();
                    let _ = std::fs::remove_file(test_media);
                }
            }
        }
    }
}

#[test]
fn test_extract_frame_rgba_accurate_seeking() {
    use another_encoder::ffmpeg::{extract_frame_rgba, FFmpegBinaries};

    if let Ok(binaries) = FFmpegBinaries::locate() {
        let test_vid = Path::new("target/test_seek_input.mp4");
        let mut cmd = std::process::Command::new(&binaries.ffmpeg);
        cmd.args([
            "-f", "lavfi",
            "-i", "testsrc=duration=4:size=640x360:rate=24",
            "-g", "48", // Keyframes every 2.0s
            "-c:v", "libx264",
            "-pix_fmt", "yuv420p",
            "-y",
        ]).arg(test_vid);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        if let Ok(status) = cmd.status() {
            if status.success() {
                // Test 1: Extract at 0.5s (short seek, target <= 2.0)
                let frame_05 = extract_frame_rgba(&binaries.ffmpeg, &test_vid.to_string_lossy(), 0.5);
                assert!(frame_05.is_ok(), "extract_frame_rgba at 0.5s failed: {:?}", frame_05.err());
                let buf_05 = frame_05.unwrap();
                assert_eq!(buf_05.len(), 640 * 360 * 4);

                // Test 2: Extract at 3.0s (two-stage seek, target > 2.0)
                let frame_30 = extract_frame_rgba(&binaries.ffmpeg, &test_vid.to_string_lossy(), 3.0);
                assert!(frame_30.is_ok(), "extract_frame_rgba at 3.0s failed: {:?}", frame_30.err());
                let buf_30 = frame_30.unwrap();
                assert_eq!(buf_30.len(), 640 * 360 * 4);

                // Test 3: The two frames must not be identical (verifies accurate temporal decoding rather than snapping to 0.0s)
                assert_ne!(buf_05, buf_30, "Frame at 0.5s and 3.0s must have different visual contents");

                let _ = std::fs::remove_file(test_vid);
            }
        }
    }
}

#[test]
fn test_video_rotation_filters() {
    use another_encoder::ffmpeg::VideoRotation;

    assert_eq!(VideoRotation::from_degrees(0), VideoRotation::None);
    assert_eq!(VideoRotation::from_degrees(90), VideoRotation::Rotate90);
    assert_eq!(VideoRotation::from_degrees(-90), VideoRotation::RotateNeg90);
    assert_eq!(VideoRotation::from_degrees(270), VideoRotation::RotateNeg90);
    assert_eq!(VideoRotation::from_degrees(180), VideoRotation::Rotate180);

    assert_eq!(VideoRotation::None.effective_dimensions(1920, 1080), (1920, 1080));
    assert_eq!(VideoRotation::Rotate90.effective_dimensions(1920, 1080), (1080, 1920));
    assert_eq!(VideoRotation::RotateNeg90.effective_dimensions(1920, 1080), (1080, 1920));
    assert_eq!(VideoRotation::Rotate180.effective_dimensions(1920, 1080), (1920, 1080));

    assert_eq!(
        FFmpegCommandBuilder::build_transform_and_crop_filters(VideoRotation::None, false, false, None),
        Vec::<String>::new()
    );
    assert_eq!(
        FFmpegCommandBuilder::build_transform_and_crop_filters(VideoRotation::Rotate90, false, false, None),
        vec!["transpose=1".to_string()]
    );
    assert_eq!(
        FFmpegCommandBuilder::build_transform_and_crop_filters(VideoRotation::RotateNeg90, false, false, None),
        vec!["transpose=2".to_string()]
    );
    assert_eq!(
        FFmpegCommandBuilder::build_transform_and_crop_filters(VideoRotation::Rotate180, false, false, None),
        vec!["hflip".to_string(), "vflip".to_string()]
    );
    assert_eq!(
        FFmpegCommandBuilder::build_transform_and_crop_filters(VideoRotation::Rotate180, true, false, None),
        vec!["hflip".to_string(), "vflip".to_string(), "hflip".to_string()]
    );
}

#[test]
fn test_video_flip_filters() {
    use another_encoder::ffmpeg::VideoRotation;

    assert_eq!(
        FFmpegCommandBuilder::build_transform_and_crop_filters(VideoRotation::None, true, false, None),
        vec!["hflip".to_string()]
    );
    assert_eq!(
        FFmpegCommandBuilder::build_transform_and_crop_filters(VideoRotation::None, false, true, None),
        vec!["vflip".to_string()]
    );
    assert_eq!(
        FFmpegCommandBuilder::build_transform_and_crop_filters(VideoRotation::Rotate90, true, true, None),
        vec!["transpose=1".to_string(), "hflip".to_string(), "vflip".to_string()]
    );
}

#[test]
fn test_crop_rect_sanitization_and_nudge() {
    use another_encoder::ffmpeg::CropRect;

    // Odd values and out-of-bounds should be clamped and rounded to even numbers
    let raw = CropRect {
        x: 15,
        y: 23,
        width: 2001,
        height: 1205,
    };
    let sanitized = raw.sanitize(1920, 1080);
    assert_eq!(sanitized.width, 1920);
    assert_eq!(sanitized.height, 1080);
    assert_eq!(sanitized.x, 0);
    assert_eq!(sanitized.y, 0);

    // Sub-region crop
    let box_crop = CropRect {
        x: 101,
        y: 55,
        width: 801,
        height: 601,
    }
    .sanitize(1920, 1080);
    assert_eq!(box_crop.x, 100);
    assert_eq!(box_crop.y, 54);
    assert_eq!(box_crop.width, 800);
    assert_eq!(box_crop.height, 600);
    assert_eq!(box_crop.to_ffmpeg_filter(), "crop=800:600:100:54");

    // Arrow key nudging
    let nudged_right_down = box_crop.nudge(10, 20, 1920, 1080);
    assert_eq!(nudged_right_down.x, 110);
    assert_eq!(nudged_right_down.y, 74);

    // Nudging past top-left boundary clamps to (0, 0)
    let nudged_tl = box_crop.nudge(-500, -500, 1920, 1080);
    assert_eq!(nudged_tl.x, 0);
    assert_eq!(nudged_tl.y, 0);

    // Nudging past bottom-right boundary clamps to (1920 - 800, 1080 - 600) = (1120, 480)
    let nudged_br = box_crop.nudge(5000, 5000, 1920, 1080);
    assert_eq!(nudged_br.x, 1120);
    assert_eq!(nudged_br.y, 480);
}

#[test]
fn test_crop_aspect_ratio_presets() {
    use another_encoder::ffmpeg::CropAspectRatio;

    // 1. 9:16 preset inside 1920x1080 landscape
    let r_9_16 = CropAspectRatio::Ratio9x16.compute_centered_rect(1920, 1080);
    assert_eq!(r_9_16.height, 1080);
    assert_eq!(r_9_16.width, 608); // (1080 * 9 / 16) = 607.5 -> rounded & even-aligned 608
    assert_eq!(r_9_16.x, 656);
    assert_eq!(r_9_16.y, 0);

    // 2. 1:1 square preset inside 1920x1080 landscape
    let r_1_1 = CropAspectRatio::Ratio1x1.compute_centered_rect(1920, 1080);
    assert_eq!(r_1_1.width, 1080);
    assert_eq!(r_1_1.height, 1080);
    assert_eq!(r_1_1.x, 420);
    assert_eq!(r_1_1.y, 0);

    // 3. 4:3 preset inside 1920x1080 landscape
    let r_4_3 = CropAspectRatio::Ratio4x3.compute_centered_rect(1920, 1080);
    assert_eq!(r_4_3.width, 1440);
    assert_eq!(r_4_3.height, 1080);
    assert_eq!(r_4_3.x, 240);
    assert_eq!(r_4_3.y, 0);

    // 4. 2:3 preset inside 1920x1080 landscape
    let r_2_3 = CropAspectRatio::Ratio2x3.compute_centered_rect(1920, 1080);
    assert_eq!(r_2_3.width, 720);
    assert_eq!(r_2_3.height, 1080);
    assert_eq!(r_2_3.x, 600);
    assert_eq!(r_2_3.y, 0);

    // 5. 16:9 preset inside rotated 1080x1920 portrait video
    let r_16_9_portrait = CropAspectRatio::Ratio16x9.compute_centered_rect(1080, 1920);
    assert_eq!(r_16_9_portrait.width, 1080);
    assert_eq!(r_16_9_portrait.height, 608);
    assert_eq!(r_16_9_portrait.x, 0);
    assert_eq!(r_16_9_portrait.y, 656);
}

#[test]
fn test_combined_transform_crop_scale_filter_order() {
    use another_encoder::ffmpeg::{
        CropRect, DeinterlaceMode, ResolutionPreset, VideoRotation,
    };

    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mp4".to_string(),
        output_path: "output.mp4".to_string(),
        video_codec: VideoCodec::H264,
        deinterlace: DeinterlaceMode::Yadif,
        rotation: VideoRotation::Rotate90,
        flip_horizontal: true,
        flip_vertical: false,
        crop: Some(CropRect {
            x: 40,
            y: 80,
            width: 720,
            height: 1280,
        }),
        resolution: ResolutionPreset::HD720p,
        fps: Some(30.0),
        ..Default::default()
    };

    let cmd = FFmpegCommandBuilder::build(&config, Path::new("ffmpeg.exe"));
    let vf_idx = cmd.iter().position(|arg| arg == "-vf").expect("Must contain -vf");
    let vf_chain = &cmd[vf_idx + 1];
    assert_eq!(
        vf_chain,
        "yadif=0:-1:0,transpose=1,hflip,crop=720:1280:40:80,scale=w=1280:h=720:force_original_aspect_ratio=decrease,scale=trunc(iw/2)*2:trunc(ih/2)*2:flags=lanczos,fps=fps=30"
    );
}

#[test]
fn test_letterbox_viewport_calculation() {
    use another_encoder::ffmpeg::compute_letterbox_viewport;

    // 16:9 fills the entire 640x360 preview frame
    let (nx, ny, nw, nh) = compute_letterbox_viewport(1920, 1080);
    assert!((nx - 0.0).abs() < 1e-4);
    assert!((ny - 0.0).abs() < 1e-4);
    assert!((nw - 1.0).abs() < 1e-4);
    assert!((nh - 1.0).abs() < 1e-4);

    // 9:16 (e.g. rotated 1080x1920) is pillarboxed in 640x360: height=360, width=202.5
    let (px, py, pw, ph) = compute_letterbox_viewport(1080, 1920);
    assert!((py - 0.0).abs() < 1e-4);
    assert!((ph - 1.0).abs() < 1e-4);
    assert!((pw - (202.5 / 640.0)).abs() < 1e-3);
    assert!((px - ((1.0 - pw) * 0.5)).abs() < 1e-3);
}

#[test]
fn test_live_transcode_with_rotation_flip_and_crop() {
    use another_encoder::ffmpeg::{
        probe_file, CropRect, FFmpegBinaries, FFmpegRunner, VideoRotation,
    };

    let binaries = FFmpegBinaries::locate().expect("Must locate local project binaries");
    let test_in = Path::new("target/test_edit_transform_in.mp4");
    let test_out = Path::new("target/test_edit_transform_out.mp4");
    let _ = std::fs::remove_file(test_in);
    let _ = std::fs::remove_file(test_out);

    // Generate 1-second 640x360 synthetic video
    let mut cmd = std::process::Command::new(&binaries.ffmpeg);
    cmd.args([
        "-f",
        "lavfi",
        "-i",
        "testsrc=duration=1:size=640x360:rate=24",
        "-c:v",
        "libx264",
        "-pix_fmt",
        "yuv420p",
        "-y",
    ])
    .arg(test_in);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let status = cmd.status().expect("Failed to run local ffmpeg");
    assert!(status.success(), "Synthetic input generation must succeed");

    // Rotate 90° (640x360 -> 360x640), flip horizontally, and crop to 300x400 at (30, 60)
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: test_in.to_string_lossy().to_string(),
        output_path: test_out.to_string_lossy().to_string(),
        video_codec: VideoCodec::H264,
        crf: 24,
        preset: "ultrafast".to_string(),
        rotation: VideoRotation::Rotate90,
        flip_horizontal: true,
        flip_vertical: false,
        crop: Some(CropRect {
            x: 30,
            y: 60,
            width: 300,
            height: 400,
        }),
        ..Default::default()
    };

    let cmd_args = FFmpegCommandBuilder::build(&config, &binaries.ffmpeg);
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let res = FFmpegRunner::run_with_progress(&cmd_args, 1.0, cancel, |_| {}, |_| {});
    assert!(res.is_ok(), "Transformed & cropped transcode should succeed: {:?}", res);

    let probed = probe_file(test_out, &binaries.ffprobe).expect("Must probe output file");
    let v_stream = probed.primary_video().expect("Must have video stream");
    assert_eq!(v_stream.width, 300, "Cropped output width must be 300");
    assert_eq!(v_stream.height, 400, "Cropped output height must be 400");

    let _ = std::fs::remove_file(test_in);
    let _ = std::fs::remove_file(test_out);
}

#[test]
fn test_audio_gain_command_generation() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "test.mp4".to_string(),
        output_path: "test_gained.mp4".to_string(),
        audio_gain_db: Some(6.0),
        audio_codec: AudioCodecChoice::AAC,
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"-af".to_string()));
    let af_idx = cmd.iter().position(|r| r == "-af").unwrap();
    assert_eq!(cmd[af_idx + 1], "volume=6.0dB");
}

#[test]
fn test_audio_gain_and_normalize_combined() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "test.mp4".to_string(),
        output_path: "test_norm_gain.mp4".to_string(),
        audio_gain_db: Some(-3.5),
        normalize_lufs: Some(-16.0),
        audio_codec: AudioCodecChoice::AAC,
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"-af".to_string()));
    let af_idx = cmd.iter().position(|r| r == "-af").unwrap();
    assert_eq!(cmd[af_idx + 1], "volume=-3.5dB,loudnorm=I=-16.0:TP=-1.0:LRA=7.0");
}

#[test]
fn test_audio_filter_forces_reencode_when_copy_selected() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "test.mp4".to_string(),
        output_path: "test_auto_aac.mp4".to_string(),
        audio_gain_db: Some(4.0),
        audio_codec: AudioCodecChoice::Copy, // User selected Copy, but audio filter requires re-encoding
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"-af".to_string()));
    assert!(cmd.contains(&"-c:a".to_string()));
    let ca_idx = cmd.iter().position(|r| r == "-c:a").unwrap();
    assert_eq!(cmd[ca_idx + 1], "aac", "Copy should be automatically replaced with AAC when audio filters are active");
}

#[test]
fn test_video_filter_forces_reencode_when_copy_selected() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "test.mp4".to_string(),
        output_path: "test_auto_h264.mp4".to_string(),
        rotation: VideoRotation::Rotate90,
        video_codec: VideoCodec::Copy, // User selected Copy, but rotation filter requires re-encoding
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"-vf".to_string()));
    assert!(cmd.contains(&"-c:v".to_string()));
    let cv_idx = cmd.iter().position(|r| r == "-c:v").unwrap();
    assert_eq!(cmd[cv_idx + 1], "libx264", "Copy should be automatically replaced with libx264 when video filters are active to prevent streamcopy clash");
}

#[test]
fn test_scale_filter_aspect_ratio_bounding_box() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "portrait.mp4".to_string(),
        output_path: "scaled.mp4".to_string(),
        resolution: ResolutionPreset::FHD1080p,
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(cmd.contains(&"-vf".to_string()));
    let vf_idx = cmd.iter().position(|r| r == "-vf").unwrap();
    let filter = &cmd[vf_idx + 1];
    assert!(filter.contains("w=1920:h=1080"), "Scale filter must specify both width and height bounding dimensions: {}", filter);
    assert!(filter.contains("force_original_aspect_ratio=decrease"), "Scale filter must decrease aspect ratio to avoid distortion: {}", filter);
}

#[test]
fn test_audioless_video_skips_audio_filter_flag() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "silent_video.mp4".to_string(),
        output_path: "out.mp4".to_string(),
        audio_gain_db: None,
        normalize_lufs: None,
        ..Default::default()
    };

    let ffmpeg_bin = Path::new("ffmpeg.exe");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);

    assert!(!cmd.contains(&"-af".to_string()), "Commands without audio adjustments must not include -af");
}

#[test]
fn test_audio_playback_session_drop_safety() {
    use another_encoder::ffmpeg::AudioPlaybackSession;

    // Spawning a harmless dummy process to verify Drop cleans up without panic or hang
    let mut cmd = std::process::Command::new(if cfg!(windows) { "cmd.exe" } else { "sh" });
    if cfg!(windows) {
        cmd.args(["/c", "ping -n 5 127.0.0.1 >nul"]);
    } else {
        cmd.args(["-c", "sleep 5"]);
    }
    if let Ok(child) = cmd.spawn() {
        let session = AudioPlaybackSession::from_child(child);
        // Explicitly drop session - this must kill and wait on the child process cleanly
        drop(session);
    }
}





