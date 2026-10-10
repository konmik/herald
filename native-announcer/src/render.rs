use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use fontdue::Font;
use image::RgbaImage;
use std::time::Duration;
use crate::settings::{FontPreference, Settings};
use crate::lightning::LightningSettings;
use crate::profile::{self, Stage};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PhysicalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl PhysicalRect {
    pub fn right(self) -> i32 { self.x + self.width as i32 }
    pub fn bottom(self) -> i32 { self.y + self.height as i32 }
    pub fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct CardPlacement {
    pub rect: PhysicalRect,
    pub scale: f32,
}

pub struct EntranceScene {
    pub monitor: PhysicalRect,
    pub card: CardPlacement,
    pub canvas: PhysicalRect,
    pub source: [f32; 2],
    pub impact: [f32; 2],
    channel: Vec<[f32; 2]>,
    forks: Vec<(usize, Vec<[f32; 2]>, f32)>,
    lightning: LightningSettings,
}

#[derive(Clone, Copy)]
pub enum LightningActivity {
    Holding(Duration, Duration, u32),
    Closing(Duration, u32),
}

fn ambient_burst(elapsed: Duration, duration: Duration, seed: u32) -> Option<(u32, f32, f32)> {
    let time = elapsed.saturating_sub(crate::state::TRANSITION_DURATION).as_secs_f32();
    let duration = duration.saturating_sub(crate::state::TRANSITION_DURATION).as_secs_f32().max(1.0);
    let mut start = 0.1 + (crate::state::noise_hash(seed) % 500) as f32 / 1000.0;
    let mut event = 0_u32;
    while start <= time {
        let random = crate::state::noise_hash(seed ^ event.wrapping_mul(7919));
        let buildup = (start / duration).min(1.0);
        let length = 0.09 + ((random >> 8) % 140) as f32 / 1000.0 + buildup * 0.05;
        if time < start + length {
            let age = (time - start) / length;
            return Some((random, (std::f32::consts::PI * age).sin(), buildup));
        }
        let gap = if random % 4 == 0 { 0.04 + ((random >> 16) % 110) as f32 / 1000.0 }
            else { 0.22 + ((random >> 16) % 900) as f32 / 1000.0 };
        start += length + gap * (1.0 - 0.55 * buildup);
        event = event.wrapping_add(1);
    }
    None
}

/// True while a holding-phase ambient burst is drawn, the frames that pay for Lightning while the card is held.
pub fn ambient_active(elapsed: Duration, duration: Duration, seed: u32) -> bool { ambient_burst(elapsed, duration, seed).is_some() }

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnouncementPhase { Leader, Impact, Propagation, Decay, Holding, Exit }

pub fn announcement_phase(elapsed: Duration, closing: bool) -> AnnouncementPhase {
    if closing { return AnnouncementPhase::Exit; }
    match elapsed.as_millis() {
        0..=119 => AnnouncementPhase::Leader,
        120..=179 => AnnouncementPhase::Impact,
        180..=449 => AnnouncementPhase::Propagation,
        450..=649 => AnnouncementPhase::Decay,
        _ => AnnouncementPhase::Holding,
    }
}

fn irregular_channel(from: [f32; 2], to: [f32; 2], seed: u32, roughness: f32, step: f32, points: &mut Vec<[f32; 2]>) {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let length = dx.hypot(dy);
    if length <= step {
        points.push(to);
        return;
    }
    let random = crate::state::noise_hash(seed);
    let fraction = 0.38 + (random & 255) as f32 / 255.0 * 0.24;
    let offset = (((random >> 8) & 65535) as f32 / 65535.0 * 2.0 - 1.0) * roughness;
    let middle = [from[0] + dx * fraction - dy / length * offset, from[1] + dy * fraction + dx / length * offset];
    irregular_channel(from, middle, random.wrapping_add(1), roughness * 0.57, step, points);
    irregular_channel(middle, to, random.wrapping_add(2), roughness * 0.57, step, points);
}

impl EntranceScene {
    pub fn new(monitor: PhysicalRect, card: CardPlacement, seed: u32, lightning: LightningSettings) -> Self {
        let scale = card.scale;
        let left = card.rect.x as f32;
        let bottom = card.rect.bottom() as f32;
        let from_right = seed & 1 != 0;
        let impact = if from_right { [left + 315.5 * scale, bottom - 70.0 * scale] }
            else { [left + 252.0 * scale, bottom - 12.5 * scale] };
        let source = if from_right {
            [monitor.right() as f32 - 1.0, (bottom + (40 + seed % 101) as f32 * scale).min(monitor.bottom() as f32 - 1.0)]
        } else {
            [(left + (80 + seed % 161) as f32 * scale).clamp(monitor.x as f32, monitor.right() as f32 - 1.0), monitor.bottom() as f32 - 1.0]
        };
        let route = [source, if from_right { [impact[0] + 18.0 * scale, impact[1] + 35.0 * scale] }
            else { [impact[0] - 30.0 * scale, impact[1] + 30.0 * scale] }, impact];
        let mut channel = vec![source];
        for (index, pair) in route.windows(2).enumerate() {
            let length = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
            irregular_channel(pair[0], pair[1], seed ^ index as u32 * 7919, (length * 0.08).min(18.0 * scale) * lightning.roughness, 6.0 * scale, &mut channel);
        }
        let guard = |point: &mut [f32; 2]| {
            point[0] = point[0].clamp(monitor.x as f32, monitor.right() as f32 - 1.0);
            point[1] = point[1].clamp(monitor.y as f32, monitor.bottom() as f32 - 1.0);
            if from_right { point[0] = point[0].max(impact[0]); }
            else { point[1] = point[1].max(impact[1]); }
        };
        for point in channel.iter_mut().skip(1) { guard(point); }
        let guard_branch = |point: &mut [f32; 2]| {
            guard(point);
            if from_right { point[0] = point[0].max(impact[0] + 5.0 * scale).min(monitor.right() as f32 - 1.0); }
            else { point[1] = point[1].max(impact[1] + 5.0 * scale).min(monitor.bottom() as f32 - 1.0); }
        };
        let mut forks = Vec::new();
        let frequency = lightning.entrance_fork_spacing;
        for index in 2..channel.len().saturating_sub(10) {
            let random = crate::state::noise_hash(seed ^ (index as u32).wrapping_mul(3571));
            if random % frequency > 1 { continue; }
            let from = channel[index];
            let before = channel[index - 1];
            let dx = from[0] - before[0];
            let dy = from[1] - before[1];
            let length = dx.hypot(dy).max(0.1);
            let side = if random & 128 == 0 { -1.0 } else { 1.0 };
            let reach = (15.0 + ((random >> 8) % 76) as f32) * scale;
            let along = 0.25 + ((random >> 20) % 70) as f32 / 100.0;
            let across = (1.0 - along * along).sqrt() * side;
            let mut tip = [from[0] + (dx * along - dy * across) / length * reach,
                from[1] + (dy * along + dx * across) / length * reach];
            guard_branch(&mut tip);
            let mut branch = vec![from];
            irregular_channel(from, tip, random, reach * 0.13, 4.0 * scale, &mut branch);
            for point in &mut branch { guard_branch(point); }
            let taper = 0.35 + ((random >> 16) % 30) as f32 / 100.0;
            if random & 3 == 0 && branch.len() > 4 {
                let junction = branch[branch.len() / 3];
                let mut tip = [junction[0] - dy / length * side * reach * 0.3,
                    junction[1] + dx / length * side * reach * 0.3];
                guard_branch(&mut tip);
                let mut child = vec![junction];
                irregular_channel(junction, tip, random ^ 9137, reach * 0.07, 3.0 * scale, &mut child);
                for point in &mut child { guard_branch(point); }
                forks.push((index, child, taper * 0.45));
            }
            forks.push((index, branch, taper));
        }
        let padding = (12.0 * scale).ceil() as i32;
        let mut bounds = [card.rect.x, card.rect.y, card.rect.right(), card.rect.bottom()];
        for point in channel.iter().chain(forks.iter().flat_map(|(_, points, _)| points)) {
            bounds[0] = bounds[0].min(point[0].floor() as i32 - padding);
            bounds[1] = bounds[1].min(point[1].floor() as i32 - padding);
            bounds[2] = bounds[2].max(point[0].ceil() as i32 + padding);
            bounds[3] = bounds[3].max(point[1].ceil() as i32 + padding);
        }
        bounds[0] = bounds[0].max(monitor.x);
        bounds[1] = bounds[1].max(monitor.y);
        bounds[2] = bounds[2].min(monitor.right());
        bounds[3] = bounds[3].min(monitor.bottom());
        let canvas = PhysicalRect { x: bounds[0], y: bounds[1], width: (bounds[2] - bounds[0]) as u32, height: (bounds[3] - bounds[1]) as u32 };
        Self { monitor, card, canvas, source, impact, channel, forks, lightning }
    }

    fn draw_incoming_bolt(&self, buffer: &mut [u32], elapsed: Duration) {
        let milliseconds = elapsed.as_secs_f32() * 1000.0;
        if milliseconds <= 0.0 || milliseconds >= 650.0 { return; }
        let mut paint = LightningPaint::new(self.canvas.width as usize, self.canvas.height as usize, self.lightning);
        let reveal = (milliseconds / 120.0).min(1.0);
        let lengths: Vec<f32> = self.channel.windows(2).map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1])).collect();
        let reach = lengths.iter().sum::<f32>() * reveal;
        let local = |point: [f32; 2]| [point[0] - self.canvas.x as f32, point[1] - self.canvas.y as f32];
        let intensity = if milliseconds < 120.0 { 0.65 } else { self.lightning.pulse_profile.entrance(milliseconds) * ((370.0 - milliseconds) / 190.0).clamp(0.0, 1.0) };
        let mut distance = 0.0;
        for (index, pair) in self.channel.windows(2).enumerate() {
            if distance >= reach { break; }
            let fraction = ((reach - distance) / lengths[index]).min(1.0);
            let to = [pair[0][0] + (pair[1][0] - pair[0][0]) * fraction, pair[0][1] + (pair[1][1] - pair[0][1]) * fraction];
            paint.stroke(local(pair[0]), local(to), self.card.scale, 0.9, intensity);
            for (_, points, taper) in self.forks.iter().filter(|(parent, _, _)| *parent == index) {
                for (branch_index, branch) in points.windows(2).enumerate() {
                    let width = taper * (1.0 - branch_index as f32 / points.len() as f32 * 0.8);
                    paint.stroke(local(branch[0]), local(branch[1]), self.card.scale, width, intensity * 0.7);
                }
            }
            distance += lengths[index];
        }
        paint.composite(buffer);
    }
}

#[derive(Clone, Copy)]
struct Stroke {
    from: [f32; 2],
    to: [f32; 2],
    scale: f32,
    taper: f32,
    intensity: f32,
}

/// Rows per parallel raster band; each band applies every stroke clipped to its rows.
const RASTER_BAND_ROWS: usize = 16;

struct LightningPaint {
    width: usize,
    height: usize,
    glow: Vec<f32>,
    lightning: LightningSettings,
    strokes: Vec<Stroke>,
}

impl LightningPaint {
    fn new(width: usize, height: usize, lightning: LightningSettings) -> Self {
        Self { width, height, glow: vec![0.0; width * height], lightning, strokes: Vec::new() }
    }

    /// Applies the queued strokes. A pixel keeps the maximum strength of all strokes, which does not depend on order,
    /// so bands of rows are rasterized in parallel with the same per-pixel arithmetic as one sequential pass.
    fn rasterize(&mut self) {
        if self.strokes.is_empty() { return; }
        use rayon::prelude::*;
        let lightning = self.lightning;
        let bounds = StrengthBounds::new(lightning, &self.strokes);
        let strokes: Vec<(Stroke, StrokeReach)> = std::mem::take(&mut self.strokes).into_iter()
            .filter_map(|stroke| stroke_reach(lightning, &bounds, stroke).map(|reach| (stroke, reach))).collect();
        let (width, height, lightning) = (self.width, self.height, self.lightning);
        self.glow.par_chunks_mut(RASTER_BAND_ROWS * width).enumerate().for_each(|(band, glow)| {
            let top = band * RASTER_BAND_ROWS;
            let rows = top..(top + RASTER_BAND_ROWS).min(height);
            for (stroke, reach) in &strokes { raster_stroke(glow, width, height, rows.clone(), lightning, &bounds, *stroke, reach); }
        });
    }

    fn bolt(&mut self, from: [f32; 2], to: [f32; 2], scale: f32, seed: u32, intensity: f32, reveal: f32) {
        let length = (to[0] - from[0]).hypot(to[1] - from[1]);
        let mut points = vec![from];
        profile::time(Stage::Geometry, || irregular_channel(from, to, seed, length * 0.16 * self.lightning.roughness, 3.0 * scale, &mut points));
        let visible = ((points.len() - 1) as f32 * reveal.clamp(0.0, 1.0)).ceil() as usize;
        let frequency = self.lightning.bolt_fork_spacing;
        for (index, pair) in points.windows(2).take(visible).enumerate() {
            let taper = 0.7 - index as f32 / points.len() as f32 * 0.3;
            self.stroke(pair[0], pair[1], scale, taper, intensity);
            let random = crate::state::noise_hash(seed ^ index as u32 * 3571);
            if index < 2 || random % frequency != 0 { continue; }
            let fraction = 0.12 + (random % 100) as f32 / 700.0;
            let dx = to[0] - pair[0][0];
            let dy = to[1] - pair[0][1];
            let side = if random & 128 == 0 { -0.7 } else { 0.7 };
            let end = [pair[0][0] + (dx - dy * side) * fraction, pair[0][1] + (dy + dx * side) * fraction];
            let mut branch = vec![pair[0]];
            profile::time(Stage::Geometry, || irregular_channel(pair[0], end, random, length * 0.07, 2.5 * scale, &mut branch));
            for (index, segment) in branch.windows(2).enumerate() {
                self.stroke(segment[0], segment[1], scale, 0.38 * (1.0 - index as f32 / branch.len() as f32 * 0.8), intensity * 0.75);
            }
        }
    }

    fn stroke(&mut self, from: [f32; 2], to: [f32; 2], scale: f32, taper: f32, intensity: f32) {
        if intensity <= 0.0 { return; }
        self.strokes.push(Stroke { from, to, scale, taper, intensity });
    }

    /// Two-pass chamfer distance. The neighbour rows are folded in first as a branch-free pass the compiler vectorizes, then
    /// the in-row neighbour runs as the only serial chain. min is exact, so the result matches the textbook pass order bit for bit.
    fn distance_field(&self) -> Vec<f32> {
        let (width, height) = (self.width, self.height);
        let mut distance: Vec<f32> = self.glow.iter().map(|strength| if *strength > 0.25 { 0.0 } else { 10000.0 }).collect();
        if width == 0 || height == 0 { return distance; }
        for y in 0..height {
            let (done, rest) = distance.split_at_mut(y * width);
            let row = &mut rest[..width];
            if y > 0 { fold_neighbour_row(row, &done[(y - 1) * width..]); }
            for x in 1..width { row[x] = row[x].min(row[x - 1] + 1.0); }
        }
        for y in (0..height).rev() {
            let (head, done) = distance.split_at_mut((y + 1) * width);
            let row = &mut head[y * width..];
            if y + 1 < height { fold_neighbour_row(row, &done[..width]); }
            for x in (0..width - 1).rev() { row[x] = row[x].min(row[x + 1] + 1.0); }
        }
        distance
    }

    fn composite(mut self, buffer: &mut [u32]) {
        self.rasterize();
        let LightningSettings { halo_color: halo, core_color: core, .. } = self.lightning;
        use rayon::prelude::*;
        let band = RASTER_BAND_ROWS * self.width.max(1);
        profile::time(Stage::Composite, || buffer.par_chunks_mut(band).zip(self.glow.par_chunks(band)).for_each(|(pixels, strengths)| {
            for (pixel, &strength) in pixels.iter_mut().zip(strengths) {
                if strength == 0.0 { continue; }
                let background = if *pixel == 0xff00ff { 0x080e20 } else { *pixel };
                let color = blend(halo, core, (strength.min(1.0).powi(3) * 255.0) as u32);
                *pixel = blend(background, color, (strength.min(1.0) * 255.0) as u32);
            }
        }));
    }
}

fn fold_neighbour_row(row: &mut [f32], neighbour: &[f32]) {
    let width = row.len();
    for x in 0..width { row[x] = row[x].min(neighbour[x] + 1.0); }
    for x in 1..width { row[x] = row[x].min(neighbour[x - 1] + 1.414); }
    for x in 0..width - 1 { row[x] = row[x].min(neighbour[x + 1] + 1.414); }
}

/// The renderer's xorshift32 state after `steps` draws, in O(log steps): the step is linear over GF(2), so it is a
/// 32x32 bit matrix and powers of two of it are precomputed once.
fn xorshift_jump(mut state: u32, mut steps: usize) -> u32 {
    static POWERS: std::sync::OnceLock<Vec<[u32; 32]>> = std::sync::OnceLock::new();
    fn apply(matrix: &[u32; 32], value: u32) -> u32 {
        (0..32).filter(|bit| value >> bit & 1 == 1).fold(0, |sum, bit| sum ^ matrix[bit])
    }
    let powers = POWERS.get_or_init(|| {
        let step = |mut value: u32| { value ^= value << 13; value ^= value >> 17; value ^= value << 5; value };
        let mut powers = vec![std::array::from_fn(|bit| step(1 << bit))];
        for _ in 1..usize::BITS {
            let last = *powers.last().unwrap();
            powers.push(std::array::from_fn(|bit| apply(&last, last[bit])));
        }
        powers
    });
    let mut power = 0;
    while steps > 0 {
        if steps & 1 == 1 { state = apply(&powers[power], state); }
        steps >>= 1;
        power += 1;
    }
    state
}

/// Per-pixel constants of the curved portrait screen: where it samples the video and its scanline times vignette shade.
#[derive(Clone, Copy)]
struct PortraitTexel { warped: [f32; 2], shade: f32, inside: bool }

/// Same arithmetic as evaluating the curvature per pixel each frame, done once per portrait size.
fn portrait_warp(face_size: usize) -> Vec<PortraitTexel> {
    let mut texels = Vec::with_capacity(face_size * face_size);
    for y in 0..face_size {
        let source_y = y * 128 / face_size;
        for x in 0..face_size {
            let horizontal = (x as f32 / face_size as f32 - 0.5) * 2.0;
            let vertical = (y as f32 / face_size as f32 - 0.5) * 2.0;
            let radius = horizontal * horizontal + vertical * vertical;
            let curvature = 1.0 + radius * 0.065;
            let warped_x = (horizontal * curvature + 1.0) * 63.5;
            let warped_y = (vertical * curvature + 1.0) * 63.5;
            let inside = (0.0..128.0).contains(&warped_x) && (0.0..128.0).contains(&warped_y);
            let scanline = if source_y.is_multiple_of(2) { 0.82 } else { 1.0 };
            let vignette = 1.0 - radius * 0.13;
            texels.push(PortraitTexel { warped: [warped_x, warped_y], shade: scanline * vignette, inside });
        }
    }
    texels
}

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
    pub lightning_activity: Option<LightningActivity>,
    impact: Option<[f32; 2]>,
    lightning: LightningSettings,
    contour_cache: Option<(Vec<bool>, Contours)>,
    glyph_cache: std::cell::RefCell<std::collections::HashMap<TextKey, std::sync::Arc<Vec<CachedGlyph>>>>,
    template: Option<CardTemplate>,
    portrait_warp: Option<(usize, Vec<PortraitTexel>)>,
}

/// Identifies one laid-out text block; its glyph rasters do not change between frames.
#[derive(Clone, PartialEq, Eq, Hash)]
struct TextKey {
    font: usize,
    text: String,
    size: u32,
    y: u32,
    max_height: u32,
    scale: u32,
}

struct CachedGlyph {
    x: f32,
    y: f32,
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

/// Lays out and rasterizes a text block, keeping only the glyphs that fit above its maximum height.
fn layout_glyphs(font: &Font, block: &TextBlock<'_>, scale: f32) -> Vec<CachedGlyph> {
    let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
    layout.reset(&LayoutSettings {
        x: 26.0 * scale,
        y: block.y * scale,
        max_width: Some(268.0 * scale),
        ..LayoutSettings::default()
    });
    layout.append(&[font], &TextStyle::new(block.text, block.size * scale, 0));
    layout.glyphs().iter().filter(|glyph| glyph.y + glyph.height as f32 <= block.max_height * scale).map(|glyph| {
        let (_, pixels) = font.rasterize_config(glyph.key);
        CachedGlyph { x: glyph.x, y: glyph.y, width: glyph.width, height: glyph.height, pixels }
    }).collect()
}

/// The bubble's per-pixel paint, traced once per card size by running `draw_bubble` itself.
struct CardTemplate {
    width: usize,
    height: usize,
    scale: u32,
    bottom: u32,
    /// 0 untouched, 1 outline, 2 fill, 3 fill highlight.
    classes: Vec<u8>,
    /// Pixels darkened by the bubble scanlines.
    scanlines: Vec<u32>,
}

const TEMPLATE_PROBES: [u32; 2] = [0x123456, 0x654321];

impl CardTemplate {
    fn new(width: usize, height: usize, scale: f32, bottom: f32) -> Self {
        let probe = |fill: u32| {
            let mut buffer = vec![0xff00ff; width * height];
            draw_bubble(&mut buffer, width, height, scale, bottom, fill, None);
            buffer
        };
        let [first, second] = TEMPLATE_PROBES.map(probe);
        let classes = first.iter().zip(&second).map(|(a, b)| match (*a, *b) {
            (0xff00ff, 0xff00ff) => 0,
            (0x080908, 0x080908) => 1,
            (a, b) if a == TEMPLATE_PROBES[0] && b == TEMPLATE_PROBES[1] => 2,
            (a, b) if a == blend(TEMPLATE_PROBES[0], 0xffffff, 30) && b == blend(TEMPLATE_PROBES[1], 0xffffff, 30) => 3,
            other => panic!("Unclassified bubble pixel {other:x?}"),
        }).collect();
        let mut scanlines = Vec::new();
        for y in 0..height {
            if (y as f32 / scale).floor() as usize % 2 != 0 { continue; }
            for x in 0..width {
                if bubble_contains(x as f32 / scale, y as f32 / scale, bottom) { scanlines.push((y * width + x) as u32); }
            }
        }
        Self { width, height, scale: scale.to_bits(), bottom: bottom.to_bits(), classes, scanlines }
    }

    fn matches(&self, width: usize, height: usize, scale: f32, bottom: f32) -> bool {
        self.width == width && self.height == height && self.scale == scale.to_bits() && self.bottom == bottom.to_bits()
    }

    /// Same pixels and coverage as `draw_bubble` on a buffer filled with the transparency key.
    fn paint(&self, buffer: &mut [u32], fill: u32, coverage: Option<&mut PaintCoverage>) {
        let colors = [0xff00ff, 0x080908, fill, blend(fill, 0xffffff, 30)];
        for (pixel, class) in buffer.iter_mut().zip(&self.classes) {
            if *class != 0 { *pixel = colors[*class as usize]; }
        }
        if let Some(coverage) = coverage {
            for (covered, class) in coverage.pixels.iter_mut().zip(&self.classes) { *covered |= *class != 0; }
        }
    }

    /// Same pixels as `shade_bubble_scanlines`.
    fn shade(&self, buffer: &mut [u32]) {
        let darker: [u32; 256] = std::array::from_fn(|channel| ((channel as f32 * 0.82).clamp(0.0, 255.0)) as u32);
        for index in &self.scanlines {
            let color = buffer[*index as usize];
            buffer[*index as usize] = darker[(color & 255) as usize] | darker[((color >> 8) & 255) as usize] << 8 | darker[((color >> 16) & 255) as usize] << 16;
        }
    }
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
            lightning_activity: None,
            impact: None,
            lightning: settings.lightning,
            contour_cache: None,
            glyph_cache: Default::default(),
            template: None,
            portrait_warp: None,
        })
    }

    pub fn set_preferences(&mut self, body: &FontPreference, title: &FontPreference) {
        if &self.body_preference == body && &self.title_preference == title { return; }
        self.body_preference = body.clone();
        self.title_preference = title.clone();
        self.body_font = self.catalog.get(&body.family);
        self.title_font = self.catalog.get(&title.family);
    }

    pub fn set_lightning(&mut self, lightning: LightningSettings) { self.lightning = lightning; }

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

    pub fn announcement_height(&self, scale: f32, max_height: u32) -> u32 {
        (self.message_height(scale) + 238).min(max_height).max(240)
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
        let key = TextKey { font: font.file_hash(), text: block.text.to_owned(), size: (block.size * scale).to_bits(),
            y: (block.y * scale).to_bits(), max_height: (block.max_height * scale).to_bits(), scale: scale.to_bits() };
        let glyphs = self.glyph_cache.borrow().get(&key).cloned();
        let glyphs = glyphs.unwrap_or_else(|| {
            let glyphs = std::sync::Arc::new(layout_glyphs(font, &block, scale));
            let mut cache = self.glyph_cache.borrow_mut();
            if cache.len() >= 8 { cache.clear(); }
            cache.insert(key, glyphs.clone());
            glyphs
        });
        for glyph in glyphs.iter() {
            let pixels = &glyph.pixels;
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
        buffer.fill(0xff00ff);
        let logical_height = height as f32 / scale;
        let strike = entrance.filter(|elapsed| !elapsed.is_zero() && *elapsed < crate::state::TRANSITION_DURATION);
        let active = match self.lightning_activity {
            Some(LightningActivity::Holding(elapsed, duration, seed)) => ambient_burst(elapsed, duration, seed).is_some(),
            Some(LightningActivity::Closing(_, _)) => true,
            None => false,
        };
        let mut coverage = (strike.is_some() || active).then(|| PaintCoverage::new(vec![false; width * height]));
        let bottom = logical_height - 152.0;
        profile::time(Stage::Bubble, || {
            if !self.template.as_ref().is_some_and(|template| template.matches(width, height, scale, bottom)) {
                self.template = Some(CardTemplate::new(width, height, scale, bottom));
            }
            self.template.as_ref().unwrap().paint(buffer, video_background(image), coverage.as_mut());
        });
        let title_y = logical_height - 188.0;
        profile::time(Stage::Text, || self.text(
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
        ));
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
        profile::time(Stage::Text, || self.text(
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
        ));
        profile::time(Stage::Scanlines, || self.template.as_ref().unwrap().shade(buffer));
        let portrait = profile::enabled().then(std::time::Instant::now);
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
        let warp = match self.portrait_warp.take() {
            Some(warp) if warp.0 == face_size => warp,
            _ => (face_size, portrait_warp(face_size)),
        };
        // Pixels draw from the serial xorshift stream in row order. xorshift is linear, so each row's starting state is
        // the frame state jumped ahead by the draws of the rows above, and rows are then painted in parallel with
        // exactly the numbers the serial loop would have used.
        let columns = face_size.min(width.saturating_sub(face_x));
        let rows = face_size.min(height.saturating_sub(face_y));
        if let Some(coverage) = &mut coverage {
            for y in 0..rows { coverage.pixels[(face_y + y) * width + face_x..][..columns].fill(true); }
        }
        let draws = 1 + usize::from(interference > 0.0);
        let mut row_seeds = Vec::with_capacity(rows);
        for y in 0..rows {
            row_seeds.push(self.seed);
            let inside = warp.1[y * face_size..][..columns].iter().filter(|texel| texel.inside).count();
            self.seed = xorshift_jump(self.seed, inside * draws);
        }
        let texels = &warp.1;
        let paint_row = |y: usize, row: &mut [u32]| {
            let mut seed = row_seeds[y];
            let mut random = || { seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5; seed };
            let source_y = y * 128 / face_size;
            let row_offset = row_displacement[source_y];
            for x in 0..columns {
                let pixel_out = &mut row[face_x + x];
                *pixel_out = 0x0b0b09;
                let PortraitTexel { warped: [warped_x, warped_y], shade, inside } = texels[y * face_size + x];
                if !inside {
                    *pixel_out = 0x080908;
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
                    *pixel_out = blend(*pixel_out, color, pixel[3] as u32);
                }
                let background = *pixel_out;
                let luminance = (((background >> 16) & 255) * 77
                    + ((background >> 8) & 255) * 150
                    + (background & 255) * 29)
                    / 256;
                let faded = blend(background, luminance * 0x010101, 24);
                let brightness = shade * picture_flicker;
                *pixel_out = multiply_color(faded, brightness);
                let ambient_grain = random() & 255;
                *pixel_out = blend(*pixel_out, ambient_grain * 0x010101, 5);
                if interference > 0.0 {
                    let random = random();
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
                    let background = *pixel_out;
                    let luminance = (((background >> 16) & 255) * 77
                        + ((background >> 8) & 255) * 150
                        + (background & 255) * 29)
                        / 256;
                    let desaturated = blend(background, luminance * 0x010101, alpha / 2);
                    *pixel_out = blend(desaturated, monochrome, alpha);
                }
            }
        };
        if rows > 0 && columns > 0 {
            use rayon::prelude::*;
            buffer[face_y * width..(face_y + rows) * width].par_chunks_mut(width).enumerate().for_each(|(y, row)| paint_row(y, row));
        }
        self.portrait_warp = Some(warp);
        if let Some(start) = portrait { profile::add(Stage::Portrait, start); }
        if let Some(coverage) = coverage {
            if let Some((pixels, contours)) = &self.contour_cache {
                if *pixels == coverage.pixels { let _ = coverage.traced.set(contours.clone()); }
            }
            profile::time(Stage::Lightning, || if let Some(elapsed) = strike {
                coverage.draw_entrance_lightning(buffer, width, height, scale, elapsed, self.impact.unwrap_or([187.5 * scale, height as f32 - 90.5 * scale]), self.lightning);
            } else if let Some(activity) = self.lightning_activity {
                coverage.draw_activity(buffer, width, height, scale, activity, self.lightning);
            });
            if let Some(contours) = coverage.traced.get() {
                if !self.contour_cache.as_ref().is_some_and(|(_, cached)| std::sync::Arc::ptr_eq(cached, contours)) {
                    self.contour_cache = Some((coverage.pixels, contours.clone()));
                }
            }
        }
    }

    pub fn draw_scene(&mut self, scene: &EntranceScene, image: Option<&RgbaImage>, interference: f32, entrance: Option<Duration>) -> Vec<u32> {
        debug_assert_eq!(self.lightning, scene.lightning);
        let card = scene.card;
        let width = card.rect.width as usize;
        let height = card.rect.height as usize;
        let mut pixels = vec![0; width * height];
        self.impact = Some([scene.impact[0] - card.rect.x as f32, scene.impact[1] - card.rect.y as f32]);
        self.draw(&mut pixels, width, height, card.scale, image, interference, entrance);
        self.impact = None;
        if entrance.is_none() { return pixels; }
        let canvas = scene.canvas;
        let canvas_width = canvas.width as usize;
        let mut buffer = profile::time(Stage::Canvas, || {
            let mut buffer = vec![0xff00ff; canvas_width * canvas.height as usize];
            let x = (card.rect.x - canvas.x) as usize;
            let y = (card.rect.y - canvas.y) as usize;
            for row in 0..height {
                buffer[(y + row) * canvas_width + x..(y + row) * canvas_width + x + width]
                    .copy_from_slice(&pixels[row * width..(row + 1) * width]);
            }
            buffer
        });
        if let Some(elapsed) = entrance { profile::time(Stage::Lightning, || scene.draw_incoming_bolt(&mut buffer, elapsed)); }
        buffer
    }

    fn random(&mut self) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed
    }
}

type Contours = std::sync::Arc<Vec<Vec<[f32; 2]>>>;

struct PaintCoverage {
    pixels: Vec<bool>,
    traced: std::cell::OnceCell<Contours>,
}

/// Paints one stroke into `glow`, which holds image rows `rows` of an image `width` by `height` pixels.
fn stroke_strength(lightning: LightningSettings, stroke: Stroke, distance: f32) -> f32 {
    let LightningSettings { core_width: core, glow_spread: halo, glow_strength: halo_strength, brightness, .. } = lightning;
    let Stroke { taper, intensity, .. } = stroke;
    let intensity = intensity * brightness;
    (1.3 * (-distance.powi(2) / (core * taper).powi(2)).exp() + halo_strength * (-distance.powi(2) / (halo * taper)).exp()) * intensity
}

/// Number of distance bins in the strength bound tables.
const STRENGTH_BINS: usize = 64;
/// Taper classes the bound tables are built for; a stroke uses the next wider class.
const TAPER_CLASSES: usize = 64;

/// Upper bounds of unit-intensity stroke strength by distance bin, shared by all strokes of one rasterization.
/// Strength only falls with distance and only grows with taper (both exp terms widen), so `unit[q][k]`, the strength
/// at distance k * step for the taper rounded up to class q, caps every pixel at distance k * step or more of any stroke
/// in that class. Built in f64 and inflated by 0.1%, which dwarfs f32 rounding, so a bound is never below a value the
/// f32 stroke arithmetic can produce.
struct StrengthBounds {
    max_taper: f32,
    step: f32,
    unit: Vec<[f32; STRENGTH_BINS + 1]>,
}

impl StrengthBounds {
    fn new(lightning: LightningSettings, strokes: &[Stroke]) -> Self {
        let max_taper = strokes.iter().map(|stroke| stroke.taper).fold(0.0_f32, f32::max);
        let step = lightning.stroke_radius * max_taper.sqrt() / STRENGTH_BINS as f32;
        let (core, halo, halo_strength) = (f64::from(lightning.core_width), f64::from(lightning.glow_spread), f64::from(lightning.glow_strength));
        let unit = (0..=TAPER_CLASSES).map(|class| {
            let taper = f64::from(max_taper) * class as f64 / TAPER_CLASSES as f64;
            std::array::from_fn(|bin| {
                if !(taper > 0.0) { return f32::INFINITY; }
                let distance = bin as f64 * f64::from(step);
                let strength = 1.3 * (-distance * distance / (core * taper).powi(2)).exp() + halo_strength * (-distance * distance / (halo * taper)).exp();
                if strength.is_finite() { (strength * 1.001) as f32 } else { f32::INFINITY }
            })
        }).collect();
        Self { max_taper, step, unit }
    }

    fn class(&self, taper: f32) -> usize {
        if !(self.max_taper > 0.0) || !(taper > 0.0) { return 0; }
        ((taper / self.max_taper * TAPER_CLASSES as f32).ceil() as usize + 1).min(TAPER_CLASSES)
    }
}

/// Where a stroke can still change the glow.
struct StrokeReach {
    /// Pixel distance past which nothing paints (0.015 threshold plus a pixel for rounding); infinity keeps the radius box.
    pixels: f32,
    class: usize,
    intensity: f32,
}

/// None when the stroke paints nothing at all.
fn stroke_reach(lightning: LightningSettings, bounds: &StrengthBounds, stroke: Stroke) -> Option<StrokeReach> {
    if !(stroke_strength(lightning, stroke, 0.0) > 0.015) { return None; }
    let class = bounds.class(stroke.taper);
    let intensity = stroke.intensity * lightning.brightness;
    let pixels = bounds.unit[class].iter().position(|unit| unit * intensity * 1.001 <= 0.015)
        .map(|bin| bin as f32 * bounds.step * stroke.scale + 0.4 + 1.0).unwrap_or(f32::INFINITY);
    Some(StrokeReach { pixels, class, intensity: intensity * 1.001 })
}

fn raster_stroke(glow: &mut [f32], width: usize, height: usize, rows: std::ops::Range<usize>, lightning: LightningSettings, bounds: &StrengthBounds, stroke: Stroke, reach: &StrokeReach) {
    let Stroke { from, to, scale, taper, .. } = stroke;
    let radius = lightning.stroke_radius * scale * taper.sqrt();
    let reach_squared = reach.pixels * reach.pixels;
    let extent = radius.min(reach.pixels);
    let top = ((from[1].min(to[1]) - extent).floor().max(0.0) as usize).min(height).max(rows.start);
    let bottom = ((from[1].max(to[1]) + extent).ceil().max(0.0) as usize).min(height).min(rows.end);
    if top >= bottom { return; }
    let left = ((from[0].min(to[0]) - extent).floor().max(0.0) as usize).min(width);
    let right = ((from[0].max(to[0]) + extent).ceil().max(0.0) as usize).min(width);
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let squared = (dx * dx + dy * dy).max(0.001);
    let per_step = if bounds.step > 0.0 { 1.0 / bounds.step } else { 0.0 };
    let unit = &bounds.unit[reach.class];
    for y in top..bottom {
        for x in left..right {
            let along = (((x as f32 - from[0]) * dx + (y as f32 - from[1]) * dy) / squared).clamp(0.0, 1.0);
            let (offset_x, offset_y) = (x as f32 - from[0] - along * dx, y as f32 - from[1] - along * dy);
            let offset_squared = offset_x * offset_x + offset_y * offset_y;
            if offset_squared > reach_squared { continue; }
            let index = (y - rows.start) * width + x;
            // sqrt stands in for hypot when picking the bound; one bin nearer absorbs their last-bit difference.
            let near = (offset_squared.sqrt() - 0.4).max(0.0) / scale;
            let bin = ((near * per_step) as usize).saturating_sub(1).min(STRENGTH_BINS);
            if unit[bin] * reach.intensity <= glow[index] { continue; }
            let distance = (offset_x.hypot(offset_y) - 0.4).max(0.0) / scale;
            let strength = stroke_strength(lightning, stroke, distance);
            if strength > 0.015 {
                glow[index] = glow[index].max(strength);
            }
        }
    }
}

fn draw_ambient_edges(coverage: &PaintCoverage, glow: &mut LightningPaint, scale: f32, seed: u32, intensity: f32, buildup: f32) {
    for (surface, contour) in coverage.contours(glow.width, glow.height).iter().enumerate() {
        for arc in 0..(1 + (buildup * 2.5) as usize) {
            let random = crate::state::noise_hash(seed ^ (surface * 17 + arc) as u32);
            let start = random as usize % contour.len();
            let length = (((100.0 + 150.0 * buildup) * scale) as usize).min(contour.len());
            let step = (28.0 * scale).round().max(1.0) as usize;
            let point = |offset: usize| {
                let index = (start + offset) % contour.len();
                let previous = contour[(index + contour.len() - 1) % contour.len()];
                let next = contour[(index + 1) % contour.len()];
                let dx = next[0] - previous[0];
                let dy = next[1] - previous[1];
                let length = dx.hypot(dy).max(1.0);
                [contour[index][0] - dy / length * 3.0 * scale, contour[index][1] + dx / length * 3.0 * scale]
            };
            for offset in (0..length).step_by(step) {
                glow.bolt(point(offset), point((offset + step).min(length)), scale, random ^ offset as u32, intensity * 1.5, 1.0);
            }
        }
    }
}

fn paint(buffer: &mut [u32], coverage: &mut Option<&mut PaintCoverage>, index: usize, color: u32) {
    buffer[index] = color;
    if let Some(coverage) = coverage { coverage.pixels[index] = true; }
}

impl PaintCoverage {
    fn draw_activity(&self, buffer: &mut [u32], width: usize, height: usize, scale: f32, activity: LightningActivity, lightning: LightningSettings) {
        match activity {
            LightningActivity::Holding(elapsed, duration, seed) => {
                let Some((seed, intensity, buildup)) = ambient_burst(elapsed, duration, seed) else { return; };
                let mut glow = LightningPaint::new(width, height, lightning);
                let intensity = lightning.pulse_profile.holding(elapsed.as_secs_f32() * 1000.0, intensity);
                draw_ambient_edges(self, &mut glow, scale, seed, intensity, buildup);
                glow.rasterize();
                for (index, strength) in glow.glow.iter_mut().enumerate() {
                    if !self.pixels[index] { *strength = 0.0; }
                }
                glow.composite(buffer);
            }
            LightningActivity::Closing(elapsed, seed) => {
                self.draw_exit(buffer, width, height, scale, elapsed, seed, lightning);
            }
        }
    }

    fn draw_exit(&self, buffer: &mut [u32], width: usize, height: usize, scale: f32, elapsed: Duration, seed: u32, lightning: LightningSettings) {
        let progress = (elapsed.as_secs_f32() / crate::state::TRANSITION_DURATION.as_secs_f32()).clamp(0.0, 1.0);
        let consumption = ((progress - 0.42) / 0.58).clamp(0.0, 1.0);
        let reach = consumption.powi(2) * width.max(height) as f32;
        // The removal field only matters once consumption starts, and at full brightness it is the glow itself.
        let mut glow = LightningPaint::new(width, height, lightning);
        let mut removal = (consumption > 0.0 && lightning.brightness != 1.0)
            .then(|| LightningPaint::new(width, height, LightningSettings { brightness: 1.0, ..lightning }));
        let intensity = lightning.pulse_profile.exit(elapsed);
        for (index, contour) in self.contours(width, height).iter().enumerate() {
            for branch in 0..2 {
                let random = crate::state::noise_hash(seed ^ (index * 17 + branch) as u32);
                let start = random as usize % contour.len();
                let end = (start + (85.0 * scale) as usize) % contour.len();
                glow.bolt(contour[start], contour[end], scale, random, 0.8, (progress / 0.16).min(1.0));
                if let Some(removal) = &mut removal { removal.bolt(contour[start], contour[end], scale, random, 0.8, (progress / 0.16).min(1.0)); }
            }
        }
        let bottom = height as f32;
        let routes = [
            ([312.0 * scale, bottom - 28.0 * scale], [201.0 * scale, bottom - 129.0 * scale]),
            ([202.0 * scale, bottom - 130.0 * scale], [300.0 * scale, bottom - 30.0 * scale]),
            ([260.0 * scale, bottom - 151.0 * scale], [25.0 * scale, 20.0 * scale]),
            ([18.0 * scale, bottom - 163.0 * scale], [288.0 * scale, 20.0 * scale]),
            ([308.0 * scale, 40.0 * scale], [18.0 * scale, bottom - 175.0 * scale]),
            ([310.0 * scale, bottom - 100.0 * scale], [195.0 * scale, bottom - 40.0 * scale]),
        ];
        for (index, (from, to)) in routes.into_iter().enumerate() {
            glow.bolt(from, to, scale, seed ^ index as u32 * 7919, 1.2, ((progress - index as f32 * 0.025) / 0.28).clamp(0.0, 1.0));
            if let Some(removal) = &mut removal { removal.bolt(from, to, scale, seed ^ index as u32 * 7919, 1.2, ((progress - index as f32 * 0.025) / 0.28).clamp(0.0, 1.0)); }
        }
        glow.rasterize();
        if let Some(removal) = &mut removal { removal.rasterize(); }
        let distance = (consumption > 0.0).then(|| profile::time(Stage::Distance, || removal.as_ref().unwrap_or(&glow).distance_field()));
        // Every pixel is independent, so rows are shaded in parallel bands with unchanged per-pixel arithmetic.
        use rayon::prelude::*;
        let band = RASTER_BAND_ROWS * width;
        let covered = &self.pixels;
        buffer.par_chunks_mut(band).zip(glow.glow.par_chunks_mut(band)).enumerate().for_each(|(chunk, (pixels, strengths))| {
            for (offset, (pixel, strength)) in pixels.iter_mut().zip(strengths.iter_mut()).enumerate() {
                let index = chunk * band + offset;
                if !covered[index] { *strength = 0.0; continue; }
                // Before consumption the shade is zero, which leaves the pixel unchanged.
                if let Some(distance) = &distance {
                    if distance[index] < reach {
                        *pixel = 0xff00ff;
                        *strength = 0.0;
                        continue;
                    }
                    let shade = (consumption * 0.8 + (1.0 - (distance[index] - reach) / (12.0 * scale)).clamp(0.0, 1.0) * consumption).min(1.0);
                    *pixel = blend(*pixel, 0x080b14, (shade * 255.0) as u32);
                    let band = (1.0 - (distance[index] - reach) / (1.8 * scale)).clamp(0.0, 1.0);
                    let grain = crate::state::noise_hash(seed ^ ((index % width) / 3) as u32 ^ (((index / width) / 3) as u32).wrapping_mul(7919));
                    *strength = strength.max(band * if grain % 5 == 0 { 1.0 } else { 0.35 });
                }
                *strength *= if progress < 0.88 { intensity } else { 0.0 };
            }
        });
        glow.composite(buffer);
    }

    fn new(pixels: Vec<bool>) -> Self { Self { pixels, traced: std::cell::OnceCell::new() } }

    /// The painted outlines, traced once per coverage; the renderer reuses them while the card geometry is unchanged.
    fn contours(&self, width: usize, height: usize) -> Contours {
        self.traced.get_or_init(|| profile::time(Stage::Contours, || std::sync::Arc::new(self.trace_contours(width, height)))).clone()
    }

    fn trace_contours(&self, width: usize, height: usize) -> Vec<Vec<[f32; 2]>> {
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

    fn draw_entrance_lightning(&self, buffer: &mut [u32], width: usize, height: usize, scale: f32, elapsed: Duration, impact: [f32; 2], lightning: LightningSettings) {
        let milliseconds = elapsed.as_secs_f32() * 1000.0;
        if milliseconds < 120.0 { return; }
        let mut paint = LightningPaint::new(width, height, lightning);
        let intensity = lightning.pulse_profile.entrance(milliseconds) * ((650.0 - milliseconds) / 140.0).clamp(0.0, 1.0);
        if milliseconds < 180.0 {
            for direction in [[-6.0, 0.0], [4.0, -9.0], [4.0, 9.0]] {
                paint.stroke(impact, [impact[0] + direction[0] * scale, impact[1] + direction[1] * scale], scale, 1.5, intensity);
            }
        } else {
            let contours = self.contours(width, height);
            let contours = contours.as_slice();
            let closest = |contour: &Vec<[f32; 2]>, point: [f32; 2]| {
                contour.iter().enumerate().min_by(|(_, a), (_, b)| {
                    (a[0] - point[0]).hypot(a[1] - point[1]).total_cmp(&(b[0] - point[0]).hypot(b[1] - point[1]))
                }).map(|(index, _)| index).unwrap()
            };
            let video = contours.iter().enumerate().min_by(|(_, a), (_, b)| {
                let a = a[closest(a, impact)];
                let b = b[closest(b, impact)];
                (a[0] - impact[0]).hypot(a[1] - impact[1]).total_cmp(&(b[0] - impact[0]).hypot(b[1] - impact[1]))
            }).map(|(index, _)| index).unwrap();
            for (contour_index, contour) in contours.iter().enumerate() {
                let length = contour.len();
                let connected = contour_index == video;
                let start = closest(contour, if connected { impact } else { [252.0 * scale, height as f32 - 134.0 * scale] });
                let delay = if connected { 0.0 } else { 55.0 };
                if milliseconds < 180.0 + delay { continue; }
                let progress = ((milliseconds - 180.0 - delay) / (270.0 - delay)).clamp(0.0, 1.0);
                if !connected && progress > 0.0 {
                    let tail = contour[start];
                    let transfer = contours[video][closest(&contours[video], tail)];
                    paint.stroke(transfer, tail, scale, 0.7, intensity);
                }
                let reach = progress * length as f32 * 0.5;
                let step = (2.0 * scale).round().max(1.0) as usize;
                let frequency = lightning.outline_fork_spacing;
                for distance in (0..length).step_by(step) {
                    let arc = distance.min(length - distance) as f32;
                    if arc > reach { continue; }
                    let index = (start + distance) % length;
                    let next = (index + step) % length;
                    let random = crate::state::noise_hash(index as u32 ^ 31973);
                    let width = 0.7 + (random & 255) as f32 / 255.0 * 0.35;
                    let front = if reach - arc < 12.0 * scale { 1.15 } else { 0.95 };
                    let before = contour[(index + length - step) % length];
                    let after = contour[next];
                    let dx = after[0] - before[0];
                    let dy = after[1] - before[1];
                    let magnitude = dx.hypot(dy).max(1.0);
                    let outward = [dy / magnitude, -dx / magnitude];
                    let offset = ((random >> 8) & 255) as f32 / 255.0 * 1.4 * scale * lightning.roughness;
                    let point = [contour[index][0] + outward[0] * offset, contour[index][1] + outward[1] * offset];
                    paint.stroke(point, contour[next], scale, width, intensity * front);
                    if random.is_multiple_of(frequency) {
                        let reach = (3.0 + ((random >> 16) % 6) as f32) * scale;
                        let fork = [point[0] + outward[0] * reach + dx / magnitude * reach * 0.4,
                            point[1] + outward[1] * reach + dy / magnitude * reach * 0.4];
                        paint.stroke(point, fork, scale, 0.35, intensity * 0.65);
                    }
                }
            }
        }
        paint.composite(buffer);
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

#[cfg(test)]
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
    fn xorshift_jump_matches_repeated_draws() {
        let mut state = 0x1234_5678_u32;
        for steps in [0_usize, 1, 2, 3, 31, 32, 255, 256, 1000, 65_537] {
            let mut serial = state;
            for _ in 0..steps { serial ^= serial << 13; serial ^= serial >> 17; serial ^= serial << 5; }
            assert_eq!(xorshift_jump(state, steps), serial, "{steps} steps");
            state = serial.wrapping_add(0x9e37_79b9);
        }
    }

    #[test]
    fn distance_field_matches_the_sequential_chamfer_passes() {
        fn reference(glow: &[f32], width: usize, height: usize) -> Vec<f32> {
            let mut distance: Vec<f32> = glow.iter().map(|strength| if *strength > 0.25 { 0.0 } else { 10000.0 }).collect();
            for y in 0..height {
                for x in 0..width {
                    let index = y * width + x;
                    if x > 0 { distance[index] = distance[index].min(distance[index - 1] + 1.0); }
                    if y > 0 { distance[index] = distance[index].min(distance[index - width] + 1.0); }
                    if x > 0 && y > 0 { distance[index] = distance[index].min(distance[index - width - 1] + 1.414); }
                    if x + 1 < width && y > 0 { distance[index] = distance[index].min(distance[index - width + 1] + 1.414); }
                }
            }
            for y in (0..height).rev() {
                for x in (0..width).rev() {
                    let index = y * width + x;
                    if x + 1 < width { distance[index] = distance[index].min(distance[index + 1] + 1.0); }
                    if y + 1 < height { distance[index] = distance[index].min(distance[index + width] + 1.0); }
                    if x + 1 < width && y + 1 < height { distance[index] = distance[index].min(distance[index + width + 1] + 1.414); }
                    if x > 0 && y + 1 < height { distance[index] = distance[index].min(distance[index + width - 1] + 1.414); }
                }
            }
            distance
        }
        let mut seed = 7_u32;
        for (width, height) in [(1, 1), (1, 9), (9, 1), (37, 23), (320, 260)] {
            let mut paint = LightningPaint::new(width, height, crate::lightning::PRESETS[0].settings);
            for strength in &mut paint.glow {
                seed = crate::state::noise_hash(seed);
                *strength = if seed % 97 == 0 { 1.0 } else { 0.0 };
            }
            let expected: Vec<u32> = reference(&paint.glow, width, height).iter().map(|value| value.to_bits()).collect();
            let actual: Vec<u32> = paint.distance_field().iter().map(|value| value.to_bits()).collect();
            assert_eq!(actual, expected, "{width}x{height}");
        }
    }

    #[test]
    fn card_template_matches_direct_bubble_and_scanline_painting() {
        for (scale, logical_height) in [(1.0, 240.0), (1.25, 263.0), (1.5, 300.0), (2.0, 310.0)] {
            let (width, height) = ((320.0 * scale) as usize, (logical_height * scale) as usize);
            let bottom = height as f32 / scale - 152.0;
            let template = CardTemplate::new(width, height, scale, bottom);
            for fill in [0x191f2a, 0x000000, 0xffffff, 0x080908, 0xff00ff] {
                let mut expected = vec![0xff00ff; width * height];
                let mut expected_coverage = PaintCoverage::new(vec![false; width * height]);
                draw_bubble(&mut expected, width, height, scale, bottom, fill, Some(&mut expected_coverage));
                shade_bubble_scanlines(&mut expected, width, height, scale, bottom);
                let mut actual = vec![0xff00ff; width * height];
                let mut coverage = PaintCoverage::new(vec![false; width * height]);
                template.paint(&mut actual, fill, Some(&mut coverage));
                template.shade(&mut actual);
                assert!(actual == expected, "Template pixels differ at scale {scale}, fill {fill:06x}");
                assert!(coverage.pixels == expected_coverage.pixels, "Template coverage differs at scale {scale}");
            }
        }
    }

    #[test]
    fn every_exposed_parameter_changes_actual_lightning_pixels() {
        let render = |lightning: LightningSettings| {
            let mut paint = LightningPaint::new(320, 260, lightning);
            paint.bolt([15.0, 235.0], [290.0, 25.0], 1.0, 567891, 0.7, 1.0);
            let mut pixels = vec![0xff00ff; 320 * 260];
            paint.composite(&mut pixels);
            pixels
        };
        let original = LightningSettings::default();
        let baseline = render(original);
        for spec in crate::lightning::PARAMETERS {
            let mut settings = original;
            spec.parameter.set(&mut settings, spec.max);
            settings.validate().unwrap();
            let pixels = render(settings);
            assert_eq!(pixels, render(settings));
            assert!(pixels.iter().zip(&baseline).filter(|(a, b)| a != b).count() > 50, "{} had no useful pixel effect", spec.label);
        }
    }

    #[test]
    fn all_presets_preserve_characterized_bolts_entrance_holding_and_exit_pixels() {
        use sha2::{Digest, Sha256};
        let pins = [
            "c74a6a69704183f727181d59d082395f933c6335c9852d26ee85436cbc51cc7b",
            "f3b121084bec70d764e15fef82fead376cfd63bfa9f4dee0f26f25281c6e4cc8",
            "081f7f2c79e80d22c10c962f5ca3ae4c4736cd7a9050285fb0b5984f3ea66234",
            "aabfc7c987b2b36bce7066a215e702a3ffd8e2f3e3b62f4d7f55c0b2978f0b1b",
        ];
        for (preset, expected) in crate::lightning::PRESETS.iter().zip(pins) {
            let mut digest = Sha256::new();
            let mut pin = |pixels: &[u32]| {
                let bytes = pixels.iter().flat_map(|pixel| pixel.to_le_bytes()).collect::<Vec<_>>();
                digest.update(format!("{:x}", Sha256::digest(bytes)).as_bytes());
            };
            for (scale, seed, reveal) in [(0.75, 1234, 0.35), (1.0, 567891, 1.0), (1.5, 99, 1.0)] {
                let mut paint = LightningPaint::new(320, 260, preset.settings);
                paint.bolt([15.0, 235.0], [290.0, 25.0], scale, seed, preset.settings.pulse_profile.entrance(160.0), reveal);
                let mut pixels = vec![0xff00ff; 320 * 260];
                paint.composite(&mut pixels);
                pin(&pixels);
            }
            let monitor = PhysicalRect { x: -800, y: -200, width: 800, height: 1100 };
            for scale in [1.0, 1.5] {
                let card = CardPlacement { rect: PhysicalRect { x: -(336.0 * scale) as i32, y: 250, width: (320.0 * scale) as u32, height: (260.0 * scale) as u32 }, scale };
                for seed in [1234, 1235] {
                    let scene = EntranceScene::new(monitor, card, seed, preset.settings);
                    let mut renderer = Renderer::with_settings(&Settings { lightning: preset.settings, ..Settings::default() }).unwrap();
                    for time in [60, 140, 260, 480] {
                        renderer.seed = 567891;
                        pin(&renderer.draw_scene(&scene, None, 0.0, Some(Duration::from_millis(time))));
                    }
                    let duration = Duration::from_secs(5);
                    let active = (3000..4500).find(|time| ambient_burst(Duration::from_millis(*time), duration, seed).is_some_and(|(_, intensity, _)| intensity > 0.8)).unwrap();
                    for activity in [
                        LightningActivity::Holding(Duration::from_millis(700), duration, seed),
                        LightningActivity::Holding(Duration::from_millis(active), duration, seed),
                        LightningActivity::Closing(Duration::from_millis(300), seed),
                        LightningActivity::Closing(Duration::from_millis(650), seed),
                    ] {
                        renderer.seed = 567891;
                        renderer.lightning_activity = Some(activity);
                        pin(&renderer.draw_scene(&scene, None, 0.0, None));
                    }
                }
            }
            assert_eq!(format!("{:x}", digest.finalize()), expected, "{} changed its original output", preset.label);
        }
    }

    #[test]
    fn saved_reload_changes_the_next_snapshot_without_changing_an_active_scene() {
        let data = std::env::temp_dir().join("opencode").join(format!("herald-lightning-snapshot-{}-{}", std::process::id(), crate::state::timestamp()));
        let mut settings = Settings::default();
        settings.save(&data).unwrap();
        let mut store = crate::settings::Store::new(&data).unwrap();
        let mut renderer = Renderer::with_settings(&store.current).unwrap();
        let monitor = PhysicalRect { x: 0, y: 0, width: 420, height: 380 };
        let card = CardPlacement { rect: PhysicalRect { x: 20, y: 20, width: 320, height: 260 }, scale: 1.0 };
        let active = EntranceScene::new(monitor, card, 1234, store.current.lightning);
        let before = renderer.draw_scene(&active, None, 0.0, Some(Duration::from_millis(260)));
        settings.lightning = crate::lightning::PRESETS[1].settings;
        settings.save(&data).unwrap();
        assert!(store.reload().unwrap());
        assert_eq!(active.lightning, LightningSettings::default());
        assert_eq!(renderer.lightning, active.lightning);
        renderer.seed = 567891;
        assert_eq!(renderer.draw_scene(&active, None, 0.0, Some(Duration::from_millis(260))), before);
        renderer.set_lightning(store.current.lightning);
        let next = EntranceScene::new(monitor, card, 1234, store.current.lightning);
        renderer.seed = 567891;
        assert_ne!(renderer.draw_scene(&next, None, 0.0, Some(Duration::from_millis(260))), before);
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    #[should_panic]
    fn mismatched_scene_and_renderer_snapshots_are_rejected() {
        let mut renderer = Renderer::new().unwrap();
        let card = CardPlacement { rect: PhysicalRect { x: 20, y: 20, width: 320, height: 260 }, scale: 1.0 };
        let scene = EntranceScene::new(PhysicalRect { x: 0, y: 0, width: 420, height: 380 }, card, 1234, crate::lightning::PRESETS[1].settings);
        renderer.draw_scene(&scene, None, 0.0, Some(Duration::from_millis(140)));
    }

    #[test]
    fn incoming_bolt_uses_the_scene_snapshot_for_leader_and_return_stroke() {
        let monitor = PhysicalRect { x: -800, y: -200, width: 800, height: 1100 };
        for preset in crate::lightning::PRESETS {
            let lightning = preset.settings;
            for scale in [1.0, 1.5] {
                let card = CardPlacement { rect: PhysicalRect { x: -(336.0 * scale) as i32, y: 250, width: (320.0 * scale) as u32, height: (260.0 * scale) as u32 }, scale };
                for seed in [1234, 1235] {
                    let mut scene = EntranceScene::new(monitor, card, seed, lightning);
                    scene.channel = vec![scene.source, scene.impact];
                    scene.forks.clear();
                    let width = scene.canvas.width as usize;
                    let height = scene.canvas.height as usize;
                    let from = [scene.source[0] - scene.canvas.x as f32, scene.source[1] - scene.canvas.y as f32];
                    let impact = [scene.impact[0] - scene.canvas.x as f32, scene.impact[1] - scene.canvas.y as f32];
                    for (time, reveal, intensity) in [(60, 0.5, 0.65), (160, 1.0, lightning.pulse_profile.entrance(160.0))] {
                        let to = [from[0] + (impact[0] - from[0]) * reveal, from[1] + (impact[1] - from[1]) * reveal];
                        let mut paint = LightningPaint::new(width, height, lightning);
                        paint.stroke(from, to, scale, 0.9, intensity);
                        let mut expected = vec![0xff00ff; width * height];
                        paint.composite(&mut expected);
                        let mut pixels = vec![0xff00ff; width * height];
                        scene.draw_incoming_bolt(&mut pixels, Duration::from_millis(time));
                        assert_eq!(pixels, expected, "Wrong incoming paint for {} at {time}ms", preset.label);
                        assert_ne!(pixels, vec![0xff00ff; width * height]);
                    }
                    for time in [0, 650] {
                        let mut pixels = vec![0xff00ff; width * height];
                        scene.draw_incoming_bolt(&mut pixels, Duration::from_millis(time));
                        assert!(pixels.iter().all(|pixel| *pixel == 0xff00ff));
                    }
                }
            }
        }
    }

    #[test]
    fn lightning_variations_are_distinct_and_repeatable() {
        let render = |lightning: LightningSettings| {
            let mut paint = LightningPaint::new(160, 100, lightning);
            paint.bolt([15.0, 85.0], [145.0, 15.0], 1.0, 1234, lightning.pulse_profile.entrance(160.0), 1.0);
            let mut pixels = vec![0xff00ff; 160 * 100];
            paint.composite(&mut pixels);
            pixels
        };
        let settings = crate::lightning::PRESETS.map(|preset| preset.settings);
        for (index, lightning) in settings.into_iter().enumerate() {
            let pixels = render(lightning);
            assert_eq!(pixels, render(lightning));
            assert!(pixels.iter().any(|pixel| *pixel != 0xff00ff));
            assert_eq!(pixels[0], 0xff00ff);
            for other in &settings[..index] { assert_ne!(pixels, render(*other)); }
        }
    }

    #[test]
    fn lightning_variations_keep_screen_edge_sources_and_video_impacts() {
        let monitor = PhysicalRect { x: -1920, y: -200, width: 1920, height: 1400 };
        for preset in &crate::lightning::PRESETS[1..] {
            for scale in [1.0, 1.5, 2.0] {
                let card = CardPlacement { rect: PhysicalRect { x: -(336.0 * scale) as i32, y: 300, width: (320.0 * scale) as u32, height: (260.0 * scale) as u32 }, scale };
                for seed in 0..40 {
                    let scene = EntranceScene::new(monitor, card, seed, preset.settings);
                    assert!(scene.source[0] == -1.0 || scene.source[1] == 1199.0);
                    assert_eq!(scene.channel.last(), Some(&scene.impact));
                    assert!(scene.canvas.contains(card.rect.x, card.rect.y));
                    assert!(scene.canvas.contains(card.rect.right() - 1, card.rect.bottom() - 1));
                    for point in scene.channel.iter().chain(scene.forks.iter().flat_map(|(_, points, _)| points)) {
                        assert!(monitor.contains(point[0] as i32, point[1] as i32));
                    }
                }
            }
        }
    }

    #[test]
    fn sources_use_only_bottom_and_right_edges_across_seeds_and_scales() {
        for scale in [1.0, 1.5, 2.0] {
            let monitor = PhysicalRect { x: -1920, y: -200, width: 1920, height: 1400 };
            let card = CardPlacement { rect: PhysicalRect { x: -(336.0 * scale) as i32, y: 300, width: (320.0 * scale) as u32, height: (260.0 * scale) as u32 }, scale };
            for seed in 0..40 {
                let scene = EntranceScene::new(monitor, card, seed, LightningSettings::default());
                assert!(scene.source[0] == -1.0 || scene.source[1] == 1199.0);
                assert!(scene.source[0] > monitor.x as f32 && scene.source[1] > monitor.y as f32);
                assert!(scene.canvas.contains(card.rect.x, card.rect.y));
                assert!(scene.canvas.contains(card.rect.right() - 1, card.rect.bottom() - 1));
                assert_eq!(scene.channel.last(), Some(&scene.impact));
            }
        }
    }

    #[test]
    fn exit_flashes_never_restore_erased_geometry() {
        for scale in [1.0, 1.5] {
            let width = (320.0 * scale) as usize;
            let height = (260.0 * scale) as usize;
            let mut renderer = Renderer::new().unwrap();
            let mut previous = vec![0; width * height];
            renderer.draw(&mut previous, width, height, scale, None, 0.0, None);
            for time in (300..=650).step_by(25) {
                renderer.lightning_activity = Some(LightningActivity::Closing(Duration::from_millis(time), 1234));
                let mut frame = vec![0; previous.len()];
                renderer.draw(&mut frame, width, height, scale, None, 0.0, None);
                assert!(previous.iter().zip(&frame).all(|(old, new)| *old != 0xff00ff || *new == 0xff00ff), "A flash restored vanished geometry at {time}ms");
                previous = frame;
            }
            assert!(previous.iter().all(|pixel| *pixel == 0xff00ff));
        }
    }

    #[test]
    fn exit_consumes_geometry_monotonically_at_brightness_and_glow_bounds() {
        for brightness in [0.1, 2.0] {
            for glow_strength in [0.05, 0.6] {
                let mut settings = Settings::default();
                settings.lightning.brightness = brightness;
                settings.lightning.glow_strength = glow_strength;
                let mut renderer = Renderer::with_settings(&settings).unwrap();
                let mut previous = usize::MAX;
                for time in [300, 400, 450, 500, 550, 600, 650] {
                    renderer.lightning_activity = Some(LightningActivity::Closing(Duration::from_millis(time), 1234));
                    let mut frame = vec![0; 320 * 260];
                    renderer.draw(&mut frame, 320, 260, 1.0, None, 0.0, None);
                    let remaining = frame.iter().filter(|pixel| **pixel != 0xff00ff).count();
                    assert!(remaining <= previous, "Exit restored geometry at brightness {brightness} and glow strength {glow_strength} at {time}ms");
                    previous = remaining;
                }
                assert_eq!(previous, 0, "Exit did not consume geometry at brightness {brightness} and glow strength {glow_strength}");
            }
        }
    }

    #[test]
    fn sporadic_flashes_stay_near_the_outline_without_projecting_outward() {
        let mut renderer = Renderer::new().unwrap();
        let mut plain = vec![0; 320 * 260];
        renderer.draw(&mut plain, 320, 260, 1.0, None, 0.0, None);
        let coverage = PaintCoverage::new(plain.iter().map(|pixel| *pixel != 0xff00ff).collect());
        let mut early = LightningPaint::new(320, 260, LightningSettings::default());
        let mut late = LightningPaint::new(320, 260, LightningSettings::default());
        draw_ambient_edges(&coverage, &mut early, 1.0, 1234, 1.0, 0.0);
        draw_ambient_edges(&coverage, &mut late, 1.0, 1234, 1.0, 1.0);
        let lit = |paint: &mut LightningPaint| { paint.rasterize(); paint.glow.iter().filter(|strength| **strength > 0.2).count() };
        assert!(lit(&mut late) > lit(&mut early) * 3);
        let card = CardPlacement { rect: PhysicalRect { x: 400, y: 400, width: 320, height: 260 }, scale: 1.0 };
        let scene = EntranceScene::new(PhysicalRect { x: 0, y: 0, width: 1200, height: 900 }, card, 1234, LightningSettings::default());
        let time = (3000..4500).find(|time| ambient_burst(Duration::from_millis(*time), Duration::from_secs(5), 1234).is_some_and(|(_, intensity, _)| intensity > 0.8)).unwrap();
        renderer.seed = 567891;
        renderer.lightning_activity = Some(LightningActivity::Holding(Duration::from_millis(time), Duration::from_secs(5), 1234));
        let frame = renderer.draw_scene(&scene, None, 0.0, None);
        for (index, pixel) in plain.iter().enumerate() {
            if *pixel != 0xff00ff { continue; }
            assert_eq!(frame[index], *pixel, "Lightning projected outside the surface");
        }
        for (left, top, right, bottom) in [(60, 25, 260, 85), (210, 140, 290, 220)] {
            let changed = (top..bottom).flat_map(|y| (left..right).map(move |x| y * 320 + x))
                .filter(|index| frame[*index] != plain[*index]).count();
            assert_eq!(changed, 0, "An edge flash crossed the surface interior");
        }
    }

    #[test]
    fn sporadic_bolts_are_fully_visible_and_stationary_during_each_flash() {
        let duration = Duration::from_secs(5);
        let mut previous = None;
        for milliseconds in 650..5000 {
            if let Some((seed, _, buildup)) = ambient_burst(Duration::from_millis(milliseconds), duration, 1234) {
                if let Some((last_seed, last_buildup)) = previous {
                    if seed == last_seed { assert_eq!(buildup, last_buildup); }
                }
                previous = Some((seed, buildup));
            }
        }
    }

    #[test]
    fn sporadic_timing_has_uneven_gaps_and_quick_clusters() {
        let duration = Duration::from_secs(30);
        let mut starts = Vec::new();
        let mut last_seed = None;
        for time in (650..30000).step_by(5) {
            if let Some((seed, _, _)) = ambient_burst(Duration::from_millis(time), duration, 1234) {
                if last_seed != Some(seed) { starts.push(time); }
                last_seed = Some(seed);
            }
        }
        let intervals: Vec<_> = starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(intervals.iter().any(|gap| *gap < 300));
        assert!(intervals.iter().any(|gap| *gap > 800));
        assert!(intervals.iter().max().unwrap() - intervals.iter().min().unwrap() > 600);
    }

    #[test]
    fn ambient_arcs_build_up_over_short_and_long_announcements() {
        for seconds in [5, 30] {
            let duration = Duration::from_secs(seconds);
            let total = duration.as_millis() as u64 - 650;
            let mut samples = [0, 0];
            let mut bursts = [0, 0];
            for notification in 0..32 {
                for (region, start) in [650, 650 + total * 2 / 3].into_iter().enumerate() {
                    let mut previous_seed = None;
                    for time in (start..start + total / 3).step_by(5) {
                        if let Some((seed, _, _)) = ambient_burst(Duration::from_millis(time), duration, notification) {
                            samples[region] += 1;
                            if previous_seed != Some(seed) { bursts[region] += 1; }
                            previous_seed = Some(seed);
                        }
                    }
                }
            }
            assert!(samples[1] > samples[0], "Later arcs should stay visible longer");
            assert!(bursts[1] >= bursts[0], "Later arcs should arrive more frequently");
        }
    }

    #[test]
    fn holding_has_sparse_bursts_and_exit_consumes_both_interiors() {
        let mut renderer = Renderer::new().unwrap();
        let mut plain = vec![0; 320 * 260];
        renderer.draw(&mut plain, 320, 260, 1.0, None, 0.0, None);
        let seed = 1234;
        let duration = Duration::from_secs(5);
        let active: Vec<_> = (650..5000).step_by(20).filter(|time| ambient_burst(Duration::from_millis(*time), duration, seed).is_some()).collect();
        assert!(active.len() >= 30 && active.len() < 140);
        for time in [700, active[active.len() / 2]] {
            let mut pixels = vec![0; plain.len()];
            renderer.seed = 567891;
            renderer.lightning_activity = Some(LightningActivity::Holding(Duration::from_millis(time), duration, seed));
            renderer.draw(&mut pixels, 320, 260, 1.0, None, 0.0, None);
            assert_eq!(pixels == plain, ambient_burst(Duration::from_millis(time), duration, seed).is_none());
            assert!(pixels.iter().zip(&plain).all(|(pixel, before)| *before != 0xff00ff || pixel == before));
        }
        let mut remaining = Vec::new();
        for time in [0, 300, 400, 650] {
            let mut pixels = vec![0; plain.len()];
            renderer.seed = 567891;
            renderer.lightning_activity = Some(LightningActivity::Closing(Duration::from_millis(time), seed));
            renderer.draw(&mut pixels, 320, 260, 1.0, None, 0.0, None);
            remaining.push(pixels.iter().filter(|pixel| **pixel != 0xff00ff).count());
            if time == 300 {
                for (top, bottom) in [(25, 80), (145, 200)] {
                    let lit = (top..bottom).flat_map(|y| (210..285).map(move |x| y * 320 + x))
                        .filter(|index| pixels[*index] != 0xff00ff && pixels[*index] & 255 > 180).count();
                    assert!(lit > 20, "Exit did not cross the interior");
                }
            }
        }
        assert!(remaining[0] > remaining[1] && remaining[1] > remaining[2]);
        assert_eq!(remaining[3], 0);
    }

    #[test]
    fn seeded_screen_edge_channel_is_stable_forked_tapered_and_outside_the_card_until_video_hit() {
        let card = CardPlacement { rect: PhysicalRect { x: -504, y: 250, width: 480, height: 390 }, scale: 1.5 };
        let monitor = PhysicalRect { x: -1920, y: -200, width: 1920, height: 1080 };
        let scene = EntranceScene::new(monitor, card, 7345, LightningSettings::default());
        let repeated = EntranceScene::new(monitor, card, 7345, LightningSettings::default());
        assert_eq!(scene.channel, repeated.channel);
        assert_eq!(scene.forks, repeated.forks);
        assert!(scene.channel.len() > 10 && !scene.forks.is_empty());
        assert_eq!(scene.channel[0][0], -1.0);
        assert_eq!(*scene.channel.last().unwrap(), [-30.75, 535.0]);
        for points in std::iter::once(&scene.channel).chain(scene.forks.iter().map(|(_, points, _)| points)) {
            for pair in points.windows(2) {
                for fraction in [0.0, 0.25, 0.5, 0.75] {
                    let x = (pair[0][0] + (pair[1][0] - pair[0][0]) * fraction - card.rect.x as f32) / card.scale;
                    let y = (pair[0][1] + (pair[1][1] - pair[0][1]) * fraction - card.rect.y as f32) / card.scale;
                    assert!(!bubble_contains(x, y, 108.0), "A premature fork hit the bubble at {x},{y}");
                    assert!(!(188.5..315.5).contains(&x) || !(120.5..247.5).contains(&y), "A fork hit the video before the main channel");
                }
            }
        }
        let mut early = vec![0xff00ff; scene.canvas.width as usize * scene.canvas.height as usize];
        scene.draw_incoming_bolt(&mut early, Duration::from_millis(60));
        let mut later = vec![0xff00ff; early.len()];
        scene.draw_incoming_bolt(&mut later, Duration::from_millis(100));
        assert!(early.iter().filter(|color| **color != 0xff00ff).count() > 1000);
        assert!(early.iter().zip(&later).filter(|(a, b)| **a != 0xff00ff && a == b).count() > 1000, "Leader regenerated instead of revealing a planted channel");
        assert!(scene.forks.iter().all(|(_, _, width)| *width < 0.9));
        let mut paint = LightningPaint::new(100, 40, LightningSettings::default());
        paint.stroke([10.0, 20.0], [90.0, 20.0], 1.0, 1.0, 1.0);
        let mut pixels = vec![0xff00ff; 4000];
        paint.composite(&mut pixels);
        assert_eq!(pixels[20 * 100 + 50], 0xf7fbff);
        assert!(pixels[23 * 100 + 50] & 255 > (pixels[23 * 100 + 50] >> 16) & 255);
        assert_eq!(pixels[30 * 100 + 50], 0xff00ff);
    }

    #[test]
    fn disconnected_bubble_receives_charge_through_its_tail_after_the_video() {
        let mut pixels = vec![0xff00ff; 320 * 260];
        let mut coverage = PaintCoverage::new(vec![false; pixels.len()]);
        draw_bubble(&mut pixels, 320, 260, 1.0, 88.0, 0x191f2a, Some(&mut coverage));
        for y in 120..248 {
            for x in 188..316 { coverage.pixels[y * 320 + x] = true; pixels[y * 320 + x] = 0x191f2a; }
        }
        let plain = pixels.clone();
        coverage.draw_entrance_lightning(&mut pixels, 320, 260, 1.0, Duration::from_millis(200), [187.5, 169.5], LightningSettings::default());
        assert_eq!(&pixels[..110 * 320], &plain[..110 * 320]);
        assert!(pixels[170 * 320 + 188] & 255 > 200);
        pixels.copy_from_slice(&plain);
        coverage.draw_entrance_lightning(&mut pixels, 320, 260, 1.0, Duration::from_millis(260), [187.5, 169.5], LightningSettings::default());
        assert!(pixels[110 * 320 + 252] & 255 > 100, "No visible transfer to the bubble tail");
        pixels.copy_from_slice(&plain);
        coverage.draw_entrance_lightning(&mut pixels, 320, 260, 1.0, Duration::from_millis(480), [187.5, 169.5], LightningSettings::default());
        let bright_top = (3..10).flat_map(|y| (40..280).map(move |x| y * 320 + x))
            .filter(|index| pixels[*index] & 255 > 220).count();
        assert!(bright_top >= 100, "Charge never reached the full far bubble edge");
    }

    #[test]
    fn restored_scene_is_identical_to_the_normal_card_without_changing_the_portrait_stream() {
        let card = CardPlacement { rect: PhysicalRect { x: 700, y: 500, width: 320, height: 260 }, scale: 1.0 };
        let scene = EntranceScene::new(PhysicalRect { x: 0, y: 0, width: 1200, height: 900 }, card, 1, LightningSettings::default());
        let mut renderer = Renderer::new().unwrap();
        let mut normal = vec![0; 320 * 260];
        renderer.draw(&mut normal, 320, 260, 1.0, None, 0.0, None);
        renderer.seed = 567891;
        assert_eq!(renderer.draw_scene(&scene, None, 0.0, None), normal);
    }

    #[test]
    fn screen_edge_leader_hits_video_before_bidirectional_full_outline_charge() {
        let mut renderer = Renderer::new().unwrap();
        renderer.text = "The task passed.".into();
        renderer.title = "Checks passed".into();
        let image = RgbaImage::from_pixel(128, 128, image::Rgba([70, 40, 20, 255]));
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for logical_height in [260, 340] {
                let width = (320.0 * scale) as usize;
                let height = (logical_height as f32 * scale) as usize;
                let bottom = logical_height as f32 - 152.0;
                let card = CardPlacement { rect: PhysicalRect { x: 1400, y: 600, width: width as u32, height: height as u32 }, scale };
                let scene = EntranceScene::new(PhysicalRect { x: -200, y: -100, width: 2600, height: 1700 }, card, 1234, LightningSettings::default());
                let zones = [(150.0, 6.0), (6.0, 50.0), (100.0, bottom), (244.0, bottom + 8.0),
                    (294.0, bottom + 12.0), (188.0, bottom + 76.0), (315.0, bottom + 76.0), (252.0, bottom + 139.0)];
                for milliseconds in [60, 140, 200, 480] {
                    renderer.seed = 567891;
                    let mut plain = vec![0; width * height];
                    renderer.draw(&mut plain, width, height, scale, Some(&image), 0.0, None);
                    renderer.seed = 567891;
                    let struck = renderer.draw_scene(&scene, Some(&image), 0.0, Some(Duration::from_millis(milliseconds)));
                    let offset_x = (card.rect.x - scene.canvas.x) as usize;
                    let offset_y = (card.rect.y - scene.canvas.y) as usize;
                    let scene_width = scene.canvas.width as usize;
                    let mut reached = [false; 8];
                    let mut white = 0;
                    for (index, &before) in plain.iter().enumerate() {
                        let after = struck[(offset_y + index / width) * scene_width + offset_x + index % width];
                        if before == after { continue; }
                        let x = (index % width) as f32 / scale;
                        let y = (index / width) as f32 / scale;
                        let red = (after >> 16) & 255;
                        let green = (after >> 8) & 255;
                        let blue = after & 255;
                        white += usize::from(red > 200 && green > 200 && blue > 200);
                        if before == 0xff00ff { assert!(blue >= red && blue >= green, "Clear pixel acquired a magenta halo"); }
                        assert!(!(40.0..290.0).contains(&x) || !(26.0..bottom - 22.0).contains(&y), "Text interior changed at {x},{y}");
                        assert!(!(214.0..290.0).contains(&x) || !(bottom + 38.0..bottom + 114.0).contains(&y), "Portrait center changed at {x},{y}");
                        for (zone, &(zx, zy)) in zones.iter().enumerate() {
                            if (x - zx).abs() < 6.0 && (y - zy).abs() < 6.0 && blue > red + 10 {
                                reached[zone] = true;
                            }
                        }
                    }
                    if milliseconds == 60 {
                        assert_eq!(reached, [false; 8]);
                        let source_x = (scene.source[0] - scene.canvas.x as f32) as usize;
                        let source_y = (scene.source[1] - scene.canvas.y as f32) as usize;
                        assert!(struck[source_y * scene_width + source_x] & 255 > 100, "Leader is not planted on the screen edge");
                    } else if milliseconds == 140 {
                        assert!(!reached[0] && !reached[1] && !reached[2]);
                        assert!(white > 5, "No video impact flare");
                    } else if milliseconds == 200 {
                        assert!(reached[7], "Charge did not leave the hit along the video");
                        assert!(!reached[0], "Bubble lit before the charge reached it");
                        for direction in [-25.0, 25.0] {
                            let y = ((logical_height as f32 - 13.0) * scale).round() as usize;
                            let x = ((252.0 + direction) * scale) as usize;
                            let radius = (3.0 * scale).ceil() as usize;
                            assert!((y - radius..=y + radius).any(|row| struck[(offset_y + row) * scene_width + offset_x + x] & 255 > 150), "Charge did not propagate in both directions");
                        }
                    } else {
                        assert_eq!(reached, [true; 8], "Missing complete outline at scale {scale}, height {logical_height}");
                        assert!(white > 50);
                        let mut coverage = PaintCoverage::new(vec![false; width * height]);
                        let mut mask = vec![0; width * height];
                        draw_bubble(&mut mask, width, height, scale, bottom, 0, Some(&mut coverage));
                        for y in ((logical_height as f32 - 140.0) * scale) as usize..((logical_height as f32 - 12.0) * scale) as usize {
                            for x in (188.0 * scale) as usize..(316.0 * scale) as usize { coverage.pixels[y * width + x] = true; }
                        }
                        for contour in coverage.contours(width, height).iter() {
                            let lit = contour.iter().filter(|point| {
                                let radius = (3.0 * scale).ceil() as i32;
                                (-radius..=radius).any(|dy| (-radius..=radius).any(|dx| {
                                    let x = point[0].round() as i32 + dx;
                                    let y = point[1].round() as i32 + dy;
                                    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 { return false; }
                                    let color = struck[(offset_y + y as usize) * scene_width + offset_x + x as usize];
                                    color & 255 >= 180 && (color >> 8) & 255 >= 130 && (color >> 16) & 255 >= 100
                                }))
                            }).count();
                            assert!(lit * 100 >= contour.len() * 95, "Only {lit}/{} painted perimeter points energized", contour.len());
                        }
                    }
                }
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
        renderer.draw(&mut struck, 320, 260, 1.0, None, 0.62, Some(Duration::from_millis(140)));
        assert!(struck.iter().filter(|pixel| (**pixel & 255) > 220 && ((**pixel >> 16) & 255) > 220).count() >= 10);
        let mut next_struck = vec![0; plain.len()];
        renderer.draw(&mut next_struck, 320, 260, 1.0, None, 0.0, None);
        assert_eq!(next_plain, next_struck);
        let mut repeated = vec![0; plain.len()];
        renderer.seed = 567891;
        renderer.draw(&mut repeated, 320, 260, 1.0, None, 0.62, Some(Duration::from_millis(140)));
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
        assert_eq!(buffer[0], 0xff00ff);
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
