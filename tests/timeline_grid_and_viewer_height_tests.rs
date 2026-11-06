slint::include_modules!();

slint::slint! {
    export component TestTimelineGridHelper inherits Window {
        in property <float> duration: 120.0;
        in property <bool> is_dual_channel: false;
        in property <length> ruler_height: 18px;

        public pure function test_get_nice_step(raw: float) -> float {
            if raw <= 0.05 { return 0.05; }
            if raw <= 0.1  { return 0.1; }
            if raw <= 0.2  { return 0.2; }
            if raw <= 0.5  { return 0.5; }
            if raw <= 1.0  { return 1.0; }
            if raw <= 2.0  { return 2.0; }
            if raw <= 5.0  { return 5.0; }
            if raw <= 10.0 { return 10.0; }
            if raw <= 15.0 { return 15.0; }
            if raw <= 30.0 { return 30.0; }
            if raw <= 60.0 { return 60.0; }
            if raw <= 120.0 { return 120.0; }
            if raw <= 300.0 { return 300.0; }
            if raw <= 600.0 { return 600.0; }
            if raw <= 900.0 { return 900.0; }
            if raw <= 1800.0 { return 1800.0; }
            if raw <= 3600.0 { return 3600.0; }
            if raw <= 7200.0 { return 7200.0; }
            return max(3600.0, floor(raw / 3600.0) * 3600.0);
        }
    }

    export component TestViewerHeightLayout inherits Window {
        width: 800px;
        height: 600px;
        in-out property <bool> selected_is_video: true;

        out property <length> viewer_height: media_screen.height;
        out property <length> spacer_height: spacer.height;

        VerticalLayout {
            padding: 0px;
            spacing: 0px;

            media_screen := Rectangle {
                vertical-stretch: root.selected_is_video ? 3 : 2;
                background: #000000;
            }

            // Fixed height control bar representing playhead and transport buttons
            Rectangle {
                height: 100px;
                background: #18181b;
            }

            spacer := Rectangle {
                vertical-stretch: root.selected_is_video ? 0 : 1;
                visible: !root.selected_is_video;
                background: transparent;
            }
        }
    }
}

#[test]
fn test_timeline_grid_and_viewer_height_lifecycle() {
    let helper = TestTimelineGridHelper::new().unwrap();

    assert_eq!(helper.invoke_test_get_nice_step(0.08), 0.1);
    assert_eq!(helper.invoke_test_get_nice_step(0.4), 0.5);
    assert_eq!(helper.invoke_test_get_nice_step(1.8), 2.0);
    assert_eq!(helper.invoke_test_get_nice_step(8.5), 10.0);
    assert_eq!(helper.invoke_test_get_nice_step(25.0), 30.0);
    assert_eq!(helper.invoke_test_get_nice_step(55.0), 60.0);
    assert_eq!(helper.invoke_test_get_nice_step(250.0), 300.0);
    assert_eq!(helper.invoke_test_get_nice_step(1500.0), 1800.0);

    let layout = TestViewerHeightLayout::new().unwrap();

    // Video is selected:
    // Window is 600px, control bar is 100px. Available vertical stretch space = 500px.
    // Video gets full 500px (100% of space, unchanged).
    layout.set_selected_is_video(true);
    let vid_h = layout.get_viewer_height();
    assert!((vid_h - 500.0).abs() < 1.0, "Video viewer should occupy all 500px: got {}", vid_h);

    // Audio is selected:
    // Available stretch space = 500px.
    // Audio viewer gets 2/3 of 500px (~333.33px), which is reduced by 1/3!
    // Spacer gets 1/3 of 500px (~166.67px).
    layout.set_selected_is_video(false);
    let audio_h = layout.get_viewer_height();
    let spacer_h = layout.get_spacer_height();

    assert!((audio_h - (500.0 * 2.0 / 3.0)).abs() < 2.0, "Audio viewer should be 2/3 of 500px (~333px): got {}", audio_h);
    assert!((spacer_h - (500.0 / 3.0)).abs() < 2.0, "Spacer should be 1/3 of 500px (~167px): got {}", spacer_h);
    assert!(((vid_h - audio_h) - (vid_h / 3.0)).abs() < 2.0, "Audio viewer height must be reduced by 1/3 compared to video viewer height");
}
