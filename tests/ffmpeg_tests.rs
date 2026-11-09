use std::path::Path;
use kabosu_converter::ffmpeg::{
    FFmpegCommandBuilder, EncodeJobConfig, EncoderFunction, VideoCodec, HwAccel,
    RateControlMode, AudioCodecChoice, ProResProfile, format_duration_hms, FFmpegBinaries,
    VideoRotation, ResolutionPreset, parse_ffmpeg_version
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
        hw_accel: HwAccel::Cpu,
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
        hw_accel: HwAccel::Cpu,
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
        hw_accel: HwAccel::Cpu,
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
    if let Ok(binaries) = FFmpegBinaries::locate() {
        assert!(binaries.ffmpeg.exists(), "ffmpeg binary must exist in project root");
        assert!(binaries.ffprobe.exists(), "ffprobe binary must exist in project root");
        assert!(binaries.ffplay.is_some(), "ffplay binary must be found in project root");
        assert!(binaries.ffplay.as_ref().unwrap().exists(), "ffplay binary must exist");

        let ffmpeg_str = binaries.ffmpeg.to_string_lossy().to_lowercase();
        let ffprobe_str = binaries.ffprobe.to_string_lossy().to_lowercase();
        let ffplay_str = binaries.ffplay.as_ref().unwrap().to_string_lossy().to_lowercase();
        assert!(ffmpeg_str.contains("kabosu") || ffmpeg_str.contains("another_encoder"), "ffmpeg must be in project root");
        assert!(ffprobe_str.contains("kabosu") || ffprobe_str.contains("another_encoder"), "ffprobe must be in project root");
        assert!(ffplay_str.contains("kabosu") || ffplay_str.contains("another_encoder"), "ffplay must be in project root");
    }
}

#[test]
fn test_live_probe_synthetic() {
    if let Ok(binaries) = FFmpegBinaries::locate() {
        let synth = Path::new("test_synthetic.mp4");
    if synth.exists() {
        let meta = kabosu_converter::ffmpeg::probe_file(synth, &binaries.ffprobe).unwrap();
        assert_eq!(meta.primary_video().unwrap().width, 1280);
        assert_eq!(meta.primary_video().unwrap().height, 720);
        assert_eq!(meta.primary_video().unwrap().codec, "h264");
        assert_eq!(meta.primary_audio().unwrap().codec, "aac");
        assert!(meta.duration_seconds >= 2.9);
        }
    }
}

#[test]
fn test_live_transcode_synthetic_hevc() {
    if let Ok(binaries) = FFmpegBinaries::locate() {
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
        let res = kabosu_converter::ffmpeg::FFmpegRunner::run_with_progress(
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
}

#[test]
fn test_parse_timecode() {
    use kabosu_converter::ffmpeg::parse_timecode_to_seconds;

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
    use kabosu_converter::ffmpeg::{extract_frame, FFmpegBinaries};

    if let Ok(binaries) = FFmpegBinaries::locate() {
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
}

#[test]
fn test_waveform_rgba_extraction() {
    use kabosu_converter::ffmpeg::{extract_waveform_rgba, FFmpegBinaries};

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
    use kabosu_converter::ffmpeg::{spawn_audio_playback, FFmpegBinaries};

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
    use kabosu_converter::ffmpeg::parse_ffplay_pts;

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
    use kabosu_converter::ffmpeg::preview::read_ffplay_clock;
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
    use kabosu_converter::ffmpeg::{spawn_audio_playback, FFmpegBinaries};

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
                    while start.elapsed().as_millis() < 2500 {
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
    use kabosu_converter::ffmpeg::{spawn_audio_playback, spawn_playback_stream_transformed, VideoRotation, FFmpegBinaries};
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
                    while start.elapsed().as_millis() < 2500 {
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
fn test_trimmed_playback_stream_with_duration() {
    use kabosu_converter::ffmpeg::{spawn_playback_stream_transformed_with_duration, VideoRotation, FFmpegBinaries};
    use std::io::Read;

    if let Ok(binaries) = FFmpegBinaries::locate() {
        let test_media = Path::new("target/test_trimmed_playback_media.mp4");
        let mut cmd = std::process::Command::new(&binaries.ffmpeg);
        cmd.args([
            "-f", "lavfi", "-i", "testsrc=duration=3:size=640x360:rate=24",
            "-c:v", "libx264", "-pix_fmt", "yuv420p",
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
                let start_sec = 0.5;
                let trimmed_duration = 0.5; // 0.5s at 24fps = 12 frames

                // Spawn video decode stream bounded to trimmed duration
                let mut video_child = spawn_playback_stream_transformed_with_duration(
                    &binaries.ffmpeg,
                    &media_str,
                    start_sec,
                    Some(trimmed_duration),
                    VideoRotation::None,
                    false,
                    false,
                ).expect("Video stream should spawn with duration");

                let mut stdout = video_child.stdout.take().expect("Should have stdout pipe");
                let mut frame_buf = vec![0u8; 640 * 360 * 4];
                let mut frame_count = 0;

                while stdout.read_exact(&mut frame_buf).is_ok() {
                    frame_count += 1;
                }

                // 0.5s at 24fps should yield ~12 frames and then stop via EOF
                assert!(
                    (11..=13).contains(&frame_count),
                    "Trimmed playback stream should output ~12 frames for 0.5s duration, got {}",
                    frame_count
                );

                let wait_res = video_child.wait();
                assert!(wait_res.is_ok(), "Video process should exit cleanly after trimmed duration");

                let _ = std::fs::remove_file(test_media);
            }
        }
    }
}

#[test]
fn test_extract_frame_rgba_accurate_seeking() {
    use kabosu_converter::ffmpeg::{extract_frame_rgba, FFmpegBinaries};

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
    use kabosu_converter::ffmpeg::VideoRotation;

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
    use kabosu_converter::ffmpeg::VideoRotation;

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
    use kabosu_converter::ffmpeg::CropRect;

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
    use kabosu_converter::ffmpeg::CropAspectRatio;

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
fn test_crop_handle_drag_session_free_mode_anchors() {
    use kabosu_converter::ffmpeg::{CropAspectRatio, CropHandleDragSession, CropRect};

    let eff_w = 1920;
    let eff_h = 1080;
    let start_rect = CropRect {
        x: 200,
        y: 150,
        width: 600,
        height: 400,
    };
    // Anchors:
    // fixed_left = 200
    // fixed_top = 150
    // fixed_right = 200 + 600 = 800
    // fixed_bottom = 150 + 400 = 550

    let start_norm_x = 0.5;
    let start_norm_y = 0.5;

    // Test Handle 0 (Top-Left): Right edge and Bottom edge MUST STAY EXACTLY FIXED.
    let session_0 = CropHandleDragSession::new(0, start_rect, start_norm_x, start_norm_y, eff_w, eff_h);
    assert_eq!(session_0.fixed_right, 800);
    assert_eq!(session_0.fixed_bottom, 550);

    for delta_px in -30..=30 {
        for delta_py in -30..=30 {
            let norm_x = start_norm_x + (delta_px as f32 / eff_w as f32);
            let norm_y = start_norm_y + (delta_py as f32 / eff_h as f32);
            let rect = session_0.compute_rect(norm_x, norm_y, eff_w, eff_h, CropAspectRatio::Free);
            assert_eq!(rect.x + rect.width, 800, "Handle 0 right edge must be exactly 800 at delta ({}, {})", delta_px, delta_py);
            assert_eq!(rect.y + rect.height, 550, "Handle 0 bottom edge must be exactly 550 at delta ({}, {})", delta_px, delta_py);
            assert_eq!(rect.x % 2, 0, "x must be even");
            assert_eq!(rect.y % 2, 0, "y must be even");
            assert_eq!(rect.width % 2, 0, "width must be even");
            assert_eq!(rect.height % 2, 0, "height must be even");
        }
    }

    // Test Handle 1 (Top-Right): Left edge and Bottom edge MUST STAY EXACTLY FIXED.
    let session_1 = CropHandleDragSession::new(1, start_rect, start_norm_x, start_norm_y, eff_w, eff_h);
    for delta in [-50, -17, -1, 0, 1, 15, 33, 99] {
        let norm_x = start_norm_x + (delta as f32 / eff_w as f32);
        let norm_y = start_norm_y + (delta as f32 / eff_h as f32);
        let rect = session_1.compute_rect(norm_x, norm_y, eff_w, eff_h, CropAspectRatio::Free);
        assert_eq!(rect.x, 200, "Handle 1 left edge must remain at 200");
        assert_eq!(rect.y + rect.height, 550, "Handle 1 bottom edge must remain at 550");
    }

    // Test Handle 2 (Bottom-Left): Right edge and Top edge MUST STAY EXACTLY FIXED.
    let session_2 = CropHandleDragSession::new(2, start_rect, start_norm_x, start_norm_y, eff_w, eff_h);
    for delta in [-50, -17, -1, 0, 1, 15, 33, 99] {
        let norm_x = start_norm_x + (delta as f32 / eff_w as f32);
        let norm_y = start_norm_y + (delta as f32 / eff_h as f32);
        let rect = session_2.compute_rect(norm_x, norm_y, eff_w, eff_h, CropAspectRatio::Free);
        assert_eq!(rect.x + rect.width, 800, "Handle 2 right edge must remain at 800");
        assert_eq!(rect.y, 150, "Handle 2 top edge must remain at 150");
    }

    // Test Handle 3 (Bottom-Right): Left edge and Top edge MUST STAY EXACTLY FIXED.
    let session_3 = CropHandleDragSession::new(3, start_rect, start_norm_x, start_norm_y, eff_w, eff_h);
    for delta in [-50, -17, -1, 0, 1, 15, 33, 99] {
        let norm_x = start_norm_x + (delta as f32 / eff_w as f32);
        let norm_y = start_norm_y + (delta as f32 / eff_h as f32);
        let rect = session_3.compute_rect(norm_x, norm_y, eff_w, eff_h, CropAspectRatio::Free);
        assert_eq!(rect.x, 200, "Handle 3 left edge must remain at 200");
        assert_eq!(rect.y, 150, "Handle 3 top edge must remain at 150");
    }

    // Test Handle 4 (Top Edge): Left, Right, Bottom MUST STAY EXACTLY FIXED.
    let session_4 = CropHandleDragSession::new(4, start_rect, start_norm_x, start_norm_y, eff_w, eff_h);
    for delta in [-50, -17, -1, 0, 1, 15, 33, 99] {
        let norm_y = start_norm_y + (delta as f32 / eff_h as f32);
        let rect = session_4.compute_rect(start_norm_x, norm_y, eff_w, eff_h, CropAspectRatio::Free);
        assert_eq!(rect.x, 200);
        assert_eq!(rect.width, 600);
        assert_eq!(rect.y + rect.height, 550);
    }

    // Test Handle 5 (Bottom Edge): Left, Right, Top MUST STAY EXACTLY FIXED.
    let session_5 = CropHandleDragSession::new(5, start_rect, start_norm_x, start_norm_y, eff_w, eff_h);
    for delta in [-50, -17, -1, 0, 1, 15, 33, 99] {
        let norm_y = start_norm_y + (delta as f32 / eff_h as f32);
        let rect = session_5.compute_rect(start_norm_x, norm_y, eff_w, eff_h, CropAspectRatio::Free);
        assert_eq!(rect.x, 200);
        assert_eq!(rect.width, 600);
        assert_eq!(rect.y, 150);
    }

    // Test Handle 6 (Left Edge): Right, Top, Bottom MUST STAY EXACTLY FIXED.
    let session_6 = CropHandleDragSession::new(6, start_rect, start_norm_x, start_norm_y, eff_w, eff_h);
    for delta in [-50, -17, -1, 0, 1, 15, 33, 99] {
        let norm_x = start_norm_x + (delta as f32 / eff_w as f32);
        let rect = session_6.compute_rect(norm_x, start_norm_y, eff_w, eff_h, CropAspectRatio::Free);
        assert_eq!(rect.x + rect.width, 800);
        assert_eq!(rect.y, 150);
        assert_eq!(rect.height, 400);
    }

    // Test Handle 7 (Right Edge): Left, Top, Bottom MUST STAY EXACTLY FIXED.
    let session_7 = CropHandleDragSession::new(7, start_rect, start_norm_x, start_norm_y, eff_w, eff_h);
    for delta in [-50, -17, -1, 0, 1, 15, 33, 99] {
        let norm_x = start_norm_x + (delta as f32 / eff_w as f32);
        let rect = session_7.compute_rect(norm_x, start_norm_y, eff_w, eff_h, CropAspectRatio::Free);
        assert_eq!(rect.x, 200);
        assert_eq!(rect.y, 150);
        assert_eq!(rect.height, 400);
    }
}

#[test]
fn test_crop_box_drag_session_preserves_dimensions() {
    use kabosu_converter::ffmpeg::{CropBoxDragSession, CropRect};

    let eff_w = 1920;
    let eff_h = 1080;
    let start_rect = CropRect {
        x: 300,
        y: 200,
        width: 640,
        height: 360,
    };
    let session = CropBoxDragSession::new(start_rect, 0.4, 0.4, eff_w, eff_h);

    for delta_x in [-200, -77, 0, 50, 123, 500] {
        for delta_y in [-150, -33, 0, 40, 99, 400] {
            let norm_x = 0.4 + (delta_x as f32 / eff_w as f32);
            let norm_y = 0.4 + (delta_y as f32 / eff_h as f32);
            let rect = session.compute_rect(norm_x, norm_y, eff_w, eff_h);
            assert_eq!(rect.width, 640, "Width must never change during box drag");
            assert_eq!(rect.height, 360, "Height must never change during box drag");
            assert!(rect.x + rect.width <= eff_w, "Box must stay within eff_w");
            assert!(rect.y + rect.height <= eff_h, "Box must stay within eff_h");
            assert_eq!(rect.x % 2, 0);
            assert_eq!(rect.y % 2, 0);
        }
    }
}

#[test]
fn test_combined_transform_crop_scale_filter_order() {
    use kabosu_converter::ffmpeg::{
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
    use kabosu_converter::ffmpeg::compute_letterbox_viewport;

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
    use kabosu_converter::ffmpeg::{
        probe_file, CropRect, FFmpegBinaries, FFmpegRunner, VideoRotation,
    };

    let binaries = match FFmpegBinaries::locate() {
        Ok(b) => b,
        Err(_) => return,
    };
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
        hw_accel: HwAccel::Cpu,
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
    use kabosu_converter::ffmpeg::AudioPlaybackSession;

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

#[test]
fn test_audio_playback_session_intentional_kill_prevents_premature_finish() {
    use kabosu_converter::ffmpeg::AudioPlaybackSession;

    let mut cmd = std::process::Command::new(if cfg!(windows) { "cmd.exe" } else { "sh" });
    if cfg!(windows) {
        cmd.args(["/c", "ping -n 5 127.0.0.1 >nul"]);
    } else {
        cmd.args(["-c", "sleep 5"]);
    }
    if let Ok(child) = cmd.spawn() {
        let mut session = AudioPlaybackSession::from_child(child);
        assert!(!session.is_finished());

        // When intentionally killed (e.g. volume or filter change restart),
        // is_finished() must NOT report true as natural stream completion.
        let _ = session.kill();
        assert!(!session.is_finished());
        assert!(session.intentional_exit.load(std::sync::atomic::Ordering::Acquire));
    }
}

#[test]
fn test_qsv_and_amf_hardware_command_generation() {
    let ffmpeg_bin = Path::new("ffmpeg.exe");

    // 1. QSV H.264 with CRF
    let config_qsv_crf = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "test.mp4".to_string(),
        output_path: "test_qsv.mp4".to_string(),
        video_codec: VideoCodec::H264,
        hw_accel: HwAccel::Qsv,
        rate_control: RateControlMode::CRF,
        crf: 22,
        ..Default::default()
    };
    let cmd = FFmpegCommandBuilder::build(&config_qsv_crf, ffmpeg_bin);
    assert!(cmd.contains(&"h264_qsv".to_string()));
    assert!(cmd.contains(&"-global_quality".to_string()));
    let idx = cmd.iter().position(|r| r == "-global_quality").unwrap();
    assert_eq!(cmd[idx + 1], "22");

    // 2. QSV H.264 with CBR
    let config_qsv_cbr = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "test.mp4".to_string(),
        output_path: "test_qsv_cbr.mp4".to_string(),
        video_codec: VideoCodec::H264,
        hw_accel: HwAccel::Qsv,
        rate_control: RateControlMode::CBR,
        video_bitrate_kbps: 8000,
        ..Default::default()
    };
    let cmd_cbr = FFmpegCommandBuilder::build(&config_qsv_cbr, ffmpeg_bin);
    assert!(cmd_cbr.contains(&"-b:v".to_string()));
    let b_idx = cmd_cbr.iter().position(|r| r == "-b:v").unwrap();
    assert_eq!(cmd_cbr[b_idx + 1], "8000k");

    // 3. AMF H.264 with CRF
    let config_amf_crf = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "test.mp4".to_string(),
        output_path: "test_amf.mp4".to_string(),
        video_codec: VideoCodec::H264,
        hw_accel: HwAccel::Amf,
        rate_control: RateControlMode::CRF,
        crf: 20,
        ..Default::default()
    };
    let cmd_amf = FFmpegCommandBuilder::build(&config_amf_crf, ffmpeg_bin);
    assert!(cmd_amf.contains(&"h264_amf".to_string()));
    assert!(cmd_amf.contains(&"-rc".to_string()));
    assert!(cmd_amf.contains(&"cqp".to_string()));
    assert!(cmd_amf.contains(&"-qp_i".to_string()));

    // 4. AMF HEVC
    let config_amf_hevc = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "test.mp4".to_string(),
        output_path: "test_amf_hevc.mp4".to_string(),
        video_codec: VideoCodec::H265,
        hw_accel: HwAccel::Amf,
        rate_control: RateControlMode::CRF,
        crf: 24,
        ..Default::default()
    };
    let cmd_hevc = FFmpegCommandBuilder::build(&config_amf_hevc, ffmpeg_bin);
    assert!(cmd_hevc.contains(&"hevc_amf".to_string()));
    assert!(cmd_hevc.contains(&"-tag:v".to_string()));
    assert!(cmd_hevc.contains(&"hvc1".to_string()));
}

#[test]
fn test_ffmpeg_version_parser() {
    // 1. FFmpeg 8.1.3 Git build (standard Windows build)
    let output_8_1 = "ffmpeg version n8.1.3-24-g92611900bb-20261024 Copyright (c) 2000-2026 the FFmpeg developers\nbuilt with gcc 16.2.0";
    let (clean, badge) = parse_ffmpeg_version(output_8_1);
    assert_eq!(clean, "8.1.3-24-g92611900bb-20261024");
    assert_eq!(badge, "FFmpeg 8.x Engine");

    // 2. Official release version
    let output_release = "ffmpeg version 8.1.0 Copyright (c) 2000-2026 the FFmpeg developers";
    let (clean, badge) = parse_ffmpeg_version(output_release);
    assert_eq!(clean, "8.1.0");
    assert_eq!(badge, "FFmpeg 8.x Engine");

    // 3. Prior version 7.x
    let output_7_1 = "ffmpeg version 7.1.1-1ubuntu1 (Ubuntu 7.1.1)";
    let (clean, badge) = parse_ffmpeg_version(output_7_1);
    assert_eq!(clean, "7.1.1-1ubuntu1");
    assert_eq!(badge, "FFmpeg 7.x Engine");

    // 4. Fallback on invalid / empty string
    let (fallback_clean, fallback_badge) = parse_ffmpeg_version("");
    assert_eq!(fallback_clean, "8.1");
    assert_eq!(fallback_badge, "FFmpeg 8.x Engine");
}

#[test]
fn test_live_ffmpeg_version_detection() {
    let binaries = match FFmpegBinaries::locate() {
        Ok(b) => b,
        Err(_) => return,
    };
    let (full_ver, badge) = binaries.detect_version();
    assert!(full_ver.starts_with("8.1"), "Expected version starting with 8.1, got: {}", full_ver);
    assert_eq!(badge, "FFmpeg 8.x Engine");
}

#[test]
fn test_videotoolbox_hardware_command_generation() {
    let ffmpeg_bin = Path::new("ffmpeg");

    // 1. VideoToolbox H.264 CRF
    let config_h264_crf = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mov".to_string(),
        output_path: "output.mp4".to_string(),
        video_codec: VideoCodec::H264,
        hw_accel: HwAccel::VideoToolbox,
        rate_control: RateControlMode::CRF,
        crf: 20,
        ..Default::default()
    };
    let cmd_h264 = FFmpegCommandBuilder::build(&config_h264_crf, ffmpeg_bin);
    assert!(cmd_h264.contains(&"-hwaccel".to_string()));
    assert!(cmd_h264.contains(&"videotoolbox".to_string()));
    assert!(cmd_h264.contains(&"-c:v".to_string()));
    assert!(cmd_h264.contains(&"h264_videotoolbox".to_string()));
    assert!(cmd_h264.contains(&"-q:v".to_string()));
    // CRF 20 -> (100 - 40) = 60
    assert!(cmd_h264.contains(&"60".to_string()));

    // 2. VideoToolbox H.264 CBR
    let config_h264_cbr = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mov".to_string(),
        output_path: "output.mp4".to_string(),
        video_codec: VideoCodec::H264,
        hw_accel: HwAccel::VideoToolbox,
        rate_control: RateControlMode::CBR,
        video_bitrate_kbps: 8000,
        ..Default::default()
    };
    let cmd_h264_cbr = FFmpegCommandBuilder::build(&config_h264_cbr, ffmpeg_bin);
    assert!(cmd_h264_cbr.contains(&"-b:v".to_string()));
    assert!(cmd_h264_cbr.contains(&"8000k".to_string()));

    // 3. VideoToolbox HEVC (H.265) with Apple compatibility tag
    let config_hevc = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mov".to_string(),
        output_path: "output.mp4".to_string(),
        video_codec: VideoCodec::H265,
        hw_accel: HwAccel::VideoToolbox,
        rate_control: RateControlMode::CRF,
        crf: 22,
        ..Default::default()
    };
    let cmd_hevc = FFmpegCommandBuilder::build(&config_hevc, ffmpeg_bin);
    assert!(cmd_hevc.contains(&"-c:v".to_string()));
    assert!(cmd_hevc.contains(&"hevc_videotoolbox".to_string()));
    assert!(cmd_hevc.contains(&"-tag:v".to_string()));
    assert!(cmd_hevc.contains(&"hvc1".to_string()));

    // 4. VideoToolbox Apple ProRes
    let config_prores = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mov".to_string(),
        output_path: "output.mov".to_string(),
        video_codec: VideoCodec::AppleProRes,
        hw_accel: HwAccel::VideoToolbox,
        prores_profile: ProResProfile::HQ,
        ..Default::default()
    };
    let cmd_prores = FFmpegCommandBuilder::build(&config_prores, ffmpeg_bin);
    assert!(cmd_prores.contains(&"-c:v".to_string()));
    assert!(cmd_prores.contains(&"prores_videotoolbox".to_string()));
    assert!(cmd_prores.contains(&"-profile:v".to_string()));
    assert!(cmd_prores.contains(&"3".to_string())); // HQ = 3
}

#[test]
fn test_svtav1_rate_control_modes() {
    let ffmpeg_bin = Path::new("ffmpeg.exe");

    // 1. SVT-AV1 with CRF
    let config_crf = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mp4".to_string(),
        output_path: "output.mp4".to_string(),
        video_codec: VideoCodec::AV1,
        rate_control: RateControlMode::CRF,
        crf: 28,
        ..Default::default()
    };
    let cmd_crf = FFmpegCommandBuilder::build(&config_crf, ffmpeg_bin);
    assert!(cmd_crf.contains(&"-c:v".to_string()));
    assert!(cmd_crf.contains(&"libsvtav1".to_string()));
    assert!(cmd_crf.contains(&"-crf".to_string()));
    let crf_idx = cmd_crf.iter().position(|r| r == "-crf").unwrap();
    assert_eq!(cmd_crf[crf_idx + 1], "28");
    assert!(!cmd_crf.contains(&"-b:v".to_string()));

    // 2. SVT-AV1 with CBR
    let config_cbr = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mp4".to_string(),
        output_path: "output.mp4".to_string(),
        video_codec: VideoCodec::AV1,
        rate_control: RateControlMode::CBR,
        video_bitrate_kbps: 4500,
        ..Default::default()
    };
    let cmd_cbr = FFmpegCommandBuilder::build(&config_cbr, ffmpeg_bin);
    assert!(cmd_cbr.contains(&"-c:v".to_string()));
    assert!(cmd_cbr.contains(&"libsvtav1".to_string()));
    assert!(cmd_cbr.contains(&"-b:v".to_string()));
    let b_idx = cmd_cbr.iter().position(|r| r == "-b:v").unwrap();
    assert_eq!(cmd_cbr[b_idx + 1], "4500k");
    assert!(!cmd_cbr.contains(&"-crf".to_string()));

    // 3. SVT-AV1 with VBR
    let config_vbr = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mp4".to_string(),
        output_path: "output.mp4".to_string(),
        video_codec: VideoCodec::AV1,
        rate_control: RateControlMode::VBR,
        video_bitrate_kbps: 6000,
        ..Default::default()
    };
    let cmd_vbr = FFmpegCommandBuilder::build(&config_vbr, ffmpeg_bin);
    assert!(cmd_vbr.contains(&"-b:v".to_string()));
    let vbr_idx = cmd_vbr.iter().position(|r| r == "-b:v").unwrap();
    assert_eq!(cmd_vbr[vbr_idx + 1], "6000k");
    assert!(!cmd_vbr.contains(&"-crf".to_string()));
}

#[test]
fn test_faststart_case_insensitivity() {
    let ffmpeg_bin = Path::new("ffmpeg.exe");

    for ext in &["out.MP4", "out.MOV", "out.M4V", "out.mp4", "out.mov", "out.m4v"] {
        let config = EncodeJobConfig {
            function: EncoderFunction::Transcode,
            input_path: "in.mkv".to_string(),
            output_path: ext.to_string(),
            faststart: true,
            ..Default::default()
        };
        let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);
        assert!(
            cmd.contains(&"-movflags".to_string()) && cmd.contains(&"+faststart".to_string()),
            "faststart must be applied for extension {}",
            ext
        );
    }

    // Rewrap case-insensitivity
    for ext in &["out.MP4", "out.MOV", "out.M4V"] {
        let config = EncodeJobConfig {
            function: EncoderFunction::Rewrap,
            input_path: "in.mkv".to_string(),
            output_path: ext.to_string(),
            faststart: true,
            ..Default::default()
        };
        let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);
        assert!(
            cmd.contains(&"-movflags".to_string()) && cmd.contains(&"+faststart".to_string()),
            "faststart must be applied in rewrap for extension {}",
            ext
        );
    }
}

#[test]
fn test_parse_astats_level() {
    use kabosu_converter::ffmpeg::parse_astats_level;

    assert_eq!(
        parse_astats_level("lavfi.astats.1.Peak_level=-18.063656"),
        Some((1, -18.063656))
    );
    assert_eq!(
        parse_astats_level("lavfi.astats.2.Peak_level=-24.082135"),
        Some((2, -24.082135))
    );
    assert_eq!(
        parse_astats_level("lavfi.astats.1.Peak_level=-inf"),
        Some((1, -60.0))
    );
    assert_eq!(
        parse_astats_level("lavfi.astats.2.Peak_level=-nan"),
        Some((2, -60.0))
    );
    assert_eq!(
        parse_astats_level("lavfi.astats.1.Peak_level=2.5"),
        Some((1, 2.5))
    );
    // Clamping to -60.0
    assert_eq!(
        parse_astats_level("lavfi.astats.2.Peak_level=-85.0"),
        Some((2, -60.0))
    );
    // Non-matching lines
    assert_eq!(parse_astats_level("frame:0 pts:0 pts_time:0"), None);
    assert_eq!(parse_astats_level("lavfi.astats.Overall.Peak_level=-18.0"), None);
    assert_eq!(parse_astats_level("random stdout message"), None);
}

#[test]
fn test_read_ffplay_meter_stream() {
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};
    use kabosu_converter::ffmpeg::read_ffplay_meter;

    let meter = Arc::new(Mutex::new((-60.0f32, -60.0f32)));
    let mock_stream = b"frame:0 pts:0\r\nlavfi.astats.1.Peak_level=-14.5\r\nlavfi.astats.2.Peak_level=-20.2\r\n";
    let cursor = Cursor::new(mock_stream);

    read_ffplay_meter(cursor, meter.clone());

    let (l, r) = *meter.lock().unwrap();
    assert!((l - (-14.5)).abs() < 0.01, "L channel should be -14.5 dB: got {}", l);
    assert!((r - (-20.2)).abs() < 0.01, "R channel should be -20.2 dB: got {}", r);
}

#[test]
fn test_live_audio_meter_playback_and_volume_isolation() {
    use std::path::Path;
    use kabosu_converter::ffmpeg::{spawn_audio_playback, FFmpegBinaries};

    let binaries = match FFmpegBinaries::locate() {
        Ok(b) => b,
        Err(_) => return,
    };
    let ffplay = match binaries.ffplay.as_ref() {
        Some(f) => f,
        None => return,
    };

    let test_wav = Path::new("target/test_meter_stereo.wav");
    if !test_wav.exists() {
        // Generate a 1-second stereo test wav (1000Hz on left @ 0.5, 500Hz on right @ 0.1)
        let mut cmd = std::process::Command::new(&binaries.ffmpeg);
        cmd.args([
            "-y",
            "-f", "lavfi", "-i", "sine=frequency=1000:duration=1",
            "-f", "lavfi", "-i", "sine=frequency=500:duration=1",
            "-filter_complex", "[0:a]volume=0.5[a0];[1:a]volume=0.1[a1];[a0][a1]join=inputs=2:channel_layout=stereo[out]",
            "-map", "[out]",
            &test_wav.to_string_lossy(),
        ]);
        let _ = cmd.output();
    }

    if test_wav.exists() {
        // 1. Test live playback with normal volume (e.g. 80%)
        let session = spawn_audio_playback(
            ffplay,
            &test_wav.to_string_lossy(),
            0.0,
            Some(0.8),
            80,
            None,
            None,
        );
        assert!(session.is_ok(), "spawn_audio_playback with meter filters should succeed");
        let mut session = session.unwrap();

        // Wait up to 500ms for ffplay to process frames and emit meter levels
        let start = std::time::Instant::now();
        let mut got_levels = false;
        while start.elapsed() < std::time::Duration::from_millis(2500) {
            let (l, r) = session.get_meter_levels();
            if l > -55.0 && r > -55.0 {
                got_levels = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let _ = session.kill();
        assert!(got_levels, "Volume meter should read active stereo audio levels during playback");

        // 2. Test isolation from toolbar volume: with volume=0, meter MUST still reflect render output!
        let session_muted = spawn_audio_playback(
            ffplay,
            &test_wav.to_string_lossy(),
            0.0,
            Some(0.8),
            0, // Muted/zero preview volume
            None,
            None,
        );
        assert!(session_muted.is_ok(), "spawn_audio_playback with volume=0 should succeed");
        let mut session_muted = session_muted.unwrap();

        let start = std::time::Instant::now();
        let mut got_muted_levels = false;
        while start.elapsed() < std::time::Duration::from_millis(2500) {
            let (l, r) = session_muted.get_meter_levels();
            if l > -55.0 && r > -55.0 {
                got_muted_levels = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let _ = session_muted.kill();
        let _ = std::fs::remove_file(test_wav);
        assert!(
            got_muted_levels,
            "Volume meter must continue to reflect audio render output even when player preview volume is 0 or muted"
        );
    }
}









#[test]
#[cfg(target_os = "macos")]
fn test_macos_auto_hw_accel_routes_to_videotoolbox() {
    let config = EncodeJobConfig {
        function: EncoderFunction::Transcode,
        input_path: "input.mov".to_string(),
        output_path: "output.mp4".to_string(),
        video_codec: VideoCodec::H264,
        hw_accel: HwAccel::Auto,
        ..Default::default()
    };
    let ffmpeg_bin = Path::new("ffmpeg");
    let cmd = FFmpegCommandBuilder::build(&config, ffmpeg_bin);
    assert!(cmd.contains(&"-hwaccel".to_string()));
    assert!(cmd.contains(&"videotoolbox".to_string()));
    assert!(cmd.contains(&"h264_videotoolbox".to_string()));
}
