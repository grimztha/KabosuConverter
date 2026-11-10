use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use slint::{ComponentHandle, SharedString};
use crate::context::AppContext;
use crate::{AppData, AppLogic, MainWindow};
use kabosu_converter::updater::{self, GITHUB_REPO};

static IS_CHECKING: AtomicBool = AtomicBool::new(false);
static IS_DOWNLOADING: AtomicBool = AtomicBool::new(false);

/// Registers update checking and downloading callbacks on the Slint MainWindow.
pub fn register(ui: &MainWindow, ctx: &AppContext) {
    // 1. Check for updates callback
    {
        let ui_weak = ui.as_weak();
        let latest_release_store = ctx.latest_release.clone();

        ui.global::<AppLogic>().on_check_for_updates(move || {
            if IS_CHECKING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
                return;
            }

            if let Some(ui) = ui_weak.upgrade() {
                let data = ui.global::<AppData>();
                data.set_update_status(1); // Checking
                data.set_update_status_text("Checking for updates...".into());
            }

            let ui_cb = ui_weak.clone();
            let store = latest_release_store.clone();

            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();

                if let Ok(rt) = rt {
                    rt.block_on(async move {
                        let res = updater::check_latest_release(GITHUB_REPO).await;
                        IS_CHECKING.store(false, Ordering::SeqCst);

                        match res {
                            Ok(release) => {
                                let remote_ver = release.tag_name.clone();
                                let current_ver = env!("CARGO_PKG_VERSION");
                                let is_newer = updater::is_newer_version(current_ver, &remote_ver);

                                if let Ok(mut lock) = store.lock() {
                                    *lock = Some(release.clone());
                                }

                                let ui_dispatch = ui_cb.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_dispatch.upgrade() {
                                        let data = ui.global::<AppData>();
                                        if is_newer {
                                            data.set_update_status(3); // Update Available
                                            data.set_update_latest_version(SharedString::from(&remote_ver));
                                            data.set_update_release_url(SharedString::from(&release.html_url));
                                            data.set_update_status_text(SharedString::from(format!("Update to {}", remote_ver)));
                                        } else {
                                            data.set_update_status(2); // Up to date
                                            data.set_update_status_text("Up to date".into());
                                        }
                                    }
                                });
                            }
                            Err(_e) => {
                                let ui_dispatch = ui_cb.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_dispatch.upgrade() {
                                        let data = ui.global::<AppData>();
                                        data.set_update_status(6); // Error
                                        data.set_update_status_text("Check failed (Click to retry)".into());
                                    }
                                });
                            }
                        }
                    });
                } else {
                    IS_CHECKING.store(false, Ordering::SeqCst);
                }
            });
        });
    }

    // 2. Download update callback
    {
        let ui_weak = ui.as_weak();
        let latest_release_store = ctx.latest_release.clone();

        ui.global::<AppLogic>().on_download_update(move || {
            let release_opt = match latest_release_store.lock() {
                Ok(guard) => guard.clone(),
                Err(_) => None,
            };

            let release = match release_opt {
                Some(r) => r,
                None => {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.global::<AppLogic>().invoke_check_for_updates();
                    }
                    return;
                }
            };

            if IS_DOWNLOADING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
                return;
            }

            if let Some(ui) = ui_weak.upgrade() {
                let data = ui.global::<AppData>();
                data.set_update_status(4); // Downloading
                data.set_update_download_progress(0.0);
                data.set_update_status_text("Downloading update... 0%".into());
            }

            let ui_cb = ui_weak.clone();

            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();

                if let Ok(rt) = rt {
                    rt.block_on(async move {
                        let asset_opt = updater::find_platform_asset(&release.assets);

                        let asset = match asset_opt {
                            Some(a) => a,
                            None => {
                                IS_DOWNLOADING.store(false, Ordering::SeqCst);
                                crate::utils::open_url(&release.html_url);
                                let ui_dispatch = ui_cb.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_dispatch.upgrade() {
                                        let data = ui.global::<AppData>();
                                        data.set_update_status(2);
                                        data.set_update_status_text("Opened in browser".into());
                                    }
                                });
                                return;
                            }
                        };

                        let download_dir = dirs::download_dir()
                            .or_else(dirs::video_dir)
                            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

                        let dest_path = download_dir.join(&asset.name);
                        let download_url = asset.browser_download_url.clone();
                        let ui_prog = ui_cb.clone();

                        let res = updater::download_release_asset(
                            &download_url,
                            &dest_path,
                            move |ratio, downloaded, total| {
                                let percent = (ratio * 100.0).clamp(0.0, 100.0);
                                let status_str = if total > 0 {
                                    format!("Downloading... {:.0}% ({:.1}/{:.1} MB)",
                                        percent,
                                        downloaded as f64 / 1_048_576.0,
                                        total as f64 / 1_048_576.0)
                                } else {
                                    format!("Downloading... {:.1} MB", downloaded as f64 / 1_048_576.0)
                                };
                                let ui_disp = ui_prog.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_disp.upgrade() {
                                        let data = ui.global::<AppData>();
                                        data.set_update_download_progress(ratio);
                                        data.set_update_status_text(SharedString::from(status_str));
                                    }
                                });
                            },
                        ).await;

                        IS_DOWNLOADING.store(false, Ordering::SeqCst);

                        match res {
                            Ok(()) => {
                                let path_str = dest_path.to_string_lossy().to_string();
                                let ui_dispatch = ui_cb.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_dispatch.upgrade() {
                                        let data = ui.global::<AppData>();
                                        data.set_update_status(5); // Downloaded
                                        data.set_update_download_progress(1.0);
                                        data.set_update_downloaded_path(SharedString::from(&path_str));
                                        data.set_update_status_text("Update downloaded (Click to open)".into());
                                    }
                                });
                                crate::utils::reveal_in_explorer(&dest_path);
                            }
                            Err(_e) => {
                                let ui_dispatch = ui_cb.clone();
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(ui) = ui_dispatch.upgrade() {
                                        let data = ui.global::<AppData>();
                                        data.set_update_status(6); // Error
                                        data.set_update_status_text("Download failed (Click to retry)".into());
                                    }
                                });
                            }
                        }
                    });
                } else {
                    IS_DOWNLOADING.store(false, Ordering::SeqCst);
                }
            });
        });
    }

    // 3. Open downloaded update callback
    {
        let ui_weak = ui.as_weak();
        ui.global::<AppLogic>().on_open_downloaded_update(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let data = ui.global::<AppData>();
                let path_str = data.get_update_downloaded_path().trim().to_string();
                if !path_str.is_empty() {
                    let path = PathBuf::from(path_str);
                    if path.exists() {
                        crate::utils::reveal_in_explorer(&path);
                        return;
                    }
                }
                let download_dir = dirs::download_dir()
                    .or_else(dirs::video_dir)
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
                crate::utils::open_folder_in_explorer(&download_dir);
            }
        });
    }
}
