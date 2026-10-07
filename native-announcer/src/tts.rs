use crate::characters::ResolvedVoice;
use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsKittenModelConfig, OfflineTtsModelConfig};
use crate::platform::SpeechPlayback;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{mpsc, Arc, Mutex};
#[cfg(test)]
use std::time::Instant;

#[cfg(test)]
const MODEL: &str = "KittenML/kitten-tts-nano-0.8-int8";
const THREADS: i32 = 4;
const VOICES: [&str; 8] = ["Jasper", "Bella", "Bruno", "Luna", "Hugo", "Rosie", "Leo", "Kiki"];
const MODEL_DIRECTORY: &str = "kitten-nano-en-v0_8-int8";
const MODEL_FILES: [&str; 5] = ["model.int8.onnx", "voices.bin", "tokens.txt", "espeak-ng-data/en_dict", "LICENSE"];
const LIBRARIES: [&str; 2] = ["onnxruntime.dll", "sherpa-onnx-c-api.dll"];
static ENGINE: Mutex<Option<OfflineTts>> = Mutex::new(None);

pub(crate) fn model_directory() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("CIVILIZED_AGENT_TTS") { return Ok(path.into()); }
    Ok(installation_directory().join(MODEL_DIRECTORY))
}

fn installation_directory() -> PathBuf {
    crate::platform::data_directory().join("offline-voice-1.13.8")
}

fn complete_installation(directory: &Path) -> bool {
    MODEL_FILES.iter().all(|file| directory.join(MODEL_DIRECTORY).join(file).is_file())
        && LIBRARIES.iter().all(|file| directory.join("lib").join(file).is_file())
}

pub fn installed() -> bool {
    complete_installation(&installation_directory())
}

pub fn install() -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::process::CommandExt;

    let destination = installation_directory();
    std::fs::create_dir_all(destination.parent().unwrap()).map_err(|error| error.to_string())?;
    let lock_path = destination.with_extension("lock");
    let lock = std::fs::OpenOptions::new().write(true).create_new(true).share_mode(0).custom_flags(0x04000000).open(&lock_path)
        .map_err(|_| "Another offline voice installation is running. Try again when it finishes.".to_string())?;
    let stage = destination.with_extension(format!("install-{}", std::process::id()));
    let result = (|| {
        if installed() { return Ok(()); }
        std::fs::create_dir(&stage).map_err(|error| error.to_string())?;
        let (arch, runtime_hash) = if cfg!(target_arch = "aarch64") {
            ("arm64", "22cd2b2b5e35c1132abc74a91ee77f683645437382617af6a4bd5818b9c509f4")
        } else {
            ("x64", "b8eedf41bd6d3779218887b48367bb7a3ece5aaa7667f01f69ee823a12b0a9e7")
        };
        let runtime = format!("sherpa-onnx-v1.13.8-win-{arch}-shared-MT-Release-lib");
        let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(120)).build();
        for (name, tag, hash) in [
            (MODEL_DIRECTORY, "tts-models", "6fa5be852612ce761094ba74ee6123b4fc4acfefa79bf64dc63acae4a83af2fd"),
            (runtime.as_str(), "v1.13.8", runtime_hash),
        ] {
            let archive = stage.join(format!("{name}.tar.bz2"));
            let response = agent.get(&format!("https://github.com/k2-fsa/sherpa-onnx/releases/download/{tag}/{name}.tar.bz2")).call().map_err(|error| error.to_string())?;
            let mut reader = response.into_reader().take(128 * 1024 * 1024);
            let mut file = std::fs::File::create(&archive).map_err(|error| error.to_string())?;
            let mut digest = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                let count = reader.read(&mut buffer).map_err(|error| error.to_string())?;
                if count == 0 { break; }
                digest.update(&buffer[..count]);
                file.write_all(&buffer[..count]).map_err(|error| error.to_string())?;
            }
            drop(file);
            if format!("{:x}", digest.finalize()) != hash { return Err("Offline voice download checksum mismatch. Try again.".into()); }
            let tar = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Missing Windows directory")?).join("System32/tar.exe");
            let output = std::process::Command::new(tar).args(["-xjf"]).arg(&archive).arg("-C").arg(&stage).creation_flags(0x08000000).output().map_err(|error| error.to_string())?;
            if !output.status.success() { return Err(format!("Offline voice extraction failed: {}", String::from_utf8_lossy(&output.stderr))); }
            std::fs::remove_file(archive).map_err(|error| error.to_string())?;
        }
        std::fs::rename(stage.join(&runtime).join("lib"), stage.join("lib")).map_err(|error| error.to_string())?;
        for (name, url) in [
            ("sherpa-onnx-LICENSE", "https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/LICENSE"),
            ("onnxruntime-LICENSE", "https://raw.githubusercontent.com/microsoft/onnxruntime/v1.28.2/LICENSE"),
        ] {
            let mut reader = agent.get(url).call().map_err(|error| error.to_string())?.into_reader();
            let mut file = std::fs::File::create(stage.join(name)).map_err(|error| error.to_string())?;
            std::io::copy(&mut reader, &mut file).map_err(|error| error.to_string())?;
        }
        if !complete_installation(&stage) { return Err("Offline voice download is incomplete.".into()); }
        let backup = destination.with_extension("previous");
        if destination.exists() { std::fs::rename(&destination, &backup).map_err(|error| error.to_string())?; }
        if let Err(error) = std::fs::rename(&stage, &destination) {
            if backup.exists() { let _ = std::fs::rename(&backup, &destination); }
            return Err(error.to_string());
        }
        if backup.exists() { let _ = std::fs::remove_dir_all(backup); }
        Ok(())
    })();
    if stage.exists() { let _ = std::fs::remove_dir_all(stage); }
    drop(lock);
    let _ = std::fs::remove_file(lock_path);
    result
}

fn load_runtime() -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::LibraryLoader::*;

    let directory = if std::env::var_os("CIVILIZED_AGENT_TTS").is_some() {
        std::env::current_exe().map_err(|error| error.to_string())?.parent().ok_or("Missing executable directory")?.to_path_buf()
    } else {
        installation_directory().join("lib")
    };
    for name in LIBRARIES {
        let path: Vec<u16> = directory.join(name).as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe { LoadLibraryExW(path.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS) }.is_null() {
            return Err(format!("Offline voice engine unavailable: {}. Install it from Settings > Offline voice.", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn native_model_directory(directory: &Path) -> Result<PathBuf, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

    let absolute = directory.canonicalize().map_err(|error| format!("Could not resolve Kitten TTS directory: {error}"))?;
    let wide: Vec<u16> = absolute.as_os_str().encode_wide().chain(Some(0)).collect();
    let required = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) };
    if required == 0 { return Err(format!("Could not resolve Kitten TTS short path: {}", std::io::Error::last_os_error())); }
    let mut buffer = vec![0u16; required as usize];
    let path = loop {
        let length = unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 { return Err(format!("Could not resolve Kitten TTS short path: {}", std::io::Error::last_os_error())); }
        if (length as usize) < buffer.len() {
            break String::from_utf16(&buffer[..length as usize]).map_err(|_| "Kitten TTS short path contains invalid Unicode")?;
        }
        buffer.resize(length as usize, 0);
    };
    let path = if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    };
    if !path.is_ascii() {
        return Err("Kitten TTS requires an ASCII installation path because Windows short names are unavailable for this directory. Reinstall the Civilized Agent bundle in an ASCII path.".into());
    }
    Ok(path.into())
}

fn create_cpu_engine(directory: &Path, threads: i32) -> Result<OfflineTts, String> {
    if !(1..=32).contains(&threads) { return Err("TTS thread count must be between 1 and 32".into()); }
    for file in MODEL_FILES {
        if !directory.join(file).is_file() { return Err("Offline voice is not installed. Open Settings > Offline voice and choose Install.".into()); }
    }
    load_runtime()?;
    #[cfg(windows)]
    let directory = native_model_directory(directory)?;
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
            provider: Some("cpu".into()),
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
    if engine.is_none() { *engine = Some(create_cpu_engine(&model_directory()?, THREADS)?); }
    action(engine.as_ref().unwrap())
}

pub fn prepare() -> Result<(), String> {
    if !installed() && std::env::var_os("CIVILIZED_AGENT_TTS").is_none() { return Ok(()); }
    with_engine(|_| Ok(()))
}

fn speaker(character: &str, preferred: Option<&str>) -> i32 {
    preferred.and_then(|name| VOICES.iter().position(|voice| voice.eq_ignore_ascii_case(name)))
        .unwrap_or(if character == "claude" { 2 } else { 0 }) as i32
}

fn pcm(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|sample| if sample.is_finite() { (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16 } else { 0 }).collect()
}

pub fn speak(text: &str, voice: &ResolvedVoice, source_character: &str, local_speaker: Option<&str>, volume: &AtomicU16, cancelled: &Arc<AtomicBool>, settings: &crate::settings::Settings, playback: Option<&SpeechPlayback<'_>>) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) || text.trim().is_empty() { return Ok(()); }
    if text.contains('\0') { return Err("Announcement text contains a null character".into()); }
    let voice_id = match voice {
        ResolvedVoice::ElevenLabs { voice_id } => voice_id.as_str(),
        ResolvedVoice::Local { .. } => settings.default_voice_id.as_str(),
    };
    let remote = match crate::elevenlabs::Client::from_settings(settings) {
        Ok(Some(client)) => Some(client.synthesize_speech(voice_id, text, settings.speech_model, cancelled)),
        Ok(None) => None,
        Err(error) => Some(Err(crate::elevenlabs::SpeechError::Message(error))),
    };
    if let Some(remote) = remote {
        match remote {
            Ok(samples) => {
                if let Some(playback) = playback {
                    if !playback.begin(cancelled) { return Ok(()); }
                }
                return crate::audio::play_pcm(&samples, 16000, volume, settings.output_device.as_deref(), cancelled);
            }
            Err(crate::elevenlabs::SpeechError::Cancelled) => return Ok(()),
            Err(error) => {
                crate::state::log(&crate::platform::data_directory(), format!("ElevenLabs speech unavailable; using local voice: {error}"));
            }
        }
    }
    let preferred = match voice {
        ResolvedVoice::Local { speaker } => speaker.as_deref().or(local_speaker),
        ResolvedVoice::ElevenLabs { .. } => local_speaker,
    };
    local_speak(text, source_character, preferred, settings.output_device.as_deref(), volume, cancelled, playback)
}

fn local_speak(text: &str, character: &str, preferred: Option<&str>, output_device: Option<&str>, volume: &AtomicU16, cancelled: &Arc<AtomicBool>, playback: Option<&SpeechPlayback<'_>>) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) { return Ok(()); }
    let text = text.to_owned();
    let config = GenerationConfig { sid: speaker(character, preferred), ..Default::default() };
    let stop = cancelled.clone();
    let (sender, chunks) = mpsc::sync_channel(1);
    let generator = std::thread::spawn(move || {
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
    let mut ready_sent = false;
    for samples in &chunks {
        if cancelled.load(Ordering::Relaxed) { break; }
        if samples.is_empty() { continue; }
        if !ready_sent {
            ready_sent = true;
            if let Some(playback) = playback {
                if !playback.begin(cancelled) { break; }
            }
        }
        if cancelled.load(Ordering::Relaxed) { break; }
        if let Err(error) = crate::audio::play_pcm(&samples, 24000, volume, output_device, cancelled) {
            cancelled.store(true, Ordering::Relaxed);
            result = Err(error);
            break;
        }
    }
    drop(chunks);
    let generation = generator.join().map_err(|_| "Kitten TTS generation failed".to_string())?;
    if cancelled.load(Ordering::Relaxed) { result } else if result.is_ok() && generation.is_ok() && !ready_sent { Err("Kitten TTS generated no audio".into()) } else { result.and(generation) }
}

#[cfg(test)]
fn benchmark(output: &Path, threads: i32) -> Result<(), String> {
    let started = Instant::now();
    let engine = create_cpu_engine(&model_directory()?, threads)?;
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

    #[cfg(windows)]
    #[test]
    fn native_model_directory_reads_long_unicode_paths_and_resolves_parents() {
        struct Scratch(PathBuf);
        impl Drop for Scratch {
            fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); }
        }
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Temp/opencode").join(format!("Civilized Agent TTS {} {unique}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let scratch = Scratch(root);
        std::fs::write(scratch.0.join("tokens.txt"), b"ASCII bundled tokens").unwrap();
        if scratch.0.to_str().unwrap().is_ascii() {
            let normalized = native_model_directory(&scratch.0).unwrap();
            assert_eq!(std::fs::read(normalized.join("tokens.txt")).unwrap(), b"ASCII bundled tokens");
        }
        let directory = scratch.0.join("installed versions Ω with spaces").join("long installed bundle version directory for native speech portability").join("another long directory matching the bundled release installation layout").join("native announcer resources with spaces").join("kitten model Ω directory");
        std::fs::create_dir_all(directory.join("child")).unwrap();
        std::fs::write(directory.join("tokens.txt"), b"bundled tokens").unwrap();
        assert!(directory.as_os_str().len() > 260);
        match native_model_directory(&directory.join("child/..")) {
            Ok(normalized) => {
                assert!(normalized.to_str().unwrap().is_ascii());
                assert!(!normalized.to_str().unwrap().starts_with(r"\\?\"));
                assert_eq!(std::fs::read(normalized.join("tokens.txt")).unwrap(), b"bundled tokens");
                assert_eq!(native_model_directory(&directory.canonicalize().unwrap()).unwrap(), normalized);
            }
            Err(error) => {
                assert!(error.contains("Windows short names are unavailable"), "{error}");
                assert!(error.contains("Reinstall the Civilized Agent bundle in an ASCII path"), "{error}");
            }
        }
    }

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
        let engine = create_cpu_engine(&model_directory().unwrap(), THREADS).unwrap();
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
        let settings = crate::settings::Settings::default();
        assert!(speak("Hello", &ResolvedVoice::Local { speaker: None }, "opencode", None, &AtomicU16::new(100), &Arc::new(AtomicBool::new(true)), &settings, None).is_ok());
        assert!(speak(" ", &ResolvedVoice::Local { speaker: None }, "opencode", None, &AtomicU16::new(100), &Arc::new(AtomicBool::new(false)), &settings, None).is_ok());
        assert!(speak("a\0b", &ResolvedVoice::Local { speaker: None }, "opencode", None, &AtomicU16::new(100), &Arc::new(AtomicBool::new(false)), &settings, None).is_err());
    }

    #[test]
    fn missing_assets_and_invalid_thread_counts_return_errors() {
        assert!(create_cpu_engine(Path::new("missing-kitten-model"), 4).is_err());
        assert!(create_cpu_engine(Path::new("."), 0).is_err());
    }

    #[test]
    fn offline_voice_requires_both_the_model_and_engine() {
        let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Temp/opencode").join(format!("civilized-offline-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(!complete_installation(&root));
        for file in MODEL_FILES {
            let path = root.join(MODEL_DIRECTORY).join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"fixture").unwrap();
        }
        assert!(!complete_installation(&root));
        std::fs::create_dir(root.join("lib")).unwrap();
        for file in LIBRARIES { std::fs::write(root.join("lib").join(file), b"fixture").unwrap(); }
        assert!(complete_installation(&root));
        std::fs::remove_file(root.join("lib").join(LIBRARIES[0])).unwrap();
        assert!(!complete_installation(&root));
        std::fs::remove_dir_all(root).unwrap();
    }
}
