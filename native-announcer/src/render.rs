use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::Font;
use image::RgbaImage;
use std::time::Duration;
use crate::settings::{FontPreference, Settings};

pub fn display_text(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some((before, tag)) = remaining.split_once('[') {
        let Some((_, after)) = tag.split_once(']') else { break; };
        plain.push_str(before);
        remaining = after;
    }
    plain.push_str(remaining);
    let mut result = String::with_capacity(plain.len());
    let mut space = false;
    for character in plain.trim().chars() {
        if character != ' ' || !space { result.push(character); }
        space = character == ' ';
    }
    result
}

pub struct Renderer {
    catalog: crate::fonts::FontCatalog,
    body_font: Font,
    title_font: Font,
    body_preference: FontPreference,
    title_preference: FontPreference,
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
    #[cfg(test)]
    pub fn new() -> Result<Self, String> {
        let settings = Settings::default();
        Self::with_settings(&settings)
    }

    pub fn with_settings(settings: &Settings) -> Result<Self, String> {
        let catalog = crate::fonts::FontCatalog::new()?;
        let body_font = catalog.get(&settings.announcement_body_font.family);
        let title_font = catalog.get(&settings.announcement_title_font.family);
        Ok(Self {
            catalog,
            body_font,
            title_font,
            body_preference: settings.announcement_body_font.clone(),
            title_preference: settings.announcement_title_font.clone(),
            text: String::new(),
            title: String::new(),
            text_interference: 0.0,
            seed: 567891,
        })
    }

    pub fn set_preferences(&mut self, body: &FontPreference, title: &FontPreference) {
        if &self.body_preference == body && &self.title_preference == title { return; }
        self.body_preference = body.clone();
        self.title_preference = title.clone();
        self.body_font = self.catalog.get(&body.family);
        self.title_font = self.catalog.get(&title.family);
    }

    #[cfg(test)]
    pub fn body_preference(&self) -> &FontPreference { &self.body_preference }

    #[cfg(test)]
    pub fn title_preference(&self) -> &FontPreference { &self.title_preference }

    pub fn message_height(&self, scale: f32) -> u32 {
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings {
            max_width: Some(268.0 * scale),
            ..LayoutSettings::default()
        });
        layout.append(&[&self.body_font], &TextStyle::new(&self.text, f32::from(self.body_preference.size) * scale, 0));
        (layout.height() / scale).ceil() as u32
    }

    fn text(
        &self,
        buffer: &mut [u32],
        width: usize,
        height: usize,
        scale: f32,
        font: &Font,
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
            &[font],
            &TextStyle::new(block.text, block.size * scale, 0),
        );
        for glyph in layout.glyphs() {
            if glyph.y + glyph.height as f32 > block.max_height * scale {
                continue;
            }
            let (_, pixels) = font.rasterize_config(glyph.key);
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
        entrance: Option<Duration>,
    ) {
        let resized = image.filter(|frame| frame.dimensions() != (128, 128)).map(|frame| {
            image::imageops::resize(frame, 128, 128, image::imageops::FilterType::Triangle)
        });
        let image = resized.as_ref().or(image);
        buffer.fill(if cfg!(any(target_os = "windows", target_os = "linux")) {
            0xff00ff
        } else {
            0x1c1b16
        });
        let logical_height = height as f32 / scale;
        let strike = entrance.map(|elapsed| (elapsed, crate::state::interference_amount(elapsed, crate::state::SIGNAL_SEED)))
            .filter(|(_, amount)| *amount > 0.0);
        let mut coverage = strike.map(|_| PaintCoverage { pixels: vec![false; width * height] });
        draw_bubble(
            buffer,
            width,
            height,
            scale,
            logical_height - 152.0,
            video_background(image),
            coverage.as_mut(),
        );
        let title_y = logical_height - 188.0;
        self.text(
            buffer,
            width,
            height,
            scale,
            &self.body_font,
            TextBlock {
                text: &self.text,
                y: 26.0,
                size: f32::from(self.body_preference.size),
                color: 0xeef2f7,
                max_height: title_y - 10.0,
            },
        );
        let mut title = self.title.clone();
        while title
            .chars()
            .map(|c| self.title_font.metrics(c, f32::from(self.title_preference.size)).advance_width)
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
            &self.title_font,
            TextBlock {
                text: &title,
                y: title_y,
                size: f32::from(self.title_preference.size),
                color: 0x9daabd,
                max_height: title_y + f32::from(self.title_preference.size) + 8.0,
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
                if let Some(coverage) = &mut coverage { coverage.pixels[index] = true; }
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
        if let (Some(coverage), Some((elapsed, amount))) = (coverage, strike) {
            coverage.lightning(buffer, width, height, scale, elapsed, amount);
        }
    }

    fn random(&mut self) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed
    }
}

struct PaintCoverage {
    pixels: Vec<bool>,
}

fn paint(buffer: &mut [u32], coverage: &mut Option<&mut PaintCoverage>, index: usize, color: u32) {
    buffer[index] = color;
    if let Some(coverage) = coverage { coverage.pixels[index] = true; }
}

impl PaintCoverage {
    fn contours(&self, width: usize, height: usize) -> Vec<Vec<[f32; 2]>> {
        let stride = width + 1;
        let mut edges = vec![0_u8; stride * (height + 1)];
        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                if !self.pixels[index] { continue; }
                if y == 0 || !self.pixels[index - width] { edges[y * stride + x] |= 1; }
                if x + 1 == width || !self.pixels[index + 1] { edges[y * stride + x + 1] |= 2; }
                if y + 1 == height || !self.pixels[index + width] { edges[(y + 1) * stride + x + 1] |= 4; }
                if x == 0 || !self.pixels[index - 1] { edges[(y + 1) * stride + x] |= 8; }
            }
        }
        let mut contours = Vec::new();
        for start in 0..edges.len() {
            while edges[start] != 0 {
                let mut contour = Vec::new();
                let mut vertex = start;
                let mut direction = edges[start].trailing_zeros() as usize;
                loop {
                    contour.push([(vertex % stride) as f32 - 0.5, (vertex / stride) as f32 - 0.5]);
                    edges[vertex] &= !(1 << direction);
                    vertex = match direction {
                        0 => vertex + 1,
                        1 => vertex + stride,
                        2 => vertex - 1,
                        _ => vertex - stride,
                    };
                    if vertex == start { break; }
                    direction = [(direction + 1) % 4, direction, (direction + 3) % 4, (direction + 2) % 4]
                        .into_iter().find(|direction| edges[vertex] & (1 << direction) != 0).unwrap();
                }
                contours.push(contour);
            }
        }
        contours
    }

    fn lightning(&self, buffer: &mut [u32], width: usize, height: usize, scale: f32, elapsed: Duration, amount: f32) {
        let mut glow = vec![0_u8; buffer.len()];
        let tick = (elapsed.as_millis() / 30) as u32;
        for contour in self.contours(width, height) {
            let length = contour.len();
            let step = (5.0 * scale).round().max(1.0) as usize;
            for run in 0..4 {
                let seed = crate::state::noise_hash(tick ^ (run + 1) * 9187);
                let start = (length * run as usize / 4 + tick as usize * step * 3 + seed as usize % (length / 8).max(1)) % length;
                let reach = ((80 + seed % 65) as f32 * scale) as usize;
                let mut previous = None;
                for distance in (0..reach.min(length)).step_by(step) {
                    let index = (start + distance) % length;
                    let before = contour[(index + length - step.min(length - 1)) % length];
                    let after = contour[(index + step) % length];
                    let dx = after[0] - before[0];
                    let dy = after[1] - before[1];
                    let magnitude = dx.hypot(dy).max(1.0);
                    let normal = [-dy / magnitude, dx / magnitude];
                    let random = crate::state::noise_hash(seed ^ distance as u32);
                    let inset = (1.0 + (random & 255) as f32 / 255.0 * 4.0) * scale;
                    let point = [contour[index][0] + normal[0] * inset, contour[index][1] + normal[1] * inset];
                    if let Some(previous) = previous {
                        self.stroke(&mut glow, width, height, scale, previous, point, 8.0);
                    }
                    if distance > 0 && (distance / step) % 7 == 3 {
                        let branch = [point[0] + normal[0] * 7.0 * scale + dx / magnitude * 3.0 * scale,
                            point[1] + normal[1] * 7.0 * scale + dy / magnitude * 3.0 * scale];
                        let tip = [branch[0] + normal[0] * 4.0 * scale - dx / magnitude * 4.0 * scale,
                            branch[1] + normal[1] * 4.0 * scale - dy / magnitude * 4.0 * scale];
                        self.stroke(&mut glow, width, height, scale, point, branch, 4.0);
                        self.stroke(&mut glow, width, height, scale, branch, tip, 2.0);
                    }
                    previous = Some(point);
                }
            }
        }
        for (index, strength) in glow.into_iter().enumerate() {
            if strength == 0 { continue; }
            let background = if self.pixels[index] { buffer[index] } else { 0x001020 };
            let color = if strength > 220 { 0xffffff } else if strength > 150 { 0x00dfff } else { 0x007dff };
            let alpha = (strength as f32 * (amount * 1.8).min(1.0)) as u32;
            buffer[index] = blend(background, color, alpha);
        }
    }

    fn stroke(&self, glow: &mut [u8], width: usize, height: usize, scale: f32, from: [f32; 2], to: [f32; 2], radius: f32) {
        let radius = radius * scale;
        let left = ((from[0].min(to[0]) - radius).floor().max(0.0) as usize).min(width);
        let right = ((from[0].max(to[0]) + radius).ceil().max(0.0) as usize).min(width);
        let top = ((from[1].min(to[1]) - radius).floor().max(0.0) as usize).min(height);
        let bottom = ((from[1].max(to[1]) + radius).ceil().max(0.0) as usize).min(height);
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let squared = (dx * dx + dy * dy).max(0.001);
        for y in top..bottom {
            for x in left..right {
                let along = (((x as f32 - from[0]) * dx + (y as f32 - from[1]) * dy) / squared).clamp(0.0, 1.0);
                let distance = (x as f32 - from[0] - along * dx).hypot(y as f32 - from[1] - along * dy);
                let index = y * width + x;
                if distance >= radius || (!self.pixels[index] && distance > 2.5 * scale) { continue; }
                let distance = distance / scale;
                let strength = if distance < 0.65 { 255 } else if distance < 1.5 { 200 }
                    else { (120.0 * (1.0 - distance / (radius / scale))) as u8 };
                glow[index] = glow[index].max(strength);
            }
        }
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
    rounded_contains(x, y, [7.0, 8.0, 315.0, bottom + 2.0], 20.0)
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
    mut coverage: Option<&mut PaintCoverage>,
) {
    for y in 0..height {
        for x in 0..width {
            let x_position = x as f32 / scale;
            let y_position = y as f32 / scale;
            if rounded_contains(
                x_position,
                y_position,
                [7.0, 8.0, 315.0, bottom + 2.0],
                20.0,
            ) {
                paint(buffer, &mut coverage, y * width + x, 0x080908);
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
                paint(buffer, &mut coverage, y * width + x, 0x080908);
            }
            if rounded_contains(x_position, y_position, [6.0, 6.0, 314.0, bottom], 20.0) {
                paint(buffer, &mut coverage, y * width + x, 0x080908);
            }
            if rounded_contains(x_position, y_position, [8.0, 8.0, 312.0, bottom - 2.0], 18.0) {
                paint(buffer, &mut coverage, y * width + x, fill);
                if (26.0..294.0).contains(&x_position)
                    && (bottom - 46.0..bottom - 45.0).contains(&y_position)
                {
                    buffer[y * width + x] = blend(fill, 0xffffff, 30);
                }
            }
            if triangle_contains(
                x_position,
                y_position,
                [
                    [239.0, bottom - 6.0],
                    [265.0, bottom - 6.0],
                    [252.0, bottom + 14.0],
                ],
            ) {
                paint(buffer, &mut coverage, y * width + x, fill);
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
    fn entrance_lightning_tracks_the_painted_union_at_different_scales_and_heights() {
        let mut renderer = Renderer::new().unwrap();
        renderer.text = "The task passed.".into();
        renderer.title = "Checks passed".into();
        let image = RgbaImage::from_pixel(128, 128, image::Rgba([70, 40, 20, 255]));
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for logical_height in [260, 340] {
                let width = (320.0 * scale) as usize;
                let height = (logical_height as f32 * scale) as usize;
                let bottom = logical_height as f32 - 152.0;
                let zones = [(14.0, 14.0), (6.0, 50.0), (100.0, bottom), (244.0, bottom + 8.0),
                    (294.0, bottom + 12.0), (188.0, bottom + 76.0), (315.0, bottom + 76.0), (252.0, bottom + 139.0)];
                let mut reached = [false; 8];
                let mut cyan = 0;
                let mut white = 0;
                let mut wide_glow = 0;
                for milliseconds in (30..630).step_by(30) {
                    renderer.seed = 567891;
                    let mut plain = vec![0; width * height];
                    renderer.draw(&mut plain, width, height, scale, Some(&image), 0.0, None);
                    renderer.seed = 567891;
                    let mut struck = vec![0; width * height];
                    renderer.draw(&mut struck, width, height, scale, Some(&image), 0.0, Some(Duration::from_millis(milliseconds)));
                    for (index, (&before, &after)) in plain.iter().zip(&struck).enumerate() {
                        if before == after { continue; }
                        let x = (index % width) as f32 / scale;
                        let y = (index / width) as f32 / scale;
                        let red = (after >> 16) & 255;
                        let green = (after >> 8) & 255;
                        let blue = after & 255;
                        cyan += usize::from(red < 90 && green > 110 && blue > 190);
                        white += usize::from(red > 200 && green > 200 && blue > 200);
                        wide_glow += usize::from((10.0..18.0).contains(&y) && (40.0..280.0).contains(&x) && blue > red + 20);
                        if before == 0xff00ff { assert!(red <= green && green <= blue, "Clear pixel acquired a magenta halo"); }
                        assert!(!(40.0..290.0).contains(&x) || !(26.0..bottom - 22.0).contains(&y), "Text interior changed at {x},{y}");
                        assert!(!(214.0..290.0).contains(&x) || !(bottom + 38.0..bottom + 114.0).contains(&y), "Portrait center changed at {x},{y}");
                        assert!(x > 0.0 && y > 0.0 && y < logical_height as f32 - 1.0);
                        for (zone, &(zx, zy)) in zones.iter().enumerate() {
                            if (x - zx).abs() < 9.0 && (y - zy).abs() < 9.0 && blue > red + 30 {
                                reached[zone] = true;
                            }
                        }
                    }
                }
                assert_eq!(reached, [true; 8], "Missing outline strike at scale {scale}, height {logical_height}");
                assert!(cyan >= 200 && white >= 30 && wide_glow >= 200, "Not a bright, wide lightning strike: cyan={cyan}, white={white}, glow={wide_glow}");
            }
        }
    }

    #[test]
    fn entrance_lightning_is_animated_deterministic_and_leaves_the_portrait_random_stream_alone() {
        let mut renderer = Renderer::new().unwrap();
        let mut plain = vec![0; 320 * 260];
        renderer.draw(&mut plain, 320, 260, 1.0, None, 0.62, None);
        let mut next_plain = vec![0; plain.len()];
        renderer.draw(&mut next_plain, 320, 260, 1.0, None, 0.0, None);
        let mut struck = vec![0; plain.len()];
        renderer.seed = 567891;
        renderer.draw(&mut struck, 320, 260, 1.0, None, 0.62, Some(Duration::from_millis(120)));
        assert!(struck.iter().filter(|pixel| **pixel == 0xffffff).count() >= 10);
        let mut next_struck = vec![0; plain.len()];
        renderer.draw(&mut next_struck, 320, 260, 1.0, None, 0.0, None);
        assert_eq!(next_plain, next_struck);
        let mut repeated = vec![0; plain.len()];
        renderer.seed = 567891;
        renderer.draw(&mut repeated, 320, 260, 1.0, None, 0.62, Some(Duration::from_millis(120)));
        assert_eq!(struck, repeated);
        renderer.seed = 567891;
        renderer.draw(&mut repeated, 320, 260, 1.0, None, 0.62, Some(Duration::from_millis(240)));
        assert!(struck.iter().zip(&repeated).filter(|(a, b)| a != b).count() >= 300);
        for milliseconds in [0, 650, 900, 5000] {
            renderer.seed = 567891;
            renderer.draw(&mut repeated, 320, 260, 1.0, None, 0.62, Some(Duration::from_millis(milliseconds)));
            assert_eq!(plain, repeated, "Lightning outside its envelope at {milliseconds}ms");
        }
    }

    #[test]
    fn announcement_height_fits_the_last_word_at_windows_display_scales() {
        let mut renderer = Renderer::new().unwrap();
        renderer.text = display_text("[excited] Committed and deployed announcement cleanup and text-editing improvements! Checks passed!");
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5] {
            let height = renderer.message_height(scale) + 238;
            let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
            layout.reset(&LayoutSettings {
                x: 26.0 * scale,
                y: 26.0 * scale,
                max_width: Some(268.0 * scale),
                ..LayoutSettings::default()
            });
            layout.append(&[&renderer.body_font], &TextStyle::new(&renderer.text, f32::from(renderer.body_preference.size) * scale, 0));
            let cutoff = (height as f32 - 198.0) * scale;
            for glyph in layout.glyphs() {
                assert!(glyph.y + glyph.height as f32 <= cutoff, "The final line is clipped at scale {scale}: {} > {cutoff}", glyph.y + glyph.height as f32);
            }
        }
    }

    #[test]
    fn displayed_text_removes_tags_and_collapses_spaces() {
        for (input, expected) in [
            ("[excited] Added Ctrl+A  and preserved scroll position; verified.", "Added Ctrl+A and preserved scroll position; verified."),
            (" [happy]  Done [pause]   successfully. [laughs] ", "Done successfully."),
            ("[excited][laughs] Hello   世界 😀", "Hello 世界 😀"),
            ("First  line.\nSecond   line.", "First line.\nSecond line."),
            ("Ordinary text.", "Ordinary text."),
            ("Keep [unfinished", "Keep [unfinished"),
            ("[pause] []", ""),
            ("", ""),
        ] {
            assert_eq!(display_text(input), expected);
        }
    }

    #[test]
    fn display_cleanup_preserves_the_original_speech_text() {
        let notification: crate::state::Notification = serde_json::from_value(serde_json::json!({
            "id": "display-cleanup", "sessionID": "opencode", "completed": 1,
            "text": "[excited] The  task passed.", "title": "[happy] Checks   passed",
        })).unwrap();
        assert_eq!(display_text(&notification.text), "The task passed.");
        assert_eq!(display_text(&notification.title), "Checks passed");
        assert_eq!(notification.text, "[excited] The  task passed.");
        assert_eq!(notification.title, "[happy] Checks   passed");
    }

    #[test]
    fn bubble_has_a_frame_and_keeps_the_footer_and_tail() {
        let mut buffer = vec![0xff00ff; 320 * 260];
        draw_bubble(&mut buffer, 320, 260, 1.0, 108.0, 0x191f2a, None);
        assert_eq!(buffer[6 * 320 + 150], 0x080908);
        assert_eq!(buffer[8 * 320 + 150], 0x191f2a);
        assert_eq!(buffer[30 * 320 + 314], 0x080908);
        assert_eq!(buffer[17 * 320 + 40], 0x191f2a);
        assert_eq!(buffer[62 * 320 + 40], blend(0x191f2a, 0xffffff, 30));
        assert_eq!(buffer[106 * 320 + 150], 0x191f2a);
        assert_eq!(buffer[108 * 320 + 150], 0x080908);
        assert_eq!(buffer[104 * 320 + 252], 0x191f2a);
        assert_eq!(buffer[122 * 320 + 252], 0x191f2a);
        assert_eq!(buffer[124 * 320 + 252], 0x080908);
        assert_eq!(buffer[30 * 320 + 6], 0x080908);
        assert_eq!(buffer[6 * 320 + 6], 0xff00ff);
        assert_eq!(buffer[30 * 320 + 315], 0x080908);
        assert_eq!(buffer[30 * 320 + 316], 0xff00ff);
    }

    #[test]
    fn bubble_scales_without_changing_its_layout() {
        let mut normal = vec![0xff00ff; 320 * 260];
        let mut doubled = vec![0xff00ff; 640 * 520];
        draw_bubble(&mut normal, 320, 260, 1.0, 108.0, 0x191f2a, None);
        draw_bubble(&mut doubled, 640, 520, 2.0, 108.0, 0x191f2a, None);
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
        renderer.draw(&mut buffer, 320, 260, 1.0, Some(&frame), 0.0, None);
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
        renderer.draw(&mut buffer, 320, 260, 1.0, None, 0.0, None);
        assert_eq!(buffer[30 * 320 + 150], 0x141922);
        assert_eq!(buffer[31 * 320 + 150], 0x191f2a);
        assert_eq!(buffer[110 * 320 + 252], 0x141922);
        assert_eq!(buffer[111 * 320 + 252], 0x191f2a);
        assert_eq!(buffer[62 * 320 + 40], 0x2a2e36);
        assert_eq!(
            buffer[0],
            if cfg!(any(target_os = "windows", target_os = "linux")) {
                0xff00ff
            } else {
                0x1c1b16
            }
        );
        for (font, text, y, size, color, cutoff) in [
            (&renderer.body_font, renderer.text.as_str(), 26.0, renderer.body_preference.size, 0xeef2f7, 62.0),
            (&renderer.title_font, renderer.title.as_str(), 72.0, renderer.title_preference.size, 0x9daabd, 72.0 + f32::from(renderer.title_preference.size) + 8.0),
        ] {
            let mut unshaded = vec![0; 320 * 260];
            draw_bubble(&mut unshaded, 320, 260, 1.0, 108.0, 0x191f2a, None);
            let background = unshaded.clone();
            renderer.text(&mut unshaded, 320, 260, 1.0, font, TextBlock {
                text, y, size: f32::from(size), color, max_height: cutoff,
            });
            let mut glyph_pixels = [0; 2];
            for index in 0..unshaded.len() {
                if unshaded[index] == background[index] { continue; }
                let row = index / 320;
                glyph_pixels[row % 2] += 1;
                let expected = if row % 2 == 0 {
                    let pixel = unshaded[index];
                    let red = (((pixel >> 16) & 255) as f32 * 0.82) as u32;
                    let green = (((pixel >> 8) & 255) as f32 * 0.82) as u32;
                    let blue = ((pixel & 255) as f32 * 0.82) as u32;
                    (red << 16) | (green << 8) | blue
                } else { unshaded[index] };
                assert_eq!(buffer[index], expected, "Glyph pixel at {}, {row}", index % 320);
            }
            assert!(glyph_pixels[0] > 0 && glyph_pixels[1] > 0, "Missing glyph pixels on either scanline parity for {text}");
        }
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
            renderer.draw(&mut buffer, width, height, scale, None, 0.0, None);
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
        renderer.draw(&mut clean, 320, 240, 1.0, None, 0.0, None);
        renderer.seed = 567891;
        renderer.text_interference = 0.62;
        let mut distorted = vec![0; 320 * 240];
        renderer.draw(&mut distorted, 320, 240, 1.0, None, 0.0, None);
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
        renderer.draw(&mut buffer, 320, 240, 1.0, Some(&frame), 0.0, None);
        let bottom_right = buffer[196 * 320 + 284];
        assert!(bottom_right & 255 > (bottom_right >> 16) & 255);
    }

    #[test]
    fn renders_decoded_video_frames_without_a_file_cache() {
        let mut renderer = Renderer::new().unwrap();
        let mut buffer = vec![0; 320 * 240];
        for _ in 0..15 {
            let frame = RgbaImage::from_pixel(128, 128, image::Rgba([20, 40, 80, 255]));
            renderer.draw(&mut buffer, 320, 240, 1.0, Some(&frame), 0.0, None);
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
        renderer.draw(&mut buffer, 320, 240, 1.0, None, 1.0, None);
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
        renderer.draw(&mut buffer, 320, 240, 1.0, Some(&frame), 0.82, None);
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
            .draw(&mut clean, 320, 240, 1.0, Some(&frame), 0.0, None);
        Renderer::new()
            .unwrap()
            .draw(&mut distorted, 320, 240, 1.0, Some(&frame), 0.62, None);
        let peak = |buffer: &[u32], y: usize| {
            (210..290)
                .max_by_key(|x| (buffer[y * 320 + x] >> 16) & 255)
                .unwrap()
        };
        assert!((120..210).any(|y| peak(&clean, y).abs_diff(peak(&distorted, y)) >= 5));
    }

    #[test]
    fn separate_font_sizes_change_body_height_and_preserve_unavailable_preferences() {
        let mut renderer = Renderer::new().unwrap();
        renderer.text = "A message that wraps across more than one line.".into();
        let body = FontPreference::new("Unavailable body family", 32);
        let title = FontPreference::new("Unavailable title family", 8);
        renderer.set_preferences(&body, &title);
        let large_height = renderer.message_height(1.0);
        assert_eq!(renderer.body_preference(), &body);
        assert_eq!(renderer.title_preference(), &title);
        renderer.set_preferences(&FontPreference::new("Unavailable body family", 8), &FontPreference::new("Unavailable title family", 32));
        assert!(renderer.message_height(1.0) < large_height);
        assert_eq!(renderer.body_preference().family, "Unavailable body family");
        assert_eq!(renderer.title_preference().family, "Unavailable title family");
    }

    #[test]
    fn title_font_size_changes_title_rendering_without_changing_message_height() {
        let mut renderer = Renderer::new().unwrap();
        renderer.text = "Message".into();
        renderer.title = "A title".into();
        let mut small = vec![0; 320 * 240];
        renderer.set_preferences(&FontPreference::new("Unavailable body family", 18), &FontPreference::new("Unavailable title family", 8));
        let height = renderer.message_height(1.0);
        renderer.draw(&mut small, 320, 240, 1.0, None, 0.0, None);
        let mut large = vec![0; 320 * 240];
        renderer.set_preferences(&FontPreference::new("Unavailable body family", 18), &FontPreference::new("Unavailable title family", 32));
        assert_eq!(renderer.message_height(1.0), height);
        renderer.seed = 567891;
        renderer.draw(&mut large, 320, 240, 1.0, None, 0.0, None);
        assert_eq!(&small[..320 * 52], &large[..320 * 52]);
        assert_ne!(&small[320 * 52..320 * 92], &large[320 * 52..320 * 92]);
    }
}
