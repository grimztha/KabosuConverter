mod common;
use std::cell::RefCell;
use std::rc::Rc;

use slint::platform::{Key, WindowEvent};
use slint::{ComponentHandle, SharedString};

slint::include_modules!();

slint::slint! {
    import { Theme, AppData } from "src/ui/globals.slint";
    import { ActionButton } from "src/ui/components/action_button.slint";
    import { ToggleSwitch } from "src/ui/components/toggle_switch.slint";
    import { Badge } from "src/ui/components/badge.slint";
    import { TextField } from "src/ui/components/text_field.slint";
    import { SelectBox } from "src/ui/components/select_box.slint";
    import { RangeSlider } from "src/ui/components/range_slider.slint";
    import { VolumeMeter } from "src/ui/components/volume_meter.slint";

    export component TestUIOverhaulHelper inherits Window {
        width: 960px;
        height: 650px;

        // Theme scale exposures
        out property <length> theme_font_title: Theme.font_title;
        out property <length> theme_font_section: Theme.font_section;
        out property <length> theme_font_body: Theme.font_body;
        out property <length> theme_font_caption: Theme.font_caption;
        out property <length> theme_font_badge: Theme.font_badge;
        out property <string> theme_font_mono: Theme.font_mono;

        // Theme interactive heights
        out property <length> theme_btn_height_sm: Theme.btn_height_sm;
        out property <length> theme_btn_height_md: Theme.btn_height_md;
        out property <length> theme_btn_height_lg: Theme.btn_height_lg;

        // Theme concentric radii
        out property <length> theme_radius_sm: Theme.radius_sm;
        out property <length> theme_radius_md: Theme.radius_md;
        out property <length> theme_radius_lg: Theme.radius_lg;

        // Studio Kabosu surface & text tokens
        out property <color> theme_app_bg: Theme.app_bg;
        out property <color> theme_card_bg: Theme.card_bg;
        out property <color> theme_card_secondary_bg: Theme.card_secondary_bg;
        out property <color> theme_input_bg: Theme.input_bg;
        out property <color> theme_text_primary: Theme.text_primary;
        out property <color> theme_text_secondary: Theme.text_secondary;
        out property <color> theme_text_muted: Theme.text_muted;
        out property <color> theme_text_on_accent: Theme.text_on_accent;

        // Studio Kabosu accent & status tokens
        out property <color> theme_accent_primary: Theme.accent_primary;
        out property <color> theme_accent_lime: Theme.accent_lime;
        out property <color> theme_accent_surface: Theme.accent_surface;
        out property <color> theme_accent_emerald: Theme.accent_emerald;
        out property <color> theme_accent_green: Theme.accent_green;
        out property <color> theme_emerald_surface: Theme.emerald_surface;
        out property <color> theme_accent_amber: Theme.accent_amber;
        out property <color> theme_amber_surface: Theme.amber_surface;
        out property <color> theme_accent_red: Theme.accent_red;
        out property <color> theme_accent_red_solid: Theme.accent_red_solid;
        out property <color> theme_accent_red_text: Theme.accent_red_text;
        out property <color> theme_red_surface: Theme.red_surface;
        out property <color> theme_accent_purple: Theme.accent_purple;
        out property <color> theme_purple_surface: Theme.purple_surface;
        out property <color> theme_focus_ring: Theme.focus_ring;

        // Queue panel responsive layout helper
        in-out property <bool> queue_expanded: true;
        in-out property <bool> edit_expanded: true;
        out property <length> computed_queue_width: root.queue_expanded ? 300px : 38px;
        out property <length> computed_edit_width: root.edit_expanded ? 320px : 34px;
        out property <length> computed_player_available_width: root.width - root.computed_queue_width - root.computed_edit_width;

        // Concentric radius test
        out property <length> inner_item_radius: 6px;
        out property <length> container_padding: 8px;
        out property <length> concentric_outer_radius: root.inner_item_radius + root.container_padding; // 14px

        // Themed component harnesses
        in-out property <string> field_text: "hello";
        in-out property <int> select_idx: 1;
        in-out property <float> slider_val: 0.0;
        out property <length> meter_width: vm.width;

        callback select_changed(int);
        callback slider_changed(float);

        VerticalLayout {
            btn := ActionButton {
                title: "Test Action";
                height: 34px;
            }
            sw := ToggleSwitch {
                checked: true;
            }
            bdg := Badge {
                label: "Ready";
            }
            tf := TextField {
                text <=> root.field_text;
                placeholder: "Enter value...";
            }
            sb := SelectBox {
                model: ["H.264", "H.265", "AV1"];
                current-index <=> root.select_idx;
                selected(idx) => { root.select_changed(idx); }
            }
            rs := RangeSlider {
                minimum: -24.0;
                maximum: 24.0;
                step: 1.0;
                is_centered: true;
                value <=> root.slider_val;
                changed(v) => { root.slider_changed(v); }
            }
            vm := VolumeMeter {
                level_l_db: -12.0;
                level_r_db: -18.0;
            }
        }
    }
}

fn srgb_channel_to_linear(c: u8) -> f64 {
    let v = (c as f64) / 255.0;
    if v <= 0.03928 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(c: slint::Color) -> f64 {
    0.2126 * srgb_channel_to_linear(c.red())
        + 0.7152 * srgb_channel_to_linear(c.green())
        + 0.0722 * srgb_channel_to_linear(c.blue())
}

fn contrast_ratio(a: slint::Color, b: slint::Color) -> f64 {
    let l1 = relative_luminance(a);
    let l2 = relative_luminance(b);
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn test_ui_layout_overhaul_lifecycle() {
    common::init_test_backend();
    let helper = TestUIOverhaulHelper::new().unwrap();

    // 1. Typography Scale and Floor Verification
    let font_title = helper.get_theme_font_title();
    let font_section = helper.get_theme_font_section();
    let font_body = helper.get_theme_font_body();
    let font_caption = helper.get_theme_font_caption();
    let font_badge = helper.get_theme_font_badge();

    assert!(font_title >= 18.0, "Title font size must be >= 18px");
    assert!(font_section >= 14.0, "Section font size must be >= 14px");
    assert!(font_body >= 13.0, "Body font size must be >= 13px");
    assert!(font_caption >= 12.0, "Caption font size must be >= 12px");
    assert!(font_badge >= 11.0, "Badge floor must be >= 11px to guarantee legibility");

    assert!(font_title > font_section, "Title must be larger than section");
    assert!(font_section > font_body, "Section must be larger than body");
    assert!(font_body > font_caption, "Body must be larger than caption");
    assert!(font_caption > font_badge, "Caption must be larger than badge");

    let mono = helper.get_theme_font_mono();
    assert_eq!(mono.as_str(), "Consolas", "Monospace font must be Consolas for numerical telemetry");

    // 2. Standardized Button Heights
    let btn_sm = helper.get_theme_btn_height_sm();
    let btn_md = helper.get_theme_btn_height_md();
    let btn_lg = helper.get_theme_btn_height_lg();

    assert_eq!(btn_sm, 28.0, "Small button height tier must be 28px");
    assert_eq!(btn_md, 34.0, "Medium button height tier must be 34px");
    assert_eq!(btn_lg, 40.0, "Large button height tier must be 40px");
    assert!(btn_sm < btn_md && btn_md < btn_lg, "Button heights must be strictly tiered");

    // 3. Concentric Radii Geometry
    let r_sm = helper.get_theme_radius_sm();
    let r_md = helper.get_theme_radius_md();
    let r_lg = helper.get_theme_radius_lg();

    assert_eq!(r_sm, 6.0, "Small radius must be 6px");
    assert_eq!(r_md, 10.0, "Medium radius must be 10px");
    assert_eq!(r_lg, 14.0, "Large radius must be 14px");

    let inner_r = helper.get_inner_item_radius();
    let padding = helper.get_container_padding();
    let outer_r = helper.get_concentric_outer_radius();
    assert_eq!(outer_r, inner_r + padding, "Outer radius must equal inner radius plus padding");
    assert_eq!(outer_r, r_lg);

    // 4. Studio Kabosu Color System & Real WCAG 2.1 AA Contrast Verification
    let app_bg = helper.get_theme_app_bg();
    let card_bg = helper.get_theme_card_bg();
    let card_sec = helper.get_theme_card_secondary_bg();
    let input_bg = helper.get_theme_input_bg();
    let text_primary = helper.get_theme_text_primary();
    let text_secondary = helper.get_theme_text_secondary();
    let text_muted = helper.get_theme_text_muted();
    let text_on_accent = helper.get_theme_text_on_accent();
    let accent_primary = helper.get_theme_accent_primary();
    let accent_lime = helper.get_theme_accent_lime();
    let accent_surface = helper.get_theme_accent_surface();
    let focus_ring = helper.get_theme_focus_ring();

    assert_eq!(text_muted.alpha(), 255);
    assert_eq!(text_primary.alpha(), 255);
    assert_eq!(focus_ring.alpha(), 255);

    // Botanical Kabosu green identity check: green channel leads both primary and zest lime
    assert!(accent_primary.green() > accent_primary.red() && accent_primary.green() > accent_primary.blue(),
        "Kabosu accent_primary must be green-dominant");
    assert!(accent_lime.green() > accent_lime.red() && accent_lime.green() > accent_lime.blue(),
        "Kabosu accent_lime must be green-dominant");

    // Botanical dark surfaces stay genuinely dark with clear elevation steps
    assert!(relative_luminance(app_bg) < relative_luminance(card_bg), "card_bg must elevate above app_bg");
    assert!(relative_luminance(card_bg) < relative_luminance(card_sec), "card_secondary_bg must elevate above card_bg");
    assert!(relative_luminance(input_bg) < relative_luminance(card_bg), "input_bg must sit recessed below card_bg");

    // Maintain legacy muted channel floor + exact WCAG AA (>= 4.5:1) assertions
    assert!(text_muted.red() >= 150, "text_muted red channel must be >= 150");
    assert!(text_muted.green() >= 150, "text_muted green channel must be >= 150");
    assert!(text_muted.blue() >= 160, "text_muted blue channel must be >= 160");

    assert!(contrast_ratio(text_primary, card_bg) >= 12.0, "text_primary on card_bg must exceed 12:1");
    assert!(contrast_ratio(text_secondary, card_bg) >= 7.0, "text_secondary on card_bg must pass WCAG AAA (7:1)");
    assert!(contrast_ratio(text_muted, card_bg) >= 4.5, "text_muted on card_bg must pass WCAG AA (4.5:1)");
    assert!(contrast_ratio(text_muted, card_sec) >= 4.5, "text_muted on card_secondary_bg must pass WCAG AA (4.5:1)");
    assert!(contrast_ratio(text_primary, input_bg) >= 12.0, "text_primary on input_bg must exceed 12:1");
    assert!(contrast_ratio(text_on_accent, accent_primary) >= 7.0, "text_on_accent on Kabosu primary button must pass WCAG AAA (7:1)");
    assert!(contrast_ratio(accent_lime, accent_surface) >= 7.0, "accent_lime on accent_surface must pass WCAG AAA (7:1)");

    let emerald = helper.get_theme_accent_emerald();
    let green = helper.get_theme_accent_green();
    let emerald_surface = helper.get_theme_emerald_surface();
    let amber = helper.get_theme_accent_amber();
    let amber_surface = helper.get_theme_amber_surface();
    let red = helper.get_theme_accent_red();
    let red_solid = helper.get_theme_accent_red_solid();
    let red_text = helper.get_theme_accent_red_text();
    let red_surface = helper.get_theme_red_surface();
    let purple = helper.get_theme_accent_purple();
    let purple_surface = helper.get_theme_purple_surface();

    assert!(emerald.green() > emerald.red(), "Emerald status must have green > red");
    assert!(amber.red() > amber.blue(), "Amber status must have red > blue");
    assert!(red.red() > red.green(), "Red status must have red dominant");

    let pure_white = slint::Color::from_rgb_u8(255, 255, 255);
    assert!(contrast_ratio(pure_white, red_solid) >= 4.5, "white on accent_red_solid must pass WCAG AA (4.5:1)");
    assert!(contrast_ratio(red_text, red_surface) >= 4.5, "accent_red_text on red_surface must pass WCAG AA (4.5:1)");
    assert!(contrast_ratio(amber, amber_surface) >= 4.5, "accent_amber on amber_surface must pass WCAG AA (4.5:1)");
    assert!(contrast_ratio(green, emerald_surface) >= 4.5, "accent_green on emerald_surface must pass WCAG AA (4.5:1)");
    assert!(contrast_ratio(purple, purple_surface) >= 4.5, "accent_purple on purple_surface must pass WCAG AA (4.5:1)");

    // 5. Responsive Layout & Queue Collapse Space Reclamation
    helper.set_queue_expanded(true);
    helper.set_edit_expanded(true);
    let queue_w = helper.get_computed_queue_width();
    let edit_w = helper.get_computed_edit_width();
    let player_w = helper.get_computed_player_available_width();

    assert_eq!(queue_w, 300.0);
    assert_eq!(edit_w, 320.0);
    assert_eq!(player_w, 340.0, "Player width when both panels expanded at 960px window is 340px");

    helper.set_queue_expanded(false);
    let collapsed_queue_w = helper.get_computed_queue_width();
    let reclaimed_player_w = helper.get_computed_player_available_width();

    assert_eq!(collapsed_queue_w, 38.0, "Collapsed queue width is 38px");
    assert_eq!(reclaimed_player_w, 602.0, "Collapsing queue reclaims 262px, giving 602px of room");
    assert!(reclaimed_player_w >= 500.0, "Reclaimed player width allows trimming controls to fit without clipping");

    // 6. Themed Component Contract Verification (TextField, SelectBox, RangeSlider, VolumeMeter)
    assert_eq!(helper.get_field_text().as_str(), "hello");
    helper.set_field_text(SharedString::from("kabosu-4k"));
    assert_eq!(helper.get_field_text().as_str(), "kabosu-4k");

    assert_eq!(helper.get_select_idx(), 1);
    helper.set_select_idx(2);
    assert_eq!(helper.get_select_idx(), 2);

    assert_eq!(helper.get_slider_val(), 0.0);
    helper.set_slider_val(-6.0);
    assert!((helper.get_slider_val() + 6.0).abs() < 0.001);

    // VolumeMeter width must be >= 56px so full "-60" dB labels render without truncation
    assert!(helper.get_meter_width() >= 56.0, "VolumeMeter width must be >= 56px to prevent dB label clipping");

    // 7. Source-Level Layout & Dark-Theme Integrity Guards
    let main_src = std::fs::read_to_string("src/ui/main.slint").expect("read main.slint");
    assert!(
        main_src.contains("Palette.color-scheme = ColorScheme.dark;"),
        "MainWindow must initialize dark Palette.color-scheme so native fallbacks never render light"
    );

    let transcoder_src = std::fs::read_to_string("src/ui/views/transcoder_tab.slint").expect("read transcoder_tab");
    assert!(!transcoder_src.contains("Rectangle { height: 1px;"), "Transcoder tab must not contain redundant 1px divider lines");

    let edit_panel_src = std::fs::read_to_string("src/ui/views/edit_panel.slint").expect("read edit_panel");
    assert!(edit_panel_src.contains("horizontal-stretch: 0;"), "EditPanel must specify horizontal-stretch: 0 to avoid being squeezed or stretched");
    assert!(edit_panel_src.contains("width: 100%;"), "EditPanel ScrollView VerticalLayout must have width: 100% to prevent child overflow");
    assert!(edit_panel_src.contains("padding-right: 12px;"), "EditPanel ScrollView VerticalLayout must provide right padding gutter for scrollbar");

    assert!(edit_panel_src.contains("Quick Preset dB Scale (-12dB to +12dB)"), "Audio gain dB presets must be organized in a compact scale");
    assert!(edit_panel_src.contains("Fine-Tuning Step Nudge (-1.0 dB / +1.0 dB)"), "Audio gain fine tuning must be organized with compact stepper buttons");
    assert!(edit_panel_src.contains("width: 68px;"), "Fine-tuning buttons must have reduced, fixed width rather than stretching");

    let action_btn_src = std::fs::read_to_string("src/ui/components/action_button.slint").expect("read action_button");
    assert!(action_btn_src.contains("overflow: elide;"), "ActionButton text must have overflow: elide to prevent layout blowouts");

    // Guard against un-themed light-mode native widgets leaking back into any view
    for view_path in [
        "src/ui/views/downloader_tab.slint",
        "src/ui/views/player_tab.slint",
        "src/ui/views/edit_panel.slint",
        "src/ui/views/transcoder_tab.slint",
        "src/ui/views/queue_panel.slint",
    ] {
        let src = std::fs::read_to_string(view_path).expect("read view slint file");
        for line in src.lines().filter(|l| l.contains("std-widgets.slint")) {
            assert!(
                !line.contains("LineEdit") && !line.contains("ComboBox") && !line.contains("Slider"),
                "{} must not import un-themed std-widgets ({})",
                view_path,
                line.trim()
            );
        }
    }
    // 8. Exercise keyboard focus + key dispatch cleanly on the same Slint platform thread
    helper.show().unwrap();
    let last_slider = Rc::new(RefCell::new(0.0f32));
    {
        let ls = last_slider.clone();
        helper.on_slider_changed(move |v| {
            *ls.borrow_mut() = v;
        });
    }
    helper.window().dispatch_event(WindowEvent::KeyPressed {
        text: Key::Tab.into(),
    });
    helper.window().dispatch_event(WindowEvent::KeyReleased {
        text: Key::Tab.into(),
    });
    assert!(helper.get_slider_val().is_finite());
}
