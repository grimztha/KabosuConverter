//! Windows native drag-and-drop support for Another Encoder.
//! Supports both OLE IDropTarget and Shell WM_DROPFILES to ensure dropped files
//! from Windows Explorer are reliably detected and added to the queue.

#![cfg(windows)]
#![allow(clippy::upper_case_acronyms)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const IID_IUNKNOWN: GUID = GUID {
    data1: 0x00000000,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

pub const IID_IDROPTARGET: GUID = GUID {
    data1: 0x00000122,
    data2: 0x0000,
    data3: 0x0000,
    data4: [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};

const CF_HDROP: u16 = 15;
const DVASPECT_CONTENT: u32 = 1;
const TYMED_HGLOBAL: u32 = 1;
const DROPEFFECT_COPY: u32 = 1;
const S_OK: i32 = 0;
const E_NOINTERFACE: i32 = 0x80004002_u32 as i32;
const E_INVALIDARG: i32 = 0x80070057_u32 as i32;
const WM_DROPFILES: u32 = 0x0233;
const WM_COPYDATA: u32 = 0x0049;
const WM_COPYGLOBALDATA: u32 = 0x004A;
const MSGFLT_ALLOW: u32 = 1;
const GWL_EXSTYLE: i32 = -20;
const WS_EX_ACCEPTFILES: isize = 0x00000010;

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[allow(non_camel_case_types)]
pub struct FORMATETC {
    pub cf_format: u16,
    pub ptd: *mut std::ffi::c_void,
    pub dw_aspect: u32,
    pub lindex: i32,
    pub tymed: u32,
}

#[repr(C)]
#[allow(non_camel_case_types)]
pub struct STGMEDIUM {
    pub tymed: u32,
    pub h_global: *mut std::ffi::c_void,
    pub p_unk_for_release: *mut std::ffi::c_void,
}

#[inline]
pub fn log_dd(_msg: &str) {
    #[cfg(debug_assertions)]
    {
        eprintln!("[DragDrop] {}", _msg);
    }
}

#[repr(C)]
struct IDropTargetVtbl {
    query_interface: unsafe extern "system" fn(
        this: *mut DropTargetCom,
        riid: *const GUID,
        ppv_object: *mut *mut std::ffi::c_void,
    ) -> i32,
    add_ref: unsafe extern "system" fn(this: *mut DropTargetCom) -> u32,
    release: unsafe extern "system" fn(this: *mut DropTargetCom) -> u32,
    drag_enter: unsafe extern "system" fn(
        this: *mut DropTargetCom,
        p_data_obj: *mut std::ffi::c_void,
        grf_key_state: u32,
        pt: POINT,
        pdw_effect: *mut u32,
    ) -> i32,
    drag_over: unsafe extern "system" fn(
        this: *mut DropTargetCom,
        grf_key_state: u32,
        pt: POINT,
        pdw_effect: *mut u32,
    ) -> i32,
    drag_leave: unsafe extern "system" fn(this: *mut DropTargetCom) -> i32,
    drop: unsafe extern "system" fn(
        this: *mut DropTargetCom,
        p_data_obj: *mut std::ffi::c_void,
        grf_key_state: u32,
        pt: POINT,
        pdw_effect: *mut u32,
    ) -> i32,
}

#[repr(C)]
pub struct DropTargetCom {
    vtbl: *const IDropTargetVtbl,
    ref_count: AtomicU32,
    callback: Box<dyn Fn(Vec<PathBuf>) + Send + Sync>,
    last_drop: Mutex<Option<(Instant, Vec<PathBuf>)>>,
}

impl DropTargetCom {
    pub fn dispatch_paths(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }

        // Debounce if identical drop occurred within 500ms
        let now = Instant::now();
        if let Ok(mut lock) = self.last_drop.lock() {
            if let Some((prev_time, ref prev_paths)) = *lock {
                if now.duration_since(prev_time).as_millis() < 500 && *prev_paths == paths {
                    log_dd("Ignoring duplicate drop within 500ms");
                    return;
                }
            }
            *lock = Some((now, paths.clone()));
        }

        log_dd(&format!("Dispatching {} path(s) to application queue", paths.len()));
        (self.callback)(paths);
    }
}

static DROP_TARGET_VTBL: IDropTargetVtbl = IDropTargetVtbl {
    query_interface,
    add_ref,
    release,
    drag_enter,
    drag_over,
    drag_leave,
    drop,
};

unsafe extern "system" fn query_interface(
    this: *mut DropTargetCom,
    riid: *const GUID,
    ppv_object: *mut *mut std::ffi::c_void,
) -> i32 {
    if ppv_object.is_null() || riid.is_null() {
        return E_INVALIDARG;
    }
    let iid = *riid;
    if iid == IID_IUNKNOWN || iid == IID_IDROPTARGET {
        (*this).ref_count.fetch_add(1, Ordering::SeqCst);
        *ppv_object = this as *mut std::ffi::c_void;
        S_OK
    } else {
        *ppv_object = std::ptr::null_mut();
        E_NOINTERFACE
    }
}

/// COM IUnknown::AddRef implementation.
/// Thread-safe reference count increment using atomic operations.
/// The raw pointer is valid as long as the COM object is alive.
unsafe extern "system" fn add_ref(this: *mut DropTargetCom) -> u32 {
    debug_assert!(!this.is_null());
    (*this).ref_count.fetch_add(1, Ordering::SeqCst) + 1
}

/// COM IUnknown::Release implementation.
/// Thread-safe reference count decrement using atomic operations.
/// Retains the singleton DropTargetCom instance in memory so that window subclass hooks
/// (SetWindowSubclass) and global message filters do not encounter a use-after-free.
unsafe extern "system" fn release(this: *mut DropTargetCom) -> u32 {
    debug_assert!(!this.is_null());
    let prev = (*this).ref_count.load(Ordering::SeqCst);
    if prev > 1 {
        (*this).ref_count.fetch_sub(1, Ordering::SeqCst) - 1
    } else {
        1
    }
}

unsafe extern "system" fn drag_enter(
    _this: *mut DropTargetCom,
    p_data_obj: *mut std::ffi::c_void,
    _grf_key_state: u32,
    pt: POINT,
    pdw_effect: *mut u32,
) -> i32 {
    log_dd(&format!("COM DragEnter called at ({}, {}), p_data_obj: {:p}", pt.x, pt.y, p_data_obj));
    if !pdw_effect.is_null() {
        *pdw_effect = DROPEFFECT_COPY;
    }
    S_OK
}

unsafe extern "system" fn drag_over(
    _this: *mut DropTargetCom,
    _grf_key_state: u32,
    _pt: POINT,
    pdw_effect: *mut u32,
) -> i32 {
    if !pdw_effect.is_null() {
        *pdw_effect = DROPEFFECT_COPY;
    }
    S_OK
}

unsafe extern "system" fn drag_leave(_this: *mut DropTargetCom) -> i32 {
    log_dd("COM DragLeave called");
    S_OK
}

unsafe extern "system" fn drop(
    this: *mut DropTargetCom,
    p_data_obj: *mut std::ffi::c_void,
    _grf_key_state: u32,
    pt: POINT,
    pdw_effect: *mut u32,
) -> i32 {
    log_dd(&format!("COM Drop called at ({}, {}), p_data_obj: {:p}", pt.x, pt.y, p_data_obj));
    if !pdw_effect.is_null() {
        *pdw_effect = DROPEFFECT_COPY;
    }

    if !p_data_obj.is_null() {
        let fmt = FORMATETC {
            cf_format: CF_HDROP,
            ptd: std::ptr::null_mut(),
            dw_aspect: DVASPECT_CONTENT,
            lindex: -1,
            tymed: TYMED_HGLOBAL,
        };

        let mut medium = STGMEDIUM {
            tymed: 0,
            h_global: std::ptr::null_mut(),
            p_unk_for_release: std::ptr::null_mut(),
        };

        // IDataObject::GetData is at vtable index 3
        let vtable_ptr = *(p_data_obj as *const *const usize);
        let get_data_fn: unsafe extern "system" fn(
            *mut std::ffi::c_void,
            *const FORMATETC,
            *mut STGMEDIUM,
        ) -> i32 = std::mem::transmute(*vtable_ptr.add(3));

        let hr = get_data_fn(p_data_obj, &fmt, &mut medium);
        log_dd(&format!("COM GetData hr={:#x}, medium.h_global={:p}", hr, medium.h_global));
        if hr == S_OK && !medium.h_global.is_null() {
            let paths = parse_hdrop(medium.h_global);
            log_dd(&format!("COM GetData parsed {} paths", paths.len()));
            ReleaseStgMedium(&mut medium);
            if !paths.is_empty() {
                (*this).dispatch_paths(paths);
            }
        }
    }

    S_OK
}

#[link(name = "shell32")]
extern "system" {
    fn DragAcceptFiles(hWnd: *mut std::ffi::c_void, fAccept: i32);
    fn DragQueryFileW(
        hDrop: *mut std::ffi::c_void,
        iFile: u32,
        lpszFile: *mut u16,
        cch: u32,
    ) -> u32;
    fn DragFinish(hDrop: *mut std::ffi::c_void);
}

#[link(name = "ole32")]
extern "system" {
    fn OleInitialize(pvReserved: *mut std::ffi::c_void) -> i32;
    fn RegisterDragDrop(hWnd: *mut std::ffi::c_void, pDropTarget: *mut std::ffi::c_void) -> i32;
    fn RevokeDragDrop(hWnd: *mut std::ffi::c_void) -> i32;
    fn ReleaseStgMedium(pmedium: *mut STGMEDIUM);
}

#[link(name = "comctl32")]
extern "system" {
    fn SetWindowSubclass(
        hWnd: *mut std::ffi::c_void,
        pfnSubclass: unsafe extern "system" fn(
            *mut std::ffi::c_void,
            u32,
            usize,
            isize,
            usize,
            usize,
        ) -> isize,
        uIdSubclass: usize,
        dwRefData: usize,
    ) -> i32;
    fn DefSubclassProc(
        hWnd: *mut std::ffi::c_void,
        uMsg: u32,
        wParam: usize,
        lParam: isize,
    ) -> isize;
}

#[link(name = "user32")]
extern "system" {
    fn ChangeWindowMessageFilterEx(
        hwnd: *mut std::ffi::c_void,
        message: u32,
        action: u32,
        pChangeFilterStruct: *mut std::ffi::c_void,
    ) -> i32;
    fn EnumWindows(
        lpEnumFunc: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32,
        lParam: isize,
    ) -> i32;
    fn EnumChildWindows(
        hWndParent: *mut std::ffi::c_void,
        lpEnumFunc: unsafe extern "system" fn(*mut std::ffi::c_void, isize) -> i32,
        lParam: isize,
    ) -> i32;
    fn GetCurrentProcessId() -> u32;
    fn GetWindowThreadProcessId(
        hWnd: *mut std::ffi::c_void,
        lpdwProcessId: *mut u32,
    ) -> u32;
    fn IsWindowVisible(hWnd: *mut std::ffi::c_void) -> i32;
    fn GetWindowLongPtrW(hWnd: *mut std::ffi::c_void, nIndex: i32) -> isize;
    fn SetWindowLongPtrW(hWnd: *mut std::ffi::c_void, nIndex: i32, dwNewLong: isize) -> isize;
    fn GetWindowTextW(hWnd: *mut std::ffi::c_void, lpString: *mut u16, nMaxCount: i32) -> i32;
    fn GetClassNameW(hWnd: *mut std::ffi::c_void, lpClassName: *mut u16, nMaxCount: i32) -> i32;
}

pub unsafe fn get_window_title(hwnd: *mut std::ffi::c_void) -> String {
    let mut buf = [0u16; 512];
    let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    }
}

pub unsafe fn get_window_class(hwnd: *mut std::ffi::c_void) -> String {
    let mut buf = [0u16; 512];
    let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    }
}

/// Parse an HDROP handle into a list of file paths.
/// # Safety
/// - `hdrop` must be a valid HDROP handle obtained from a drop operation.
/// - This function assumes the Windows API returns valid data.
pub unsafe fn parse_hdrop(hdrop: *mut std::ffi::c_void) -> Vec<PathBuf> {
    debug_assert!(!hdrop.is_null());
    let count = DragQueryFileW(hdrop, 0xFFFFFFFF, std::ptr::null_mut(), 0);
    let mut paths = Vec::with_capacity(count as usize);
    let mut buffer = [0u16; 2048];
    for i in 0..count {
        let len = DragQueryFileW(hdrop, i, buffer.as_mut_ptr(), buffer.len() as u32);
        if len > 0 {
            let path_str = String::from_utf16_lossy(&buffer[..len as usize]);
            paths.push(PathBuf::from(path_str));
        }
    }
    paths
}

unsafe extern "system" fn drop_subclass_proc(
    hwnd: *mut std::ffi::c_void,
    msg: u32,
    wparam: usize,
    lparam: isize,
    _id: usize,
    ref_data: usize,
) -> isize {
    if msg == WM_DROPFILES {
        log_dd(&format!("WM_DROPFILES received on hwnd {:p}!", hwnd));
        let hdrop = wparam as *mut std::ffi::c_void;
        let paths = parse_hdrop(hdrop);
        DragFinish(hdrop);
        if !paths.is_empty() {
            log_dd(&format!("WM_DROPFILES dispatched {} file(s)", paths.len()));
            let target = &*(ref_data as *const DropTargetCom);
            target.dispatch_paths(paths);
        }
        return 0;
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// Discovers the active top-level HWND for the current process (ignoring message/IME windows)
pub fn find_app_window() -> Option<*mut std::ffi::c_void> {
    unsafe {
        let my_pid = GetCurrentProcessId();

        unsafe extern "system" fn enum_all_windows_cb(
            hwnd: *mut std::ffi::c_void,
            lparam: isize,
        ) -> i32 {
            let (target_pid, out) = &mut *(lparam as *mut (u32, Option<*mut std::ffi::c_void>));
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == *target_pid {
                let class = get_window_class(hwnd);
                let title = get_window_title(hwnd);
                let visible = IsWindowVisible(hwnd) != 0;
                if class != "Winit Thread Event Target"
                    && class != "IME"
                    && class != "MSCTFIME UI"
                    && class != "NVOpenGLPbuffer"
                {
                    if title.contains("Another Encoder") {
                        *out = Some(hwnd);
                        return 0;
                    } else if visible && out.is_none() {
                        *out = Some(hwnd);
                    }
                }
            }
            1 // Continue to find best candidate
        }

        let mut data = (my_pid, None);
        EnumWindows(enum_all_windows_cb, &mut data as *mut _ as isize);
        data.1
    }
}

unsafe extern "system" fn enum_child_windows_cb(
    child: *mut std::ffi::c_void,
    lparam: isize,
) -> i32 {
    let raw_com_ptr = lparam as *mut DropTargetCom;
    let title = get_window_title(child);
    let class = get_window_class(child);
    log_dd(&format!("Found Child HWND {:p}, class='{}', title='{}'", child, class, title));

    let _ = RevokeDragDrop(child);
    ChangeWindowMessageFilterEx(child, WM_DROPFILES, MSGFLT_ALLOW, std::ptr::null_mut());
    ChangeWindowMessageFilterEx(child, WM_COPYDATA, MSGFLT_ALLOW, std::ptr::null_mut());
    ChangeWindowMessageFilterEx(child, WM_COPYGLOBALDATA, MSGFLT_ALLOW, std::ptr::null_mut());

    DragAcceptFiles(child, 1);
    let ex = GetWindowLongPtrW(child, GWL_EXSTYLE);
    SetWindowLongPtrW(child, GWL_EXSTYLE, ex | WS_EX_ACCEPTFILES);

    SetWindowSubclass(child, drop_subclass_proc, 1001, raw_com_ptr as usize);
    let hr = RegisterDragDrop(child, raw_com_ptr as *mut std::ffi::c_void);
    log_dd(&format!("Child HWND {:p} RegisterDragDrop hr={:#x}", child, hr));

    1 // Continue
}

static ATTACHED_REAL_HWND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn is_attached() -> bool {
    ATTACHED_REAL_HWND.load(Ordering::SeqCst) != 0
}

pub unsafe fn attach_to_hwnd(hwnd: *mut std::ffi::c_void, raw_com_ptr: *mut DropTargetCom) {
    if hwnd.is_null() {
        return;
    }
    let title = get_window_title(hwnd);
    let class = get_window_class(hwnd);
    let visible = IsWindowVisible(hwnd) != 0;

    let is_real = visible
        && title.contains("Another Encoder")
        && class != "Winit Thread Event Target"
        && class != "IME"
        && class != "MSCTFIME UI"
        && class != "NVOpenGLPbuffer";
    if is_real {
        ATTACHED_REAL_HWND.store(hwnd as usize, Ordering::SeqCst);
    }

    log_dd(&format!(
        "Attaching drop target to HWND {:p}, class='{}', title='{}'",
        hwnd, class, title
    ));

    let revoke_hr = RevokeDragDrop(hwnd);
    log_dd(&format!("RevokeDragDrop({:p}) hr={:#x}", hwnd, revoke_hr));

    ChangeWindowMessageFilterEx(hwnd, WM_DROPFILES, MSGFLT_ALLOW, std::ptr::null_mut());
    ChangeWindowMessageFilterEx(hwnd, WM_COPYDATA, MSGFLT_ALLOW, std::ptr::null_mut());
    ChangeWindowMessageFilterEx(hwnd, WM_COPYGLOBALDATA, MSGFLT_ALLOW, std::ptr::null_mut());

    DragAcceptFiles(hwnd, 1);
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_ACCEPTFILES);

    SetWindowSubclass(hwnd, drop_subclass_proc, 1001, raw_com_ptr as usize);

    let hr = RegisterDragDrop(hwnd, raw_com_ptr as *mut std::ffi::c_void);
    log_dd(&format!("RegisterDragDrop({:p}) hr={:#x}", hwnd, hr));

    EnumChildWindows(hwnd, enum_child_windows_cb, raw_com_ptr as isize);
}

static REGISTERED_TARGET: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Initializes both OLE IDropTarget and Shell WM_DROPFILES on the active application window
pub fn setup_drop_target<F>(hwnd: *mut std::ffi::c_void, callback: F)
where
    F: Fn(Vec<PathBuf>) + Send + Sync + 'static,
{
    unsafe {
        let ole_hr = OleInitialize(std::ptr::null_mut());
        log_dd(&format!("setup_drop_target called with hint HWND {:p}, OleInitialize hr={:#x}", hwnd, ole_hr));

        let com_target = Box::new(DropTargetCom {
            vtbl: &DROP_TARGET_VTBL,
            ref_count: AtomicU32::new(1),
            callback: Box::new(callback),
            last_drop: Mutex::new(None),
        });
        let raw_com_ptr = Box::into_raw(com_target);
        REGISTERED_TARGET.store(raw_com_ptr as usize, Ordering::SeqCst);

        // First attach to the hint HWND if non-null
        if !hwnd.is_null() {
            attach_to_hwnd(hwnd, raw_com_ptr);
        }

        // Also search for any real visible app window immediately
        if let Some(real_hwnd) = find_app_window() {
            if real_hwnd != hwnd {
                log_dd(&format!("Found distinct real app HWND {:p}, attaching as well", real_hwnd));
                attach_to_hwnd(real_hwnd, raw_com_ptr);
            }
        }
    }
}

/// Re-scans for any newly created top-level or child windows and attaches the drop target
pub fn reapply_drop_target() {
    let ptr = REGISTERED_TARGET.load(Ordering::SeqCst);
    if ptr != 0 {
        unsafe {
            let raw_com_ptr = ptr as *mut DropTargetCom;
            if let Some(real_hwnd) = find_app_window() {
                if ATTACHED_REAL_HWND.load(Ordering::SeqCst) != real_hwnd as usize {
                    log_dd(&format!("reapply_drop_target: Found new real app HWND {:p}, attaching", real_hwnd));
                    attach_to_hwnd(real_hwnd, raw_com_ptr);
                }
            }
        }
    }
}

