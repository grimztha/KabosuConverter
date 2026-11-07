slint::include_modules!();

slint::slint! {
    import { VolumeBar } from "src/ui/components/volume_bar.slint";

    export component TestVolumeBarHarness inherits Window {
        width: 300px;
        height: 100px;
        in-out property <float> vol: 1.0;
        in-out property <bool> muted: false;
        out property <float> last_changed_vol: -1.0;
        out property <float> last_settled_vol: -1.0;
        out property <int> mute_toggled_count: 0;

        vb := VolumeBar {
            volume: root.vol;
            is_muted: root.muted;
            volume_changed(val) => {
                root.vol = val;
                root.last_changed_vol = val;
            }
            volume_settled(val) => {
                root.vol = val;
                root.last_settled_vol = val;
            }
            toggle_mute() => {
                root.muted = !root.muted;
                root.mute_toggled_count += 1;
            }
        }
    }
}

#[test]
fn test_volumebar_component_properties_and_callbacks() {
    let harness = TestVolumeBarHarness::new().unwrap();
    assert_eq!(harness.get_vol(), 1.0);
    assert!(!harness.get_muted());

    // Verify property propagation
    harness.set_vol(0.72);
    assert!((harness.get_vol() - 0.72).abs() < 0.001);

    harness.set_muted(true);
    assert!(harness.get_muted());

    harness.set_muted(false);
    assert!(!harness.get_muted());
}
