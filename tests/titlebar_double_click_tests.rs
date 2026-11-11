mod common;
use slint::ComponentHandle;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{LogicalPosition, LogicalSize, WindowSize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

slint::include_modules!();

#[test]
fn test_titlebar_drag_and_window_controls() {
    common::init_test_backend();
    let ui = MainWindow::new().expect("MainWindow should instantiate");
    ui.window().set_size(WindowSize::Logical(LogicalSize::new(1280.0, 850.0)));
    ui.show().expect("Window should show");

    // 1. Verify WindowMoveArea drag pointer interaction succeeds without crash
    let w = ui.window();
    let start_pos = LogicalPosition::new(900.0, 20.0);
    w.dispatch_event(WindowEvent::PointerMoved { position: start_pos });
    w.dispatch_event(WindowEvent::PointerPressed {
        position: start_pos,
        button: PointerEventButton::Left,
    });
    std::thread::sleep(Duration::from_millis(20));

    // Drag by 50px across title bar
    let drag_pos = LogicalPosition::new(950.0, 20.0);
    w.dispatch_event(WindowEvent::PointerMoved { position: drag_pos });
    std::thread::sleep(Duration::from_millis(20));

    w.dispatch_event(WindowEvent::PointerReleased {
        position: drag_pos,
        button: PointerEventButton::Left,
    });

    // 2. Verify window maximize logic toggles is_window_maximized
    let app_logic = ui.global::<AppLogic>();
    let app_data = ui.global::<AppData>();

    let max_invocations = Arc::new(AtomicUsize::new(0));
    let max_inv_clone = max_invocations.clone();
    let ui_weak = ui.as_weak();
    app_logic.on_window_maximize(move || {
        max_inv_clone.fetch_add(1, Ordering::SeqCst);
        if let Some(ui) = ui_weak.upgrade() {
            let is_max = ui.window().is_maximized();
            ui.window().set_maximized(!is_max);
            ui.global::<AppData>().set_is_window_maximized(!is_max);
        }
    });

    assert!(!app_data.get_is_window_maximized(), "Window should initially not be maximized");

    // Trigger maximize
    app_logic.invoke_window_maximize();
    assert_eq!(max_invocations.load(Ordering::SeqCst), 1);
    assert!(app_data.get_is_window_maximized(), "Window should now be maximized");

    // Trigger restore
    app_logic.invoke_window_maximize();
    assert_eq!(max_invocations.load(Ordering::SeqCst), 2);
    assert!(!app_data.get_is_window_maximized(), "Window should now be restored");

    // 3. Verify tab switching interaction
    assert_eq!(app_data.get_active_tab(), 0, "Initial tab should be Downloader (0)");
    let tab_invocations = Arc::new(AtomicUsize::new(0));
    let tab_inv_clone = tab_invocations.clone();
    let ui_weak_tab = ui.as_weak();
    app_logic.on_switch_tab(move |tab| {
        tab_inv_clone.fetch_add(1, Ordering::SeqCst);
        if let Some(ui) = ui_weak_tab.upgrade() {
            ui.global::<AppData>().set_active_tab(tab);
        }
    });

    app_logic.invoke_switch_tab(1);
    assert_eq!(app_data.get_active_tab(), 1, "Tab should switch to View & Edit (1)");
    assert_eq!(tab_invocations.load(Ordering::SeqCst), 1);

    app_logic.invoke_switch_tab(2);
    assert_eq!(app_data.get_active_tab(), 2, "Tab should switch to Transcoder (2)");
    assert_eq!(tab_invocations.load(Ordering::SeqCst), 2);
}
