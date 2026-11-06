use slint::ComponentHandle;
use crate::context::AppContext;
use crate::{AppData, AppLogic, MainWindow};
use another_encoder::settings;

/// Registers general settings handlers for user preferences (folder reveal, sound alerts).
pub fn register(ui: &MainWindow, _ctx: &AppContext) {
    // Update and persist whether to open destination folder when export finishes
    let ui_folder = ui.as_weak();
    ui.global::<AppLogic>().on_set_open_folder_on_finish(move |val| {
        if let Some(ui) = ui_folder.upgrade() {
            ui.global::<AppData>().set_open_folder_on_finish(val);
            let mut s = settings::load_settings();
            s.open_folder_on_finish = val;
            let _ = settings::save_settings(&s);
        }
    });

    // Update and persist whether to play a notification sound when export completes
    let ui_sound = ui.as_weak();
    ui.global::<AppLogic>().on_set_play_sound_on_finish(move |val| {
        if let Some(ui) = ui_sound.upgrade() {
            ui.global::<AppData>().set_play_sound_on_finish(val);
            let mut s = settings::load_settings();
            s.play_sound_on_finish = val;
            let _ = settings::save_settings(&s);
        }
    });
}
