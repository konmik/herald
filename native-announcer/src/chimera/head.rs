use std::f64::consts::PI;

use chimera_voice::anatomy::geom::{catmull_rom, rotate_about, v, Vec2};
use chimera_voice::anatomy::human::{GridSample, HumanArtic, HumanFrame, HumanVocalTract, BODY_MALE, NEUTRAL};
use image::{Rgba, RgbaImage};

type Point = [f64; 3];
type Color = [u8; 3];

const BLUE: Color = [79, 182, 242];
const BLUE_MID: Color = [52, 135, 196];
const BLUE_DIM: Color = [31, 95, 140];
const GREEN: Color = [91, 227, 129];
const RED: Color = [255, 93, 64];
const PINK: Color = [255, 115, 207];
const WHITE: Color = [238, 246, 248];
const WIDTH: &[(f64, f64)] = &[(-4.6, 5.3), (0.0, 5.4), (2.5, 5.5), (4.5, 5.25), (6.5, 5.5), (8.5, 6.1), (10.5, 6.6), (12.5, 7.0), (15.0, 7.3), (17.5, 7.65), (19.5, 7.4), (21.0, 6.5), (22.2, 4.6), (23.0, 1.0)];
const PLANE: &[(f64, f64)] = &[(3.4, 5.9), (4.6, 6.2), (5.7, 6.15), (6.5, 6.2), (8.0, 6.35), (9.5, 6.35), (10.6, 6.55), (11.6, 6.75), (12.6, 6.9), (13.9, 7.15), (15.0, 7.45), (16.1, 7.95)];
const FEATURE: &[(f64, f64)] = &[(3.4, 2.0), (4.6, 2.2), (5.7, 2.4), (6.5, 2.6), (10.6, 1.7), (11.0, 1.75), (11.65, 1.2), (12.6, 0.95), (13.9, 0.75), (15.0, 0.9), (16.1, 1.6)];
const WIDEST: &[(f64, f64)] = &[(-4.6, 0.5), (3.0, 0.5), (6.0, 0.42), (12.0, 0.42), (16.0, 0.45), (23.0, 0.45)];
const JAW_BORDER: &[(f64, f64)] = &[(-3.0, 8.5), (-1.6, 8.0), (-0.6, 6.4), (1.2, 4.6), (3.5, 3.6), (5.3, 3.0), (7.3, 3.3), (8.6, 4.4)];
const RAMUS_BACK: &[(f64, f64)] = &[(6.0, -0.6), (7.4, -0.8), (9.6, -1.9), (10.6, -2.5), (12.0, -2.6)];
const BRAIN_WIDTH: &[(f64, f64)] = &[(12.3, 4.0), (13.0, 5.6), (14.5, 6.4), (16.5, 6.9), (18.5, 6.6), (20.0, 5.4), (21.0, 3.6), (21.45, 0.5)];
const TONGUE_WIDTH: &[(f64, f64)] = &[(0.0, 1.5), (0.15, 1.9), (0.35, 2.35), (0.6, 2.45), (0.8, 2.2), (0.92, 1.5), (1.0, 0.55)];
const TONGUE_HEIGHT: &[(f64, f64)] = &[(0.0, 1.3), (0.35, 1.1), (0.7, 0.9), (0.9, 0.55), (1.0, 0.25)];
const UPPER_TEETH: &[[f64; 3]] = &[[0.85, 1.05, 0.7], [0.68, 0.95, 0.65], [0.78, 1.1, 0.8], [0.7, 0.85, 0.9], [0.68, 0.8, 0.9], [1.0, 0.75, 1.1], [0.92, 0.7, 1.05]];
const LOWER_TEETH: &[[f64; 3]] = &[[0.54, 0.9, 0.6], [0.6, 0.92, 0.62], [0.7, 1.0, 0.75], [0.7, 0.85, 0.8], [0.72, 0.8, 0.85], [1.1, 0.72, 1.05], [1.05, 0.7, 1.0]];

#[derive(Clone)]
struct Line {
    points: Vec<Point>,
    color: Color,
    width: f64,
    alpha: f64,
    hot: bool,
    lightning: bool,
}

impl Line {
    fn new(points: Vec<Point>, color: Color, width: f64, alpha: f64) -> Self {
        Self { points, color, width, alpha, hot: false, lightning: false }
    }

    fn hot(mut self) -> Self {
        self.hot = true;
        self
    }

    fn closed(mut self) -> Self {
        if let Some(&first) = self.points.first() {
            self.points.push(first);
        }
        self
    }
}

#[derive(Clone, Copy)]
struct Section {
    back: f64,
    face: f64,
    width: f64,
    widest: f64,
    protrusion: f64,
    feature: f64,
    exponent: f64,
}

impl Section {
    fn front(self, z: f64, bare: bool) -> f64 {
        self.widest + (self.face - self.widest) * (1.0 - (z.abs() / self.width).min(1.0).powf(self.exponent)).powf(1.0 / self.exponent)
            + if bare { 0.0 } else { self.protrusion * bump(z / self.feature) }
    }

    fn side(self, x: f64) -> f64 {
        let (ratio, exponent) = if x >= self.widest {
            ((x - self.widest) / (self.face - self.widest).max(0.001), self.exponent)
        } else {
            ((self.widest - x) / (self.widest - self.back).max(0.001), 2.2)
        };
        self.width * (1.0 - ratio.clamp(0.0, 1.0).powf(exponent)).powf(1.0 / exponent)
    }
}

struct Shape {
    rows: Vec<Option<Section>>,
    bottom: f64,
    top: f64,
}

impl Shape {
    fn new(outline: &[Vec2], brain: bool) -> Self {
        let bottom = outline.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        let top = outline.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
        let mut rows = Vec::new();
        let mut y = bottom;
        while y <= top {
            let mut front = f64::NEG_INFINITY;
            let mut back = f64::INFINITY;
            for pair in outline.windows(2) {
                let [a, b] = [pair[0], pair[1]];
                if (a.y > y) != (b.y > y) {
                    let x = a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y);
                    front = front.max(x);
                    back = back.min(x);
                }
            }
            rows.push(if front > back {
                let face = if !brain && (3.4..=16.1).contains(&y) { front.min(interpolate(PLANE, y)) } else { front };
                Some(Section {
                    back,
                    face,
                    width: interpolate(if brain { BRAIN_WIDTH } else { WIDTH }, y),
                    widest: back + (face - back) * if brain { 0.48 } else { interpolate(WIDEST, y) },
                    protrusion: front - face,
                    feature: if brain { 1.0 } else { interpolate(FEATURE, y) },
                    exponent: if brain { 2.3 } else { 2.2 + 0.4 * smooth(2.5, 3.5, y) * (1.0 - smooth(17.0, 18.5, y)) },
                })
            } else {
                None
            });
            y += 0.08;
        }
        Self { rows, bottom, top }
    }

    fn at(&self, y: f64) -> Option<Section> {
        let u = (y - self.bottom) / 0.08;
        if u < 0.0 || u >= self.rows.len().saturating_sub(1) as f64 {
            return None;
        }
        let i = u as usize;
        match (self.rows[i], self.rows[i + 1]) {
            (Some(a), Some(b)) => {
                let t = u - i as f64;
                let lerp = |p: f64, q: f64| p + (q - p) * t;
                Some(Section { back: lerp(a.back, b.back), face: lerp(a.face, b.face), width: lerp(a.width, b.width), widest: lerp(a.widest, b.widest), protrusion: lerp(a.protrusion, b.protrusion), feature: lerp(a.feature, b.feature), exponent: lerp(a.exponent, b.exponent) })
            }
            (a, b) => a.or(b),
        }
    }
}

pub(super) struct Head {
    image: RgbaImage,
    tract: HumanVocalTract,
    anatomy: HumanFrame,
    shape: Shape,
    fixed: Vec<Line>,
    jaw: Vec<Line>,
    shell: Vec<Vec<Point>>,
    moving: Vec<Line>,
    anatomy_paths: Vec<Vec<[f32; 2]>>,
    eye_rays: Vec<[[f32; 2]; 2]>,
    mouth_discharge: Option<super::MouthDischarge>,
    last_artic: Option<HumanArtic>,
}

impl Head {
    pub(super) fn new(size: u32) -> Self {
        let mut tract = HumanVocalTract::new(BODY_MALE);
        let rest = HumanArtic { jaw: 0.0, lip_ap: 0.0, labiodental: 0.0, ..NEUTRAL };
        let anatomy = tract.compute_new(&rest);
        let (lower, upper) = skin_profile(&tract, &anatomy, &rest);
        let mut outline = lower;
        outline.extend(upper);
        let shape = Shape::new(&outline, false);
        let shell = shell_lines(&shape, &anatomy);
        let (fixed, jaw) = rest_lines(&tract, &shape);
        Self { image: RgbaImage::from_pixel(size.max(1), size.max(1), Rgba([0, 0, 0, 0])), tract, anatomy, shape, fixed, jaw, shell, moving: Vec::new(), anatomy_paths: Vec::new(), eye_rays: Vec::new(), mouth_discharge: None, last_artic: None }
    }

    pub(super) fn draw(&mut self, pose: &super::Pose, elapsed: f64) {
        let time = if elapsed.is_finite() { elapsed.max(0.0) } else { 0.0 };
        if self.last_artic != Some(pose.artic) {
            self.tract.compute(&pose.artic, &mut self.anatomy);
            self.moving = moving_lines(&self.tract, &self.anatomy, &pose.artic, &self.shape, &self.jaw, &self.shell);
            self.last_artic = Some(pose.artic);
        }
        let level = if pose.level.is_finite() { f64::from(pose.level).clamp(0.0, 1.0) } else { 0.0 };
        let pitch = if level > 0.001 && pose.f0.is_finite() && pose.f0 > 0.0 { (pose.f0 / 110.0).log2().clamp(-1.0, 1.0) } else { 0.0 };
        let camera = Camera::new(self.image.width(), time, level, pitch);
        let mut eyes = eye_geometry(time, level, pitch);
        for line in &mut eyes.lines { line.lightning = true; }
        self.eye_rays = eyes.rays.iter().map(|ray| ray.map(|point| {
            let projected = camera.project(point);
            [(projected[0] / f64::from(self.image.width())) as f32, (projected[1] / f64::from(self.image.height())) as f32]
        })).collect();
        let mouth = mouth_geometry(&self.anatomy, self.shape.bottom);
        let project = |point| {
            let projected = camera.project(point);
            [(projected[0] / f64::from(self.image.width())) as f32, (projected[1] / f64::from(self.image.height())) as f32]
        };
        self.mouth_discharge = Some(super::MouthDischarge {
            tract: mouth.tract.into_iter().map(project).collect(), ray: mouth.ray.map(project),
        });
        self.image.pixels_mut().for_each(|p| *p = Rgba([0, 0, 0, 0]));
        self.anatomy_paths.clear();
        for line in self.fixed.iter().chain(self.moving.iter()).chain(eyes.lines.iter()) {
            draw_line(&mut self.image, line, &camera);
            if line.lightning && line.alpha > 0.0 {
                let size = f64::from(self.image.width());
                self.anatomy_paths.push(line.points.iter().map(|point| {
                    let projected = camera.project(*point);
                    [(projected[0] / size) as f32, (projected[1] / size) as f32]
                }).collect());
            }
        }
        for pixel in self.image.pixels_mut() {
            let alpha = u16::from(pixel.0[3]);
            if alpha > 0 {
                for channel in &mut pixel.0[..3] { *channel = (u16::from(*channel) * 255 / alpha).min(255) as u8; }
            }
        }
    }

    pub(super) fn frame(&self) -> &RgbaImage {
        &self.image
    }

    pub(super) fn anatomy_paths(&self) -> &[Vec<[f32; 2]>] {
        &self.anatomy_paths
    }

    pub(super) fn eye_rays(&self) -> &[[[f32; 2]; 2]] {
        &self.eye_rays
    }

    pub(super) fn mouth_discharge(&self) -> Option<&super::MouthDischarge> {
        self.mouth_discharge.as_ref()
    }
}

fn interpolate(table: &[(f64, f64)], x: f64) -> f64 {
    for pair in table.windows(2) {
        if x <= pair[1].0 {
            let t = ((x - pair[0].0) / (pair[1].0 - pair[0].0)).clamp(0.0, 1.0);
            return pair[0].1 + (pair[1].1 - pair[0].1) * t;
        }
    }
    table.last().map_or(0.0, |p| p.1)
}

fn smooth(low: f64, high: f64, x: f64) -> f64 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn bump(x: f64) -> f64 {
    if x.abs() < 1.0 { (1.0 - x * x).powi(2) } else { 0.0 }
}

fn flat(points: &[Vec2], z: f64) -> Vec<Point> {
    points.iter().map(|p| [p.x, p.y, z]).collect()
}

fn curve(points: &[[f64; 2]]) -> Vec<Vec2> {
    catmull_rom(&points.iter().map(|p| v(p[0], p[1])).collect::<Vec<_>>(), 4)
}

fn curve3(points: &[Point]) -> Vec<Point> {
    let mut out = Vec::new();
    for i in 0..points.len().saturating_sub(1) {
        let p0 = points[i.saturating_sub(1)];
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = points[(i + 2).min(points.len() - 1)];
        for j in 0..4 {
            let t = j as f64 / 4.0;
            let mut p = [0.0; 3];
            for d in 0..3 {
                p[d] = 0.5 * (2.0 * p1[d] + (-p0[d] + p2[d]) * t + (2.0 * p0[d] - 5.0 * p1[d] + 4.0 * p2[d] - p3[d]) * t * t + (-p0[d] + 3.0 * p1[d] - 3.0 * p2[d] + p3[d]) * t * t * t);
            }
            out.push(p);
        }
    }
    if let Some(&p) = points.last() { out.push(p); }
    out
}

fn ellipse(center: Point, u: Point, v: Point, count: usize) -> Vec<Point> {
    (0..=count).map(|i| {
        let angle = i as f64 / count as f64 * PI * 2.0;
        [center[0] + u[0] * angle.cos() + v[0] * angle.sin(), center[1] + u[1] * angle.cos() + v[1] * angle.sin(), center[2] + u[2] * angle.cos() + v[2] * angle.sin()]
    }).collect()
}

fn jaw_angle(tract: &HumanVocalTract, jaw: f64) -> f64 {
    let a = tract.p(-2.5, 10.5);
    let b = tract.p(6.95, 7.75);
    -jaw.clamp(-0.2, 3.0) / (a.x - b.x).hypot(a.y - b.y)
}

fn jaw_weight(x: f64, y: f64) -> f64 {
    let border = interpolate(JAW_BORDER, x);
    let back = interpolate(RAMUS_BACK, y);
    (1.0 - smooth(7.6, 9.1, y)) * smooth(border - 1.5, border + 0.15, y) * smooth(back - 0.7, back + 0.9, x)
}

fn skin_profile(tract: &HumanVocalTract, f: &HumanFrame, a: &HumanArtic) -> (Vec<Vec2>, Vec<Vec2>) {
    let condyle = tract.p(-2.5, 10.5);
    let angle = jaw_angle(tract, a.jaw);
    let jaw = |x, y| rotate_about(tract.p(x, y), condyle, angle);
    let mut lower = catmull_rom(&[v(2.3, -4.6), v(2.38, -1.5), v(2.52, 0.4), v(3.0, 1.7), jaw(5.3, 3.0), jaw(7.25, 3.35), jaw(7.95, 4.6), jaw(7.55, 5.7)], 4);
    lower.extend(f.lower_lip.iter().skip(7).rev().copied());
    let mut upper: Vec<Vec2> = f.upper_lip.iter().skip(7).copied().collect();
    let mut face = vec![*upper.last().unwrap_or(&v(7.0, 10.0))];
    face.extend([[7.05, 10.6], [7.75, 10.78], [8.75, 11.05], [9.25, 11.65], [8.95, 12.6], [8.35, 13.9], [7.7, 15.0], [7.95, 16.1], [7.9, 18.1], [6.9, 20.5]].map(|p| v(p[0], p[1])));
    upper.extend(catmull_rom(&face, 4).into_iter().skip(1));
    upper.extend(curve(&[[6.9, 20.5], [3.0, 22.9], [-2.2, 22.7], [-6.9, 19.6], [-8.7, 15.4], [-8.3, 11.9], [-7.4, 8.9], [-6.8, 4.8], [-6.6, 0.0], [-6.6, -4.6]]).into_iter().skip(1));
    (lower, upper)
}

fn shell_lines(shape: &Shape, frame: &HumanFrame) -> Vec<Vec<Point>> {
    let lip_top = frame.upper_lip.last().map_or(10.0, |p| p.y);
    let lip_bottom = frame.lower_lip.last().map_or(6.0, |p| p.y);
    let inside_lips = |p: Point, section: Section| p[1] > lip_bottom && p[1] < lip_top && p[2].abs() < 3.0 && p[0] > section.widest;
    let mut lines = Vec::new();
    let mut y = shape.bottom + 0.25;
    while y < shape.top - 0.2 {
        if let Some(s) = shape.at(y).filter(|s| s.width >= 0.2) {
            let mut half = Vec::new();
            for i in 0..=12 {
                let angle = PI - PI * 0.5 * i as f64 / 12.0;
                half.push([s.widest - (s.widest - s.back) * angle.cos().abs().powf(2.0 / 2.2), y, s.width * angle.sin().powf(2.0 / 2.2)]);
            }
            let zf = (0.6 * s.width).min(3.6);
            let end = (zf / s.width).min(1.0).powf(s.exponent / 2.0).asin();
            for i in 1..=10 {
                let angle = PI * 0.5 + (end - PI * 0.5) * i as f64 / 10.0;
                let z = s.width * angle.sin().powf(2.0 / s.exponent);
                half.push([s.front(z, false), y, z]);
            }
            for i in 1..=16 {
                let z = zf * (1.0 - i as f64 / 16.0);
                half.push([s.front(z, false), y, z]);
            }
            let mut ring = half.clone();
            ring.extend(half[1..half.len() - 1].iter().rev().map(|p| [p[0], p[1], -p[2]]));
            ring.push(ring[0]);
            keep_runs(&mut lines, ring, |p| !inside_lips(p, s));
        }
        y += 1.4;
    }
    for z in [-3.6f64, -2.4, -1.2, 1.2, 2.4, 3.6] {
        let mut points = Vec::new();
        let mut y = shape.bottom + 0.1;
        while y <= shape.top {
            if let Some(s) = shape.at(y).filter(|s| z.abs() <= 0.97 * s.width) {
                let p = [s.front(z, false), y, z];
                if !inside_lips(p, s) { points.push(p); } else if points.len() > 1 { lines.push(std::mem::take(&mut points)); } else { points.clear(); }
            } else if points.len() > 1 { lines.push(std::mem::take(&mut points)); } else { points.clear(); }
            y += 0.3;
        }
        if points.len() > 1 { lines.push(points); }
    }
    for angle in [0.5 * PI, 0.8 * PI] {
        for side in [1.0, -1.0] {
            let mut points = Vec::new();
            let mut y = shape.bottom + 0.1;
            while y <= shape.top {
                if let Some(s) = shape.at(y).filter(|s| s.width >= 0.2) {
                    points.push([s.widest - (s.widest - s.back) * angle.cos().abs().powf(2.0 / 2.2), y, side * s.width * angle.sin().powf(2.0 / 2.2)]);
                }
                y += 0.3;
            }
            lines.push(points);
        }
    }
    lines
}

fn keep_runs(lines: &mut Vec<Vec<Point>>, points: Vec<Point>, keep: impl Fn(Point) -> bool) {
    let mut current = Vec::new();
    for p in points {
        if keep(p) { current.push(p); } else if current.len() > 1 { lines.push(std::mem::take(&mut current)); } else { current.clear(); }
    }
    if current.len() > 1 { lines.push(current); }
}

fn rest_lines(tract: &HumanVocalTract, shape: &Shape) -> (Vec<Line>, Vec<Line>) {
    let mut fixed = Vec::new();
    let mut jaw = Vec::new();
    for side in [1.0, -1.0] {
        let ears: &[(&[[f64; 2]], f64)] = &[
            (&[[-2.55, 13.4], [-2.85, 14.75], [-3.65, 15.45], [-4.6, 15.25], [-5.2, 14.25], [-5.4, 12.8], [-5.05, 11.3], [-4.55, 10.2], [-3.95, 9.55], [-3.35, 9.65], [-3.0, 10.35]], -1.0),
            (&[[-3.15, 14.3], [-3.95, 14.45], [-4.55, 13.5], [-4.6, 12.1], [-4.15, 11.05]], 0.55),
            (&[[-2.7, 11.2], [-2.55, 11.85], [-2.75, 12.35]], 0.35),
        ];
        for &(outline, offset) in ears {
            let points = curve(outline);
            fixed.push(Line::new(points.iter().enumerate().map(|(i, p)| {
                let off = if offset < 0.0 { 0.25 + 1.2 * (PI * i as f64 / (points.len() - 1) as f64).sin().powf(1.5) } else { offset };
                [p.x, p.y, side * (shape.at(p.y).map_or(0.0, |s| s.side(p.x)) + off)]
            }).collect(), BLUE_DIM, 1.05, 0.9));
        }
        let mut nose = Vec::new();
        for i in 0..=22 {
            let y = 14.6 - 3.3 * i as f64 / 22.0;
            if let Some(s) = shape.at(y) {
                let z = side * (0.3 + 1.05 * (14.6 - y) / 3.3);
                nose.push([s.front(z, false), y, z]);
            }
        }
        fixed.push(Line::new(nose, BLUE_MID, 1.0, 0.8));
        fixed.push(Line::new([[7.95, 11.05, 0.95], [7.4, 11.15, 1.45], [7.25, 11.6, 1.55], [7.55, 12.1, 1.35], [8.2, 12.15, 1.0]].map(|p| [p[0], p[1], side * p[2]]).to_vec(), BLUE_MID, 1.0, 0.8));
        fixed.push(Line::new(ellipse([8.05, 10.98, side * 0.62], [0.42, 0.04, 0.0], [0.0, 0.0, 0.26], 16), BLUE_MID, 1.0, 0.9));
    }
    let skull = curve(&[[7.0, 16.2], [7.3, 18.5], [6.0, 20.7], [3.0, 22.1], [-2.0, 22.0], [-6.3, 19.2], [-7.9, 15.3], [-7.4, 12.6], [-4.6, 12.2], [-2.2, 13.3], [1.0, 13.7], [4.1, 14.1], [6.2, 14.9]]);
    let mut brain = curve(&[[6.55, 16.3], [6.75, 18.4], [5.5, 20.1], [2.8, 21.4], [-1.9, 21.3], [-5.8, 18.8], [-7.1, 15.4], [-6.7, 13.1], [-4.4, 12.8], [-2.1, 13.9], [1.0, 14.3], [4.0, 14.7], [5.9, 15.4]]);
    brain.push(brain[0]);
    fixed.push(Line::new(flat(&skull, 0.0), BLUE_DIM, 0.9, 0.6).closed());
    fixed.push(Line::new(flat(&brain, 0.0), BLUE_MID, 1.0, 0.75));
    fixed.push(Line::new(flat(&curve(&[[5.4, 19.4], [2.4, 20.5], [-1.4, 20.4], [-4.8, 18.4]]), 0.0), BLUE_DIM, 0.9, 1.0));
    let brain_shape = Shape::new(&brain, true);
    let brain_point = |s: Section, y: f64, angle: f64| {
        let c = angle.cos();
        let sn = angle.sin();
        let exponent = if c >= 0.0 { s.exponent } else { 2.2 };
        let x = if c >= 0.0 { s.widest + (s.face - s.widest) * c.powf(2.0 / exponent) } else { s.widest - (s.widest - s.back) * (-c).powf(2.0 / exponent) };
        [x, y, sn.signum() * s.width * sn.abs().powf(2.0 / exponent)]
    };
    let mut y = brain_shape.bottom + 0.6;
    while y < brain_shape.top - 0.15 {
        if let Some(s) = brain_shape.at(y) {
            fixed.push(Line::new((0..=48).map(|i| brain_point(s, y, i as f64 / 48.0 * PI * 2.0)).collect(), BLUE_MID, 0.85, 0.32));
        }
        y += 1.5;
    }
    for angle in [0.3 * PI, 0.7 * PI, -0.3 * PI, -0.7 * PI] {
        let mut points = Vec::new();
        let mut y = brain_shape.bottom;
        while y <= brain_shape.top {
            if let Some(s) = brain_shape.at(y) { points.push(brain_point(s, y, angle)); }
            y += 0.3;
        }
        fixed.push(Line::new(points, BLUE_MID, 0.85, 0.32));
    }
    for i in 0..9 {
        let y = -4.4 + i as f64 * 1.75;
        if y > 10.5 { break; }
        let x = -1.2 - 0.025 * y - 2.25;
        let lo = tract.p(x, y + 0.08).y;
        let hi = tract.p(x, y + 1.32).y;
        let cx = tract.p(x + 0.8, 0.0).x;
        for y in [lo, hi] {
            fixed.push(Line::new(ellipse([cx, y, 0.0], [0.8, 0.0, 0.0], [0.0, 0.0, 1.05], 24), BLUE_DIM, 0.9, 0.6));
        }
        for angle in [0.25 * PI, 0.75 * PI, 1.25 * PI, 1.75 * PI] {
            fixed.push(Line::new(vec![[cx + 0.8 * angle.cos(), lo, 1.05 * angle.sin()], [cx + 0.8 * angle.cos(), hi, 1.05 * angle.sin()]], BLUE_DIM, 0.9, 0.6));
        }
        let mid = (lo + hi) / 2.0;
        let ax = cx - 2.15;
        fixed.push(Line::new(ellipse([ax, mid, 0.0], [0.72, 0.0, 0.0], [0.0, 0.0, 0.88], 20), BLUE_DIM, 0.9, 0.6));
        for side in [1.0, -1.0] {
            fixed.push(Line::new(vec![[cx - 0.7, mid, side * 0.55], [ax + 0.45, mid, side * 0.72]], BLUE_DIM, 0.9, 0.6));
            fixed.push(Line::new(vec![[cx - 0.5, mid, side * 0.95], [cx - 0.9, mid - 0.1, side * 2.3]], BLUE_DIM, 0.9, 0.6));
        }
        fixed.push(Line::new(vec![[ax - 0.72, mid, 0.0], [ax - 1.8, mid - 0.55, 0.0]], BLUE_DIM, 0.9, 0.6));
    }
    let nasal = curve(&[[0.3, 11.2], [1.1, 10.25], [2.7, 10.75], [4.6, 10.85], [6.4, 10.65], [7.6, 10.85], [8.45, 11.3], [8.0, 12.2], [6.7, 13.4], [4.4, 13.9], [1.8, 13.6], [0.2, 12.6]]);
    fixed.push(Line::new(flat(&nasal, 0.0), BLUE_MID, 1.0, 1.0).closed());
    for side in [1.0, -1.0] {
        fixed.push(Line::new(flat(&nasal, side * 1.2), BLUE_DIM, 0.9, 0.7).closed());
        for [x, y, rx, ry] in [[3.0, 11.45, 2.1, 0.32], [3.5, 12.45, 1.9, 0.3], [4.0, 13.3, 1.45, 0.22]] {
            fixed.push(Line::new(ellipse([x, y, side * 0.95], [rx, 0.0, 0.0], [0.0, ry, 0.0], 28), BLUE_DIM, 0.9, 0.7));
        }
    }
    let palate: Vec<Vec2> = [[1.0, 9.6], [1.8, 9.92], [2.7, 10.18], [3.6, 10.3], [4.4, 10.25], [5.1, 10.05], [5.7, 9.72], [6.15, 9.36], [6.55, 9.15], [7.0, 9.6]].map(|p| tract.p(p[0], p[1])).to_vec();
    fixed.push(Line::new(flat(&palate, 0.0), BLUE_MID, 1.1, 1.0));
    fixed.push(Line::new(palate.iter().map(|p| [p.x, p.y + 0.55, 0.0]).collect(), BLUE_MID, 1.1, 1.0));
    let up_front = tract.p(7.3, 0.0).x;
    fixed.extend(teeth(up_front, 4.2, 2.65, UPPER_TEETH, true));
    jaw.extend(teeth(tract.p(6.98, 0.0).x, 4.0, 2.5, LOWER_TEETH, false));
    for i in [0, 2, 4, 5, 6, 7] {
        let p = palate[i];
        let u = ((up_front - p.x) / 4.2).max(0.0);
        let zmax = if u >= 1.0 { 2.65 } else { 2.65 * u.powf(1.0 / 1.85) } - 0.45;
        let y = 8.95 + 0.3 * (zmax / 2.65).min(1.0).powi(2);
        if zmax < 0.6 || p.y < y + 0.15 { continue; }
        fixed.push(Line::new((0..=20).map(|j| {
            let z = zmax * (j as f64 / 10.0 - 1.0);
            [p.x, y + (p.y - y) * (1.0 - (z.abs() / zmax).powf(2.4)).max(0.0).powf(0.7), z]
        }).collect(), BLUE_MID, 1.0, 0.6));
    }
    let mandible: &[&[Point]] = &[
        &[[6.4, 4.6, 0.0], [5.6, 4.45, 1.5], [4.5, 4.4, 2.6], [3.2, 4.5, 3.5], [1.8, 4.85, 4.15], [0.4, 5.6, 4.6], [-0.55, 6.6, 4.85], [-0.9, 7.6, 4.9], [-1.45, 8.9, 4.95], [-1.9, 9.7, 5.0], [-2.15, 10.2, 5.0]],
        &[[-1.8, 10.25, 4.95], [-0.9, 9.4, 4.85], [0.15, 10.1, 4.6], [0.6, 8.9, 4.35], [1.1, 7.7, 4.0], [1.9, 7.2, 3.55]],
        &[[1.9, 7.2, 3.55], [3.0, 7.15, 3.15], [4.3, 7.1, 2.6], [5.4, 7.05, 1.85], [6.2, 7.05, 1.05], [6.72, 7.15, 0.0]],
    ];
    for side in [1.0, -1.0] {
        for (index, points) in mandible.iter().enumerate() {
            let mapped: Vec<Point> = points.iter().map(|p| { let xy = tract.p(p[0], p[1]); [xy.x, xy.y, side * p[2]] }).collect();
            jaw.push(Line::new(curve3(&mapped), RED, if index == 2 { 1.0 } else { 1.25 }, if index == 2 { 0.55 } else { 0.9 }));
        }
        let condyle = tract.p(-2.5, 10.5);
        jaw.push(Line::new(ellipse([condyle.x, condyle.y, side * 4.95], [0.45, 0.0, 0.0], [0.0, 0.12, 0.85], 16), RED, 1.25, 0.9));
        for [xa, za, xb, yb, zb] in [[3.0, 3.15, 3.2, 4.5, 3.5], [5.4, 1.85, 5.6, 4.45, 1.5], [1.9, 3.55, 1.8, 4.85, 4.15]] {
            let mapped = [[xa, 7.15, za], [(xa + xb) / 2.0, 5.9, (za + zb) / 2.0 + 0.15], [xb, yb, zb]].map(|p| { let xy = tract.p(p[0], p[1]); [xy.x, xy.y, side * p[2]] });
            jaw.push(Line::new(curve3(&mapped), RED, 1.0, 0.55));
        }
    }
    let chin = [[7.25, 6.75, 0.0], [7.4, 5.4, 0.0], [6.4, 4.6, 0.0], [5.6, 5.0, 0.0], [5.9, 6.2, 0.0], [6.45, 6.55, 0.0]].map(|p| { let xy = tract.p(p[0], p[1]); [xy.x, xy.y, 0.0] });
    jaw.push(Line::new(curve3(&chin), RED, 1.5, 1.0).hot());
    (fixed, jaw)
}

fn cumulative(points: &[Vec2]) -> Vec<f64> {
    let mut lengths = vec![0.0];
    for pair in points.windows(2) {
        lengths.push(lengths.last().copied().unwrap_or(0.0) + (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y));
    }
    lengths
}

fn along(points: &[Vec2], lengths: &[f64], t: f64) -> Vec2 {
    if points.len() < 2 { return points.first().copied().unwrap_or_default(); }
    let distance = t.clamp(0.0, 1.0) * lengths.last().copied().unwrap_or(0.0);
    let mut i = 0;
    while i < points.len() - 2 && lengths[i + 1] < distance { i += 1; }
    let u = (distance - lengths[i]) / (lengths[i + 1] - lengths[i]).max(1e-9);
    v(points[i].x + (points[i + 1].x - points[i].x) * u, points[i].y + (points[i + 1].y - points[i].y) * u)
}

fn teeth(front: f64, depth: f64, half_width: f64, spec: &[[f64; 3]], upper: bool) -> Vec<Line> {
    let arch: Vec<Vec2> = (0..=60).map(|i| { let z = half_width * i as f64 / 60.0; v(front - depth * (z / half_width).powf(1.85), z) }).collect();
    let lengths = cumulative(&arch);
    let length = *lengths.last().unwrap_or(&1.0);
    let scale = 0.97 * length / spec.iter().map(|t| t[0]).sum::<f64>();
    let mut lines = Vec::new();
    for side in [1.0, -1.0] {
        let mut start = 0.0;
        for &[width, height, thickness] in spec {
            let w = width * scale;
            let t = (start + w / 2.0) / length;
            start += w;
            let center = along(&arch, &lengths, t);
            let after = along(&arch, &lengths, t + 0.01);
            let before = along(&arch, &lengths, t - 0.01);
            let length = (after.x - before.x).hypot(after.y - before.y).max(1e-9);
            let tx = (after.x - before.x) / length;
            let tz = (after.y - before.y) / length;
            let y = if upper { 7.95 + 0.3 * (center.y / half_width).powi(2) } else { 7.75 + 0.45 * (center.y / half_width).powf(1.5) };
            let point = |u: f64, v: f64, inward: f64| [center.x + tx * u - tz * inward, y + if upper { v } else { -v }, side * (center.y + tz * u + tx * inward)];
            lines.push(Line::new([[-0.5, 0.0], [-0.5, 0.55], [-0.38, 1.0], [0.0, 1.08], [0.38, 1.0], [0.5, 0.55], [0.5, 0.0]].map(|p| point(p[0] * w, p[1] * height, 0.0)).to_vec(), BLUE, 1.0, 0.8).closed());
            lines.push(Line::new([[-0.45, 0.1], [0.45, 0.1], [0.45, 0.9], [-0.45, 0.9]].map(|p| point(p[0] * w, 0.0, p[1] * thickness)).to_vec(), BLUE, 1.0, 0.8).closed());
        }
    }
    lines
}

fn moving_lines(tract: &HumanVocalTract, frame: &HumanFrame, artic: &HumanArtic, shape: &Shape, jaw: &[Line], shell: &[Vec<Point>]) -> Vec<Line> {
    let condyle = tract.p(-2.5, 10.5);
    let angle = jaw_angle(tract, artic.jaw);
    let rotate = |p: Point, weight: f64| {
        let xy = rotate_about(v(p[0], p[1]), condyle, angle * weight);
        [xy.x, xy.y, p[2]]
    };
    let mut lines: Vec<Line> = jaw.iter().map(|line| Line { points: line.points.iter().map(|&p| rotate(p, 1.0)).collect(), ..line.clone() }).collect();
    lines.extend(shell.iter().map(|points| Line::new(points.iter().map(|&p| rotate(p, jaw_weight(p[0], p[1]))).collect(), BLUE_DIM, 0.85, 0.42)));
    let (lower, upper) = skin_profile(tract, frame, artic);
    lines.push(Line::new(flat(&lower, 0.0), BLUE, 1.5, 1.0));
    lines.push(Line::new(flat(&upper, 0.0), BLUE, 1.5, 1.0));
    lines.push(Line::new(frame.outer.iter().filter(|p| p.y > frame.glottis.y || p.x > 0.0).map(|p| [p.x, p.y, 0.0]).collect(), BLUE, 1.4, 1.0).hot());
    lines.push(Line::new(flat(&frame.inner, 0.0), BLUE, 1.3, 0.9));
    lines.push(Line::new(flat(&frame.tongue, 0.0), GREEN, 1.6, 1.0).hot());
    lines.push(Line::new(flat(&frame.upper_lip, 0.0), PINK, 1.5, 1.0).hot());
    lines.push(Line::new(flat(&frame.lower_lip, 0.0), PINK, 1.5, 1.0).hot());
    lines.push(Line::new(flat(&frame.velum, 0.0), WHITE, 1.25, 0.92));
    lips(&mut lines, shape, frame, artic, condyle, angle);
    tongue(&mut lines, frame, artic);
    airway(&mut lines, frame);
    velum(&mut lines, frame);
    let throat_start = lines.len();
    larynx(&mut lines, tract, frame, artic, shape.bottom);
    for line in &mut lines[throat_start..] { line.lightning = true; }
    lines
}

fn lips(lines: &mut Vec<Line>, head: &Shape, frame: &HumanFrame, artic: &HumanArtic, condyle: Vec2, angle: f64) {
    if frame.upper_lip.len() <= 6 || frame.lower_lip.len() <= 6 { return; }
    let up = &frame.upper_lip[6..];
    let low: Vec<Vec2> = frame.lower_lip[6..].iter().map(|&p| rotate_about(p, condyle, -angle)).collect();
    let up_length = cumulative(up);
    let low_length = cumulative(&low);
    let width = ((1.1 + (BODY_MALE.lip_width - 1.1) * artic.lip_width.clamp(0.0, 1.0)) / 2.0).max(0.3);
    let base = |y, z| head.at(y).map_or(0.0, |s| s.front(z, true));
    let center_y = (up[0].y + low[0].y) / 2.0;
    let up_bulge = up[0].x - base(up[0].y, 0.0);
    let low_bulge = low[0].x - base(low[0].y, 0.0);
    let mean_bulge = (up_bulge + low_bulge) / 2.0;
    let point = |upper: bool, t: f64, z: f64| {
        let span_z = (1.0 - (z / width).powi(2)).max(0.0).sqrt();
        let (outline, lengths) = if upper { (up, up_length.as_slice()) } else { (low.as_slice(), low_length.as_slice()) };
        let p = along(outline, lengths, t);
        let edge = outline[0];
        let far = outline[outline.len() - 1].y;
        let edge_y = center_y + (edge.y - center_y) * span_z;
        let span = far - edge.y;
        let y = edge_y + (p.y - edge.y) * (far - edge_y) / if span.abs() < 1e-4 { 1e-4 } else { span };
        let bulge = p.x - base(p.y, 0.0) + (mean_bulge - if upper { up_bulge } else { low_bulge }) * (1.0 - span_z) * (1.0 - t);
        let x = base(y, z) + bulge * (1.0 - (z / 3.0).powi(2)).max(0.0).powf(1.5);
        let natural = if upper { 0.5 * (1.0 - span_z) * (1.0 - t) } else { 0.5 * (1.0 + span_z) + 0.5 * (1.0 - span_z) * t };
        let weight = natural + (jaw_weight(x, y) - natural) * smooth(width, 3.0, z.abs());
        let rotated = rotate_about(v(x, y), condyle, angle * weight);
        [rotated.x, rotated.y, z]
    };
    for upper in [true, false] {
        for t in [0.66, 0.88] {
            lines.push(Line::new((0..=24).map(|i| point(upper, t, -3.0 + i as f64 / 4.0)).collect(), BLUE_DIM, 0.85, 0.42));
        }
        for z in [-2.4, -1.5, 1.5, 2.4] {
            lines.push(Line::new((0..=8).map(|i| point(upper, 0.45 + 0.55 * i as f64 / 8.0, z)).collect(), BLUE_DIM, 0.85, 0.42));
        }
        let outer_width = (width + 0.3).min(3.0);
        let border = |z: f64| {
            let s = (1.0 - (z / outer_width).powi(2)).max(0.0).sqrt();
            let bow = if upper { -0.07 * (-(z / 0.32).powi(2)).exp() + 0.05 * (-((z.abs() - 0.55) / 0.3).powi(2)).exp() } else { 0.0 };
            ((if upper { 0.42 } else { 0.45 }) * s + bow * s).max(0.0)
        };
        for (multiplier, width, alpha) in [(1.0, 1.3, 0.95), (0.45, 1.0, 0.45)] {
            lines.push(Line::new((0..=28).map(|i| { let z = -outer_width + 2.0 * outer_width * i as f64 / 28.0; point(upper, multiplier * border(z), z) }).collect(), PINK, width, alpha));
        }
    }
    for side in [1.0, -1.0] {
        lines.push(Line::new((0..=6).map(|i| point(true, 0.0, side * (width + (3.0 - width) * i as f64 / 6.0))).collect(), PINK, 1.0, 0.6));
    }
    let mut opening: Vec<Point> = (0..=24).map(|i| point(true, 0.0, -width + 2.0 * width * i as f64 / 24.0)).collect();
    if frame.upper_lip[6].y - frame.lower_lip[6].y > 0.02 {
        opening.extend((1..24).rev().map(|i| point(false, 0.0, -width + 2.0 * width * i as f64 / 24.0)));
        opening.push(opening[0]);
    }
    lines.push(Line::new(opening, PINK, 2.1, 1.0).hot());
}

fn tongue(lines: &mut Vec<Line>, frame: &HumanFrame, artic: &HumanArtic) {
    let tip_index = frame.tongue.iter().enumerate().min_by(|(_, a), (_, b)| {
        (a.x - frame.tip.x).hypot(a.y - frame.tip.y).total_cmp(&(b.x - frame.tip.x).hypot(b.y - frame.tip.y))
    }).map_or(0, |(i, _)| i);
    if tip_index < 1 { return; }
    let surface = &frame.tongue[..=tip_index];
    let lengths = cumulative(surface);
    let stations: Vec<Vec2> = (0..22).map(|i| along(surface, &lengths, i as f64 / 21.0)).collect();
    let point = |index: usize, angle: f64| {
        let before = stations[index.saturating_sub(1)];
        let after = stations[(index + 1).min(21)];
        let length = (after.x - before.x).hypot(after.y - before.y).max(1e-9);
        let nx = -(after.y - before.y) / length;
        let ny = (after.x - before.x) / length;
        let fraction = index as f64 / 21.0;
        let height = interpolate(TONGUE_HEIGHT, fraction) + 0.8 * artic.lateral.clamp(0.0, 1.0) * smooth(0.45, 0.8, fraction);
        let drop = height * (1.0 - angle.cos());
        [stations[index].x - nx * drop, stations[index].y - ny * drop, interpolate(TONGUE_WIDTH, fraction) * angle.sin()]
    };
    for index in (1..21).step_by(2) {
        lines.push(Line::new((0..=12).map(|i| point(index, -PI / 2.0 + PI * i as f64 / 12.0)).collect(), GREEN, 1.0, 0.5));
    }
    for angle in [-PI / 2.0, -PI / 4.0, PI / 4.0, PI / 2.0] {
        lines.push(Line::new((0..22).map(|i| point(i, angle)).collect(), GREEN, if angle.abs() > 1.0 { 1.25 } else { 1.0 }, if angle.abs() > 1.0 { 0.8 } else { 0.45 }));
    }
}

fn airway_center(sample: &GridSample) -> Point {
    let dx = sample.outer.x - sample.inner.x;
    let dy = sample.outer.y - sample.inner.y;
    let length = dx.hypot(dy);
    let half = if length > 0.0 { sample.d / (2.0 * length) } else { 0.0 };
    [sample.outer.x - dx * half, sample.outer.y - dy * half, 0.0]
}

struct MouthGeometry {
    tract: Vec<Point>,
    ray: [Point; 2],
}

fn mouth_geometry(frame: &HumanFrame, bottom: f64) -> MouthGeometry {
    let mut tract = vec![[frame.glottis.x + 0.08, bottom, 0.0], [frame.glottis.x, frame.glottis.y, 0.0]];
    tract.extend(frame.grid.iter().map(airway_center));
    let aperture = |offset: usize| {
        let upper = frame.outer[frame.outer.len() - 1 - offset];
        let lower = frame.inner[frame.inner.len() - 1 - offset];
        [(upper.x + lower.x) * 0.5, (upper.y + lower.y) * 0.5, 0.0]
    };
    let mouth = aperture(0);
    let before = aperture(1);
    let direction = normalize([mouth[0] - before[0], mouth[1] - before[1], 0.0]);
    tract.push(mouth);
    MouthGeometry { tract, ray: [mouth, std::array::from_fn(|axis| mouth[axis] + direction[axis] * 5.0)] }
}

fn airway(lines: &mut Vec<Line>, frame: &HumanFrame) {
    let mut sides = [Vec::new(), Vec::new()];
    let mut direction = [-1.0, 0.0];
    for (index, sample) in frame.grid.iter().enumerate() {
        let dx = sample.outer.x - sample.inner.x;
        let dy = sample.outer.y - sample.inner.y;
        let length = dx.hypot(dy);
        if length > 1e-4 { direction = [dx / length, dy / length]; }
        let half = sample.d / 2.0;
        let [cx, cy, _] = airway_center(sample);
        let width = if sample.d > 0.01 { (2.0 * sample.area / (PI * sample.d)).min(3.5) } else { 0.0 };
        if index % 2 == 0 {
            lines.push(Line::new(ellipse([cx, cy, 0.0], [direction[0] * half, direction[1] * half, 0.0], [0.0, 0.0, width], 24), BLUE_DIM, 0.85, 0.37));
        }
        sides[0].push([cx, cy, width]);
        sides[1].push([cx, cy, -width]);
    }
    for side in sides { lines.push(Line::new(side, BLUE, 1.0, 0.55)); }
    if frame.epiglottis.len() >= 2 {
        let base = frame.epiglottis[0];
        let tip = frame.epiglottis[1];
        let dx = tip.x - base.x;
        let dy = tip.y - base.y;
        let length = dx.hypot(dy).max(1e-9);
        let mut leaf = Vec::new();
        for side in [1.0, -1.0] {
            for i in 0..=10 {
                let u = if side > 0.0 { i as f64 / 10.0 } else { 1.0 - i as f64 / 10.0 };
                let width = 0.22 + 0.9 * (u * PI / 2.0).sin().powf(1.3);
                let curl = 0.28 * width / 1.12;
                leaf.push([base.x + dx * u - dy / length * curl, base.y + dy * u + dx / length * curl, side * width]);
            }
        }
        lines.push(Line::new(leaf, BLUE, 1.1, 0.75).closed());
        lines.push(Line::new(vec![[base.x, base.y, 0.0], [tip.x, tip.y, 0.0]], BLUE, 2.1, 0.85));
    }
}

fn velum(lines: &mut Vec<Line>, frame: &HumanFrame) {
    if frame.velum.len() < 4 { return; }
    let count = frame.velum.len() / 2 - 1;
    let point = |i: usize, u: f64| {
        let a = frame.velum[i];
        let b = frame.velum[frame.velum.len() - 1 - i];
        let length = (a.x - b.x).hypot(a.y - b.y).max(1e-9);
        let fraction = i as f64 / count as f64;
        let offset = 0.12 * (1.0 - u * u) - 0.45 * fraction * u * u;
        [(a.x + b.x) / 2.0 + (a.x - b.x) / length * offset, (a.y + b.y) / 2.0 + (a.y - b.y) / length * offset, (1.95 - 1.5 * fraction.powf(1.5)) * u]
    };
    for index in [1, 3, 5, 7, 9].into_iter().filter(|&i| i <= count) {
        lines.push(Line::new((0..=12).map(|i| point(index, -1.0 + i as f64 / 6.0)).collect(), WHITE, 1.0, 0.5));
    }
    for u in [-1.0, 1.0] { lines.push(Line::new((0..=count).map(|i| point(i, u)).collect(), WHITE, 1.0, 0.65)); }
}

fn larynx(lines: &mut Vec<Line>, tract: &HumanVocalTract, frame: &HumanFrame, artic: &HumanArtic, bottom: f64) {
    let glottis = frame.glottis;
    let at = |x, y, z| [glottis.x + x, glottis.y + y, z];
    for side in [1.0, -1.0] {
        lines.push(Line::new([[1.45, 1.75, 0.0], [0.75, 2.0, side * 1.2], [-0.5, 1.95, side * 1.9], [-0.6, -0.6, side * 1.85], [0.25, -0.85, side * 1.5], [1.05, -0.8, side * 0.55], [1.2, -0.75, 0.0], [1.6, 0.05, 0.0], [1.9, 1.15, 0.0], [1.45, 1.75, 0.0]].map(|p| at(p[0], p[1], p[2])).to_vec(), BLUE_MID, 1.0, 0.85));
        lines.push(Line::new(vec![at(-0.5, 1.95, side * 1.9), at(-0.7, 2.85, side * 1.85)], BLUE_MID, 1.0, 0.85));
        lines.push(Line::new(vec![at(0.72, 0.1, 0.0), at(-0.55, 0.1, side * 0.06)], WHITE, 1.3, 0.9).hot());
        lines.push(Line::new(vec![at(0.72, 0.0, 0.0), at(-0.6, 0.0, side * 0.62)], BLUE_MID, 1.0, 0.9));
    }
    for (y, rise) in [(-1.75, 0.15), (-1.4, 1.1)] {
        lines.push(Line::new((0..=28).map(|i| { let angle = i as f64 / 28.0 * 2.0 * PI; at(0.1 + angle.cos(), y + rise * (1.0 - angle.cos()) / 2.0, 1.1 * angle.sin()) }).collect(), BLUE_MID, 1.0, 0.85));
    }
    let top = glottis.y - 2.1;
    let cx = glottis.x + 0.08;
    let mut y = top;
    while y > bottom {
        lines.push(Line::new((0..=20).map(|i| { let angle = -0.8 * PI + 1.6 * PI * i as f64 / 20.0; [cx + 0.82 * angle.cos(), y, 0.95 * angle.sin()] }).collect(), BLUE_DIM, 0.9, 0.85));
        y -= 0.5;
    }
    for side in [1.0, -1.0] {
        lines.push(Line::new(vec![[cx, top, side * 0.95], [cx, bottom, side * 0.95]], BLUE_DIM, 0.9, 0.85));
        lines.push(Line::new(vec![[cx + side * 0.82, glottis.y, 0.0], [cx + side * 0.82, bottom, 0.0]], BLUE_MID, 1.0, 1.0));
    }
    let hy = tract.p(0.0, 2.9).y + 0.6 * artic.larynx;
    let hx = tract.p(1.6, 0.0).x;
    for offset in [-0.16, 0.16] {
        lines.push(Line::new((0..=20).map(|i| { let u = -1.0 + i as f64 / 10.0; [hx + 0.35 - 0.65 * u * u, hy + offset, 1.15 * u] }).collect(), BLUE_MID, 1.0, 0.85));
        for side in [1.0, -1.0] {
            lines.push(Line::new(vec![[hx - 0.3, hy + offset, side * 1.15], [hx - 2.8, hy + 0.7 + offset, side * 2.05]], BLUE_MID, 1.0, 0.85));
        }
    }
}

struct EyeGeometry {
    lines: Vec<Line>,
    rays: Vec<[Point; 2]>,
}

fn eye_geometry(time: f64, level: f64, pitch: f64) -> EyeGeometry {
    let blink_phase = time.rem_euclid(4.7);
    let lid = if (3.9..4.12).contains(&blink_phase) { ((blink_phase - 3.9) / 0.22 * PI).sin().powi(2) } else { 0.0 };
    let gaze_yaw = 0.13 * (time * 0.43).sin();
    let gaze_pitch = 0.055 * (time * 0.67).sin() + 0.07 * pitch * level;
    let direction = [gaze_pitch.cos() * gaze_yaw.cos(), gaze_pitch.sin(), gaze_pitch.cos() * gaze_yaw.sin()];
    let basis = |normal: Point| {
        let u = normalize([normal[2], 0.0, -normal[0]]);
        (u, [normal[1] * u[2] - normal[2] * u[1], normal[2] * u[0] - normal[0] * u[2], normal[0] * u[1] - normal[1] * u[0]])
    };
    let scale = |point: Point, amount: f64| [point[0] * amount, point[1] * amount, point[2] * amount];
    let mut lines = Vec::new();
    let mut rays = Vec::new();
    for side in [1.0, -1.0] {
        let center = [6.15, 15.35, side * 3.15];
        let radius = 1.05;
        let to_camera = normalize([45.0 - center[0], 25.0 - center[1], -63.0 - center[2]]);
        let (u, v) = basis(to_camera);
        lines.push(Line::new(ellipse(center, scale(u, radius), scale(v, radius), 36), BLUE_MID, 1.0, 0.35));
        let (iris_u, iris_v) = basis(direction);
        let seen = 1.0 - smooth(0.4, 0.9, lid);
        let center_at = |offset| [center[0] + direction[0] * radius * offset, center[1] + direction[1] * radius * offset, center[2] + direction[2] * radius * offset];
        let pupil = center_at(0.97);
        if seen > 0.0 { rays.push([pupil, std::array::from_fn(|axis| pupil[axis] + direction[axis] * 5.0)]); }
        for (iris, size, alpha, count) in [(center_at(0.86), 0.5, 0.9, 24), (pupil, 0.2, 0.7, 14)] {
            lines.push(Line::new(ellipse(iris, scale(iris_u, radius * size), scale(iris_v, radius * size), count), BLUE, 1.0, alpha * seen));
        }
        for upper in [true, false] {
            lines.push(Line::new((0..=16).map(|i| {
                let sv = -1.0 + i as f64 / 8.0;
                let az = side * sv * 0.95;
                let lower = -0.36 * (1.0 - sv * sv) - 0.06 * sv * side;
                let elevation = if upper { ((0.5 + 0.2 * pitch.max(0.0) * level) * (1.0 - sv * sv) + 0.06 * sv * side) * (1.0 - lid) + (lower + 0.03) * lid } else { lower };
                [center[0] + 1.04 * radius * elevation.cos() * az.cos(), center[1] + 1.04 * radius * elevation.sin(), center[2] + 1.04 * radius * elevation.cos() * az.sin()]
            }).collect(), BLUE, if upper { 1.25 } else { 1.05 }, if upper { 0.95 } else { 0.75 }));
        }
        lines.push(Line::new((0..=12).map(|i| {
            let sv = -1.0 + i as f64 / 6.0;
            let az = side * (0.15 + sv * 0.62);
            let elevation = 0.82 + 0.12 * (1.0 - sv * sv) - 0.06 * sv + 0.3 * pitch * level - 0.28 * (-pitch).max(0.0) * level * (1.0 - sv) / 2.0;
            [center[0] + 1.32 * radius * elevation.cos() * az.cos(), center[1] + 1.32 * radius * elevation.sin(), center[2] + 1.32 * radius * elevation.cos() * az.sin()]
        }).collect(), BLUE_MID, 1.3, 0.8));
    }
    EyeGeometry { lines, rays }
}

fn normalize(p: Point) -> Point {
    let length = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt().max(1e-9);
    [p[0] / length, p[1] / length, p[2] / length]
}

struct Camera {
    right: Point,
    up: Point,
    forward: Point,
    distance: f64,
    focal: f64,
    center: f64,
    nod: f64,
    tilt: f64,
}

impl Camera {
    fn new(size: u32, time: f64, level: f64, pitch: f64) -> Self {
        let azimuth = PI - 0.62 + 0.075 * (time * 0.70).sin();
        let elevation: f64 = 0.2;
        let tangent = (12.0 * PI / 180.0).tan();
        Self {
            right: [azimuth.cos(), 0.0, -azimuth.sin()],
            up: [-azimuth.sin() * elevation.sin(), elevation.cos(), -azimuth.cos() * elevation.sin()],
            forward: [azimuth.sin() * elevation.cos(), elevation.sin(), azimuth.cos() * elevation.cos()],
            distance: 1.1 * 15.2 / tangent,
            focal: size as f64 / (2.0 * tangent),
            center: size as f64 / 2.0,
            nod: 0.035 * (time * 1.10).sin() + pitch * level * 0.045,
            tilt: 0.022 * (time * 0.65).sin(),
        }
    }

    fn project(&self, point: Point) -> Point {
        let [x, y, z] = [point[0] + 2.4, point[1] - 1.75, point[2]];
        let tilted_y = y * self.tilt.cos() - z * self.tilt.sin();
        let tilted_z = y * self.tilt.sin() + z * self.tilt.cos();
        let p = [x * self.nod.cos() - tilted_y * self.nod.sin() - 2.7, x * self.nod.sin() + tilted_y * self.nod.cos() - 7.4, tilted_z];
        let dot = |axis: Point| p[0] * axis[0] + p[1] * axis[1] + p[2] * axis[2];
        let depth = self.distance - dot(self.forward);
        [self.center + self.focal * dot(self.right) / depth, self.center - self.focal * dot(self.up) / depth, 1.0 - smooth(self.distance - 3.0, self.distance + 30.0, depth)]
    }
}

fn beam_width(width: f64) -> f64 {
    [0.6_f64, 0.85, 1.05, 1.3, 1.6, 2.1].into_iter().fold(0.6, |best, candidate| {
        if (candidate - width).abs() < (best - width).abs() { candidate } else { best }
    })
}

fn draw_line(image: &mut RgbaImage, line: &Line, camera: &Camera) {
    if line.points.len() < 2 || line.alpha <= 0.0 { return; }
    let size_scale = image.width() as f64 / f64::from(super::Head::LOGICAL_SIZE);
    let width = beam_width(line.width) * size_scale;
    let radius = width * 0.5;
    let hot_radius = beam_width(line.width * 0.42) * size_scale * 0.5;
    let glow = (width * 1.8).max(1.35 * size_scale);
    let reach = glow.max(radius + 0.5);
    let mut before = camera.project(line.points[0]);
    for &point in &line.points[1..] {
        let after = camera.project(point);
        if before.iter().chain(after.iter()).all(|value| value.is_finite()) {
            let x0 = (before[0].min(after[0]) - reach).floor().max(0.0) as u32;
            let x1 = (before[0].max(after[0]) + reach).ceil().clamp(0.0, image.width().saturating_sub(1) as f64) as u32;
            let y0 = (before[1].min(after[1]) - reach).floor().max(0.0) as u32;
            let y1 = (before[1].max(after[1]) + reach).ceil().clamp(0.0, image.height().saturating_sub(1) as f64) as u32;
            let dx = after[0] - before[0];
            let dy = after[1] - before[1];
            let squared_length = (dx * dx + dy * dy).max(1e-9);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let px = x as f64 + 0.5 - before[0];
                    let py = y as f64 + 0.5 - before[1];
                    let t = ((px * dx + py * dy) / squared_length).clamp(0.0, 1.0);
                    let distance = (px - t * dx).hypot(py - t * dy);
                    if distance > reach { continue; }
                    let core = (radius + 0.5 - distance).clamp(0.0, width.min(1.0));
                    let halo = 0.24 * (1.0 - distance / glow).max(0.0).powi(2);
                    let opacity = core.max(halo) * line.alpha * (before[2] + (after[2] - before[2]) * t);
                    let hot = if line.hot {
                        (hot_radius + 0.5 - distance).clamp(0.0, (hot_radius * 2.0).min(1.0))
                            * line.alpha * (before[2] + (after[2] - before[2]) * t)
                    } else { 0.0 };
                    let pixel = image.get_pixel_mut(x, y);
                    pixel.0[3] = pixel.0[3].max((opacity.clamp(0.0, 1.0) * 255.0) as u8);
                    for (channel, &color) in pixel.0[..3].iter_mut().zip(line.color.iter()) {
                        let color = f64::from(color);
                        let value = (color * opacity).max((color + (255.0 - color) * 0.55) * hot);
                        *channel = (*channel).max(value.clamp(0.0, 255.0) as u8);
                    }
                }
            }
        }
        before = after;
    }
}

#[cfg(test)]
#[path = "../../unit_tests/chimera/head.rs"]
mod tests;
