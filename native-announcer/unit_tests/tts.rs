use super::*;

#[test]
fn only_first_nonempty_local_chunk_takes_the_start_plan() {
    let cancelled = AtomicBool::new(false);
    let mut start = Some(speech_start(2, None, &cancelled));
    assert!(chunk_start(&[], &mut start).is_none());
    assert!(start.is_some());
    assert!(chunk_start(&[7, -9], &mut start).is_some());
    assert!(start.is_none());
    assert!(chunk_start(&[], &mut start).is_none());
    assert!(chunk_start(&[11, -13], &mut start).is_none());
}

#[cfg(windows)]
#[test]
fn native_model_directory_reads_long_unicode_paths_and_resolves_parents() {
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); }
    }
    let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Temp/opencode").join(format!("Herald TTS {} {unique}", std::process::id()));
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
            assert!(error.contains("Reinstall the Herald bundle in an ASCII path"), "{error}");
        }
    }
}

#[test]
#[ignore = "Measures native Kitten loading and synthesis with installed model assets"]
fn benchmark_native_kitten() {
    let output = PathBuf::from(std::env::var_os("HERALD_TTS_BENCHMARK").expect("Set benchmark output path"));
    let threads = std::env::var("HERALD_TTS_THREADS").unwrap_or_else(|_| "4".into()).parse().unwrap();
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
#[ignore = "Requires installed Kitten model assets"]
fn released_preview_engine_can_be_loaded_again() {
    for _ in 0..3 {
        with_engine(|engine| {
            let audio = engine.generate_with_config("This is an announcement.", &GenerationConfig::default(), None::<fn(&[f32], f32) -> bool>).ok_or("Synthesis failed")?;
            assert!(!audio.samples().is_empty());
            assert!(audio.samples().iter().any(|sample| sample.abs() > 0.01));
            Ok(())
        }).unwrap();
        assert!(ENGINE.lock().unwrap().is_some());
        release_engine();
        assert!(ENGINE.lock().unwrap().is_none());
    }
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
    let settings = crate::settings::Settings { silent_sound_seconds: 10, ..crate::settings::Settings::default() };
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
    let root = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap()).join("Temp/opencode").join(format!("herald-offline-test-{}", std::process::id()));
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
