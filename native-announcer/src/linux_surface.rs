use gtk::cairo::{Format, ImageSurface, Operator};
use gtk::prelude::*;
use std::cell::RefCell;
use std::num::NonZeroU32;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use tao::platform::unix::WindowExtUnix;

pub struct Surface {
    window: Rc<tao::window::Window>,
    image: Rc<RefCell<Option<ImageSurface>>>,
    pixels: Vec<u32>,
    width: i32,
    height: i32,
}

impl Surface {
    pub fn new(window: Rc<tao::window::Window>) -> Self {
        let image = Rc::new(RefCell::new(None::<ImageSurface>));
        let drawing = image.clone();
        window.gtk_window().connect_draw(move |widget, context| {
            context.set_operator(Operator::Source);
            context.set_source_rgba(0.0, 0.0, 0.0, 0.0);
            let _ = context.paint();
            if let Some(image) = drawing.borrow().as_ref() {
                let scale = f64::from(widget.scale_factor());
                context.scale(1.0 / scale, 1.0 / scale);
                if context.set_source_surface(image, 0.0, 0.0).is_ok() {
                    let _ = context.paint();
                }
            }
            gtk::glib::Propagation::Stop
        });
        Self { window, image, pixels: Vec::new(), width: 0, height: 0 }
    }

    pub fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), String> {
        self.width = i32::try_from(width.get()).map_err(|error| error.to_string())?;
        self.height = i32::try_from(height.get()).map_err(|error| error.to_string())?;
        self.pixels.resize(width.get() as usize * height.get() as usize, 0);
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

impl Buffer<'_> {
    pub fn present(self) -> Result<(), String> {
        let bytes: Vec<u8> = self.0.pixels.iter().flat_map(|color| argb(*color).to_ne_bytes()).collect();
        let image = ImageSurface::create_for_data(bytes, Format::ARgb32, self.0.width, self.0.height, self.0.width * 4).map_err(|error| error.to_string())?;
        *self.0.image.borrow_mut() = Some(image);
        self.0.window.gtk_window().queue_draw();
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
