use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    bits: *mut u32,
    width: u32,
    height: u32,
}

impl Dib {
    fn new(width: u32, height: u32) -> Result<Self, String> {
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            if dc.is_null() { return Err(std::io::Error::last_os_error().to_string()); }
            let mut info = BITMAPINFO::default();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32, biHeight: -(height as i32), biPlanes: 1,
                biBitCount: 32, biCompression: BI_RGB, ..BITMAPINFOHEADER::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            if bitmap.is_null() {
                let error = std::io::Error::last_os_error().to_string();
                DeleteDC(dc);
                return Err(error);
            }
            let previous = SelectObject(dc, bitmap);
            if previous.is_null() || previous as isize == -1 {
                let error = std::io::Error::last_os_error().to_string();
                DeleteObject(bitmap);
                DeleteDC(dc);
                return Err(error);
            }
            Ok(Self { dc, bitmap, previous, bits: bits.cast(), width, height })
        }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

pub struct Surface {
    window: HWND,
    dib: Option<Dib>,
}

impl Surface {
    pub fn new(window: HWND) -> Self { Self { window, dib: None } }

    pub fn present(&mut self, width: u32, height: u32, pixels: &[u32]) -> Result<(), String> {
        if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 || pixels.len() != width as usize * height as usize {
            return Err("Invalid layered announcement dimensions".into());
        }
        if !self.dib.as_ref().is_some_and(|dib| dib.width == width && dib.height == height) {
            self.dib = Some(Dib::new(width, height)?);
        }
        let dib = self.dib.as_ref().unwrap();
        unsafe {
            let mut flags = 0;
            if GetLayeredWindowAttributes(self.window, std::ptr::null_mut(), std::ptr::null_mut(), &mut flags) != 0 {
                let style = GetWindowLongPtrW(self.window, GWL_EXSTYLE);
                SetWindowLongPtrW(self.window, GWL_EXSTYLE, style & !(WS_EX_LAYERED as isize));
                SetWindowLongPtrW(self.window, GWL_EXSTYLE, style | WS_EX_LAYERED as isize);
            }
            GdiFlush();
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), dib.bits, pixels.len());
            let size = SIZE { cx: width as i32, cy: height as i32 };
            let origin = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
            if UpdateLayeredWindow(self.window, std::ptr::null_mut(), std::ptr::null(), &size, dib.dc, &origin, 0, &blend, ULW_ALPHA) == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../unit_tests/windows_surface.rs"]
mod tests;
