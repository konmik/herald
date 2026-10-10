use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::msg_send;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, CGImageComponentInfo, CGImagePixelFormatInfo,
};
use objc2_quartz_core::{CALayer, CATransaction};
use std::ffi::c_void;
use std::num::NonZeroU32;
use std::ops::{Deref, DerefMut};
use std::ptr::{self, NonNull};
use tao::platform::macos::WindowExtMacOS;

// softbuffer presents opaque images on macOS, so the transparent window needs its own layer contents.
pub struct Surface {
    root: Retained<CALayer>,
    layer: Retained<CALayer>,
    color_space: CFRetained<CGColorSpace>,
    pixels: Vec<u32>,
    width: usize,
    height: usize,
}

impl Surface {
    pub fn new(window: &tao::window::Window) -> Result<Self, String> {
        let view = window.ns_view() as *mut AnyObject;
        let root: Option<Retained<CALayer>> = unsafe {
            let () = msg_send![view, setWantsLayer: true];
            msg_send![view, layer]
        };
        let root = root.ok_or("The announcement view has no layer.")?;
        let layer = CALayer::new();
        root.addSublayer(&layer);
        let color_space = CGColorSpace::new_device_rgb().ok_or("Device RGB is unavailable.")?;
        Ok(Self { root, layer, color_space, pixels: Vec::new(), width: 0, height: 0 })
    }

    pub fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), String> {
        self.width = width.get() as usize;
        self.height = height.get() as usize;
        self.pixels.resize(self.width * self.height, 0);
        Ok(())
    }

    pub fn buffer_mut(&mut self) -> Result<Buffer<'_>, String> {
        Ok(Buffer(self))
    }
}

pub struct Buffer<'a>(&'a mut Surface);

impl Deref for Buffer<'_> {
    type Target = [u32];
    fn deref(&self) -> &[u32] { &self.0.pixels }
}

impl DerefMut for Buffer<'_> {
    fn deref_mut(&mut self) -> &mut [u32] { &mut self.0.pixels }
}

fn argb(color: u32) -> u32 {
    if color == 0xff00ff { 0 } else { color | 0xff000000 }
}

/// The premultiplied ARGB copy handed to Core Animation, with the transparency key cleared.
pub fn layer_pixels(pixels: &[u32]) -> Box<[u32]> { pixels.iter().map(|color| argb(*color)).collect() }

impl Buffer<'_> {
    pub fn present(self) -> Result<(), String> {
        unsafe extern "C-unwind" fn release(_info: *mut c_void, data: NonNull<c_void>, size: usize) {
            drop(unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(data.cast::<u32>().as_ptr(), size / 4)) });
        }
        let surface = self.0;
        let pixels = layer_pixels(&surface.pixels);
        let length = pixels.len() * 4;
        let provider = unsafe { CGDataProvider::with_data(ptr::null_mut(), Box::into_raw(pixels).cast(), length, Some(release)) }
            .ok_or("Could not wrap the announcement pixels.")?;
        let info = CGBitmapInfo(CGImageAlphaInfo::PremultipliedFirst.0 | CGImageComponentInfo::Integer.0
            | CGImageByteOrderInfo::Order32Little.0 | CGImagePixelFormatInfo::Packed.0);
        let image = unsafe {
            CGImage::new(surface.width, surface.height, 8, 32, surface.width * 4, Some(&surface.color_space), info,
                Some(&provider), ptr::null(), false, CGColorRenderingIntent::RenderingIntentDefault)
        }.ok_or("Could not create the announcement image.")?;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        surface.layer.setFrame(surface.root.bounds());
        unsafe { surface.layer.setContents(Some(image.as_ref())) };
        CATransaction::commit();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_background_is_transparent_and_content_is_opaque() {
        assert_eq!(argb(0xff00ff), 0x00000000);
        assert_eq!(argb(0x1c1b16), 0xff1c1b16);
        assert_eq!(argb(0xeef2f7), 0xffeef2f7);
    }
}
