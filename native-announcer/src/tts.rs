use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKittenModelConfig, OfflineTtsModelConfig};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{mpsc, Arc, Mutex};
#[cfg(test)]
use std::time::Instant;

#[cfg(test)]
const MODEL: &str = "KittenML/kitten-tts-nano-0.8-int8";
const THREADS: i32 = 4;
const VOICES: [&str; 8] = ["Jasper", "Bella", "Bruno", "Luna", "Hugo", "Rosie", "Leo", "Kiki"];
static ENGINE: Mutex<Option<OfflineTts>> = Mutex::new(None);

pub(crate) fn model_directory() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CIVILIZED_AGENT_TTS") { return Ok(path.into()); }
    Ok(std::env::current_exe().map_err(|error| error.to_string())?.parent().ok_or("Missing executable directory")?.join("../resources/tts/kitten-nano-en-v0_8-int8"))
}

fn create(directory: &Path, threads: i32, provider: &str) -> Result<OfflineTts, String> {
    if !(1..=32).contains(&threads) { return Err("TTS thread count must be between 1 and 32".into()); }
    for file in ["model.int8.onnx", "voices.bin", "tokens.txt", "espeak-ng-data/en_dict"] {
        if !directory.join(file).is_file() { return Err(format!("Missing Kitten TTS asset: {}. Run node development_tools/prepare-tts.mjs", directory.join(file).display())); }
    }
    let path = |name| Some(directory.join(name).to_string_lossy().into_owned());
    let config = OfflineTtsConfig {
        model: OfflineTtsModelConfig {
            kitten: OfflineTtsKittenModelConfig {
                model: path("model.int8.onnx"),
                voices: path("voices.bin"),
                tokens: path("tokens.txt"),
                data_dir: path("espeak-ng-data"),
                ..Default::default()
            },
            num_threads: threads,
            provider: Some(provider.into()),
            ..Default::default()
        },
        max_num_sentences: 1,
        silence_scale: 0.2,
        ..Default::default()
    };
    let engine = OfflineTts::create(&config).ok_or("Could not initialize Kitten Nano TTS")?;
    if engine.sample_rate() != 24000 || engine.num_speakers() != 8 { return Err("Unexpected Kitten Nano model format".into()); }
    Ok(engine)
}

fn with_engine<T>(action: impl FnOnce(&OfflineTts) -> Result<T, String>) -> Result<T, String> {
    let mut engine = ENGINE.lock().map_err(|_| "Kitten TTS worker failed")?;
    if engine.is_none() { *engine = Some(create(&model_directory()?, THREADS, "cpu")?); }
    action(engine.as_ref().unwrap())
}

pub fn prepare(use_gpu: bool) -> Result<(), String> {
    if use_gpu {
        if let Err(error) = crate::gpu::prepare() { crate::state::log(&crate::platform::data_directory(), format!("GPU speech unavailable, using CPU: {error}")); }
        else { return Ok(()); }
    } else { crate::gpu::release(); }
    with_engine(|_| Ok(()))
}

fn speaker(character: &str, preferred: Option<&str>) -> i32 {
    preferred.and_then(|name| VOICES.iter().position(|voice| voice.eq_ignore_ascii_case(name)))
        .unwrap_or(if character == "claude" { 2 } else { 0 }) as i32
}

fn pcm(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|sample| if sample.is_finite() { (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16 } else { 0 }).collect()
}

pub fn gpu_worker() -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    use crate::gpu::{Request, Response, respond};
    let engine = create(&model_directory()?, THREADS, "cuda")?;
    respond(&Response::Ready)?;
    for line in BufReader::new(std::io::stdin().lock()).lines() {
        let request: Request = serde_json::from_str(&line.map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        if request.text.contains('\0') || !(0..8).contains(&request.sid) { return Err("Invalid GPU speech request".into()); }
        let config = GenerationConfig { sid: request.sid, ..Default::default() };
        let audio = engine.generate_with_config(&request.text, &config, Some(|samples: &[f32], _| respond(&Response::Audio { samples: pcm(samples) }).is_ok()));
        match audio {
            Some(audio) if !audio.samples().is_empty() => respond(&Response::Done)?,
            _ => respond(&Response::Error { message: "GPU generated no audio".into() })?,
        }
    }
    Ok(())
}

pub fn speak(text: &str, character: &str, preferred: Option<&str>, output_device: Option<&str>, volume: &AtomicU16, cancelled: &Arc<AtomicBool>, use_gpu: bool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) || text.trim().is_empty() { return Ok(()); }
    if text.contains('\0') { return Err("Announcement text contains a null character".into()); }
    let text = text.to_owned();
    let config = GenerationConfig { sid: speaker(character, preferred), ..Default::default() };
    let stop = cancelled.clone();
    let (sender, chunks) = mpsc::sync_channel(1);
    let generator = std::thread::spawn(move || {
        if use_gpu {
            match crate::gpu::generate(&text, config.sid, &stop, &sender) {
                Ok(()) => return Ok(()),
                Err((error, true)) => return Err(error),
                Err((error, false)) => crate::state::log(&crate::platform::data_directory(), format!("GPU speech unavailable, using CPU: {error}")),
            }
        } else { crate::gpu::release(); }
        with_engine(|engine| {
            if stop.load(Ordering::Relaxed) { return Ok(()); }
            let audio = engine.generate_with_config(&text, &config, Some(move |samples: &[f32], _| {
                !stop.load(Ordering::Relaxed) && sender.send(pcm(samples)).is_ok() && !stop.load(Ordering::Relaxed)
            })).ok_or("Kitten TTS could not generate audio")?;
            if audio.samples().is_empty() { return Err("Kitten TTS generated no audio".into()); }
            Ok(())
        })
    });
    let mut result = Ok(());
    for samples in &chunks {
        if cancelled.load(Ordering::Relaxed) { break; }
        if let Err(error) = crate::audio::play_pcm(&samples, 24000, volume, output_device, cancelled) {
            cancelled.store(true, Ordering::Relaxed);
            result = Err(error);
            break;
        }
    }
    drop(chunks);
    let generation = generator.join().map_err(|_| "Kitten TTS generation failed".to_string())?;
    if cancelled.load(Ordering::Relaxed) { result } else { result.and(generation) }
}

#[cfg(test)]
fn benchmark(output: &Path, threads: i32) -> Result<(), String> {
    let started = Instant::now();
    let engine = create(&model_directory()?, threads, "cpu")?;
    let loading_ms = started.elapsed().as_secs_f64() * 1000.0;
    let config = GenerationConfig { sid: 0, ..Default::default() };
    let first = Instant::now();
    let audio = engine.generate_with_config("This is an announcement.", &config, None::<fn(&[f32], f32) -> bool>).ok_or("Kitten benchmark synthesis failed")?;
    let first_generation_ms = first.elapsed().as_secs_f64() * 1000.0;
    if audio.samples().is_empty() { return Err("Kitten benchmark generated no audio".into()); }
    let sample_path = output.with_extension("wav");
    if !audio.save(&sample_path.to_string_lossy()) { return Err("Could not save Kitten benchmark sample".into()); }
    let warm = Instant::now();
    engine.generate_with_config("This is an announcement.", &config, None::<fn(&[f32], f32) -> bool>).ok_or("Kitten benchmark warm synthesis failed")?;
    let result = serde_json::json!({
        "model": MODEL,
        "provider": "cpu",
        "threads": threads,
        "loadingMs": loading_ms,
        "firstGenerationMs": first_generation_ms,
        "warmGenerationMs": warm.elapsed().as_secs_f64() * 1000.0,
        "audioSeconds": audio.samples().len() as f64 / f64::from(audio.sample_rate()),
        "sampleRate": audio.sample_rate(),
        "voices": VOICES,
    });
    std::fs::write(output, serde_json::to_vec_pretty(&result).map_err(|error| error.to_string())?).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Measures native Kitten loading and synthesis with installed model assets"]
    fn benchmark_native_kitten() {
        let output = PathBuf::from(std::env::var_os("CIVILIZED_AGENT_TTS_BENCHMARK").expect("Set benchmark output path"));
        let threads = std::env::var("CIVILIZED_AGENT_TTS_THREADS").unwrap_or_else(|_| "4".into()).parse().unwrap();
        benchmark(&output, threads).unwrap();
    }

    #[test]
    #[ignore = "Requires installed Kitten model assets"]
    fn native_synthesis_streams_each_sentence_once_and_can_stop_early() {
        let engine = create(&model_directory().unwrap(), THREADS, "cpu").unwrap();
        let config = GenerationConfig::default();
        let sizes = Arc::new(Mutex::new(Vec::new()));
        let chunks = sizes.clone();
        let audio = engine.generate_with_config("This is an announcement. Your task is complete.", &config, Some(move |samples: &[f32], _| {
            chunks.lock().unwrap().push(samples.len());
            true
        })).unwrap();
        let sizes = sizes.lock().unwrap();
        assert_eq!(sizes.len(), 2);
        assert_eq!(sizes.iter().sum::<usize>(), audio.samples().len());
        assert!(audio.samples().iter().all(|sample| sample.is_finite()));
        assert!(audio.samples().iter().any(|sample| sample.abs() > 0.01));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let callbacks = calls.clone();
        engine.generate_with_config("This is an announcement. Your task is complete.", &config, Some(move |_: &[f32], _| {
            callbacks.fetch_add(1, Ordering::Relaxed);
            false
        })).unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn kitten_voice_selection_accepts_names_and_migrates_old_preferences() {
        assert_eq!(speaker("opencode", None), 0);
        assert_eq!(speaker("claude", None), 2);
        assert_eq!(speaker("claude", Some("Microsoft David Desktop")), 2);
        assert_eq!(speaker("opencode", Some("luna")), 3);
        assert_eq!(speaker("opencode", Some("Leo")), 6);
    }

    #[test]
    fn float_audio_conversion_clamps_and_rejects_nonfinite_samples() {
        assert_eq!(pcm(&[-2.0, -1.0, 0.0, 1.0, 2.0, f32::NAN, f32::INFINITY]), [-32767, -32767, 0, 32767, 32767, 0, 0]);
    }

    #[test]
    fn cancelled_or_empty_speech_never_loads_the_model() {
        assert!(speak("Hello", "opencode", None, None, &AtomicU16::new(100), &Arc::new(AtomicBool::new(true)), false).is_ok());
        assert!(speak(" ", "opencode", None, None, &AtomicU16::new(100), &Arc::new(AtomicBool::new(false)), false).is_ok());
        assert!(speak("a\0b", "opencode", None, None, &AtomicU16::new(100), &Arc::new(AtomicBool::new(false)), false).is_err());
    }

    #[test]
    fn missing_assets_and_invalid_thread_counts_return_errors() {
        assert!(create(Path::new("missing-kitten-model"), 4, "cpu").is_err());
        assert!(create(Path::new("."), 0, "cpu").is_err());
    }
}
