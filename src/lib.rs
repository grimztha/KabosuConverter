pub mod binaries;
pub mod ffmpeg;
pub mod sanitize;
pub mod settings;
pub mod updater;
pub mod ytdlp;

/// Prevents a console window from flashing when spawning CLI tools on Windows.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Suppresses the console window for spawned child processes on Windows.
#[cfg(windows)]
pub fn set_no_window(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn set_no_window(_cmd: &mut std::process::Command) {}
