//! Opt-in per-stage frame timing for `--benchmark-lightning` and `HERALD_FRAME_LOG`; one relaxed load per stage when off.
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage { Video, Bubble, Text, Scanlines, Portrait, Lightning, Contours, Geometry, Distance, Composite, Canvas, Present }

pub const STAGES: [Stage; 12] = [Stage::Video, Stage::Bubble, Stage::Text, Stage::Scanlines, Stage::Portrait, Stage::Lightning,
    Stage::Contours, Stage::Geometry, Stage::Distance, Stage::Composite, Stage::Canvas, Stage::Present];

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Video => "video", Self::Bubble => "bubble", Self::Text => "text", Self::Scanlines => "scanlines",
            Self::Portrait => "portrait", Self::Lightning => "lightning", Self::Contours => "contours", Self::Geometry => "geometry",
            Self::Distance => "distance", Self::Composite => "composite", Self::Canvas => "canvas", Self::Present => "present",
        }
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static TOTALS: Cell<[f64; STAGES.len()]> = const { Cell::new([0.0; STAGES.len()]) };
}

pub fn enable(enabled: bool) { ENABLED.store(enabled, Ordering::Relaxed); }

pub fn enabled() -> bool { ENABLED.load(Ordering::Relaxed) }

/// Runs `work`, adding its wall time in milliseconds to `stage` on this thread while profiling is on.
#[inline]
pub fn time<R>(stage: Stage, work: impl FnOnce() -> R) -> R {
    if !enabled() { return work(); }
    let start = Instant::now();
    let result = work();
    add(stage, start);
    result
}

/// Adds the time since `start` to `stage`, for spans too long to wrap in a closure.
pub fn add(stage: Stage, start: Instant) {
    let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
    TOTALS.with(|totals| {
        let mut values = totals.get();
        values[stage as usize] += milliseconds;
        totals.set(values);
    });
}

/// Returns and clears this thread's stage totals in milliseconds, in `STAGES` order.
pub fn take() -> [f64; STAGES.len()] { TOTALS.with(|totals| totals.replace([0.0; STAGES.len()])) }

/// Appends one JSON line per presented frame to the file named by an environment variable, for live jitter runs.
pub struct FrameLog {
    file: std::io::BufWriter<std::fs::File>,
    origin: Instant,
    previous: Option<Instant>,
}

impl FrameLog {
    pub fn from_env(variable: &str) -> Option<Self> {
        let path = std::env::var_os(variable)?;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
        enable(true);
        Some(Self { file: std::io::BufWriter::new(file), origin: Instant::now(), previous: None })
    }

    /// `start` is when the frame's work began; `late` is how far past its deadline the loop woke, in milliseconds.
    pub fn record(&mut self, kind: &str, start: Instant, phase: &str, late: f64) {
        use std::io::Write;
        let now = Instant::now();
        let interval = self.previous.map(|previous| start.saturating_duration_since(previous).as_secs_f64() * 1000.0);
        self.previous = Some(start);
        let stages = take();
        let stages: serde_json::Map<_, _> = STAGES.iter().zip(stages).filter(|(_, value)| *value > 0.0)
            .map(|(stage, value)| (stage.name().to_owned(), serde_json::json!(value))).collect();
        let line = serde_json::json!({
            "kind": kind, "at": start.saturating_duration_since(self.origin).as_secs_f64() * 1000.0,
            "interval": interval, "work": now.saturating_duration_since(start).as_secs_f64() * 1000.0,
            "late": late, "phase": phase, "stages": stages,
        });
        let _ = writeln!(self.file, "{line}");
        let _ = self.file.flush();
    }

    /// Starts a new interval series, e.g. when a new announcement begins.
    pub fn reset(&mut self) { self.previous = None; }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_accumulate_only_while_enabled() {
        take();
        time(Stage::Bubble, || std::thread::sleep(std::time::Duration::from_millis(2)));
        assert_eq!(take()[Stage::Bubble as usize], 0.0);
        enable(true);
        time(Stage::Bubble, || std::thread::sleep(std::time::Duration::from_millis(2)));
        enable(false);
        assert!(take()[Stage::Bubble as usize] >= 2.0);
        assert_eq!(take()[Stage::Bubble as usize], 0.0);
    }
}
