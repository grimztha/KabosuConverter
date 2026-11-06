use slint::ComponentHandle;
use crate::context::AppContext;
use crate::{AppData, AppLogic, MainWindow};
use another_encoder::settings;

pub fn register(ui: &MainWindow, _ctx: &AppContext) {
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
