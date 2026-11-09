pub mod downloader;
pub mod editor;
pub mod encoder;
pub mod playback;
pub mod queue;
pub mod settings;
pub mod timeline;

use crate::context::AppContext;
use crate::MainWindow;

/// Register all Slint event callbacks onto the MainWindow and global controllers.
pub fn register_all(ui: &MainWindow, ctx: &AppContext) {
    editor::register(ui, ctx);
    timeline::register(ui, ctx);
    playback::register(ui, ctx);
    queue::register(ui, ctx);
    downloader::register(ui, ctx);
    encoder::register(ui, ctx);
    settings::register(ui, ctx);
}

#[cfg(test)]
pub fn init_test_backend() {
    let _ = slint::platform::set_platform(Box::new(
        i_slint_backend_testing::TestingBackend::new(
            i_slint_backend_testing::TestingBackendOptions {
                mock_time: false,
                threading: true,
            },
        ),
    ));
}
