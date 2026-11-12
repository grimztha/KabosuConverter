pub mod binaries;
pub mod ffmpeg;
pub mod sanitize;
pub mod settings;
pub mod updater;
pub mod ytdlp;

/// Windows CREATE_NO_WINDOW creation flag to hide console windows for spawned processes.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Helper to set the CREATE_NO_WINDOW creation flag on a Command (Windows only).
#[cfg(windows)]
pub fn set_no_window(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub fn set_no_window(_cmd: &mut std::process::Command) {
    // No console window to hide on non-Windows platforms.
}
