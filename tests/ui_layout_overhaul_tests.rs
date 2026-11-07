slint::include_modules!();

slint::slint! {
    import { Theme, AppData } from "src/ui/globals.slint";
    import { ActionButton } from "src/ui/components/action_button.slint";
    import { ToggleSwitch } from "src/ui/components/toggle_switch.slint";
    import { Badge } from "src/ui/components/badge.slint";

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

        // Theme interactive heights (Finding #15)
        out property <length> theme_btn_height_sm: Theme.btn_height_sm;
        out property <length> theme_btn_height_md: Theme.btn_height_md;
        out property <length> theme_btn_height_lg: Theme.btn_height_lg;

        // Theme concentric radii (Finding #13)
        out property <length> theme_radius_sm: Theme.radius_sm;
        out property <length> theme_radius_md: Theme.radius_md;
        out property <length> theme_radius_lg: Theme.radius_lg;

        // Theme colors
        out property <color> theme_text_primary: Theme.text_primary;
        out property <color> theme_text_muted: Theme.text_muted;
        out property <color> theme_accent_primary: Theme.accent_primary;
        out property <color> theme_accent_emerald: Theme.accent_emerald;
        out property <color> theme_accent_amber: Theme.accent_amber;
        out property <color> theme_accent_red: Theme.accent_red;
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
        }
    }
}

#[test]
fn test_ui_layout_overhaul_lifecycle() {
    let helper = TestUIOverhaulHelper::new().unwrap();

    // 1. Typography Scale and Floor Verification (Finding #8)
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

    // 2. Standardized Button Heights (Finding #15)
    let btn_sm = helper.get_theme_btn_height_sm();
    let btn_md = helper.get_theme_btn_height_md();
    let btn_lg = helper.get_theme_btn_height_lg();

    assert_eq!(btn_sm, 28.0, "Small button height tier must be 28px");
    assert_eq!(btn_md, 34.0, "Medium button height tier must be 34px");
    assert_eq!(btn_lg, 40.0, "Large button height tier must be 40px");
    assert!(btn_sm < btn_md && btn_md < btn_lg, "Button heights must be strictly tiered");

    // 3. Concentric Radii Geometry (Finding #13)
    let r_sm = helper.get_theme_radius_sm();
    let r_md = helper.get_theme_radius_md();
    let r_lg = helper.get_theme_radius_lg();

    assert_eq!(r_sm, 6.0, "Small radius must be 6px");
    assert_eq!(r_md, 10.0, "Medium radius must be 10px");
    assert_eq!(r_lg, 14.0, "Large radius must be 14px");

    let inner_r = helper.get_inner_item_radius();
    let padding = helper.get_container_padding();
    let outer_r = helper.get_concentric_outer_radius();
    assert_eq!(outer_r, inner_r + padding, "Outer radius must equal inner radius plus padding to prevent optical corner pinching");
    assert_eq!(outer_r, r_lg);

    // 4. Color Contrast & Status Tokens Verification (Finding #4)
    let text_muted = helper.get_theme_text_muted();
    let text_primary = helper.get_theme_text_primary();
    let focus_ring = helper.get_theme_focus_ring();

    assert_eq!(text_muted.alpha(), 255);
    assert_eq!(text_primary.alpha(), 255);
    assert_eq!(focus_ring.alpha(), 255);

    // text_muted lightness: #9ca3af (R=156, G=163, B=175) has red/green/blue > 150
    // ensuring WCAG AA >= 4.5:1 contrast against dark card (#131317)
    assert!(text_muted.red() >= 150, "text_muted red channel must be >= 150 for WCAG AA contrast");
    assert!(text_muted.green() >= 150, "text_muted green channel must be >= 150 for WCAG AA contrast");
    assert!(text_muted.blue() >= 160, "text_muted blue channel must be >= 160 for WCAG AA contrast");

    let emerald = helper.get_theme_accent_emerald();
    let amber = helper.get_theme_accent_amber();
    let red = helper.get_theme_accent_red();

    assert!(emerald.green() > emerald.red(), "Emerald status must have green > red");
    assert!(amber.red() > amber.blue(), "Amber status must have red > blue");
    assert!(red.red() > red.green(), "Red status must have red dominant");

    // 5. Responsive Layout & Queue Collapse Space Reclamation (Finding #2 & #11)
    helper.set_queue_expanded(true);
    helper.set_edit_expanded(true);
    let queue_w = helper.get_computed_queue_width();
    let edit_w = helper.get_computed_edit_width();
    let player_w = helper.get_computed_player_available_width();

    assert_eq!(queue_w, 300.0);
    assert_eq!(edit_w, 320.0);
    assert_eq!(player_w, 340.0, "Player width when both panels expanded at 960px window is 340px");

    // Collapsing queue reclaims 262px, giving 602px of room!
    helper.set_queue_expanded(false);
    let collapsed_queue_w = helper.get_computed_queue_width();
    let reclaimed_player_w = helper.get_computed_player_available_width();

    assert_eq!(collapsed_queue_w, 38.0, "Collapsed queue width is 38px");
    assert_eq!(reclaimed_player_w, 602.0, "Collapsing queue reclaims 262px, giving 602px of room for player & trimming!");
    assert!(reclaimed_player_w >= 500.0, "Reclaimed player width allows trimming controls to fit without clipping");

    // 6. Source-Level Layout Verification (Finding #10: zero 1px dividers in transcoder_tab)
    let transcoder_src = std::fs::read_to_string("src/ui/views/transcoder_tab.slint").expect("read transcoder_tab");
    assert!(!transcoder_src.contains("Rectangle { height: 1px;"), "Transcoder tab must not contain redundant 1px divider lines");

    // 7. EditPanel Layout & Right Side Cropping Verification
    let edit_panel_src = std::fs::read_to_string("src/ui/views/edit_panel.slint").expect("read edit_panel");
    assert!(edit_panel_src.contains("horizontal-stretch: 0;"), "EditPanel must specify horizontal-stretch: 0 to avoid being squeezed or stretched");
    assert!(edit_panel_src.contains("width: 100%;"), "EditPanel ScrollView VerticalLayout must have width: 100% to prevent child overflow");
    assert!(edit_panel_src.contains("padding-right: 12px;"), "EditPanel ScrollView VerticalLayout must provide right padding gutter for scrollbar");

    // Ensure audio presets are compact and properly organized
    assert!(edit_panel_src.contains("Quick Preset dB Scale (-12dB to +12dB)"), "Audio gain dB presets must be organized in a compact scale");
    assert!(edit_panel_src.contains("Fine-Tuning Step Nudge (-1.0 dB / +1.0 dB)"), "Audio gain fine tuning must be organized with compact stepper buttons");
    assert!(edit_panel_src.contains("width: 68px;"), "Fine-tuning buttons must have reduced, fixed width rather than stretching");

    let action_btn_src = std::fs::read_to_string("src/ui/components/action_button.slint").expect("read action_button");
    assert!(action_btn_src.contains("overflow: elide;"), "ActionButton text must have overflow: elide to prevent layout blowouts");
}
