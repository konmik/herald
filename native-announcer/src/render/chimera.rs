use super::*;
use tiny_skia::{Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Paint, Path, PathBuilder, Pixmap, Point, Shader, SpreadMode, Stroke, Transform};

pub(super) struct Frame {
    width: usize,
    height: usize,
    scale: u32,
    body_glyphs: std::sync::Arc<Vec<CachedGlyph>>,
    pixels: Vec<u32>,
    coverage: Vec<bool>,
}

impl Frame {
    pub(super) fn matches(&self, width: usize, height: usize, scale: f32, body_glyphs: &std::sync::Arc<Vec<CachedGlyph>>) -> bool {
        self.width == width && self.height == height && self.scale == scale.to_bits() && std::sync::Arc::ptr_eq(&self.body_glyphs, body_glyphs)
    }

    fn new(width: usize, height: usize, scale: f32, body_glyphs: std::sync::Arc<Vec<CachedGlyph>>, body_y: f32) -> Self {
        let mut raster = Pixmap::new(width as u32, height as u32).unwrap();
        let text_bottom = body_glyphs.iter().filter(|glyph| glyph.width > 0 && glyph.height > 0).filter_map(|glyph| {
            glyph.pixels.iter().rposition(|alpha| *alpha > 0)
                .map(|last| (glyph.y as i32 + (last / glyph.width) as i32 + 1) as f32 / scale)
        }).fold(body_y, f32::max);
        let original_bottom = 306.0 + (height as f32 / scale - 432.0).max(0.0);
        let extra = (original_bottom + text_bottom) * 0.5 - 306.0;
        let transform = Transform::from_scale(scale, scale);
        let skin = gradient([30.0, 40.0], [415.0, 290.0 + extra], &[(0.0, 0x0b1c29, 0.96), (1.0, 0x06121a, 0.97)]);
        let edge = gradient([16.0, 74.0], [429.0, 291.0 + extra], &[(0.0, 0x8eb9ef, 0.5), (0.45, 0x96daff, 0.9), (1.0, 0x72c6dc, 0.3)]);
        let mouth = gradient([350.0, 280.0 + extra], [383.0, 278.0 + extra], &[(0.0, 0x82c5df, 0.24), (0.7, 0xbf77b9, 0.65), (1.0, 0x8ad4ae, 0.65)]);
        let outer = path(OUTER, extra);
        raster.fill_path(&outer, &Paint { shader: skin, ..Paint::default() }, FillRule::Winding, transform, None);
        stroke(&mut raster, &outer, edge, 1.1, transform);
        for (commands, color, alpha, width) in [
            (INNER, 0x9bc8e7, 0.38, 0.7), (INNERMOST, 0x8dcae3, 0.2, 0.65),
            (LOWER, 0x9bd2ff, 0.43, 0.7), (LOWER_INNER, 0x7db7e3, 0.2, 0.6),
            (RIBS, 0x96c8f2, 0.27, 0.65), (TOP_LIGHT, 0xc5e6ff, 0.6, 1.0),
            (BOTTOM_LIGHT, 0xb3d9fd, 0.55, 0.8),
        ] {
            stroke(&mut raster, &path(commands, extra), Shader::SolidColor(color_value(color, alpha)), width, transform);
        }
        stroke(&mut raster, &path(MOUTH, extra), mouth, 1.0, transform);
        let mut pixels = Vec::with_capacity(width * height);
        let mut coverage = Vec::with_capacity(width * height);
        for pixel in raster.pixels() {
            let covered = pixel.alpha() > 0;
            pixels.push(u32::from(pixel.alpha()) << 24 | u32::from(pixel.red()) << 16 | u32::from(pixel.green()) << 8 | u32::from(pixel.blue()));
            coverage.push(covered);
        }
        Self { width, height, scale: scale.to_bits(), body_glyphs, pixels, coverage }
    }
}

fn color_value(color: u32, alpha: f32) -> Color {
    Color::from_rgba8((color >> 16) as u8, (color >> 8) as u8, color as u8, (alpha * 255.0) as u8)
}

fn gradient(from: [f32; 2], to: [f32; 2], stops: &[(f32, u32, f32)]) -> Shader<'static> {
    LinearGradient::new(Point::from_xy(from[0] + 10.0, from[1]), Point::from_xy(to[0] + 10.0, to[1]),
        stops.iter().map(|&(position, color, alpha)| GradientStop::new(position, color_value(color, alpha))).collect(),
        SpreadMode::Pad, Transform::identity()).unwrap()
}

fn stroke(raster: &mut Pixmap, path: &Path, shader: Shader<'_>, width: f32, transform: Transform) {
    raster.stroke_path(path, &Paint { shader, ..Paint::default() },
        &Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() }, transform, None);
}

enum Command { Move(f32, f32), Line(f32, f32), Cubic([f32; 6]), Close }
use Command::{Close, Cubic, Line, Move};

fn path(commands: &[Command], extra: f32) -> Path {
    let mut builder = PathBuilder::new();
    let y = |value: f32| if value <= 150.0 { value }
        else if extra < 0.0 { 150.0 + (value - 150.0) * (156.0 + extra) / 156.0 }
        else { value + extra };
    for command in commands {
        match *command {
            Move(x, top) => builder.move_to(x + 10.0, y(top)),
            Line(x, top) => builder.line_to(x + 10.0, y(top)),
            Cubic([x1, y1, x2, y2, x3, y3]) => builder.cubic_to(x1 + 10.0, y(y1), x2 + 10.0, y(y2), x3 + 10.0, y(y3)),
            Close => builder.close(),
        }
    }
    builder.finish().unwrap()
}

impl Renderer {
    pub(super) fn draw_chimera(&mut self, buffer: &mut [u32], width: usize, height: usize, scale: f32, image: Option<&RgbaImage>, entrance: Option<Duration>) {
        let layout = self.style.layout((height as f32 / scale).round() as u32, self.title_preference.size);
        let body = TextBlock {
            text: &self.text, x: layout.body[0], y: layout.body[1], width: layout.body[2], max_height: layout.body[3],
            size: f32::from(self.body_preference.size), color: 0xecf6fc, line_height: Some(f32::from(self.body_preference.size) * 1.65),
        };
        let body_glyphs = self.glyphs(&self.body_font, &body, scale);
        let strike = entrance.filter(|elapsed| !elapsed.is_zero() && *elapsed < self.lightning.transition_duration());
        let active = match self.lightning_activity {
            Some(LightningActivity::Holding(elapsed, duration, seed)) => ambient_burst(elapsed, duration, seed, self.lightning).is_some(),
            Some(LightningActivity::Closing(_, _)) => true,
            None => false,
        };
        profile::time(Stage::Bubble, || {
            if !self.chimera_template.as_ref().is_some_and(|frame| frame.matches(width, height, scale, &body_glyphs)) {
                self.chimera_template = Some(Frame::new(width, height, scale, body_glyphs, body.y));
            }
            buffer.copy_from_slice(&self.chimera_template.as_ref().unwrap().pixels);
        });
        let mut coverage = (strike.is_some() || active).then(|| PaintCoverage::new(self.chimera_template.as_ref().unwrap().coverage.clone(), PixelFormat::PremultipliedArgb));
        let mut title = self.title.clone();
        while title.chars().map(|c| self.title_font.metrics(c, f32::from(self.title_preference.size)).advance_width).sum::<f32>() > layout.title[2] { title.pop(); }
        if title != self.title {
            title = title.chars().take(title.chars().count().saturating_sub(3)).collect::<String>() + "...";
        }
        profile::time(Stage::Text, || {
            self.text(buffer, width, height, scale, &self.title_font, TextBlock {
                text: &title, x: layout.title[0], y: layout.title[1], width: layout.title[2], max_height: layout.title[3],
                size: f32::from(self.title_preference.size), color: 0x9cceeb, line_height: None,
            });
            self.text(buffer, width, height, scale, &self.body_font, body);
        });
        profile::time(Stage::Portrait, || {
            if let Some(image) = image {
                let size = (layout.portrait[2] * scale).round() as u32;
                let premultiplied = RgbaImage::from_fn(image.width(), image.height(), |x, y| {
                    let [red, green, blue, alpha] = image.get_pixel(x, y).0;
                    image::Rgba([(u16::from(red) * u16::from(alpha) / 255) as u8,
                        (u16::from(green) * u16::from(alpha) / 255) as u8,
                        (u16::from(blue) * u16::from(alpha) / 255) as u8, alpha])
                });
                let frame = image::imageops::resize(&premultiplied, size, size, image::imageops::FilterType::Triangle);
                let left = (layout.portrait[0] * scale).round() as usize;
                let top = (layout.portrait[1] * scale).round() as usize;
                for (y, row) in frame.rows().enumerate().take(height.saturating_sub(top)) {
                    let mut span = None;
                    for (x, pixel) in row.enumerate().take(width.saturating_sub(left)) {
                        let [red, green, blue, alpha] = pixel.0;
                        if alpha == 0 { continue; }
                        let color = u32::from(alpha) << 24 | u32::from(red) << 16 | u32::from(green) << 8 | u32::from(blue);
                        let index = (top + y) * width + left + x;
                        buffer[index] = crate::pixels::over(buffer[index], color);
                        let span = span.get_or_insert((x, x));
                        span.1 = x;
                    }
                    if let (Some(coverage), Some((start, end))) = (&mut coverage, span) {
                        coverage.pixels[(top + y) * width + left + start..=(top + y) * width + left + end].fill(true);
                    }
                }
            }
        });
        if let (Some(coverage), Some(_)) = (&mut coverage, image) {
            coverage.wires = self.anatomy_paths.iter().map(|path| path.iter().map(|point| [
                (layout.portrait[0] + point[0] * layout.portrait[2]) * scale,
                (layout.portrait[1] + point[1] * layout.portrait[3]) * scale,
            ]).collect()).collect();
        }
        if let Some(coverage) = coverage {
            self.draw_lightning(buffer, width, height, scale, strike, coverage, layout);
        }
    }
}

const OUTER: &[Command] = &[
    Move(25.0, 91.0),
    Cubic([30.0, 49.0, 64.0, 27.0, 111.0, 27.0]),
    Line(299.0, 27.0),
    Cubic([350.0, 27.0, 382.0, 51.0, 394.0, 93.0]),
    Cubic([402.0, 124.0, 397.0, 162.0, 386.0, 191.0]),
    Cubic([377.0, 217.0, 371.0, 237.0, 372.0, 255.0]),
    Cubic([372.0, 265.0, 378.0, 271.0, 383.0, 274.0]),
    Cubic([381.0, 293.0, 369.0, 304.0, 342.0, 306.0]),
    Line(107.0, 306.0),
    Cubic([58.0, 306.0, 28.0, 282.0, 23.0, 244.0]),
    Cubic([17.0, 201.0, 15.0, 132.0, 25.0, 91.0]),
    Close,
];

const INNER: &[Command] = &[
    Move(33.0, 94.0),
    Cubic([41.0, 57.0, 69.0, 38.0, 112.0, 38.0]),
    Line(294.0, 38.0),
    Cubic([339.0, 38.0, 369.0, 59.0, 380.0, 96.0]),
    Cubic([389.0, 124.0, 383.0, 160.0, 374.0, 186.0]),
    Cubic([363.0, 220.0, 359.0, 246.0, 368.0, 269.0]),
    Cubic([374.0, 283.0, 380.0, 285.0, 383.0, 282.0]),
];

const INNERMOST: &[Command] = &[
    Move(39.0, 106.0),
    Cubic([43.0, 72.0, 71.0, 49.0, 115.0, 49.0]),
    Line(289.0, 49.0),
    Cubic([329.0, 49.0, 358.0, 66.0, 368.0, 101.0]),
    Cubic([378.0, 134.0, 367.0, 162.0, 361.0, 190.0]),
    Cubic([353.0, 223.0, 347.0, 249.0, 355.0, 278.0]),
    Cubic([361.0, 290.0, 368.0, 293.0, 372.0, 291.0]),
];

const LOWER: &[Command] = &[
    Move(22.0, 170.0),
    Cubic([24.0, 225.0, 29.0, 268.0, 58.0, 285.0]),
    Cubic([72.0, 294.0, 90.0, 298.0, 112.0, 298.0]),
    Line(319.0, 298.0),
    Cubic([340.0, 299.0, 354.0, 298.0, 370.0, 287.0]),
];

const LOWER_INNER: &[Command] = &[
    Move(31.0, 187.0),
    Cubic([33.0, 238.0, 40.0, 267.0, 64.0, 281.0]),
    Cubic([79.0, 290.0, 94.0, 289.0, 114.0, 289.0]),
    Line(307.0, 289.0),
    Cubic([327.0, 289.0, 343.0, 286.0, 352.0, 277.0]),
];

const RIBS: &[Command] = &[
    Move(33.0, 66.0),
    Line(50.0, 71.0),
    Line(44.0, 91.0),
    Line(23.0, 111.0),
    Move(64.0, 36.0),
    Line(70.0, 52.0),
    Line(92.0, 40.0),
    Line(105.0, 27.0),
    Move(110.0, 27.0),
    Line(123.0, 38.0),
    Line(143.0, 27.0),
    Move(153.0, 27.0),
    Line(171.0, 38.0),
    Line(191.0, 27.0),
    Move(204.0, 27.0),
    Line(221.0, 38.0),
    Line(240.0, 27.0),
    Move(253.0, 27.0),
    Line(271.0, 38.0),
    Line(292.0, 27.0),
    Move(319.0, 29.0),
    Line(324.0, 43.0),
    Line(345.0, 40.0),
    Line(345.0, 56.0),
    Line(368.0, 59.0),
    Move(380.0, 69.0),
    Line(370.0, 75.0),
    Line(385.0, 95.0),
    Line(394.0, 94.0),
    Move(394.0, 117.0),
    Line(385.0, 125.0),
    Line(394.0, 147.0),
    Line(380.0, 150.0),
    Line(386.0, 171.0),
    Line(373.0, 179.0),
    Move(377.0, 215.0),
    Line(365.0, 214.0),
    Line(361.0, 239.0),
    Line(372.0, 240.0),
    Move(372.0, 255.0),
    Line(362.0, 258.0),
    Line(372.0, 272.0),
    Line(378.0, 267.0),
    Move(27.0, 259.0),
    Line(40.0, 265.0),
    Line(43.0, 280.0),
    Line(59.0, 282.0),
    Line(67.0, 296.0),
    Move(87.0, 304.0),
    Line(97.0, 298.0),
    Line(115.0, 306.0),
    Move(137.0, 306.0),
    Line(155.0, 298.0),
    Line(174.0, 306.0),
    Move(197.0, 306.0),
    Line(218.0, 298.0),
    Line(236.0, 306.0),
    Move(261.0, 306.0),
    Line(282.0, 298.0),
    Line(302.0, 306.0),
    Move(314.0, 306.0),
    Line(331.0, 297.0),
    Line(342.0, 306.0),
];

const MOUTH: &[Command] = &[
    Move(350.0, 262.0),
    Cubic([359.0, 285.0, 370.0, 290.0, 383.0, 274.0]),
    Move(356.0, 279.0),
    Cubic([365.0, 294.0, 375.0, 292.0, 380.0, 279.0]),
];

const TOP_LIGHT: &[Command] = &[
    Move(38.0, 101.0),
    Cubic([45.0, 58.0, 82.0, 39.0, 115.0, 39.0]),
];

const BOTTOM_LIGHT: &[Command] = &[
    Move(60.0, 287.0),
    Cubic([76.0, 298.0, 96.0, 298.0, 120.0, 298.0]),
];
