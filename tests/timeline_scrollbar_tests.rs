use slint::ComponentHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::LogicalPosition;

slint::include_modules!();

slint::slint! {
    import { ScrollView } from "std-widgets.slint";

    component MockTimelineScrollBar inherits Rectangle {
        in property <length> track_width;
        in property <length> viewport_width;
        in property <float> zoom_level: 1.0;
        in property <length> content_x: 0px;
        in property <float> duration: 1.0;
        in property <float> current_time: 0.0;
        in property <bool> has_trim: false;
        in property <float> in_point_norm: 0.0;
        in property <float> out_point_norm: 1.0;
        callback scroll_to(length);

        height: 12px;
        background: #09090d;
        border-radius: 4px;
        border-width: 1px;
        border-color: #1c1c24;
        clip: true;

        out property <length> max_scroll: root.track_width > root.viewport_width
            ? (root.track_width - root.viewport_width)
            : max(0px, root.viewport_width * (max(1.0, root.zoom_level) - 1.0));
        out property <length> current_scroll: (-root.content_x).clamp(0px, self.max_scroll);
        out property <float> scroll_ratio: self.max_scroll > 0px ? (self.current_scroll / self.max_scroll) : 0.0;
        out property <length> thumb_w: root.width > 0px ? min(root.width, max(28px, root.width / max(1.0, root.zoom_level))) : 0px;
        out property <length> max_travel: max(0px, root.width - self.thumb_w);
        out property <length> thumb_x: self.max_travel * self.scroll_ratio;

        sb_ta := TouchArea {
            mouse-cursor: pointer;
            property <length> drag_offset: 0px;

            pointer-event(event) => {
                if event.kind == PointerEventKind.down {
                    if self.mouse-x >= root.thumb_x && self.mouse-x <= root.thumb_x + root.thumb_w {
                        self.drag_offset = self.mouse-x - root.thumb_x;
                    } else {
                        self.drag_offset = root.thumb_w / 2;
                        if root.max_travel > 0px && root.max_scroll > 0px {
                            let target_x = (self.mouse-x - self.drag_offset).clamp(0px, root.max_travel);
                            let r = target_x / root.max_travel;
                            root.scroll_to(- (r * root.max_scroll));
                        }
                    }
                }
            }

            clicked => {
                if root.max_travel > 0px && root.max_scroll > 0px {
                    if self.mouse-x < root.thumb_x || self.mouse-x > root.thumb_x + root.thumb_w {
                        let target_x = (self.mouse-x - root.thumb_w / 2).clamp(0px, root.max_travel);
                        let r = target_x / root.max_travel;
                        root.scroll_to(- (r * root.max_scroll));
                    }
                }
            }

            moved => {
                if self.pressed && root.max_travel > 0px && root.max_scroll > 0px {
                    let target_x = (self.mouse-x - self.drag_offset).clamp(0px, root.max_travel);
                    let r = target_x / root.max_travel;
                    root.scroll_to(- (r * root.max_scroll));
                }
            }
        }
    }

    component MockVisualTimeline inherits Rectangle {
        in property <float> zoom_level: 2.0;
        out property <length> current_cx: sv.content-x;
        out property <length> sb_pos_y: sb.absolute-position.y;
        out property <length> sb_pos_x: sb.absolute-position.x;
        out property <length> sb_w: sb.width;
        out property <length> sb_thumb_x: sb.thumb_x;

        height: 87px;

        VerticalLayout {
            padding: 0px;
            spacing: 0px;

            sv := ScrollView {
                horizontal-scrollbar-policy: ScrollBarPolicy.always-off;
                vertical-scrollbar-policy: ScrollBarPolicy.always-off;
                vertical-stretch: 1;
                content-width: max(self.width, self.width * root.zoom_level);

                track := Rectangle {
                    width: max(parent.width, parent.width * root.zoom_level);
                    height: 100%;
                }
            }

            Rectangle {
                height: 1px;
                background: #1c1c22;
            }

            HorizontalLayout {
                padding-left: 6px;
                padding-right: 6px;
                padding-top: 3px;
                padding-bottom: 3px;

                sb := MockTimelineScrollBar {
                    height: 12px;
                    track_width: track.width;
                    viewport_width: sv.width;
                    zoom_level: root.zoom_level;
                    content_x: sv.content-x;
                    scroll_to(val) => {
                        let max_s = max(0px, max(sv.width, sv.width * root.zoom_level) - sv.width);
                        sv.content-x = val.clamp(-max_s, 0px);
                    }
                }
            }
        }
    }

    export component TestTimelineWindow inherits Window {
        width: 1000px;
        height: 600px;

        vt := MockVisualTimeline {
            x: 50px;
            y: 50px;
            width: 900px;
        }

        out property <length> timeline_cx: vt.current_cx;
        out property <length> sb_y: vt.sb_pos_y;
        out property <length> sb_x: vt.sb_pos_x;
        out property <length> sb_w: vt.sb_w;
        out property <length> sb_thumb_x: vt.sb_thumb_x;
    }
}

#[test]
fn test_timeline_scrollbar_full_lifecycle() {
    // 1. Verify MainWindow global zoom binding
    let ui = MainWindow::new().expect("MainWindow should instantiate successfully");
    let app_data = ui.global::<AppData>();
    assert_eq!(app_data.get_timeline_zoom(), 1.0);
    app_data.set_timeline_zoom(2.0);
    assert_eq!(app_data.get_timeline_zoom(), 2.0);
    let trigger_before = app_data.get_center_timeline_trigger();
    app_data.set_center_timeline_trigger(trigger_before + 1);
    assert_eq!(app_data.get_center_timeline_trigger(), trigger_before + 1);
    drop(ui);

    // 2. Test Timeline ScrollBar click and drag navigation
    let win = TestTimelineWindow::new().expect("TestTimelineWindow should instantiate");
    win.show().expect("Window should show");
    let w = win.window();

    let sx = win.get_sb_x();
    let sy = win.get_sb_y();
    let sw = win.get_sb_w();

    // Viewport content-x and thumb position start at 0
    assert_eq!(win.get_timeline_cx(), 0.0);
    assert_eq!(win.get_sb_thumb_x(), 0.0);

    // Click on the track at 75% width (to the right)
    let click_x = sx + sw * 0.75;
    let click_y = sy + 6.0;
    w.dispatch_event(WindowEvent::PointerPressed {
        position: LogicalPosition::new(click_x, click_y),
        button: PointerEventButton::Left,
    });

    assert!(win.get_timeline_cx() < -400.0, "Timeline viewport content-x should have scrolled left");
    assert!(win.get_sb_thumb_x() > 200.0, "Scrollbar thumb should have moved towards the right");

    // Drag right towards the end
    let drag_x = sx + sw - 10.0;
    w.dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(drag_x, click_y),
    });

    assert!(win.get_timeline_cx() <= -850.0, "Timeline should be panned close to the maximum scrollable range");

    // Release mouse pointer
    w.dispatch_event(WindowEvent::PointerReleased {
        position: LogicalPosition::new(drag_x, click_y),
        button: PointerEventButton::Left,
    });

    // Drag back towards the beginning
    w.dispatch_event(WindowEvent::PointerPressed {
        position: LogicalPosition::new(drag_x, click_y),
        button: PointerEventButton::Left,
    });
    let drag_back_x = sx + 50.0;
    w.dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(drag_back_x, click_y),
    });

    assert!(win.get_timeline_cx() > -200.0, "Timeline should have scrolled back near the beginning");

    w.dispatch_event(WindowEvent::PointerReleased {
        position: LogicalPosition::new(drag_back_x, click_y),
        button: PointerEventButton::Left,
    });
}
