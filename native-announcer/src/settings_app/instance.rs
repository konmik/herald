use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, EnumWindows, GetForegroundWindow, GetPropW,
    GetWindowThreadProcessId, SetForegroundWindow, SetPropW, ShowWindow, SW_RESTORE,
    SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_COPYDATA, WM_NCDESTROY,
};

const SHOW_ON_DESKTOP: usize = 0x43415354;

#[repr(C)]
struct DesktopRequest {
    kind: usize,
    size: u32,
    desktop: *const windows::core::GUID,
}

unsafe extern "system" fn desktop_message(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM, id: usize, _: usize) -> LRESULT {
    if message == WM_COPYDATA && lparam != 0 {
        let request = &*(lparam as *const DesktopRequest);
        if request.kind == SHOW_ON_DESKTOP && request.size as usize == std::mem::size_of::<windows::core::GUID>() && !request.desktop.is_null() {
            let desktop = std::ptr::read_unaligned(request.desktop);
            let _ = move_to_desktop(window, &desktop);
            ShowWindow(window, SW_RESTORE);
            SetForegroundWindow(window);
            return 1;
        }
    }
    if message == WM_NCDESTROY {
        windows_sys::Win32::UI::Shell::RemoveWindowSubclass(window, Some(desktop_message), id);
    }
    windows_sys::Win32::UI::Shell::DefSubclassProc(window, message, wparam, lparam)
}

pub(super) struct InstanceGuard {
    lock: File,
    property: Vec<u16>,
}

impl InstanceGuard {
    pub(super) fn acquire(data: &Path) -> Result<Option<Self>, String> {
        std::fs::create_dir_all(data).map_err(|error| error.to_string())?;
        let path = data.join(".settings-app.lock");
        let property = property_name(data);
        for _ in 0..40 {
            if let Ok(lock) = open_lock(&path) {
                return Ok(Some(Self { lock, property }));
            }
            if activate_existing(&property) {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if let Ok(lock) = open_lock(&path) {
            return Ok(Some(Self { lock, property }));
        }
        Err("Another Herald settings window is running, but it could not be activated.".into())
    }

    pub(super) fn register(&self, window: &gpui_kit::Window) -> Result<(), String> {
        let hwnd = hwnd(window)?;
        if unsafe { windows_sys::Win32::UI::Shell::SetWindowSubclass(hwnd, Some(desktop_message), SHOW_ON_DESKTOP, 0) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let result = unsafe { SetPropW(hwnd, self.property.as_ptr(), 1usize as *mut _) };
        if result == 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    }

    pub(super) fn keep_alive(&self) {
        let _ = &self.lock;
    }

    pub(super) fn activate_pending(&self, _: &mut gpui_kit::Window) {}
}

fn open_lock(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .share_mode(0)
        .open(path)
}

fn property_name(data: &Path) -> Vec<u16> {
    let path = data.canonicalize().unwrap_or_else(|_| PathBuf::from(data));
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    path.to_string_lossy().to_lowercase().hash(&mut hash);
    format!("HeraldSettings-{:#x}", hash.finish())
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

struct WindowSearch {
    property: Vec<u16>,
    window: HWND,
}

unsafe extern "system" fn find_window(window: HWND, data: LPARAM) -> windows_sys::core::BOOL {
    let search = &mut *(data as *mut WindowSearch);
    if !GetPropW(window, search.property.as_ptr()).is_null() {
        search.window = window;
        0
    } else {
        1
    }
}

fn existing_window(property: &[u16]) -> HWND {
    let mut search = WindowSearch { property: property.to_vec(), window: std::ptr::null_mut() };
    unsafe {
        EnumWindows(Some(find_window), &mut search as *mut _ as LPARAM);
    }
    search.window
}

fn activate_existing(property: &[u16]) -> bool {
    let existing = existing_window(property);
    if existing.is_null() {
        return false;
    }
    unsafe {
        let foreground = GetForegroundWindow();
        if !foreground.is_null() {
            if let Some(desktop) = current_desktop(foreground) {
                let request = DesktopRequest { kind: SHOW_ON_DESKTOP, size: std::mem::size_of::<windows::core::GUID>() as u32, desktop: &desktop };
                let mut result = 0;
                SendMessageTimeoutW(existing, WM_COPYDATA, 0, &request as *const _ as LPARAM, SMTO_ABORTIFHUNG, 2000, &mut result);
            }
        }
        let mut process = 0;
        GetWindowThreadProcessId(existing, &mut process);
        if process != 0 {
            AllowSetForegroundWindow(process);
        }
        ShowWindow(existing, SW_RESTORE);
        SetForegroundWindow(existing);
    }
    true
}

fn current_desktop(window: HWND) -> Option<windows::core::GUID> {
    with_desktops(|desktops| unsafe { desktops.GetWindowDesktopId(windows::Win32::Foundation::HWND(window)) }).ok()
}

fn move_to_desktop(window: HWND, desktop: &windows::core::GUID) -> windows::core::Result<()> {
    with_desktops(|desktops| unsafe {
        desktops.MoveWindowToDesktop(windows::Win32::Foundation::HWND(window), desktop)
    })
}

fn with_desktops<T>(action: impl FnOnce(&windows::Win32::UI::Shell::IVirtualDesktopManager) -> windows::core::Result<T>) -> windows::core::Result<T> {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let result = (|| {
            let desktops: IVirtualDesktopManager = CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL)?;
            action(&desktops)
        })();
        CoUninitialize();
        result
    }
}

pub(super) fn hwnd(window: &gpui_kit::Window) -> Result<HWND, String> {
    let handle = HasWindowHandle::window_handle(window).map_err(|error| error.to_string())?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(handle.hwnd.get() as HWND),
        _ => Err("Herald settings requires a Win32 window handle.".into()),
    }
}
