#[cfg(not(target_os = "windows"))]
pub fn run(_data: &std::path::Path) -> Result<(), String> {
    Err("The settings app is currently available on Windows.".into())
}

#[cfg(target_os = "windows")]
mod native {
    use crate::audio::OutputDevice;
    use crate::settings::{format_time, parse_time, Settings};
    use std::path::{Path, PathBuf};
    use std::hash::{Hash, Hasher};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::{Controls::*, WindowsAndMessaging::*};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;

    const TBM_GETPOS: u32 = WM_USER;

    const QUIET: i32 = 101;
    const SCHEDULE: i32 = 102;
    const START: i32 = 103;
    const END: i32 = 104;
    const VOLUME: i32 = 105;
    const OUTPUT: i32 = 106;
    const APPLY: i32 = 107;
    const CLOSE: i32 = 108;
    const STATUS: i32 = 109;
    const VOLUME_LABEL: i32 = 110;
    const REFRESH: i32 = 111;
    const PREVIEW: i32 = 112;
    const GPU: i32 = 113;
    const SHOW_ON_DESKTOP: usize = 0x43415354;

    #[repr(C)]
    struct DesktopRequest {
        kind: usize,
        size: u32,
        desktop: *const windows::core::GUID,
    }

    fn with_desktops<T>(action: impl FnOnce(&windows::Win32::UI::Shell::IVirtualDesktopManager) -> windows::core::Result<T>) -> windows::core::Result<T> {
        use windows::Win32::System::Com::*;
        use windows::Win32::UI::Shell::*;
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

    unsafe fn show_on_desktop(window: HWND, desktop: Option<&windows::core::GUID>) {
        if let Some(desktop) = desktop {
            let _ = with_desktops(|desktops| desktops.MoveWindowToDesktop(windows::Win32::Foundation::HWND(window), desktop));
        }
        ShowWindow(window, SW_RESTORE);
        SetWindowPos(window, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW);
        SetForegroundWindow(window);
    }

    struct Form {
        data: PathBuf,
        settings: Settings,
        devices: Vec<OutputDevice>,
        missing: Option<String>,
        preview: Option<crate::platform::Preview>,
    }

    fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() }

    fn volume_label(volume: u16) -> String {
        if volume == 0 { "Announcer volume: muted".into() }
        else { format!("Announcer volume: {volume}% ({:.1} dB)", -60.0 + 0.6 * f64::from(volume)) }
    }

    unsafe fn text(window: HWND, id: i32) -> String {
        let control = GetDlgItem(window, id);
        let mut buffer = vec![0u16; GetWindowTextLengthW(control) as usize + 1];
        let length = GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..length as usize])
    }

    unsafe fn label(window: HWND, id: i32, value: &str) {
        SetWindowTextW(GetDlgItem(window, id), wide(value).as_ptr());
    }

    unsafe fn checked(window: HWND, id: i32) -> bool {
        SendMessageW(GetDlgItem(window, id), BM_GETCHECK, 0, 0) == BST_CHECKED as isize
    }

    unsafe fn set_checked(window: HWND, id: i32, value: bool) {
        SendMessageW(GetDlgItem(window, id), BM_SETCHECK, if value { BST_CHECKED } else { BST_UNCHECKED } as usize, 0);
    }

    unsafe fn control(window: HWND, class: &str, title: &str, id: i32, style: u32, bounds: (i32, i32, i32, i32)) -> Result<(), String> {
        let (x, y, width, height) = bounds;
        let control = CreateWindowExW(0, wide(class).as_ptr(), wide(title).as_ptr(), WS_CHILD | WS_VISIBLE | style,
            x, y, width, height, window, id as usize as HMENU, GetModuleHandleW(std::ptr::null()), std::ptr::null());
        if control.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
        SendMessageW(control, WM_SETFONT, GetStockObject(DEFAULT_GUI_FONT) as usize, 1);
        Ok(())
    }

    unsafe fn populate_outputs(window: HWND, form: &mut Form, selected: Option<&str>) {
        let output = GetDlgItem(window, OUTPUT);
        SendMessageW(output, CB_RESETCONTENT, 0, 0);
        SendMessageW(output, CB_ADDSTRING, 0, wide("System default").as_ptr() as isize);
        form.devices = crate::audio::output_devices();
        let mut selection = 0;
        for (index, device) in form.devices.iter().enumerate() {
            SendMessageW(output, CB_ADDSTRING, 0, wide(&device.name).as_ptr() as isize);
            if selected == Some(device.id.as_str()) { selection = index + 1; }
        }
        form.missing = None;
        if selection == 0 && selected.is_some() {
            SendMessageW(output, CB_ADDSTRING, 0, wide("Selected device unavailable (using system default)").as_ptr() as isize);
            selection = form.devices.len() + 1;
            form.missing = selected.map(String::from);
        }
        SendMessageW(output, CB_SETCURSEL, selection, 0);
    }

    unsafe fn selected_output(window: HWND, form: &Form) -> Option<String> {
        let index = SendMessageW(GetDlgItem(window, OUTPUT), CB_GETCURSEL, 0, 0);
        if index <= 0 { None }
        else { form.devices.get(index as usize - 1).map(|device| device.id.clone()).or_else(|| form.missing.clone()) }
    }

    unsafe fn audio_settings(window: HWND, form: &Form) -> Settings {
        let mut settings = form.settings.clone();
        settings.volume = SendMessageW(GetDlgItem(window, VOLUME), TBM_GETPOS, 0, 0) as u16;
        settings.output_device = selected_output(window, form);
        settings.use_gpu = checked(window, GPU);
        settings
    }

    unsafe fn save(window: HWND, form: &mut Form) -> Result<(), String> {
        let mut settings = audio_settings(window, form);
        settings.quiet_mode = checked(window, QUIET);
        settings.schedule_enabled = checked(window, SCHEDULE);
        settings.quiet_start = parse_time(&text(window, START), false)?;
        settings.quiet_end = parse_time(&text(window, END), true)?;
        settings.save(&form.data)?;
        form.settings = settings;
        label(window, STATUS, "Saved. Changes apply to the next announcement.");
        Ok(())
    }

    unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let form = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut Form;
        match message {
            WM_COPYDATA if lparam != 0 => {
                let request = &*(lparam as *const DesktopRequest);
                if request.kind == SHOW_ON_DESKTOP && request.size as usize == std::mem::size_of::<windows::core::GUID>() && !request.desktop.is_null() {
                    let desktop = std::ptr::read_unaligned(request.desktop);
                    show_on_desktop(window, Some(&desktop));
                    return 1;
                }
                0
            }
            WM_COMMAND if !form.is_null() => {
                match (wparam & 0xffff) as i32 {
                    APPLY => match save(window, &mut *form) {
                        Ok(()) => {}
                        Err(error) => { MessageBoxW(window, wide(&error).as_ptr(), wide("Civilized Agent settings").as_ptr(), MB_OK | MB_ICONERROR); }
                    },
                    CLOSE => { DestroyWindow(window); }
                    REFRESH => {
                        let selected = selected_output(window, &*form);
                        populate_outputs(window, &mut *form, selected.as_deref());
                    }
                    PREVIEW => {
                        if (*form).preview.take().is_some() {
                            label(window, PREVIEW, "Play example");
                            label(window, STATUS, "Preview stopped.");
                            KillTimer(window, 1);
                        } else {
                            let settings = audio_settings(window, &*form);
                            if settings.volume == 0 {
                                label(window, STATUS, "Preview is silent at 0% volume.");
                            } else {
                                (*form).preview = Some(crate::platform::Preview::start((*form).data.clone(), settings));
                                label(window, PREVIEW, "Stop example");
                                label(window, STATUS, "Playing static, then: This is an announcement");
                                SetTimer(window, 1, 100, None);
                            }
                        }
                    }
                    SCHEDULE => {
                        let enabled = checked(window, SCHEDULE);
                        EnableWindow(GetDlgItem(window, START), enabled as i32);
                        EnableWindow(GetDlgItem(window, END), enabled as i32);
                    }
                    _ => {}
                }
                0
            }
            WM_HSCROLL => {
                let volume = SendMessageW(GetDlgItem(window, VOLUME), TBM_GETPOS, 0, 0);
                label(window, VOLUME_LABEL, &volume_label(volume as u16));
                0
            }
            WM_TIMER if !form.is_null() && wparam == 1 => {
                if let Some(result) = (*form).preview.as_mut().and_then(crate::platform::Preview::finished) {
                    (*form).preview = None;
                    KillTimer(window, 1);
                    label(window, PREVIEW, "Play example");
                    match result {
                        Ok(()) => label(window, STATUS, "Preview finished."),
                        Err(error) => label(window, STATUS, &format!("Preview failed: {error}")),
                    }
                }
                0
            }
            WM_CLOSE => { DestroyWindow(window); 0 }
            WM_DESTROY => { PostQuitMessage(0); 0 }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }

    pub fn run(data: &Path) -> Result<(), String> {
        let mut form = Box::new(Form { data: data.into(), settings: Settings::load(data)?, devices: Vec::new(), missing: None, preview: None });
        unsafe {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            std::fs::canonicalize(data).unwrap_or_else(|_| data.into()).to_string_lossy().to_lowercase().hash(&mut hash);
            let class = wide(&format!("CivilizedAgentSettings-{:x}", hash.finish()));
            let foreground = GetForegroundWindow();
            let desktop = with_desktops(|desktops| desktops.GetWindowDesktopId(windows::Win32::Foundation::HWND(foreground))).ok();
            let existing = FindWindowW(class.as_ptr(), std::ptr::null());
            if !existing.is_null() {
                let mut process = 0;
                GetWindowThreadProcessId(existing, &mut process);
                AllowSetForegroundWindow(process);
                if let Some(desktop) = desktop.as_ref() {
                    let request = DesktopRequest { kind: SHOW_ON_DESKTOP, size: std::mem::size_of::<windows::core::GUID>() as u32, desktop };
                    let mut result = 0;
                    SendMessageTimeoutW(existing, WM_COPYDATA, 0, &request as *const _ as isize, SMTO_ABORTIFHUNG, 2000, &mut result);
                }
                ShowWindow(existing, SW_RESTORE);
                SetForegroundWindow(existing);
                return Ok(());
            }
            InitCommonControlsEx(&INITCOMMONCONTROLSEX { dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_BAR_CLASSES });
            let instance = GetModuleHandleW(std::ptr::null());
            let window_class = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: instance, lpszClassName: class.as_ptr(),
                hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW), hbrBackground: (COLOR_BTNFACE + 1) as usize as HBRUSH, ..WNDCLASSW::default() };
            if RegisterClassW(&window_class) == 0 { return Err(std::io::Error::last_os_error().to_string()); }
            let window = CreateWindowExW(WS_EX_CONTROLPARENT, class.as_ptr(), wide("Civilized Agent settings").as_ptr(),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX, CW_USEDEFAULT, CW_USEDEFAULT, 520, 566,
                std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null());
            if window.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
            SetWindowLongPtrW(window, GWLP_USERDATA, &mut *form as *mut Form as isize);
            control(window, "BUTTON", "Quiet mode (mute speech and static)", QUIET, BS_AUTOCHECKBOX as u32 | WS_TABSTOP, (24, 20, 460, 28))?;
            control(window, "STATIC", "Announcements still appear while quiet mode is on.", 0, 0, (24, 52, 460, 24))?;
            control(window, "BUTTON", "Quiet mode on a daily schedule", SCHEDULE, BS_AUTOCHECKBOX as u32 | WS_TABSTOP, (24, 88, 460, 28))?;
            control(window, "STATIC", "From", 0, 0, (24, 124, 45, 24))?;
            control(window, "EDIT", &format_time(form.settings.quiet_start), START, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, (72, 120, 78, 28))?;
            control(window, "STATIC", "to", 0, 0, (166, 124, 24, 24))?;
            control(window, "EDIT", &format_time(form.settings.quiet_end), END, WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32, (200, 120, 78, 28))?;
            control(window, "STATIC", "Local time, HH:MM", 0, 0, (294, 124, 190, 24))?;
            control(window, "STATIC", &volume_label(form.settings.volume), VOLUME_LABEL, 0, (24, 170, 460, 24))?;
            control(window, "msctls_trackbar32", "Announcer volume", VOLUME, WS_TABSTOP | TBS_AUTOTICKS, (24, 198, 460, 40))?;
            SendMessageW(GetDlgItem(window, VOLUME), TBM_SETRANGEMAX, 0, 100);
            SendMessageW(GetDlgItem(window, VOLUME), TBM_SETPOS, 1, form.settings.volume as isize);
            control(window, "STATIC", "Audio output (speech and static)", 0, 0, (24, 250, 460, 24))?;
            control(window, "COMBOBOX", "Audio output", OUTPUT, WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32, (24, 278, 358, 220))?;
            control(window, "BUTTON", "Refresh", REFRESH, WS_TABSTOP, (392, 278, 92, 28))?;
            control(window, "BUTTON", "Use GPU for speech (NVIDIA CUDA)", GPU, BS_AUTOCHECKBOX as u32 | WS_TABSTOP, (24, 322, 460, 28))?;
            control(window, "STATIC", "Falls back to CPU if GPU startup is unavailable.", 0, 0, (24, 352, 460, 24))?;
            control(window, "BUTTON", "Play example", PREVIEW, WS_TABSTOP, (24, 382, 140, 30))?;
            control(window, "STATIC", "Previews your selected settings without saving.", 0, 0, (176, 384, 308, 36))?;
            control(window, "STATIC", "Uses the system default if your selected device is unavailable.", STATUS, 0, (24, 436, 460, 36))?;
            control(window, "BUTTON", "Apply", APPLY, WS_TABSTOP | BS_DEFPUSHBUTTON as u32, (272, 486, 100, 30))?;
            control(window, "BUTTON", "Close", CLOSE, WS_TABSTOP, (384, 486, 100, 30))?;
            set_checked(window, QUIET, form.settings.quiet_mode);
            set_checked(window, SCHEDULE, form.settings.schedule_enabled);
            set_checked(window, GPU, form.settings.use_gpu);
            EnableWindow(GetDlgItem(window, START), form.settings.schedule_enabled as i32);
            EnableWindow(GetDlgItem(window, END), form.settings.schedule_enabled as i32);
            let selected = form.settings.output_device.clone();
            populate_outputs(window, &mut form, selected.as_deref());
            show_on_desktop(window, desktop.as_ref());
            let speech_data = data.to_path_buf();
            let use_gpu = form.settings.use_gpu;
            std::thread::spawn(move || {
                if let Err(error) = crate::tts::prepare(use_gpu) { crate::state::log(&speech_data, error); }
            });
            let mut message = MSG::default();
            loop {
                let result = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
                if result == 0 { break; }
                if result == -1 { return Err(std::io::Error::last_os_error().to_string()); }
                if IsDialogMessageW(window, &message) == 0 { TranslateMessage(&message); DispatchMessageW(&message); }
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub use native::run;
