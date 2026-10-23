use std::path::Path;
use another_encoder::ffmpeg::{
    FFmpegCommandBuilder, EncodeJobConfig, EncoderFunction, VideoCodec,
    RateControlMode, AudioCodecChoice, ProResProfile, format_duration_hms, FFmpegBinaries
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
                    let child = spawn_audio_playback(ffplay, &test_wav.to_string_lossy(), 0.0, Some(0.2), 80);
                    assert!(child.is_ok(), "spawn_audio_playback should succeed");
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



