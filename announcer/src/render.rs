use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::Font;
use image::RgbaImage;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

pub struct Renderer {
    font: Font,
    cache: VecDeque<(PathBuf, RgbaImage)>,
    pub text: String,
    pub title: String,
    pub color: u32,
    seed: u32,
}

struct TextBlock<'a> {
    text: &'a str,
    y: f32,
    size: f32,
    color: u32,
    max_height: f32,
}

impl Renderer {
    pub fn new() -> Result<Self, String> {
        let candidates: Vec<PathBuf> = if cfg!(target_os = "windows") {
            let directory = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
            vec![
                PathBuf::from(&directory).join("Fonts/arial.ttf"),
                PathBuf::from(directory).join("Fonts/segoeui.ttf"),
            ]
        } else if cfg!(target_os = "macos") {
            vec![
                "/System/Library/Fonts/Supplemental/Arial.ttf".into(),
                "/Library/Fonts/Arial.ttf".into(),
            ]
        } else {
            vec![
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into(),
                "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf".into(),
                "/usr/share/fonts/TTF/DejaVuSans.ttf".into(),
            ]
        };
        let bytes = candidates
            .into_iter()
            .find_map(|p| std::fs::read(p).ok())
            .ok_or("No system font found")?;
        let font =
            Font::from_bytes(bytes, fontdue::FontSettings::default()).map_err(str::to_owned)?;
        Ok(Self {
            font,
            cache: VecDeque::new(),
            text: String::new(),
            title: String::new(),
            color: 0x3080e0,
            seed: 567891,
        })
    }

    pub fn message_height(&self) -> u32 {
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            max_width: Some(268.0),
            ..LayoutSettings::default()
        });
        layout.append(&[&self.font], &TextStyle::new(&self.text, 15.0, 0));
        layout.height().ceil() as u32
    }

    fn text(
        &self,
        buffer: &mut [u32],
        width: usize,
        height: usize,
        scale: f32,
        block: TextBlock<'_>,
    ) {
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            x: 26.0 * scale,
            y: block.y * scale,
            max_width: Some(268.0 * scale),
            ..LayoutSettings::default()
        });
        layout.append(
            &[&self.font],
            &TextStyle::new(block.text, block.size * scale, 0),
        );
        for glyph in layout.glyphs() {
            if glyph.y + glyph.height as f32 > block.max_height * scale {
                continue;
            }
            let (_, pixels) = self.font.rasterize_config(glyph.key);
            for row in 0..glyph.height {
                for column in 0..glyph.width {
                    let x = glyph.x as i32 + column as i32;
                    let y = glyph.y as i32 + row as i32;
                    if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                        let index = y as usize * width + x as usize;
                        buffer[index] = blend(
                            buffer[index],
                            block.color,
                            pixels[row * glyph.width + column] as u32,
                        );
                    }
                }
            }
        }
    }

    pub fn draw(
        &mut self,
        buffer: &mut [u32],
        width: usize,
        height: usize,
        scale: f32,
        image: Option<&Path>,
        interference: f32,
    ) {
        buffer.fill(if cfg!(target_os = "windows") {
            0xff00ff
        } else {
            0x1c1b16
        });
        let logical_height = height as f32 / scale;
        draw_bubble(
            buffer,
            width,
            height,
            scale,
            logical_height - 152.0,
            self.color,
        );
        let title_y = logical_height - 188.0;
        self.text(
            buffer,
            width,
            height,
            scale,
            TextBlock {
                text: &self.text,
                y: 26.0,
                size: 15.0,
                color: 0xe8e4d8,
                max_height: title_y - 10.0,
            },
        );
        let mut title = self.title.clone();
        while title
            .chars()
            .map(|c| self.font.metrics(c, 12.0).advance_width)
            .sum::<f32>()
            > 268.0
        {
            title.pop();
        }
        if title != self.title {
            title = title
                .chars()
                .take(title.chars().count().saturating_sub(3))
                .collect::<String>()
                + "…";
        }
        self.text(
            buffer,
            width,
            height,
            scale,
            TextBlock {
                text: &title,
                y: title_y,
                size: 12.0,
                color: 0xa8ac9c,
                max_height: title_y + 22.0,
            },
        );
        let face_x = (188.0 * scale) as usize;
        let face_y = ((logical_height - 140.0) * scale) as usize;
        let face_size = (128.0 * scale) as usize;
        if let Some(path) = image {
            if let Some(index) = self.cache.iter().position(|(p, _)| p == path) {
                let entry = self.cache.remove(index).unwrap();
                self.cache.push_back(entry);
            } else if let Ok(image) = image::open(path) {
                let image = image
                    .resize_exact(128, 128, image::imageops::FilterType::Triangle)
                    .to_rgba8();
                self.cache.push_back((path.to_owned(), image));
                if self.cache.len() > 12 {
                    self.cache.pop_front();
                }
            }
        }
        let flicker = 0.85 + (self.random() % 16) as f32 / 100.0;
        let picture_flicker = 0.98 + (self.random() % 5) as f32 / 200.0;
        let vertical_offset = if self.random().is_multiple_of(7) {
            ((self.random() % 7) as i32 - 3) as f32 * interference
        } else {
            0.0
        };
        let mut row_displacement = [0.0_f32; 128];
        let mut row_snow = [false; 128];
        for _ in 0..3 {
            let start = self.random() as usize % 128;
            let length = 1 + self.random() as usize % 8;
            let shift = ((self.random() % 13) as i32 - 6) as f32 * interference;
            for row in start..(start + length).min(128) {
                row_displacement[row] += shift;
                row_snow[row] = true;
            }
        }
        let scanline_phase = self.random() as usize % 3;
        for y in 0..face_size {
            let source_y = y * 128 / face_size;
            let row_offset = row_displacement[source_y].round() as i32;
            for x in 0..face_size {
                if face_x + x >= width || face_y + y >= height {
                    continue;
                }
                let index = (face_y + y) * width + face_x + x;
                buffer[index] = 0x0b0b09;
                let horizontal = (x as f32 / face_size as f32 - 0.5) * 2.0;
                let vertical = (y as f32 / face_size as f32 - 0.5) * 2.0;
                let radius = horizontal * horizontal + vertical * vertical;
                let curvature = 1.0 + radius * 0.065;
                let warped_x = (horizontal * curvature + 1.0) * 63.5 + row_offset as f32;
                let warped_y = (vertical * curvature + 1.0) * 63.5 + vertical_offset;
                if !(0.0..128.0).contains(&warped_x) || !(0.0..128.0).contains(&warped_y) {
                    buffer[index] = 0x080908;
                    continue;
                }
                if let Some((path, frame)) = self.cache.back() {
                    if image.is_some_and(|p| p == path) {
                        let pixel = frame.get_pixel(warped_x as u32, warped_y as u32).0;
                        let color =
                            ((pixel[0] as u32) << 16) | ((pixel[1] as u32) << 8) | pixel[2] as u32;
                        buffer[index] = blend(buffer[index], color, pixel[3] as u32);
                    }
                }
                let background = buffer[index];
                let luminance = (((background >> 16) & 255) * 77
                    + ((background >> 8) & 255) * 150
                    + (background & 255) * 29)
                    / 256;
                let faded = blend(background, luminance * 0x010101, 24);
                let scanline = if source_y.is_multiple_of(2) {
                    0.82
                } else {
                    1.0
                };
                let vignette = 1.0 - radius * 0.13;
                let brightness = scanline * vignette * picture_flicker;
                buffer[index] = multiply_color(faded, brightness);
                let ambient_grain = self.random() & 255;
                buffer[index] = blend(buffer[index], ambient_grain * 0x010101, 5);
                if interference > 0.0 {
                    let random = self.random();
                    let mut grain =
                        ((random & 255) + ((random >> 8) & 255) + ((random >> 16) & 255)) / 3;
                    if row_snow[source_y] && random & 3 == 0 {
                        grain = if random & 4 == 0 { 0 } else { 255 };
                    }
                    if (source_y + scanline_phase).is_multiple_of(3) {
                        grain = grain * 2 / 3;
                    }
                    let alpha = (interference.clamp(0.0, 1.0) * flicker * 255.0) as u32;
                    let monochrome = grain * 0x010101;
                    let background = buffer[index];
                    let luminance = (((background >> 16) & 255) * 77
                        + ((background >> 8) & 255) * 150
                        + (background & 255) * 29)
                        / 256;
                    let desaturated = blend(background, luminance * 0x010101, alpha / 2);
                    buffer[index] = blend(desaturated, monochrome, alpha);
                }
            }
        }
    }

    fn random(&mut self) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed
    }
}

fn blend(background: u32, foreground: u32, alpha: u32) -> u32 {
    let mut result = 0;
    for shift in [0, 8, 16] {
        result |= ((((foreground >> shift) & 255) * alpha
            + ((background >> shift) & 255) * (255 - alpha))
            / 255)
            << shift;
    }
    result
}

fn multiply_color(color: u32, amount: f32) -> u32 {
    let mut result = 0;
    for shift in [0, 8, 16] {
        result |= ((((color >> shift) & 255) as f32 * amount).clamp(0.0, 255.0) as u32) << shift;
    }
    result
}

fn draw_bubble(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    scale: f32,
    bottom: f32,
    border: u32,
) {
    let border = blend(border, 0x45463b, 200);
    for y in 0..height {
        for x in 0..width {
            let x_position = x as f32 / scale;
            let y_position = y as f32 / scale;
            let texture = crate::state::noise_hash(
                (x_position as u32).wrapping_mul(16807) ^ y_position as u32,
            ) & 3;
            let scanline = if (y_position as u32).is_multiple_of(2) {
                0.95
            } else {
                1.0
            };
            let shading = (1.0 - y_position / bottom * 0.12) * scanline;
            let fill = blend(multiply_color(0x282a25, shading), 0x808080, texture * 2);
            let outer_tail = triangle_contains(
                x_position,
                y_position,
                [
                    [150.0, bottom - 6.0],
                    [186.0, bottom - 6.0],
                    [179.0, bottom + 18.0],
                ],
            );
            let inner_tail = triangle_contains(
                x_position,
                y_position,
                [
                    [154.0, bottom - 6.0],
                    [182.0, bottom - 6.0],
                    [179.0, bottom + 13.0],
                ],
            );
            if outer_tail {
                buffer[y * width + x] = if inner_tail { fill } else { border };
            }
            if rounded_contains(x_position, y_position, [6.0, 6.0, 314.0, bottom], 16.0) {
                buffer[y * width + x] = if rounded_contains(
                    x_position,
                    y_position,
                    [8.0, 8.0, 312.0, bottom - 2.0],
                    14.0,
                ) {
                    fill
                } else {
                    border
                };
            }
        }
    }
}

fn rounded_contains(x: f32, y: f32, rectangle: [f32; 4], radius: f32) -> bool {
    let [left, top, right, bottom] = rectangle;
    if x < left || x > right || y < top || y > bottom {
        return false;
    }
    let horizontal = (x - x.clamp(left + radius, right - radius)).abs();
    let vertical = (y - y.clamp(top + radius, bottom - radius)).abs();
    horizontal * horizontal + vertical * vertical <= radius * radius
}

fn triangle_contains(x: f32, y: f32, points: [[f32; 2]; 3]) -> bool {
    let mut positive = false;
    let mut negative = false;
    for index in 0..3 {
        let [ax, ay] = points[index];
        let [bx, by] = points[(index + 1) % 3];
        let side = (x - bx) * (ay - by) - (ax - bx) * (y - by);
        positive |= side > 0.0;
        negative |= side < 0.0;
    }
    !(positive && negative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_cache_stays_at_twelve_images() {
        let directory = std::env::temp_dir()
            .join("opencode")
            .join(format!("civilized-render-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut renderer = Renderer::new().unwrap();
        let mut buffer = vec![0; 320 * 240];
        for index in 0..15 {
            let path = directory.join(format!("frame-{index}.png"));
            RgbaImage::from_pixel(128, 128, image::Rgba([20, 40, 80, 255]))
                .save(&path)
                .unwrap();
            renderer.draw(&mut buffer, 320, 240, 1.0, Some(&path), 0.0);
        }
        assert_eq!(renderer.cache.len(), 12);
        assert_eq!(renderer.cache.back().unwrap().1.dimensions(), (128, 128));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn static_has_monochrome_grain_and_title_renders_below_message() {
        let mut renderer = Renderer::new().unwrap();
        renderer.text = "Done.".into();
        renderer.title = "My session".into();
        let mut buffer = vec![0; 320 * 240];
        renderer.draw(&mut buffer, 320, 240, 1.0, None, 1.0);
        let face: Vec<_> = (100..228)
            .flat_map(|y| (188..316).map(move |x| y * 320 + x))
            .map(|i| buffer[i])
            .collect();
        assert!(face.iter().any(|p| *p > 0x808080));
        assert!(face.iter().any(|p| *p < 0x404040));
        assert!(
            face.iter()
                .filter(|p| (**p & 255).abs_diff((**p >> 8) & 255) <= 1
                    && (**p & 255).abs_diff((**p >> 16) & 255) <= 1)
                .count()
                > face.len() * 9 / 10
        );
        assert!((54..68)
            .flat_map(|y| (26..180).map(move |x| y * 320 + x))
            .any(|index| buffer[index] > 0x606060));
    }

    #[test]
    fn interference_overlays_the_picture_instead_of_replacing_it() {
        let path = std::env::temp_dir()
            .join("opencode")
            .join(format!("civilized-overlay-{}.png", std::process::id()));
        RgbaImage::from_pixel(128, 128, image::Rgba([220, 60, 20, 255]))
            .save(&path)
            .unwrap();
        let mut renderer = Renderer::new().unwrap();
        let mut buffer = vec![0; 320 * 240];
        renderer.draw(&mut buffer, 320, 240, 1.0, Some(&path), 0.82);
        let center = buffer[164 * 320 + 252];
        assert!((center >> 16) & 255 > center & 255);
        assert_eq!(buffer[100 * 320 + 188], 0x080908);
        std::fs::remove_file(path).unwrap();
    }
}
