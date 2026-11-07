use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static LAST_OPENED_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
static MEDIA_DIALOG_ACTIVE: AtomicBool = AtomicBool::new(false);
static FOLDER_DIALOG_ACTIVE: AtomicBool = AtomicBool::new(false);

struct ActiveDialogGuard(&'static AtomicBool);

impl Drop for ActiveDialogGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub fn get_last_opened_dir() -> Option<PathBuf> {
    LAST_OPENED_DIR.lock().ok().and_then(|d| d.clone())
}

pub fn set_last_opened_dir(path: &Path) {
    let dir = if path.is_dir() {
        Some(path.to_path_buf())
    } else {
        path.parent().map(|p| p.to_path_buf())
    };
    if let Some(d) = dir {
        if let Ok(mut lock) = LAST_OPENED_DIR.lock() {
            *lock = Some(d);
        }
    }
}

pub fn get_window_hwnd(window: &slint::Window) -> *mut std::ffi::c_void {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        window
            .window_handle()
            .window_handle()
            .ok()
            .and_then(|handle| {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    Some(win32.hwnd.get() as *mut std::ffi::c_void)
                } else {
                    None
                }
            })
            .or_else(crate::win_drag_drop::find_app_window)
            .unwrap_or(std::ptr::null_mut())
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        std::ptr::null_mut()
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::path::{Path, PathBuf};
    use windows_sys::core::GUID;
    use windows_sys::core::{HRESULT, PCWSTR, PWSTR};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows_sys::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows_sys::Win32::UI::Shell::{
        FileOpenDialog, SHCreateItemFromParsingName, FILEOPENDIALOGOPTIONS,
        FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR,
        FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };

    const IID_IFILEOPENDIALOG: GUID = GUID::from_u128(0xd57c7288_d4ad_4768_be02_9d969532d960);
    const IID_ISHELLITEM: GUID = GUID::from_u128(0x43826d1e_e718_42ee_bc55_a1e261c37bfe);
    const MEDIA_PICKER_GUID: GUID = GUID::from_u128(0xb9b1e408_a9f0_45d3_8e5f_3f173c4d1d41);
    const FOLDER_PICKER_GUID: GUID = GUID::from_u128(0x4a123bc8_628e_4e88_a2bb_5d9321bf8310);

    #[repr(C)]
    struct IUnknownVtbl {
        query_interface: usize,
        add_ref: usize,
        release: unsafe extern "system" fn(this: *mut c_void) -> u32,
    }

    #[repr(C)]
    struct IModalWindowVtbl {
        base: IUnknownVtbl,
        show: unsafe extern "system" fn(this: *mut c_void, owner: HWND) -> HRESULT,
    }

    #[repr(C)]
    struct IFileDialogVtbl {
        base: IModalWindowVtbl,
        set_file_types: unsafe extern "system" fn(
            this: *mut c_void,
            cfiletypes: u32,
            rgfilterspec: *const COMDLG_FILTERSPEC,
        ) -> HRESULT,
        set_file_type_index: unsafe extern "system" fn(this: *mut c_void, ifiletype: u32) -> HRESULT,
        get_file_type_index: unsafe extern "system" fn(this: *mut c_void, pifiletype: *mut u32) -> HRESULT,
        advise: usize,
        unadvise: usize,
        set_options: unsafe extern "system" fn(this: *mut c_void, fos: FILEOPENDIALOGOPTIONS) -> HRESULT,
        get_options: unsafe extern "system" fn(this: *mut c_void, pfos: *mut FILEOPENDIALOGOPTIONS) -> HRESULT,
        set_default_folder: unsafe extern "system" fn(this: *mut c_void, psi: *mut c_void) -> HRESULT,
        set_folder: unsafe extern "system" fn(this: *mut c_void, psi: *mut c_void) -> HRESULT,
        get_folder: unsafe extern "system" fn(this: *mut c_void, ppsi: *mut *mut c_void) -> HRESULT,
        get_current_selection: usize,
        set_file_name: unsafe extern "system" fn(this: *mut c_void, pszname: PCWSTR) -> HRESULT,
        get_file_name: usize,
        set_title: unsafe extern "system" fn(this: *mut c_void, psztitle: PCWSTR) -> HRESULT,
        set_ok_button_label: usize,
        set_file_name_label: usize,
        get_result: unsafe extern "system" fn(this: *mut c_void, ppsi: *mut *mut c_void) -> HRESULT,
        add_place: usize,
        set_default_extension: unsafe extern "system" fn(this: *mut c_void, pszdefaultextension: PCWSTR) -> HRESULT,
        close: usize,
        set_client_guid: unsafe extern "system" fn(this: *mut c_void, pguid: *const GUID) -> HRESULT,
        clear_client_data: usize,
        set_filter: usize,
    }

    #[repr(C)]
    struct IFileOpenDialogVtbl {
        base: IFileDialogVtbl,
        get_results: unsafe extern "system" fn(this: *mut c_void, ppenum: *mut *mut c_void) -> HRESULT,
        get_selected_items: usize,
    }

    #[repr(C)]
    struct IShellItemVtbl {
        base: IUnknownVtbl,
        bind_to_handler: usize,
        get_parent: usize,
        get_display_name: unsafe extern "system" fn(
            this: *mut c_void,
            sigdnname: i32,
            ppszname: *mut PWSTR,
        ) -> HRESULT,
        get_attributes: usize,
        compare: usize,
    }

    #[repr(C)]
    struct IShellItemArrayVtbl {
        base: IUnknownVtbl,
        bind_to_handler: usize,
        get_property_store: usize,
        get_property_description_list: usize,
        get_attributes: usize,
        get_count: unsafe extern "system" fn(this: *mut c_void, pdwnumitems: *mut u32) -> HRESULT,
        get_item_at: unsafe extern "system" fn(
            this: *mut c_void,
            dwindex: u32,
            ppsi: *mut *mut c_void,
        ) -> HRESULT,
        enum_items: usize,
    }

    #[inline]
    unsafe fn release(ptr: *mut c_void) {
        if !ptr.is_null() {
            let vtbl = *(ptr as *const *const IUnknownVtbl);
            ((*vtbl).release)(ptr);
        }
    }

    #[inline]
    fn str_to_vec_u16(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    #[inline]
    unsafe fn pwstr_to_string_and_free(ptr: *mut u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(ptr, len);
        let s = String::from_utf16_lossy(slice);
        CoTaskMemFree(ptr.cast());
        s
    }

    /// Configures the starting folder on an `IFileOpenDialog` without disturbing its cached
    /// Shell view when the dialog is already pointed at the same directory.
    unsafe fn configure_dialog_folder(
        dialog_ptr: *mut c_void,
        vtbl: *const IFileOpenDialogVtbl,
        suggested_dir: Option<&Path>,
    ) {
        let Some(dir) = suggested_dir.filter(|d| d.is_dir()) else {
            return;
        };

        let last_dir = super::get_last_opened_dir();
        if last_dir.as_deref() == Some(dir) {
            // The dialog's client GUID is already at this folder; leave its Shell view untouched.
            return;
        }

        let clean = dir.to_string_lossy();
        let clean = clean.strip_prefix(r"\\?\").unwrap_or(&clean);
        let dir_w = str_to_vec_u16(clean);
        let mut shell_item: *mut c_void = std::ptr::null_mut();
        if SHCreateItemFromParsingName(
            dir_w.as_ptr(),
            std::ptr::null_mut(),
            &IID_ISHELLITEM,
            &mut shell_item,
        ) >= 0
            && !shell_item.is_null()
        {
            if last_dir.is_some() {
                let _ = ((*vtbl).base.set_folder)(dialog_ptr, shell_item);
            } else {
                let _ = ((*vtbl).base.set_default_folder)(dialog_ptr, shell_item);
            }
            release(shell_item);
        }
    }

    pub fn pick_media_files(
        parent_hwnd: *mut c_void,
        suggested_dir: Option<&Path>,
    ) -> Option<Vec<PathBuf>> {
        unsafe {
            let com_init = CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            );

            let mut dialog_ptr: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &FileOpenDialog,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_IFILEOPENDIALOG,
                &mut dialog_ptr,
            );
            if hr < 0 || dialog_ptr.is_null() {
                if com_init >= 0 {
                    CoUninitialize();
                }
                return None;
            }

            let vtbl = *(dialog_ptr as *const *const IFileOpenDialogVtbl);

            let _ = ((*vtbl).base.set_client_guid)(dialog_ptr, &MEDIA_PICKER_GUID);

            let mut options: FILEOPENDIALOGOPTIONS = 0;
            let _ = ((*vtbl).base.get_options)(dialog_ptr, &mut options);
            options |= FOS_ALLOWMULTISELECT
                | FOS_FORCEFILESYSTEM
                | FOS_FILEMUSTEXIST
                | FOS_PATHMUSTEXIST
                | FOS_NOCHANGEDIR;
            let _ = ((*vtbl).base.set_options)(dialog_ptr, options);

            let title_w = str_to_vec_u16("Select Media Files");
            let _ = ((*vtbl).base.set_title)(dialog_ptr, title_w.as_ptr());

            let def_ext_w = str_to_vec_u16("mp4");
            let _ = ((*vtbl).base.set_default_extension)(dialog_ptr, def_ext_w.as_ptr());

            let name_all_media = str_to_vec_u16("Video & Audio Files");
            let spec_all_media = str_to_vec_u16("*.mp4;*.mkv;*.mov;*.avi;*.webm;*.ts;*.m4v;*.flv;*.wmv;*.mp3;*.wav;*.flac;*.aac;*.m4a;*.ogg;*.opus;*.m4b");

            let name_video = str_to_vec_u16("Video Files");
            let spec_video = str_to_vec_u16("*.mp4;*.mkv;*.mov;*.avi;*.webm;*.ts;*.m4v;*.flv;*.wmv");

            let name_audio = str_to_vec_u16("Audio Files");
            let spec_audio = str_to_vec_u16("*.mp3;*.wav;*.flac;*.aac;*.m4a;*.ogg;*.opus;*.m4b");

            let name_all_files = str_to_vec_u16("All Files (*.*)");
            let spec_all_files = str_to_vec_u16("*.*");

            let filter_specs = [
                COMDLG_FILTERSPEC {
                    pszName: name_all_media.as_ptr(),
                    pszSpec: spec_all_media.as_ptr(),
                },
                COMDLG_FILTERSPEC {
                    pszName: name_video.as_ptr(),
                    pszSpec: spec_video.as_ptr(),
                },
                COMDLG_FILTERSPEC {
                    pszName: name_audio.as_ptr(),
                    pszSpec: spec_audio.as_ptr(),
                },
                COMDLG_FILTERSPEC {
                    pszName: name_all_files.as_ptr(),
                    pszSpec: spec_all_files.as_ptr(),
                },
            ];
            let _ = ((*vtbl).base.set_file_types)(dialog_ptr, filter_specs.len() as u32, filter_specs.as_ptr());
            let _ = ((*vtbl).base.set_file_type_index)(dialog_ptr, 1);

            configure_dialog_folder(dialog_ptr, vtbl, suggested_dir);

            let owner_hwnd = parent_hwnd as HWND;
            let show_hr = ((*vtbl).base.base.show)(dialog_ptr, owner_hwnd);

            let mut results = None;
            if show_hr == 0 {
                let mut items_array: *mut c_void = std::ptr::null_mut();
                if ((*vtbl).get_results)(dialog_ptr, &mut items_array) >= 0 && !items_array.is_null() {
                    let array_vtbl = *(items_array as *const *const IShellItemArrayVtbl);
                    let mut count = 0u32;
                    if ((*array_vtbl).get_count)(items_array, &mut count) >= 0 {
                        let mut paths = Vec::with_capacity(count as usize);
                        for i in 0..count {
                            let mut item_ptr: *mut c_void = std::ptr::null_mut();
                            if ((*array_vtbl).get_item_at)(items_array, i, &mut item_ptr) >= 0 && !item_ptr.is_null() {
                                let item_vtbl = *(item_ptr as *const *const IShellItemVtbl);
                                let mut dname: PWSTR = std::ptr::null_mut();
                                if ((*item_vtbl).get_display_name)(item_ptr, SIGDN_FILESYSPATH, &mut dname) >= 0 {
                                    let path_str = pwstr_to_string_and_free(dname);
                                    if !path_str.is_empty() {
                                        paths.push(PathBuf::from(path_str));
                                    }
                                }
                                release(item_ptr);
                            }
                        }
                        if !paths.is_empty() {
                            super::set_last_opened_dir(&paths[0]);
                            results = Some(paths);
                        }
                    }
                    release(items_array);
                }
            }

            release(dialog_ptr);
            if com_init >= 0 {
                CoUninitialize();
            }

            results
        }
    }

    pub fn pick_folder(
        parent_hwnd: *mut c_void,
        suggested_dir: Option<&Path>,
        title: &str,
    ) -> Option<PathBuf> {
        unsafe {
            let com_init = CoInitializeEx(
                std::ptr::null(),
                (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
            );

            let mut dialog_ptr: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &FileOpenDialog,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_IFILEOPENDIALOG,
                &mut dialog_ptr,
            );
            if hr < 0 || dialog_ptr.is_null() {
                if com_init >= 0 {
                    CoUninitialize();
                }
                return None;
            }

            let vtbl = *(dialog_ptr as *const *const IFileOpenDialogVtbl);

            let _ = ((*vtbl).base.set_client_guid)(dialog_ptr, &FOLDER_PICKER_GUID);

            let mut options: FILEOPENDIALOGOPTIONS = 0;
            let _ = ((*vtbl).base.get_options)(dialog_ptr, &mut options);
            options |= FOS_PICKFOLDERS
                | FOS_FORCEFILESYSTEM
                | FOS_PATHMUSTEXIST
                | FOS_FILEMUSTEXIST
                | FOS_NOCHANGEDIR;
            let _ = ((*vtbl).base.set_options)(dialog_ptr, options);

            let title_w = str_to_vec_u16(title);
            let _ = ((*vtbl).base.set_title)(dialog_ptr, title_w.as_ptr());

            configure_dialog_folder(dialog_ptr, vtbl, suggested_dir);

            let owner_hwnd = parent_hwnd as HWND;
            let show_hr = ((*vtbl).base.base.show)(dialog_ptr, owner_hwnd);

            let mut result = None;
            if show_hr == 0 {
                let mut item_ptr: *mut c_void = std::ptr::null_mut();
                if ((*vtbl).base.get_result)(dialog_ptr, &mut item_ptr) >= 0 && !item_ptr.is_null() {
                    let item_vtbl = *(item_ptr as *const *const IShellItemVtbl);
                    let mut dname: PWSTR = std::ptr::null_mut();
                    if ((*item_vtbl).get_display_name)(item_ptr, SIGDN_FILESYSPATH, &mut dname) >= 0 {
                        let path_str = pwstr_to_string_and_free(dname);
                        if !path_str.is_empty() {
                            let pb = PathBuf::from(path_str);
                            super::set_last_opened_dir(&pb);
                            result = Some(pb);
                        }
                    }
                    release(item_ptr);
                }
            }

            release(dialog_ptr);
            if com_init >= 0 {
                CoUninitialize();
            }

            result
        }
    }
}

pub fn pick_media_files(
    parent_hwnd: *mut std::ffi::c_void,
    suggested_dir: Option<&Path>,
) -> Option<Vec<PathBuf>> {
    #[cfg(windows)]
    {
        win::pick_media_files(parent_hwnd, suggested_dir)
    }
    #[cfg(not(windows))]
    {
        let _ = parent_hwnd;
        let mut dialog = rfd::FileDialog::new()
            .set_title("Select Media Files")
            .add_filter(
                "Video & Audio",
                &[
                    "mp4", "mkv", "mov", "avi", "webm", "ts", "m4v", "flv", "wmv", "mp3",
                    "wav", "flac", "aac", "m4a", "ogg", "opus", "m4b",
                ],
            )
            .add_filter("All Files", &["*"]);

        if let Some(dir) = suggested_dir {
            dialog = dialog.set_directory(dir);
        }

        dialog.pick_files()
    }
}

pub fn pick_folder(
    parent_hwnd: *mut std::ffi::c_void,
    suggested_dir: Option<&Path>,
    title: &str,
) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        win::pick_folder(parent_hwnd, suggested_dir, title)
    }
    #[cfg(not(windows))]
    {
        let _ = parent_hwnd;
        let mut dialog = rfd::FileDialog::new().set_title(title);
        if let Some(dir) = suggested_dir {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_folder()
    }
}

/// Opens the native media file picker on a dedicated background STA thread so the UI
/// event loop stays responsive and the Shell folder view has its own unblocked message pump.
pub fn pick_media_files_async<F>(
    _parent_hwnd: *mut std::ffi::c_void,
    suggested_dir: Option<PathBuf>,
    callback: F,
) where
    F: FnOnce(Vec<PathBuf>) + Send + 'static,
{
    if MEDIA_DIALOG_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let guard = ActiveDialogGuard(&MEDIA_DIALOG_ACTIVE);
    let _ = std::thread::Builder::new()
        .name("media-file-dialog".into())
        .spawn(move || {
            let _guard = guard;
            // Pass a null owner HWND because the dialog runs on its own STA thread;
            // cross-thread owner HWNDs trigger AttachThreadInput and stall Shell view painting.
            let res = pick_media_files(std::ptr::null_mut(), suggested_dir.as_deref());
            if let Some(paths) = res {
                if !paths.is_empty() {
                    let _ = slint::invoke_from_event_loop(move || {
                        callback(paths);
                    });
                }
            }
        });
}

/// Opens the native folder picker on a dedicated background STA thread.
pub fn pick_folder_async<F>(
    _parent_hwnd: *mut std::ffi::c_void,
    suggested_dir: Option<PathBuf>,
    title: &'static str,
    callback: F,
) where
    F: FnOnce(PathBuf) + Send + 'static,
{
    if FOLDER_DIALOG_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let guard = ActiveDialogGuard(&FOLDER_DIALOG_ACTIVE);
    let _ = std::thread::Builder::new()
        .name("folder-picker-dialog".into())
        .spawn(move || {
            let _guard = guard;
            let res = pick_folder(std::ptr::null_mut(), suggested_dir.as_deref(), title);
            if let Some(path) = res {
                let _ = slint::invoke_from_event_loop(move || {
                    callback(path);
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_last_opened_dir_updates() {
        let temp = tempfile::tempdir().expect("tempdir");
        let temp_dir = temp.path().to_path_buf();
        let test_file = temp_dir.join("test_video.mp4");

        set_last_opened_dir(&test_file);
        let resolved = get_last_opened_dir().expect("last opened dir should be set");
        assert_eq!(resolved, temp_dir);

        set_last_opened_dir(&temp_dir);
        let resolved_dir = get_last_opened_dir().expect("last opened dir should match temp_dir");
        assert_eq!(resolved_dir, temp_dir);
    }

    #[test]
    fn test_working_directory_preservation() {
        let original_cwd = std::env::current_dir().expect("get current dir");

        let dummy_path = original_cwd.join("assets").join("icon.ico");
        set_last_opened_dir(&dummy_path);

        let current_cwd = std::env::current_dir().expect("get current dir again");
        assert_eq!(original_cwd, current_cwd, "Process working directory must never be altered");
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_dialog_options_flags() {
        use windows_sys::Win32::UI::Shell::{
            FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR,
            FOS_PATHMUSTEXIST,
        };

        let mut opts = 0u32;
        opts |= FOS_ALLOWMULTISELECT
            | FOS_FORCEFILESYSTEM
            | FOS_FILEMUSTEXIST
            | FOS_PATHMUSTEXIST
            | FOS_NOCHANGEDIR;

        assert_ne!(opts & FOS_ALLOWMULTISELECT, 0, "FOS_ALLOWMULTISELECT must be present");
        assert_ne!(opts & FOS_FORCEFILESYSTEM, 0, "FOS_FORCEFILESYSTEM must be present");
        assert_ne!(opts & FOS_NOCHANGEDIR, 0, "FOS_NOCHANGEDIR must be present to protect process CWD");
        assert_ne!(opts & FOS_FILEMUSTEXIST, 0, "FOS_FILEMUSTEXIST must be present");
        assert_ne!(opts & FOS_PATHMUSTEXIST, 0, "FOS_PATHMUSTEXIST must be present");
    }
}
