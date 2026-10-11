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

fn ambient_burst(elapsed: Duration, duration: Duration, seed: u32, lightning: LightningSettings) -> Option<(u32, f32, f32)> {
    let time = elapsed.saturating_sub(lightning.transition_duration()).as_secs_f32();
    let duration = duration.saturating_sub(lightning.transition_duration()).as_secs_f32().max(1.0);
    let mut start = (0.1 + (crate::state::noise_hash(seed) % 500) as f32 / 1000.0) / lightning.frequency;
    let mut event = 0_u32;
    let mut burst = None;
    while start <= time {
        let random = crate::state::noise_hash(seed ^ event.wrapping_mul(7919));
        let buildup = (start / duration).min(1.0);
        let length = 0.09 + ((random >> 8) % 140) as f32 / 1000.0 + buildup * 0.05;
        let flash_length = length / lightning.speed;
        if time < start + flash_length {
            let age = (time - start) / flash_length;
            burst = Some((random, (std::f32::consts::PI * age).sin(), buildup));
        }
        let gap = if random % 4 == 0 { 0.04 + ((random >> 16) % 110) as f32 / 1000.0 }
            else { 0.22 + ((random >> 16) % 900) as f32 / 1000.0 };
        start += ((length + gap * (1.0 - 0.55 * buildup)) / lightning.frequency).max(length + 0.015);
        event = event.wrapping_add(1);
    }
    burst
}

/// True while a holding-phase ambient burst is drawn, the frames that pay for Lightning while the card is held.
pub fn ambient_active(elapsed: Duration, duration: Duration, seed: u32, lightning: LightningSettings) -> bool { ambient_burst(elapsed, duration, seed, lightning).is_some() }

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
    pub fn lightning(&self) -> LightningSettings { self.lightning }

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
        let milliseconds = elapsed.as_secs_f32() * 1000.0 * self.lightning.speed;
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
/// Rows per band when rasterizing strokes in parallel.
const STROKE_BAND_ROWS: usize = 4;

struct LightningPaint {
    width: usize,
    height: usize,
    glow: Vec<f32>,
    lightning: LightningSettings,
    strokes: Vec<Stroke>,
    /// Glow is still all zero, so a rasterization depends only on the size, settings and strokes.
    fresh: bool,
    /// Identifies this paint as the owner of the cached rasterization while its glow is that result.
    cached: Option<u64>,
}

/// Everything a rasterization of a fresh paint depends on, bit for bit.
#[derive(PartialEq)]
struct RasterKey { width: usize, height: usize, lightning: String, strokes: Vec<u32> }

impl RasterKey {
    fn new(paint: &LightningPaint) -> Self {
        let strokes = paint.strokes.iter().flat_map(|stroke| [stroke.from[0], stroke.from[1], stroke.to[0], stroke.to[1], stroke.scale, stroke.taper, stroke.intensity])
            .map(f32::to_bits).collect();
        Self { width: paint.width, height: paint.height, lightning: format!("{:?}", paint.lightning), strokes }
    }
}

/// The last fresh rasterization on this thread and, once asked for, its distance field. Once the exit's bolts are fully
/// revealed they are the same strokes every frame (fixed seed, geometry and settings; the pulse is applied afterwards),
/// so later frames reuse the glow and distance instead of repainting thousands of strokes.
struct RasterCache { key: RasterKey, owner: u64, glow: Vec<f32>, distance: Option<Vec<f32>> }

thread_local! {
    static RASTER_CACHE: std::cell::RefCell<Option<RasterCache>> = const { std::cell::RefCell::new(None) };
}

fn next_raster_owner() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl LightningPaint {
    fn new(width: usize, height: usize, lightning: LightningSettings) -> Self {
        Self { width, height, glow: vec![0.0; width * height], lightning, strokes: Vec::new(), fresh: true, cached: None }
    }

    /// Applies the queued strokes, reusing this thread's previous result when a fresh paint queues identical strokes.
    fn rasterize(&mut self) {
        if self.strokes.is_empty() { return; }
        let fresh = std::mem::replace(&mut self.fresh, false);
        self.cached = None;
        if !fresh {
            let strokes = std::mem::take(&mut self.strokes);
            self.paint_strokes(strokes);
            return;
        }
        let key = RasterKey::new(self);
        let owner = next_raster_owner();
        let hit = RASTER_CACHE.with(|cache| match cache.borrow_mut().as_mut() {
            Some(cache) if cache.key == key && cache.glow.len() == self.glow.len() => {
                self.glow.copy_from_slice(&cache.glow);
                cache.owner = owner;
                true
            }
            _ => false,
        });
        let strokes = std::mem::take(&mut self.strokes);
        if !hit {
            self.paint_strokes(strokes);
            let glow = self.glow.clone();
            RASTER_CACHE.with(|cache| *cache.borrow_mut() = Some(RasterCache { key, owner, glow, distance: None }));
        }
        self.cached = Some(owner);
    }

    /// Paints strokes into the glow. A pixel keeps the maximum strength of all strokes, which does not depend on order,
    /// so bands of rows are rasterized in parallel with the same per-pixel arithmetic as one sequential pass.
    fn paint_strokes(&mut self, strokes: Vec<Stroke>) {
        use rayon::prelude::*;
        let lightning = self.lightning;
        let bounds = StrengthBounds::new(lightning, &strokes);
        let strokes: Vec<(Stroke, StrokeReach)> = strokes.into_iter()
            .filter_map(|stroke| stroke_reach(lightning, &bounds, stroke).map(|reach| (stroke, reach))).collect();
        let (width, height, lightning) = (self.width, self.height, self.lightning);
        if width == 0 || height == 0 { return; }
        // Strokes crowd along the card edges, so thin bands keep the busiest rows from serializing one thread; each band
        // only visits the strokes whose box reaches it.
        let bands = height.div_ceil(STROKE_BAND_ROWS);
        let mut members: Vec<Vec<u32>> = vec![Vec::new(); bands];
        for (index, (stroke, reach)) in strokes.iter().enumerate() {
            let extent = (lightning.stroke_radius * stroke.scale * stroke.taper.sqrt()).min(reach.pixels);
            let top = ((stroke.from[1].min(stroke.to[1]) - extent).floor().max(0.0) as usize).min(height);
            let bottom = ((stroke.from[1].max(stroke.to[1]) + extent).ceil().max(0.0) as usize).min(height);
            if top >= bottom { continue; }
            for band in top / STROKE_BAND_ROWS..bottom.div_ceil(STROKE_BAND_ROWS) { members[band].push(index as u32); }
        }
        self.glow.par_chunks_mut(STROKE_BAND_ROWS * width).zip(members.par_iter()).enumerate().for_each(|(band, (glow, members))| {
            let top = band * STROKE_BAND_ROWS;
            let rows = top..(top + STROKE_BAND_ROWS).min(height);
            for &index in members {
                let (stroke, reach) = &strokes[index as usize];
                raster_stroke(glow, width, height, rows.clone(), lightning, &bounds, *stroke, reach);
            }
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
        if let Some(owner) = self.cached {
            let cached = RASTER_CACHE.with(|cache| cache.borrow().as_ref().filter(|cache| cache.owner == owner).and_then(|cache| cache.distance.clone()));
            if let Some(distance) = cached { return distance; }
        }
        let distance = self.compute_distance_field();
        if let Some(owner) = self.cached {
            RASTER_CACHE.with(|cache| if let Some(cache) = cache.borrow_mut().as_mut().filter(|cache| cache.owner == owner) { cache.distance = Some(distance.clone()); });
        }
        distance
    }

    fn compute_distance_field(&self) -> Vec<f32> {
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

/// The bubble's per-pixel paint, classified once per card size.
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

impl CardTemplate {
    fn new(width: usize, height: usize, scale: f32, bottom: f32) -> Self {
        use rayon::prelude::*;
        let mut classes = vec![0; width * height];
        let mut scanlines = vec![Vec::new(); height];
        classes.par_chunks_mut(width.max(1)).zip(scanlines.par_iter_mut()).enumerate().for_each(|(y, (row, scanlines))| {
            let y_position = y as f32 / scale;
            let scanline = y_position.floor() as usize % 2 == 0;
            for (x, class) in row.iter_mut().enumerate() {
                let x_position = x as f32 / scale;
                let outer = bubble_contains(x_position, y_position, bottom);
                *class = u8::from(outer);
                if rounded_contains(x_position, y_position, [8.0, 8.0, 312.0, bottom - 2.0], 18.0) {
                    *class = if (26.0..294.0).contains(&x_position) && (bottom - 46.0..bottom - 45.0).contains(&y_position) { 3 } else { 2 };
                }
                // The inner tail paints last, exactly as in draw_bubble.
                if triangle_contains(x_position, y_position, [[239.0, bottom - 6.0], [265.0, bottom - 6.0], [252.0, bottom + 14.0]]) {
                    *class = 2;
                }
                if scanline && outer { scanlines.push((y * width + x) as u32); }
            }
        });
        let scanlines = scanlines.into_iter().flatten().collect();
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

    pub fn ambient_active(&self, elapsed: Duration, duration: Duration, seed: u32) -> bool { ambient_active(elapsed, duration, seed, self.lightning) }

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
        let strike = entrance.filter(|elapsed| !elapsed.is_zero() && *elapsed < self.lightning.transition_duration());
        let active = match self.lightning_activity {
            Some(LightningActivity::Holding(elapsed, duration, seed)) => ambient_burst(elapsed, duration, seed, self.lightning).is_some(),
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
        let row_offset = y as f32 - from[1];
        // Rounded along * dy stays between 0 and dy, so the vertical gap to that interval bounds every pixel distance.
        let near = ((row_offset - row_offset.clamp(dy.min(0.0), dy.max(0.0))).abs() - 0.4).max(0.0) / scale;
        let bin = ((near * per_step) as usize).saturating_sub(1).min(STRENGTH_BINS);
        let upper = unit[bin] * reach.intensity;
        for x in left..right {
            let index = (y - rows.start) * width + x;
            if upper <= glow[index] { continue; }
            let along = (((x as f32 - from[0]) * dx + row_offset * dy) / squared).clamp(0.0, 1.0);
            let (offset_x, offset_y) = (x as f32 - from[0] - along * dx, y as f32 - from[1] - along * dy);
            let offset_squared = offset_x * offset_x + offset_y * offset_y;
            if offset_squared > reach_squared { continue; }
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

#[cfg(test)]
fn paint(buffer: &mut [u32], coverage: &mut Option<&mut PaintCoverage>, index: usize, color: u32) {
    buffer[index] = color;
    if let Some(coverage) = coverage { coverage.pixels[index] = true; }
}

impl PaintCoverage {
    fn draw_activity(&self, buffer: &mut [u32], width: usize, height: usize, scale: f32, activity: LightningActivity, lightning: LightningSettings) {
        match activity {
            LightningActivity::Holding(elapsed, duration, seed) => {
                let Some((seed, intensity, buildup)) = ambient_burst(elapsed, duration, seed, lightning) else { return; };
                let mut glow = LightningPaint::new(width, height, lightning);
                let intensity = lightning.pulse_profile.holding(elapsed.as_secs_f32() * 1000.0 * lightning.speed, intensity);
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
        let elapsed = elapsed.mul_f32(lightning.speed);
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
        let milliseconds = elapsed.as_secs_f32() * 1000.0 * lightning.speed;
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

#[cfg(test)]
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
#[path = "../unit_tests/render.rs"]
mod tests;
