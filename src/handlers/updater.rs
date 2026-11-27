use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use slint::{ComponentHandle, SharedString};

use crate::context::AppContext;
use crate::{AppData, AppLogic, MainWindow};
use kabosu_converter::sanitize::ensure_path_within_dir;
use kabosu_converter::updater::{self, GITHUB_REPO};

static IS_CHECKING: AtomicBool = AtomicBool::new(false);
static IS_DOWNLOADING: AtomicBool = AtomicBool::new(false);

/// Resets an `AtomicBool` flag when dropped so early returns or panics never leave it stuck.
struct AtomicFlagGuard(&'static AtomicBool);

impl Drop for AtomicFlagGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn default_download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::video_dir)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Registers update checking, downloading, and reveal-in-explorer callbacks.
pub fn register(ui: &MainWindow, ctx: &AppContext) {
    // Check for updates
    {
        let ui_weak = ui.as_weak();
        let latest_release_store = ctx.latest_release.clone();

        ui.global::<AppLogic>().on_check_for_updates(move || {
            if IS_CHECKING
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return;
            }
            let guard = AtomicFlagGuard(&IS_CHECKING);

            if let Some(ui) = ui_weak.upgrade() {
                let data = ui.global::<AppData>();
                data.set_update_status(1); // Checking
                data.set_update_status_text("Checking for updates...".into());
            }

            let ui_cb = ui_weak.clone();
            let store = latest_release_store.clone();

            std::thread::spawn(move || {
                let _guard = guard;
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };

                rt.block_on(async move {
                    let res = updater::check_latest_release(GITHUB_REPO).await;

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
                                        data.set_has_update_available(true);
                                        data.set_update_status(3); // Update Available
                                        data.set_update_latest_version(SharedString::from(
                                            &remote_ver,
                                        ));
                                        data.set_update_release_url(SharedString::from(
                                            &release.html_url,
                                        ));
                                        data.set_update_status_text(SharedString::from(format!(
                                            "Update to {}",
                                            remote_ver
                                        )));
                                    } else {
                                        data.set_has_update_available(false);
                                        data.set_update_status(2); // Up to date
                                        data.set_update_status_text("Up to date".into());
                                    }
                                }
                            });
                        }
                        Err(_) => {
                            let ui_dispatch = ui_cb.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_dispatch.upgrade() {
                                    let data = ui.global::<AppData>();
                                    data.set_has_update_available(false);
                                    data.set_update_status(6); // Error
                                    data.set_update_status_text(
                                        "Check failed (Click to retry)".into(),
                                    );
                                }
                            });
                        }
                    }
                });
            });
        });
    }

    // Download update
    {
        let ui_weak = ui.as_weak();
        let latest_release_store = ctx.latest_release.clone();

        ui.global::<AppLogic>().on_download_update(move || {
            let release_opt = latest_release_store
                .lock()
                .ok()
                .and_then(|guard| guard.clone());

            let Some(release) = release_opt else {
                if let Some(ui) = ui_weak.upgrade() {
                    ui.global::<AppLogic>().invoke_check_for_updates();
                }
                return;
            };

            if IS_DOWNLOADING
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return;
            }
            let guard = AtomicFlagGuard(&IS_DOWNLOADING);

            if let Some(ui) = ui_weak.upgrade() {
                let data = ui.global::<AppData>();
                data.set_update_status(4); // Downloading
                data.set_update_download_progress(0.0);
                data.set_update_status_text("Downloading update... 0%".into());
            }

            let ui_cb = ui_weak.clone();

            std::thread::spawn(move || {
                let _guard = guard;
                let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };

                rt.block_on(async move {
                    let Some(asset) = updater::find_platform_asset(&release.assets) else {
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
                    };

                    let download_dir = default_download_dir();
                    let safe_name = match updater::sanitize_asset_filename(&asset.name) {
                        Ok(name) => name,
                        Err(_) => {
                            let ui_dispatch = ui_cb.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_dispatch.upgrade() {
                                    let data = ui.global::<AppData>();
                                    data.set_update_status(6);
                                    data.set_update_status_text(
                                        "Download blocked: invalid filename".into(),
                                    );
                                }
                            });
                            return;
                        }
                    };

                    let dest_path = download_dir.join(safe_name);
                    if ensure_path_within_dir(&dest_path, &download_dir).is_err() {
                        let ui_dispatch = ui_cb.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = ui_dispatch.upgrade() {
                                let data = ui.global::<AppData>();
                                data.set_update_status(6);
                                data.set_update_status_text(
                                    "Download blocked: invalid path".into(),
                                );
                            }
                        });
                        return;
                    }

                    let expected_hash =
                        match updater::build_http_client(Some(Duration::from_secs(30))) {
                            Ok(client) => {
                                updater::fetch_release_checksum(&client, &release.assets, safe_name)
                                    .await
                            }
                            Err(_) => None,
                        };

                    let ui_prog = ui_cb.clone();
                    let download_start = Instant::now();
                    let last_tick_ms = AtomicU64::new(0);

                    let res = updater::download_release_asset(
                        &asset.browser_download_url,
                        &dest_path,
                        expected_hash.as_deref(),
                        move |ratio, downloaded, total| {
                            let elapsed_ms = download_start.elapsed().as_millis() as u64;
                            let prev_ms = last_tick_ms.load(Ordering::Relaxed);
                            if ratio < 1.0 && prev_ms > 0 && elapsed_ms.saturating_sub(prev_ms) < 50
                            {
                                return;
                            }
                            last_tick_ms.store(elapsed_ms.max(1), Ordering::Relaxed);

                            let percent = (ratio * 100.0).clamp(0.0, 100.0);
                            let status_str = if total > 0 {
                                format!(
                                    "Downloading... {:.0}% ({:.1}/{:.1} MB)",
                                    percent,
                                    downloaded as f64 / 1_048_576.0,
                                    total as f64 / 1_048_576.0
                                )
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
                    )
                    .await;

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
                                    data.set_update_status_text(
                                        "Update downloaded (Click to open)".into(),
                                    );
                                }
                            });
                            crate::utils::reveal_in_explorer(&dest_path);
                        }
                        Err(_) => {
                            let ui_dispatch = ui_cb.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(ui) = ui_dispatch.upgrade() {
                                    let data = ui.global::<AppData>();
                                    data.set_update_status(6); // Error
                                    data.set_update_status_text(
                                        "Download failed (Click to retry)".into(),
                                    );
                                }
                            });
                        }
                    }
                });
            });
        });
    }

    // Open downloaded update
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
                crate::utils::open_folder_in_explorer(&default_download_dir());
            }
        });
    }
}
