//! `--benchmark-lightning <output.json>`: renders whole announcements frame by frame for every Lightning preset, as the
//! macOS announcer and the Settings preview do, and writes each frame's time, phase and stage breakdown.
use crate::lightning::PRESETS;
use crate::profile::{self, STAGES};
use crate::render::{self, AnnouncementPhase, LightningActivity, Renderer};
use crate::settings::Settings;
use crate::state::{display_duration, visual_interference_amount};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) type Preview = fn(&Path, &Settings, &str, &Options, &mut Recorder) -> Result<u64, String>;

const TEXT: &str = "The native voice adviser is ready. Announcements stay visible without taking focus.";
pub(crate) const SEED: u32 = 1234;

pub(crate) struct Options { pub cycles: usize, pub scale: f32, pub fps: f64, profile: bool, targets: Vec<String> }

fn options(arguments: Vec<String>, preview: Option<Preview>) -> Result<Options, String> {
    let targets = if preview.is_some() { vec!["announcement".into(), "preview".into()] } else { vec!["announcement".into()] };
    let mut options = Options { cycles: 3, scale: 2.0, fps: 120.0, profile: false, targets };
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let mut value = || arguments.next().ok_or(format!("Missing value for {argument}"));
        match argument.as_str() {
            "--cycles" => options.cycles = value()?.parse().map_err(|_| "Invalid --cycles")?,
            "--scale" => options.scale = value()?.parse().map_err(|_| "Invalid --scale")?,
            "--fps" => options.fps = value()?.parse().map_err(|_| "Invalid --fps")?,
            "--targets" => options.targets = value()?.split(',').map(str::to_owned).collect(),
            "--profile" => options.profile = true,
            _ => return Err(format!("Unknown benchmark option: {argument}")),
        }
    }
    if options.cycles == 0 || !(0.5..=4.0).contains(&options.scale) || !(1.0..=1000.0).contains(&options.fps) { return Err("Invalid benchmark options".into()); }
    Ok(options)
}

pub(crate) fn video_path(assets: &Path) -> Result<std::path::PathBuf, String> {
    crate::characters::bundled_characters().values().filter_map(|character| character.animation_path.as_ref().map(|path| assets.join(path)))
        .find(|path| path.is_file()).ok_or_else(|| "No bundled character video is available.".into())
}

fn phase_name(phase: AnnouncementPhase) -> String { format!("{phase:?}").to_lowercase() }

pub(crate) struct Recorder { frames: Vec<serde_json::Value>, digest: Sha256, started: Instant }

impl Recorder {
    pub(crate) fn record(&mut self, target: &str, preset: &str, cycle: usize, at: Duration, phase: AnnouncementPhase, burst: bool, start: Instant, pixels: &[u8]) {
        let milliseconds = start.elapsed().as_secs_f64() * 1000.0;
        self.digest.update(pixels);
        let stages = profile::take();
        let mut frame = serde_json::json!({ "target": target, "preset": preset, "cycle": cycle, "at": at.as_secs_f64() * 1000.0,
            "phase": phase_name(phase), "burst": burst, "ms": milliseconds });
        if profile::enabled() {
            frame["stages"] = STAGES.iter().zip(stages).map(|(stage, value)| (stage.name().to_owned(), serde_json::json!(value))).collect::<serde_json::Map<_, _>>().into();
        }
        self.frames.push(frame);
    }
}

fn announcement(assets: &Path, settings: &Settings, preset: &str, options: &Options, recorder: &mut Recorder) -> Result<u64, String> {
    let scale = options.scale;
    let mut renderer = Renderer::with_settings(settings)?;
    renderer.text = render::display_text(TEXT);
    renderer.title = "Herald verification".into();
    let height = renderer.announcement_height(scale, 700);
    let (width, height) = ((320.0 * scale) as usize, (height as f32 * scale) as usize);
    let duration = display_duration(TEXT);
    let mut video = crate::video::Video::open(&video_path(assets)?)?;
    #[cfg(target_os = "macos")]
    let mut surface = vec![0_u32; width * height];
    let period = Duration::from_secs_f64(1.0 / options.fps);
    let mut decoded = 0;
    for cycle in 0..options.cycles {
        let mut elapsed = Duration::ZERO;
        let video_origin = Duration::from_secs_f64((duration + settings.lightning.transition_duration()).as_secs_f64() * cycle as f64);
        while elapsed < duration + settings.lightning.transition_duration() {
            let closing = elapsed >= duration;
            let transition = if closing { elapsed - duration } else { elapsed };
            let entrance = (!closing && elapsed < settings.lightning.transition_duration()).then_some(elapsed);
            let interference = visual_interference_amount(transition, SEED);
            let activity = if closing { Some(LightningActivity::Closing(transition, SEED)) }
                else if entrance.is_none() { Some(LightningActivity::Holding(elapsed, duration, SEED)) } else { None };
            let burst = closing || entrance.is_some_and(|time| time >= Duration::from_millis(120))
                || matches!(activity, Some(LightningActivity::Holding(..))) && render::ambient_active(elapsed, duration, SEED, settings.lightning);
            let start = Instant::now();
            let before = video.decoded_frames;
            video.advance(video_origin + elapsed)?;
            decoded += video.decoded_frames - before;
            renderer.text_interference = if entrance.is_some() || closing { interference } else { 0.0 };
            renderer.lightning_activity = activity;
            let mut pixels = vec![0; width * height];
            renderer.draw(&mut pixels, width, height, scale, Some(video.frame()), interference, entrance);
            #[cfg(target_os = "macos")]
            let layer = profile::time(profile::Stage::Present, || {
                surface.copy_from_slice(&pixels);
                crate::macos_surface::layer_pixels(&surface)
            });
            #[cfg(not(target_os = "macos"))]
            let layer = pixels.clone().into_boxed_slice();
            let bytes = unsafe { std::slice::from_raw_parts(layer.as_ptr().cast::<u8>(), layer.len() * 4) };
            recorder.record("announcement", preset, cycle, elapsed, render::announcement_phase(transition.mul_f32(settings.lightning.speed), closing), burst, start, bytes);
            elapsed += period;
        }
    }
    Ok(decoded)
}

pub fn run(assets: &Path, output: &Path, arguments: Vec<String>, preview: Option<Preview>) -> Result<(), String> {
    let options = options(arguments, preview)?;
    profile::enable(options.profile);
    profile::take();
    let mut report = serde_json::json!({
        "scale": options.scale, "fps": options.fps, "cycles": options.cycles, "profiled": options.profile,
        "build": if cfg!(debug_assertions) { "debug" } else { "release" }, "digests": {}, "decodedVideoFrames": {},
    });
    let mut frames = Vec::new();
    for target in &options.targets {
        for preset in &PRESETS {
            let settings = Settings { lightning: preset.settings, ..Settings::default() };
            let mut recorder = Recorder { frames: Vec::new(), digest: Sha256::new(), started: Instant::now() };
            let decoded = match target.as_str() {
                "announcement" => announcement(assets, &settings, preset.id, &options, &mut recorder)?,
                "preview" => {
                    let preview = preview.ok_or("Preview benchmarks require the Settings executable")?;
                    preview(assets, &settings, preset.id, &options, &mut recorder)?
                }
                _ => return Err(format!("Unknown benchmark target: {target}")),
            };
            let key = format!("{target}/{}", preset.id);
            report["digests"][&key] = format!("{:x}", recorder.digest.finalize()).into();
            report["decodedVideoFrames"][&key] = decoded.into();
            report["wallSeconds"][&key] = recorder.started.elapsed().as_secs_f64().into();
            frames.extend(recorder.frames);
        }
    }
    report["frames"] = frames.into();
    std::fs::write(output, serde_json::to_vec(&report).map_err(|error| error.to_string())?).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(_: &Path, _: &Settings, _: &str, _: &Options, _: &mut Recorder) -> Result<u64, String> {
        Ok(0)
    }

    #[test]
    fn announcer_benchmark_defaults_to_announcement() {
        assert_eq!(options(Vec::new(), None).unwrap().targets, ["announcement"]);
    }

    #[test]
    fn settings_benchmark_defaults_include_preview() {
        assert_eq!(options(Vec::new(), Some(preview)).unwrap().targets, ["announcement", "preview"]);
    }

    #[test]
    fn explicit_targets_override_the_role_default() {
        assert_eq!(options(vec!["--targets".into(), "announcement".into()], Some(preview)).unwrap().targets, ["announcement"]);
    }
}
