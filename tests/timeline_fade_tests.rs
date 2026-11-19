mod common;

use std::cell::RefCell;
use std::rc::Rc;

use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, SharedString};

slint::slint! {
    import { TimelineFadeOverlay } from "src/ui/timeline/fade_overlay.slint";

    export component TestFadeWindow inherits Window {
        width: 500px;
        height: 120px;

        in-out property <float> duration: 50.0;
        in-out property <float> in_point_norm: 0.10;
        in-out property <float> out_point_norm: 0.90;
        in-out property <bool> has_trim: false;
        in-out property <float> fade_in_sec: 0.0;
        in-out property <float> fade_out_sec: 0.0;
        in-out property <string> fade_in_text: "0.00s";
        in-out property <string> fade_out_text: "0.00s";
        in-out property <bool> has_audio: true;

        callback fade_in_dragged(float);
        callback fade_out_dragged(float);
        callback fade_settled();

        TimelineFadeOverlay {
            width: 100%;
            height: 100%;
            duration: root.duration;
            in_point_norm: root.in_point_norm;
            out_point_norm: root.out_point_norm;
            has_trim: root.has_trim;
            fade_in_sec: root.fade_in_sec;
            fade_out_sec: root.fade_out_sec;
            fade_in_text: root.fade_in_text;
            fade_out_text: root.fade_out_text;
            has_audio: root.has_audio;
            fade_in_dragged(v) => { root.fade_in_dragged(v); }
            fade_out_dragged(v) => { root.fade_out_dragged(v); }
            fade_settled() => { root.fade_settled(); }
        }
    }
}

#[test]
fn test_timeline_fade_corner_drag_and_clamping() {
    common::init_test_backend();
    let win = TestFadeWindow::new().expect("create TestFadeWindow");
    win.show().expect("show TestFadeWindow");

    let settled_count = Rc::new(RefCell::new(0usize));

    // Wire realistic trim-clamping fade handlers matching src/handlers/timeline.rs
    {
        let w = win.as_weak();
        win.on_fade_in_dragged(move |raw| {
            if let Some(win) = w.upgrade() {
                let trim_len = (win.get_out_point_norm() - win.get_in_point_norm()) * win.get_duration();
                let max_in = (trim_len - win.get_fade_out_sec()).max(0.0);
                let val = if raw < 0.03 {
                    0.0
                } else {
                    ((raw.clamp(0.0, max_in) * 100.0).round() / 100.0).min(max_in)
                };
                win.set_fade_in_sec(val);
                win.set_fade_in_text(SharedString::from(format!("{:.2}s", val)));
            }
        });
    }
    {
        let w = win.as_weak();
        win.on_fade_out_dragged(move |raw| {
            if let Some(win) = w.upgrade() {
                let trim_len = (win.get_out_point_norm() - win.get_in_point_norm()) * win.get_duration();
                let max_out = (trim_len - win.get_fade_in_sec()).max(0.0);
                let val = if raw < 0.03 {
                    0.0
                } else {
                    ((raw.clamp(0.0, max_out) * 100.0).round() / 100.0).min(max_out)
                };
                win.set_fade_out_sec(val);
                win.set_fade_out_text(SharedString::from(format!("{:.2}s", val)));
            }
        });
    }
    {
        let c = settled_count.clone();
        win.on_fade_settled(move || {
            *c.borrow_mut() += 1;
        });
    }

    // Track width = 500px, duration = 50s => 10px per second.
    // Trim is 0.10..0.90 => in_x = 50px, out_x = 450px (trim_len = 40s).
    // Fade handles sit vertically around y = 18px (TouchArea spans y: 10..26px).

    // 0. When has_trim is false, fade corner handles must be hidden/disabled and ignore drags
    win.set_has_trim(false);
    win.window().dispatch_event(WindowEvent::PointerPressed {
        position: LogicalPosition::new(50.0, 18.0),
        button: PointerEventButton::Left,
    });
    win.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(95.0, 18.0),
    });
    win.window().dispatch_event(WindowEvent::PointerReleased {
        position: LogicalPosition::new(95.0, 18.0),
        button: PointerEventButton::Left,
    });
    assert_eq!(win.get_fade_in_sec(), 0.0, "Fade handles must stay disabled when has_trim is false");
    assert_eq!(*settled_count.borrow(), 0);

    // Enable trim highlight section so fade corner handles appear
    win.set_has_trim(true);

    // 1. Drag top-left ("IN") fade handle right by +45px => +4.50s fade-in
    win.window().dispatch_event(WindowEvent::PointerPressed {
        position: LogicalPosition::new(50.0, 18.0),
        button: PointerEventButton::Left,
    });
    win.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(95.0, 18.0),
    });
    win.window().dispatch_event(WindowEvent::PointerReleased {
        position: LogicalPosition::new(95.0, 18.0),
        button: PointerEventButton::Left,
    });

    assert!(
        (win.get_fade_in_sec() - 4.5).abs() < 0.05,
        "Expected fade-in ~4.50s after dragging right by 45px, got {}",
        win.get_fade_in_sec()
    );
    assert_eq!(win.get_fade_in_text().as_str(), "4.50s");
    assert_eq!(*settled_count.borrow(), 1);

    // 2. Drag top-right ("OUT") fade handle left by -72.6px => +7.26s fade-out
    win.window().dispatch_event(WindowEvent::PointerPressed {
        position: LogicalPosition::new(450.0, 18.0),
        button: PointerEventButton::Left,
    });
    win.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(377.4, 18.0),
    });
    win.window().dispatch_event(WindowEvent::PointerReleased {
        position: LogicalPosition::new(377.4, 18.0),
        button: PointerEventButton::Left,
    });

    assert!(
        (win.get_fade_out_sec() - 7.26).abs() < 0.05,
        "Expected fade-out ~7.26s after dragging left by 72.6px, got {}",
        win.get_fade_out_sec()
    );
    assert_eq!(win.get_fade_out_text().as_str(), "7.26s");
    assert_eq!(*settled_count.borrow(), 2);

    // 3. Attempting to drag fade-out past fade-in clamps cleanly so fade_in + fade_out <= trim_len (40s)
    win.window().dispatch_event(WindowEvent::PointerPressed {
        position: LogicalPosition::new(377.4, 18.0),
        button: PointerEventButton::Left,
    });
    win.window().dispatch_event(WindowEvent::PointerMoved {
        position: LogicalPosition::new(10.0, 18.0),
    });
    win.window().dispatch_event(WindowEvent::PointerReleased {
        position: LogicalPosition::new(10.0, 18.0),
        button: PointerEventButton::Left,
    });
    assert!(
        (win.get_fade_in_sec() + win.get_fade_out_sec() - 40.0).abs() < 0.05,
        "fade_in ({}) + fade_out ({}) must clamp to trim_len 40.0s",
        win.get_fade_in_sec(),
        win.get_fade_out_sec()
    );
}
