use super::*;

struct TestWindow(HWND);

impl Drop for TestWindow {
    fn drop(&mut self) { unsafe { DestroyWindow(self.0); } }
}

#[test]
fn layered_surface_reuses_pixels_and_switches_from_keyed_video() {
    unsafe {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let window = TestWindow(CreateWindowExW(WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class.as_ptr(), std::ptr::null(), WS_POPUP, 0, 0, 8, 8, std::ptr::null_mut(),
            std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null()));
        assert!(!window.0.is_null());
        let mut surface = Surface::new(window.0);
        let pixels = vec![0x80402010; 64];
        assert_ne!(SetLayeredWindowAttributes(window.0, 0xff00ff, 255, LWA_COLORKEY | LWA_ALPHA), 0);
        surface.present(8, 8, &pixels).unwrap();
        let bitmap = surface.dib.as_ref().unwrap().bitmap;
        surface.present(8, 8, &pixels).unwrap();
        assert_eq!(surface.dib.as_ref().unwrap().bitmap, bitmap);
        assert_eq!(std::slice::from_raw_parts(surface.dib.as_ref().unwrap().bits, 64), pixels);
        assert_ne!(SetLayeredWindowAttributes(window.0, 0xff00ff, 255, LWA_COLORKEY | LWA_ALPHA), 0);
        surface.present(8, 8, &pixels).unwrap();
        surface.present(12, 10, &vec![0; 120]).unwrap();
        assert_eq!((surface.dib.as_ref().unwrap().width, surface.dib.as_ref().unwrap().height), (12, 10));
        assert_eq!(IsWindowVisible(window.0), 0);
    }
}
