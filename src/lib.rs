pub mod binaries;
pub mod ffmpeg;
pub mod sanitize;
pub mod settings;
pub mod updater;
pub mod ytdlp;

/// Windows creation flag that prevents a console window from flashing when spawning CLI tools.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Suppresses the console window on Windows for spawned background processes.
#[cfg(windows)]
pub fn set_no_window(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// No-op on non-Windows platforms.
#[cfg(not(windows))]
pub fn set_no_window(_cmd: &mut std::process::Command) {}
