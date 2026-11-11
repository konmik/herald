use chimera_voice::anatomy::human::{HumanArtic, NEUTRAL};
use chimera_voice::engine::{Engine, MAX_SCHEDULE};
use chimera_voice::phonetics::english::EnglishG2P;
use chimera_voice::phonetics::planner::{plan_phrases, PhraseInput, PlanOptions};
use chimera_voice::voice::human_voice::VoiceTelemetry;
use image::RgbaImage;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

mod head;

pub const SAMPLE_RATE: u32 = 48_000;
pub const POSE_HOP: usize = 800;
const MAX_TEXT_BYTES: usize = 8_000;
const MAX_DURATION: f64 = 180.0;

#[derive(Clone)]
pub(super) struct Pose {
    pub(super) artic: HumanArtic,
    pub(super) f0: f64,
    pub(super) level: f32,
}

impl Pose {
    fn neutral() -> Self {
        Self {
            artic: HumanArtic { jaw: 0.0, lip_ap: 0.0, ..NEUTRAL },
            f0: 110.0,
            level: 0.0,
        }
    }
}

pub struct Utterance {
    pub samples: Vec<i16>,
    poses: Vec<Pose>,
}

impl Utterance {
    pub fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    fn pose(&self, position: u64) -> Pose {
        if position == 0 || position >= self.samples.len() as u64 {
            Pose::neutral()
        } else {
            let index = position as usize / POSE_HOP;
            let start = index * POSE_HOP;
            let count = POSE_HOP.min(self.samples.len() - start);
            let fraction = (position as usize - start) as f64 / count as f64;
            let before = &self.poses[index];
            let after = &self.poses[index + 1];
            let mut artic = before.artic.to_array();
            for (value, next) in artic.iter_mut().zip(after.artic.to_array()) {
                *value += (next - *value) * fraction;
            }
            Pose {
                artic: HumanArtic::from_array(&artic),
                f0: before.f0 + (after.f0 - before.f0) * fraction,
                level: before.level + (after.level - before.level) * fraction as f32,
            }
        }
    }
}

pub struct Playback {
    pub utterance: Arc<Utterance>,
    pub position: Arc<AtomicU64>,
    stopped: AtomicBool,
}

impl Playback {
    pub fn new(utterance: Arc<Utterance>) -> Self {
        Self {
            utterance,
            position: Arc::new(AtomicU64::new(0)),
            stopped: AtomicBool::new(false),
        }
    }

    pub fn sample_position(&self) -> u64 {
        self.position.load(Ordering::Acquire).min(self.utterance.samples.len() as u64)
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }
}

fn dictionary() -> Result<&'static EnglishG2P, String> {
    static DICTIONARY: OnceLock<Result<EnglishG2P, String>> = OnceLock::new();
    DICTIONARY.get_or_init(|| {
        let mut text = String::new();
        flate2::read::GzDecoder::new(&include_bytes!("../chimera-data/cmudict.txt.gz")[..])
            .read_to_string(&mut text)
            .map_err(|error| format!("Could not read the CHIMERA dictionary: {error}"))?;
        let mut dictionary = EnglishG2P::new();
        dictionary.load_dict(&text);
        Ok(dictionary)
    }).as_ref().map_err(Clone::clone)
}

pub fn prepare(text: &str, cancelled: &AtomicBool) -> Result<Arc<Utterance>, String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("CHIMERA speech was cancelled".into());
    }
    if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
        return Err("CHIMERA needs between 1 and 8000 bytes of speech text".into());
    }
    let phrases: Vec<_> = dictionary()?.to_phrases(text).into_iter().map(|phrase| PhraseInput {
        segments: phrase.segments,
        intonation: Some(phrase.intonation),
        pause: Some(phrase.pause),
    }).collect();
    if phrases.is_empty() {
        return Err("CHIMERA could not find any words to speak".into());
    }
    let plan = plan_phrases(&phrases, &PlanOptions {
        t0: 0.03,
        f0: Some(110.0),
        ..Default::default()
    });
    if plan.gestures.len() > MAX_SCHEDULE {
        return Err("CHIMERA speech exceeds the engine's gesture limit".into());
    }
    let duration = plan.end + 0.3;
    if !duration.is_finite() || duration <= 0.0 || duration > MAX_DURATION {
        return Err("CHIMERA speech must fit within three minutes".into());
    }
    if cancelled.load(Ordering::Acquire) {
        return Err("CHIMERA speech was cancelled".into());
    }
    let sample_count = (duration * SAMPLE_RATE as f64).round() as usize;
    let mut engine = Engine::new(SAMPLE_RATE as f64);
    engine.schedule(&plan.gestures, 0.0, true);
    let mut telemetry = VoiceTelemetry::default();
    let mut samples = Vec::with_capacity(sample_count);
    let mut poses = Vec::with_capacity(sample_count / POSE_HOP + 2);
    poses.push(Pose::neutral());
    let mut block = [0.0f32; POSE_HOP];
    while samples.len() < sample_count {
        if cancelled.load(Ordering::Acquire) {
            return Err("CHIMERA speech was cancelled".into());
        }
        let count = (sample_count - samples.len()).min(POSE_HOP);
        engine.render(&mut block[..count]);
        for value in &block[..count] {
            if !value.is_finite() {
                return Err("CHIMERA produced an invalid audio sample".into());
            }
            samples.push((value.clamp(-1.0, 1.0) * 0.8 * i16::MAX as f32).round() as i16);
        }
        engine.telemetry(&mut telemetry);
        poses.push(Pose {
            artic: engine.voice().artic.x,
            f0: telemetry.f0,
            level: telemetry.out_rms as f32,
        });
    }
    if cancelled.load(Ordering::Acquire) {
        return Err("CHIMERA speech was cancelled".into());
    }
    Ok(Arc::new(Utterance { samples, poses }))
}

pub struct Head {
    renderer: head::Head,
    playback: Option<Arc<Playback>>,
    capture_start: Option<Duration>,
    frames: u64,
    sample_position: u64,
}

impl Head {
    pub const LOGICAL_SIZE: f32 = 336.0;

    pub fn new() -> Self {
        let mut renderer = head::Head::new(384);
        renderer.draw(&Pose::neutral(), 0.0);
        Self { renderer, playback: None, capture_start: None, frames: 0, sample_position: 0 }
    }

    pub fn attach(&mut self, playback: Arc<Playback>) {
        self.playback = Some(playback);
        self.capture_start = None;
        self.sample_position = 0;
    }

    pub fn attach_capture(&mut self, utterance: Arc<Utterance>, speech_start: Duration) {
        self.attach(Arc::new(Playback::new(utterance)));
        self.capture_start = Some(speech_start);
    }

    pub fn advance(&mut self, elapsed: Duration) {
        let pose = if let Some(playback) = self.playback.as_ref() {
            if let Some(start) = self.capture_start {
                let position = elapsed.saturating_sub(start).as_nanos() * u128::from(SAMPLE_RATE) / 1_000_000_000;
                playback.position.store(position.min(playback.utterance.samples.len() as u128) as u64, Ordering::Release);
            }
            self.sample_position = playback.sample_position();
            if playback.stopped.load(Ordering::Acquire) || self.sample_position >= playback.utterance.samples.len() as u64 {
                self.playback = None;
                Pose::neutral()
            } else {
                playback.utterance.pose(self.sample_position)
            }
        } else {
            Pose::neutral()
        };
        self.renderer.draw(&pose, elapsed.as_secs_f64());
        self.frames += 1;
    }

    pub fn frame(&self) -> &RgbaImage {
        self.renderer.frame()
    }

    pub fn anatomy_paths(&self) -> &[Vec<[f32; 2]>] {
        self.renderer.anatomy_paths()
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn sample_position(&self) -> u64 {
        self.sample_position
    }
}

#[cfg(test)]
#[path = "../unit_tests/chimera.rs"]
mod tests;
