#[cfg(not(target_os = "windows"))]
pub use tao::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, Event, MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{Window, WindowBuilder},
};

#[cfg(target_os = "windows")]
mod native {
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
        RawWindowHandle, Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
    };
    use std::cell::RefCell;
    use std::num::NonZeroIsize;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    #[derive(Clone, Copy)]
    pub struct LogicalSize {
        pub width: f64,
        pub height: f64,
    }
    impl LogicalSize {
        pub fn new(width: f64, height: f64) -> Self {
            Self { width, height }
        }
    }
    #[derive(Clone, Copy)]
    pub struct PhysicalPosition {
        pub x: i32,
        pub y: i32,
    }
    impl PhysicalPosition {
        pub fn new(x: i32, y: i32) -> Self {
            Self { x, y }
        }
    }
    pub struct PhysicalSize {
        pub width: u32,
        pub height: u32,
    }
    pub enum MouseButton {
        Left,
        Right,
    }
    pub enum ElementState {
        Pressed,
    }
    pub enum WindowEvent {
        CloseRequested,
        Focused(bool),
        MouseInput {
            state: ElementState,
            button: MouseButton,
        },
        Other,
    }
    pub enum NativeEvent {
        WindowEvent { event: WindowEvent },
        MainEventsCleared,
        RedrawRequested(()),
        LoopDestroyed,
    }
    pub use NativeEvent as Event;
    pub enum ControlFlow {
        WaitUntil(Instant),
        Exit,
    }
    thread_local! {
        static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
        static CARD_REGION: RefCell<Option<crate::render::PhysicalRect>> = const { RefCell::new(None) };
    }

    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            WM_NCHITTEST => {
                let x = lparam as i16 as i32;
                let y = (lparam >> 16) as i16 as i32;
                if CARD_REGION.with(|region| region.borrow().is_some_and(|rect| !rect.contains(x, y))) {
                    HTTRANSPARENT as isize
                } else { HTCLIENT as isize }
            }
            WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
            WM_ERASEBKGND => 1,
            WM_ACTIVATE => {
                EVENTS.with(|events| {
                    events.borrow_mut().push(Event::WindowEvent {
                        event: WindowEvent::Focused((wparam as u32 & 0xffff) != WA_INACTIVE),
                    })
                });
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
            WM_SIZE => {
                EVENTS.with(|events| {
                    events.borrow_mut().push(Event::WindowEvent {
                        event: WindowEvent::Other,
                    })
                });
                0
            }
            WM_CLOSE => {
                EVENTS.with(|events| {
                    events.borrow_mut().push(Event::WindowEvent {
                        event: WindowEvent::CloseRequested,
                    })
                });
                0
            }
            WM_LBUTTONDOWN | WM_RBUTTONDOWN => {
                let button = if message == WM_LBUTTONDOWN {
                    MouseButton::Left
                } else {
                    MouseButton::Right
                };
                EVENTS.with(|events| {
                    events.borrow_mut().push(Event::WindowEvent {
                        event: WindowEvent::MouseInput {
                            state: ElementState::Pressed,
                            button,
                        },
                    })
                });
                0
            }
            WM_PAINT => {
                let mut paint = PAINTSTRUCT::default();
                unsafe {
                    BeginPaint(hwnd, &mut paint);
                    EndPaint(hwnd, &paint);
                }
                EVENTS.with(|events| events.borrow_mut().push(Event::RedrawRequested(())));
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    pub struct EventLoop;
    impl EventLoop {
        pub fn new() -> Self {
            Self
        }
        pub fn run(self, mut callback: impl FnMut(Event, &Self, &mut ControlFlow) + 'static) -> ! {
            let mut flow = ControlFlow::WaitUntil(Instant::now());
            loop {
                let mut message = MSG::default();
                unsafe {
                    while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                        if message.message == WM_QUIT {
                            flow = ControlFlow::Exit;
                            break;
                        }
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                for event in EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut())) {
                    callback(event, &self, &mut flow);
                    if matches!(flow, ControlFlow::Exit) {
                        break;
                    }
                }
                if matches!(flow, ControlFlow::Exit) {
                    break;
                }
                callback(Event::MainEventsCleared, &self, &mut flow);
                match flow {
                    ControlFlow::Exit => break,
                    ControlFlow::WaitUntil(deadline) => {
                        let wait = deadline
                            .saturating_duration_since(Instant::now())
                            .min(Duration::from_millis(50));
                        unsafe {
                            MsgWaitForMultipleObjects(
                                0,
                                std::ptr::null(),
                                0,
                                wait.as_millis() as u32,
                                QS_ALLINPUT,
                            );
                        }
                    }
                }
            }
            callback(Event::LoopDestroyed, &self, &mut flow);
            std::process::exit(0)
        }
    }

    pub struct Window {
        hwnd: HWND,
    }
    impl Window {
        pub fn hwnd(&self) -> isize {
            self.hwnd as isize
        }
        pub fn scale_factor(&self) -> f64 {
            unsafe { GetDpiForWindow(self.hwnd).max(96) as f64 / 96.0 }
        }
        pub fn set_physical_bounds(&self, bounds: crate::render::PhysicalRect) -> Result<(), std::io::Error> {
            let width = i32::try_from(bounds.width).map_err(std::io::Error::other)?;
            let height = i32::try_from(bounds.height).map_err(std::io::Error::other)?;
            if unsafe { SetWindowPos(self.hwnd, std::ptr::null_mut(), bounds.x, bounds.y, width, height, SWP_NOACTIVATE | SWP_NOZORDER) } == 0 {
                Err(std::io::Error::last_os_error())
            } else { Ok(()) }
        }
        pub fn set_card_region(&self, bounds: crate::render::PhysicalRect) {
            CARD_REGION.with(|region| *region.borrow_mut() = Some(bounds));
            self.refresh_card_input();
        }
        pub fn refresh_card_input(&self) {
            let mut pointer = POINT::default();
            let mut window = RECT::default();
            unsafe {
                if GetCursorPos(&mut pointer) == 0 || GetWindowRect(self.hwnd, &mut window) == 0 { return; }
                let style = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
                let next = CARD_REGION.with(|region| card_input_style(style, physical_rect(window), *region.borrow(), pointer));
                if style != next { SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, next); }
            }
        }
        pub fn request_redraw(&self) {
            EVENTS.with(|events| events.borrow_mut().push(Event::RedrawRequested(())));
        }
        pub fn current_monitor(&self) -> Option<Monitor> {
            let handle = unsafe { MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST) };
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..MONITORINFO::default()
            };
            if unsafe { GetMonitorInfoW(handle, &mut info) } == 0 {
                None
            } else {
                Some(Monitor {
                    work_area: physical_rect(info.rcWork),
                    bounds: physical_rect(info.rcMonitor),
                    scale: self.scale_factor(),
                })
            }
        }
        pub fn primary_monitor(&self) -> Option<Monitor> {
            self.current_monitor()
        }
    }
    impl HasWindowHandle for Window {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let hwnd = NonZeroIsize::new(self.hwnd()).ok_or(HandleError::Unavailable)?;
            Ok(unsafe {
                WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(hwnd)))
            })
        }
    }
    impl HasDisplayHandle for Window {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Ok(unsafe {
                DisplayHandle::borrow_raw(RawDisplayHandle::Windows(WindowsDisplayHandle::new()))
            })
        }
    }
    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.hwnd);
            }
        }
    }
    pub struct Monitor {
        work_area: crate::render::PhysicalRect,
        bounds: crate::render::PhysicalRect,
        scale: f64,
    }
    impl Monitor {
        pub fn size(&self) -> PhysicalSize {
            PhysicalSize {
                width: self.work_area.width,
                height: self.work_area.height,
            }
        }
        pub fn position(&self) -> PhysicalPosition {
            PhysicalPosition::new(self.work_area.x, self.work_area.y)
        }
        pub fn scale_factor(&self) -> f64 {
            self.scale
        }
        pub fn bounds(&self) -> crate::render::PhysicalRect {
            self.bounds
        }
    }

    fn physical_rect(rect: RECT) -> crate::render::PhysicalRect {
        crate::render::PhysicalRect { x: rect.left, y: rect.top, width: (rect.right - rect.left) as u32, height: (rect.bottom - rect.top) as u32 }
    }

    fn card_input_style(style: isize, window: crate::render::PhysicalRect, card: Option<crate::render::PhysicalRect>, pointer: POINT) -> isize {
        if card.is_some_and(|card| window != card && !card.contains(pointer.x, pointer.y)) {
            style | WS_EX_TRANSPARENT as isize
        } else { style & !(WS_EX_TRANSPARENT as isize) }
    }

    #[cfg(test)]
    #[path = "../../../unit_tests/window.rs"]
    mod tests;

    pub struct WindowBuilder {
        title: String,
        size: LogicalSize,
    }
    impl WindowBuilder {
        pub fn new() -> Self {
            Self {
                title: String::new(),
                size: LogicalSize::new(320.0, 240.0),
            }
        }
        pub fn with_title(mut self, title: &str) -> Self {
            self.title = title.into();
            self
        }
        pub fn with_visible(self, _: bool) -> Self {
            self
        }
        pub fn with_focused(self, _: bool) -> Self {
            self
        }
        pub fn with_focusable(self, _: bool) -> Self {
            self
        }
        pub fn with_decorations(self, _: bool) -> Self {
            self
        }
        pub fn with_resizable(self, _: bool) -> Self {
            self
        }
        pub fn with_always_on_top(self, _: bool) -> Self {
            self
        }
        pub fn with_inner_size(mut self, size: LogicalSize) -> Self {
            self.size = size;
            self
        }
        pub fn build(self, _: &EventLoop) -> Result<Window, std::io::Error> {
            unsafe {
                SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
                let module = GetModuleHandleW(std::ptr::null());
                let name: Vec<u16> = "HeraldNativeWindow\0".encode_utf16().collect();
                let class = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(procedure),
                    hInstance: module,
                    hIcon: LoadIconW(module, 1 as *const u16),
                    lpszClassName: name.as_ptr(),
                    hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
                    ..WNDCLASSW::default()
                };
                if RegisterClassW(&class) == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let title: Vec<u16> = self.title.encode_utf16().chain(Some(0)).collect();
                let hwnd = CreateWindowExW(
                    WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                    name.as_ptr(),
                    title.as_ptr(),
                    WS_POPUP,
                    CW_USEDEFAULT,
                    CW_USEDEFAULT,
                    self.size.width as i32,
                    self.size.height as i32,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    module,
                    std::ptr::null(),
                );
                if hwnd.is_null() {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(Window { hwnd })
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use native::*;
