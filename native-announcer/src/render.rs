use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::Font;
use image::RgbaImage;
use std::path::PathBuf;

pub struct Renderer {
    font: Font,
    pub text: String,
    pub title: String,
    pub text_interference: f32,
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
                PathBuf::from(&directory).join("Fonts/GOTHIC.TTF"),
                PathBuf::from(directory).join("Fonts/segoeui.ttf"),
            ]
        } else if cfg!(target_os = "macos") {
            vec![
                "/Library/Fonts/Century Gothic.ttf".into(),
                "/System/Library/Fonts/Supplemental/Arial.ttf".into(),
            ]
        } else {
            vec![
                "/usr/share/fonts/truetype/msttcorefonts/Century_Gothic.ttf".into(),
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into(),
                "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf".into(),
            ]
        };
        let bytes = candidates
            .into_iter()
            .find_map(|path| std::fs::read(path).ok())
            .ok_or("No system font found")?;
        let font =
            Font::from_bytes(bytes, fontdue::FontSettings::default()).map_err(str::to_owned)?;
        Ok(Self {
            font,
            text: String::new(),
            title: String::new(),
            text_interference: 0.0,
            seed: 567891,
        })
    }

    pub fn message_height(&self) -> u32 {
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            max_width: Some(268.0),
            ..LayoutSettings::default()
        });
        layout.append(&[&self.font], &TextStyle::new(&self.text, 18.0, 0));
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
                    let mut x = glyph.x as i32 + column as i32;
                    let y = glyph.y as i32 + row as i32;
                    if self.text_interference > 0.0 {
                        let band = (y as f32 / (3.0 * scale)).floor() as u32;
                        let random = crate::state::noise_hash(band ^ self.seed);
                        if random.is_multiple_of(3) {
                            let offset = ((3 + (random >> 8) % 7) as f32 * scale).round() as i32;
                            x += if random & 1 == 0 { offset } else { -offset };
                        }
                    }
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
        image: Option<&RgbaImage>,
        interference: f32,
    ) {
        let resized = image.filter(|frame| frame.dimensions() != (128, 128)).map(|frame| {
            image::imageops::resize(frame, 128, 128, image::imageops::FilterType::Triangle)
        });
        let image = resized.as_ref().or(image);
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
            video_background(image),
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
                size: 18.0,
                color: 0xeef2f7,
                max_height: title_y - 10.0,
            },
        );
        let mut title = self.title.clone();
        while title
            .chars()
            .map(|c| self.font.metrics(c, 14.0).advance_width)
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
                size: 14.0,
                color: 0x9daabd,
                max_height: title_y + 22.0,
            },
        );
        shade_bubble_scanlines(buffer, width, height, scale, logical_height - 152.0);
        let face_x = (188.0 * scale) as usize;
        let face_y = ((logical_height - 140.0) * scale) as usize;
        let face_size = (128.0 * scale) as usize;
        let flicker = 0.85 + (self.random() % 16) as f32 / 100.0;
        let picture_flicker = 0.98 + (self.random() % 5) as f32 / 200.0;
        let vertical_offset = if interference > 0.0 {
            let jump = 2 + self.random() % 5;
            if self.random().is_multiple_of(2) {
                jump as f32
            } else {
                -(jump as f32)
            }
        } else {
            0.0
        };
        let mut row_displacement = [0_i32; 128];
        let mut row_snow = [false; 128];
        if interference > 0.0 {
            for _ in 0..4 {
                let start = self.random() as usize % 128;
                let length = 6 + self.random() as usize % 18;
                let displacement = 5 + self.random() % 15;
                let shift = if self.random().is_multiple_of(2) {
                    displacement as i32
                } else {
                    -(displacement as i32)
                };
                for row in start..(start + length).min(128) {
                    row_displacement[row] = shift;
                    row_snow[row] = true;
                }
            }
        }
        let scanline_phase = self.random() as usize % 3;
        for y in 0..face_size {
            let source_y = y * 128 / face_size;
            let row_offset = row_displacement[source_y];
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
                let warped_x = (horizontal * curvature + 1.0) * 63.5;
                let warped_y = (vertical * curvature + 1.0) * 63.5;
                if !(0.0..128.0).contains(&warped_x) || !(0.0..128.0).contains(&warped_y) {
                    buffer[index] = 0x080908;
                    continue;
                }
                if let Some(frame) = image {
                    let sample_x = (warped_x + row_offset as f32).rem_euclid(128.0) as u32;
                    let sample_y = (warped_y + vertical_offset).rem_euclid(128.0) as u32;
                    let mut pixel = frame.get_pixel(sample_x, sample_y).0;
                    if row_snow[source_y] {
                        pixel[0] = frame.get_pixel((sample_x + 126) % 128, sample_y).0[0];
                        pixel[2] = frame.get_pixel((sample_x + 2) % 128, sample_y).0[2];
                    }
                    let color =
                        ((pixel[0] as u32) << 16) | ((pixel[1] as u32) << 8) | pixel[2] as u32;
                    buffer[index] = blend(buffer[index], color, pixel[3] as u32);
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
                        grain = if random & 4 == 0 { 0 } else { 180 };
                    }
                    if (source_y + scanline_phase).is_multiple_of(3) {
                        grain = grain * 2 / 3;
                    }
                    grain = grain.min(180);
                    let strength =
                        interference.clamp(0.0, 1.0) * if row_snow[source_y] { 1.0 } else { 0.12 };
                    let alpha = (strength * flicker * 180.0) as u32;
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

fn shade_bubble_scanlines(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    scale: f32,
    bottom: f32,
) {
    for y in 0..height {
        if (y as f32 / scale).floor() as usize % 2 != 0 {
            continue;
        }
        let y_position = y as f32 / scale;
        for x in 0..width {
            let x_position = x as f32 / scale;
            if bubble_contains(x_position, y_position, bottom) {
                let index = y * width + x;
                buffer[index] = multiply_color(buffer[index], 0.82);
            }
        }
    }
}

fn bubble_contains(x: f32, y: f32, bottom: f32) -> bool {
    rounded_contains(x, y, [8.0, 10.0, 316.0, bottom + 4.0], 20.0)
        || triangle_contains(
            x,
            y,
            [
                [236.0, bottom - 4.0],
                [268.0, bottom - 4.0],
                [252.0, bottom + 18.0],
            ],
        )
        || rounded_contains(x, y, [6.0, 6.0, 314.0, bottom], 20.0)
}

fn draw_bubble(
    buffer: &mut [u32],
    width: usize,
    height: usize,
    scale: f32,
    bottom: f32,
    fill: u32,
) {
    for y in 0..height {
        for x in 0..width {
            let x_position = x as f32 / scale;
            let y_position = y as f32 / scale;
            if rounded_contains(
                x_position,
                y_position,
                [8.0, 10.0, 316.0, bottom + 4.0],
                20.0,
            ) {
                buffer[y * width + x] = 0x0d1119;
            }
            let outer_tail = triangle_contains(
                x_position,
                y_position,
                [
                    [236.0, bottom - 4.0],
                    [268.0, bottom - 4.0],
                    [252.0, bottom + 18.0],
                ],
            );
            if outer_tail {
                buffer[y * width + x] = fill;
            }
            if rounded_contains(x_position, y_position, [6.0, 6.0, 314.0, bottom], 20.0) {
                buffer[y * width + x] = fill;
                if (26.0..294.0).contains(&x_position)
                    && (bottom - 46.0..bottom - 45.0).contains(&y_position)
                {
                    buffer[y * width + x] = blend(fill, 0xffffff, 30);
                }
            }
        }
    }
}

fn video_background(image: Option<&RgbaImage>) -> u32 {
    let Some(image) = image.filter(|image| image.width() > 0 && image.height() > 0) else {
        return 0x191f2a;
    };
    let right = image.width() - 1;
    let bottom = image.height() - 1;
    let mut channels = [0_u32; 3];
    let mut weight = 0;
    for x in [
        right / 32,
        right / 16,
        right - right / 16,
        right - right / 32,
    ] {
        for y in [bottom / 32, bottom / 16] {
            let pixel = image.get_pixel(x, y).0;
            let alpha = pixel[3] as u32;
            weight += alpha;
            for index in 0..3 {
                channels[index] += pixel[index] as u32 * alpha;
            }
        }
    }
    if weight == 0 {
        return 0x191f2a;
    }
    ((channels[0] / weight) << 16) | ((channels[1] / weight) << 8) | (channels[2] / weight)
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
    fn bubble_has_no_accent_and_keeps_the_footer_and_tail() {
        let mut buffer = vec![0xff00ff; 320 * 260];
        draw_bubble(&mut buffer, 320, 260, 1.0, 108.0, 0x191f2a);
        assert_eq!(buffer[17 * 320 + 40], 0x191f2a);
        assert_eq!(buffer[62 * 320 + 40], blend(0x191f2a, 0xffffff, 30));
        assert_eq!(buffer[122 * 320 + 252], 0x191f2a);
        assert_eq!(buffer[30 * 320 + 6], 0x191f2a);
        assert_eq!(buffer[6 * 320 + 6], 0xff00ff);
        assert_eq!(buffer[30 * 320 + 315], 0x0d1119);
    }

    #[test]
    fn bubble_scales_without_changing_its_layout() {
        let mut normal = vec![0xff00ff; 320 * 260];
        let mut doubled = vec![0xff00ff; 640 * 520];
        draw_bubble(&mut normal, 320, 260, 1.0, 108.0, 0x191f2a);
        draw_bubble(&mut doubled, 640, 520, 2.0, 108.0, 0x191f2a);
        for y in 0..260 {
            for x in 0..320 {
                assert_eq!(normal[y * 320 + x], doubled[y * 2 * 640 + x * 2]);
            }
        }
    }

    #[test]
    fn bubble_uses_the_average_of_both_video_top_corners() {
        let frame = RgbaImage::from_fn(128, 128, |x, y| {
            if y < 10 && x < 10 {
                image::Rgba([20, 30, 40, 255])
            } else if y < 10 && x > 117 {
                image::Rgba([40, 50, 60, 255])
            } else {
                image::Rgba([220, 180, 140, 255])
            }
        });
        assert_eq!(video_background(Some(&frame)), 0x1e2832);
        let mut renderer = Renderer::new().unwrap();
        let mut buffer = vec![0; 320 * 260];
        renderer.draw(&mut buffer, 320, 260, 1.0, Some(&frame), 0.0);
        assert_eq!(buffer[30 * 320 + 150], 0x182029);
    }

    #[test]
    fn video_background_ignores_transparent_samples_and_handles_missing_frames() {
        let frame = RgbaImage::from_fn(128, 128, |x, _| {
            if x < 64 {
                image::Rgba([255, 0, 255, 0])
            } else {
                image::Rgba([20, 30, 40, 255])
            }
        });
        assert_eq!(video_background(Some(&frame)), 0x141e28);
        assert_eq!(video_background(None), 0x191f2a);
        assert_eq!(video_background(Some(&RgbaImage::new(0, 0))), 0x191f2a);
    }

    #[test]
    fn renderer_scanlines_shade_bubble_contents_but_not_background() {
        let mut renderer = Renderer::new().unwrap();
        renderer.text = "Done.".into();
        renderer.title = "Session".into();
        let mut buffer = vec![0; 320 * 260];
        renderer.draw(&mut buffer, 320, 260, 1.0, None, 0.0);
        assert_eq!(buffer[30 * 320 + 150], 0x141922);
        assert_eq!(buffer[31 * 320 + 150], 0x191f2a);
        assert_eq!(buffer[110 * 320 + 252], 0x141922);
        assert_eq!(buffer[111 * 320 + 252], 0x191f2a);
        assert_eq!(buffer[62 * 320 + 40], 0x2a2e36);
        assert_eq!(
            buffer[0],
            if cfg!(target_os = "windows") {
                0xff00ff
            } else {
                0x1c1b16
            }
        );
        assert!((26..43)
            .flat_map(|y| (26..120).map(move |x| y * 320 + x))
            .any(|index| buffer[index] == 0xc3c6ca));
        assert!((26..43)
            .flat_map(|y| (26..120).map(move |x| y * 320 + x))
            .any(|index| buffer[index] == 0xecf0f5));
        assert!((72..94)
            .flat_map(|y| (26..120).map(move |x| y * 320 + x))
            .any(|index| buffer[index] == 0x808b9a));
        assert!((72..94)
            .flat_map(|y| (26..120).map(move |x| y * 320 + x))
            .any(|index| buffer[index] == 0x99a6b9));
    }

    #[test]
    fn renderer_scanlines_follow_logical_rows_at_multiple_scales() {
        let mut renderer = Renderer::new().unwrap();
        for (scale, width, height, even_row, odd_row) in [
            (1.0, 320, 240, 30, 31),
            (1.5, 480, 360, 45, 47),
            (2.0, 640, 480, 60, 63),
        ] {
            let mut buffer = vec![0; width * height];
            renderer.draw(&mut buffer, width, height, scale, None, 0.0);
            let x = (150.0 * scale) as usize;
            assert_eq!(buffer[even_row * width + x], 0x141922);
            assert_eq!(buffer[odd_row * width + x], 0x191f2a);
        }
    }

    #[test]
    fn text_tearing_leaves_bubble_and_picture_unchanged() {
        let mut renderer = Renderer::new().unwrap();
        renderer.text = "The task is complete.".into();
        renderer.title = "My session".into();
        let mut clean = vec![0; 320 * 240];
        renderer.draw(&mut clean, 320, 240, 1.0, None, 0.0);
        renderer.seed = 567891;
        renderer.text_interference = 0.62;
        let mut distorted = vec![0; 320 * 240];
        renderer.draw(&mut distorted, 320, 240, 1.0, None, 0.0);
        assert_ne!(clean, distorted);
        for y in 0..240 {
            for x in 0..320 {
                if !(26..75).contains(&y) || !(14..306).contains(&x) {
                    assert_eq!(clean[y * 320 + x], distorted[y * 320 + x]);
                }
            }
        }
    }

    #[test]
    fn scales_the_whole_large_video_into_the_portrait_frame() {
        let frame = RgbaImage::from_fn(256, 256, |x, y| {
            if x >= 128 && y >= 128 {
                image::Rgba([20, 40, 220, 255])
            } else {
                image::Rgba([220, 40, 20, 255])
            }
        });
        let mut renderer = Renderer::new().unwrap();
        let mut buffer = vec![0; 320 * 240];
        renderer.draw(&mut buffer, 320, 240, 1.0, Some(&frame), 0.0);
        let bottom_right = buffer[196 * 320 + 284];
        assert!(bottom_right & 255 > (bottom_right >> 16) & 255);
    }

    #[test]
    fn renders_decoded_video_frames_without_a_file_cache() {
        let mut renderer = Renderer::new().unwrap();
        let mut buffer = vec![0; 320 * 240];
        for _ in 0..15 {
            let frame = RgbaImage::from_pixel(128, 128, image::Rgba([20, 40, 80, 255]));
            renderer.draw(&mut buffer, 320, 240, 1.0, Some(&frame), 0.0);
        }
        let center = buffer[164 * 320 + 252];
        assert!(center & 255 > (center >> 16) & 255);
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
        assert!(face.iter().any(|p| *p > 0x606060));
        assert!(face.iter().any(|p| *p < 0x404040));
        assert!(
            face.iter()
                .filter(|p| (**p & 255).abs_diff((**p >> 8) & 255) <= 2
                    && (**p & 255).abs_diff((**p >> 16) & 255) <= 2)
                .count()
                > face.len() * 9 / 10
        );
        assert!((54..68)
            .flat_map(|y| (26..180).map(move |x| y * 320 + x))
            .any(|index| buffer[index] > 0x606060));
    }

    #[test]
    fn interference_overlays_the_picture_instead_of_replacing_it() {
        let frame = RgbaImage::from_pixel(128, 128, image::Rgba([220, 60, 20, 255]));
        let mut renderer = Renderer::new().unwrap();
        let mut buffer = vec![0; 320 * 240];
        renderer.draw(&mut buffer, 320, 240, 1.0, Some(&frame), 0.82);
        let center = buffer[164 * 320 + 252];
        assert!((center >> 16) & 255 > center & 255);
        assert_eq!(buffer[100 * 320 + 188], 0x080908);
    }

    #[test]
    fn interference_displaces_picture_features_before_adding_noise() {
        let frame = RgbaImage::from_fn(128, 128, |x, _| {
            image::Rgba([if (60..64).contains(&x) { 255 } else { 0 }, 0, 0, 255])
        });
        let mut clean = vec![0; 320 * 240];
        let mut distorted = vec![0; 320 * 240];
        Renderer::new()
            .unwrap()
            .draw(&mut clean, 320, 240, 1.0, Some(&frame), 0.0);
        Renderer::new()
            .unwrap()
            .draw(&mut distorted, 320, 240, 1.0, Some(&frame), 0.62);
        let peak = |buffer: &[u32], y: usize| {
            (210..290)
                .max_by_key(|x| (buffer[y * 320 + x] >> 16) & 255)
                .unwrap()
        };
        assert!((120..210).any(|y| peak(&clean, y).abs_diff(peak(&distorted, y)) >= 5));
    }
}
