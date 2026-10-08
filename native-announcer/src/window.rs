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
    thread_local! { static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) }; }

    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
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
        pub fn inner_size(&self) -> PhysicalSize {
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(self.hwnd, &mut rect);
            }
            PhysicalSize {
                width: (rect.right - rect.left).max(1) as u32,
                height: (rect.bottom - rect.top).max(1) as u32,
            }
        }
        pub fn set_inner_size(&self, size: LogicalSize) {
            let scale = self.scale_factor();
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    (size.width * scale) as i32,
                    (size.height * scale) as i32,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        pub fn set_outer_position(&self, position: PhysicalPosition) {
            unsafe {
                SetWindowPos(
                    self.hwnd,
                    std::ptr::null_mut(),
                    position.x,
                    position.y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
        pub fn request_redraw(&self) {
            unsafe {
                InvalidateRect(self.hwnd, std::ptr::null(), 0);
            }
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
                    rect: info.rcWork,
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
        rect: RECT,
        scale: f64,
    }
    impl Monitor {
        pub fn size(&self) -> PhysicalSize {
            PhysicalSize {
                width: (self.rect.right - self.rect.left) as u32,
                height: (self.rect.bottom - self.rect.top) as u32,
            }
        }
        pub fn position(&self) -> PhysicalPosition {
            PhysicalPosition::new(self.rect.left, self.rect.top)
        }
        pub fn scale_factor(&self) -> f64 {
            self.scale
        }
    }

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
