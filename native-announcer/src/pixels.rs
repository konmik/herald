#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat { ColorKeyRgb, PremultipliedArgb }

impl PixelFormat {
    pub fn clear(self) -> u32 {
        match self { Self::ColorKeyRgb => 0xff00ff, Self::PremultipliedArgb => 0 }
    }

    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    pub fn argb(self, color: u32) -> u32 {
        match self {
            Self::ColorKeyRgb => if color == self.clear() { 0 } else { color | 0xff000000 },
            Self::PremultipliedArgb => color,
        }
    }

    pub fn over(self, background: u32, foreground: u32, alpha: u32) -> u32 {
        match self {
            Self::ColorKeyRgb => {
                let mut result = 0;
                for shift in [0, 8, 16] {
                    result |= ((((foreground >> shift) & 255) * alpha + ((background >> shift) & 255) * (255 - alpha)) / 255) << shift;
                }
                result
            }
            Self::PremultipliedArgb => over(background, premultiply(foreground, alpha)),
        }
    }

    pub fn shade(self, color: u32, foreground: u32, amount: u32) -> u32 {
        match self {
            Self::ColorKeyRgb => self.over(color, foreground, amount),
            Self::PremultipliedArgb => {
                let alpha = color >> 24;
                let foreground = premultiply(foreground, alpha);
                let mut result = alpha << 24;
                for shift in [0, 8, 16] {
                    result |= ((((foreground >> shift) & 255) * amount + ((color >> shift) & 255) * (255 - amount)) / 255) << shift;
                }
                result
            }
        }
    }

    pub fn rgba(self, color: u32) -> image::Rgba<u8> {
        match self {
            Self::ColorKeyRgb => image::Rgba([(color >> 16) as u8, (color >> 8) as u8, color as u8, if color == self.clear() { 0 } else { 255 }]),
            Self::PremultipliedArgb => {
                let alpha = color >> 24;
                if alpha == 0 { return image::Rgba([0; 4]); }
                image::Rgba([(((color >> 16) & 255) * 255 / alpha).min(255) as u8,
                    (((color >> 8) & 255) * 255 / alpha).min(255) as u8,
                    ((color & 255) * 255 / alpha).min(255) as u8, alpha as u8])
            }
        }
    }

    pub fn on_background(self, color: u32, background: u32) -> u32 {
        match self {
            Self::ColorKeyRgb => if color == self.clear() { background } else { color },
            Self::PremultipliedArgb => over(background | 0xff000000, color) & 0xffffff,
        }
    }
}

pub fn premultiply(color: u32, alpha: u32) -> u32 {
    let mut result = alpha << 24;
    for shift in [0, 8, 16] { result |= (((color >> shift) & 255) * alpha / 255) << shift; }
    result
}

pub fn over(background: u32, foreground: u32) -> u32 {
    let inverse = 255 - (foreground >> 24);
    let mut result = 0;
    for shift in [0, 8, 16, 24] {
        result |= (((foreground >> shift) & 255) + ((background >> shift) & 255) * inverse / 255).min(255) << shift;
    }
    result
}

#[cfg(test)]
#[path = "../unit_tests/pixels.rs"]
mod tests;
