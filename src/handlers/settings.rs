use slint::ComponentHandle;
use crate::context::AppContext;
use crate::{AppData, AppLogic, MainWindow};
use another_encoder::settings;

/// Registers general settings handlers for user preferences (folder reveal, sound alerts).
pub fn register(ui: &MainWindow, _ctx: &AppContext) {
    // Toggle whether to open destination folder when batch finishes
    let ui_folder = ui.as_weak();
    ui.global::<AppLogic>().on_toggle_open_folder_on_finish(move || {
        if let Some(ui) = ui_folder.upgrade() {
            let cur = ui.global::<AppData>().get_open_folder_on_finish();
            let new_val = !cur;
            ui.global::<AppData>().set_open_folder_on_finish(new_val);
            let mut s = settings::load_settings();
            s.open_folder_on_finish = new_val;
            let _ = settings::save_settings(&s);
        }
    });

    // Toggle whether to play a notification sound when encoding completes
    let ui_sound = ui.as_weak();
    ui.global::<AppLogic>().on_toggle_play_sound_on_finish(move || {
        if let Some(ui) = ui_sound.upgrade() {
            let cur = ui.global::<AppData>().get_play_sound_on_finish();
            let new_val = !cur;
            ui.global::<AppData>().set_play_sound_on_finish(new_val);
            let mut s = settings::load_settings();
            s.play_sound_on_finish = new_val;
            let _ = settings::save_settings(&s);
        }
    });
}
