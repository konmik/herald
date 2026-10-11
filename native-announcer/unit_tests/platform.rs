use super::*;

#[test]
fn prepared_speech_waits_for_warmup_then_release_and_cancels_cleanly() {
    for early in [true, false] {
        let warmup = Arc::new(Warmup { ready: SpeechGate::new(), cancelled: AtomicBool::new(false) });
        let gate = Arc::new(SpeechGate::new());
        let (ready_tx, ready_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        if !early { warmup.ready.release(); }
        let worker_warmup = warmup.clone();
        let worker_gate = gate.clone();
        let worker = std::thread::spawn(move || {
            let ready = || { ready_tx.send(()).unwrap(); };
            let playback = SpeechPlayback { gate: &worker_gate, ready: &ready, warmup: Some(&worker_warmup) };
            finished_tx.send(playback.begin(&AtomicBool::new(false))).unwrap();
        });
        if early {
            assert!(ready_rx.recv_timeout(Duration::from_millis(50)).is_err());
            warmup.ready.release();
        }
        ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(finished_rx.recv_timeout(Duration::from_millis(50)).is_err());
        assert!(!warmup.cancelled.load(Ordering::Relaxed));
        gate.release();
        assert!(finished_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        worker.join().unwrap();
        assert!(warmup.cancelled.load(Ordering::Relaxed));
    }
    let warmup = Warmup { ready: SpeechGate::new(), cancelled: AtomicBool::new(false) };
    warmup.cancel();
    let ready = || panic!("Cancelled warm-up must not show the announcement");
    let gate = SpeechGate::new();
    assert!(!SpeechPlayback { gate: &gate, ready: &ready, warmup: Some(&warmup) }.begin(&AtomicBool::new(false)));
}

#[test]
fn remote_speech_does_not_preload_the_local_model() {
    let mut settings = Settings { schedule_enabled: false, ..Settings::default() };
    assert!(Speech::should_preload(&settings, 720, false));
    settings.elevenlabs_api_key = Some("test-remote-speech-key".into());
    assert!(!Speech::should_preload(&settings, 720, false));
}

#[test]
fn local_speech_preloading_respects_mute_quiet_hours_and_meetings() {
    let mut settings = Settings::default();
    assert!(Speech::should_preload(&settings, 720, false));
    assert!(!Speech::should_preload(&settings, 720, true));
    assert!(!Speech::should_preload(&settings, 0, false));
    settings.quiet_mode = true;
    assert!(!Speech::should_preload(&settings, 720, false));
    settings.quiet_mode = false;
    settings.volume = 0;
    assert!(!Speech::should_preload(&settings, 720, false));
}

#[test]
fn preview_timeout_allows_selected_silent_sound() {
    for (seconds, timeout_seconds) in [(0, 30), (1, 31), (5, 35), (10, 40)] {
        let settings = Settings { silent_sound_seconds: seconds, ..Settings::default() };
        assert_eq!(preview_timeout(&settings), Duration::from_secs(timeout_seconds));
    }
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "Plays speech on the system audio output"]
fn speech_uses_the_same_pcm_volume_path_as_static() {
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    std::thread::spawn(move || { std::thread::sleep(Duration::from_secs(10)); stop.store(true, Ordering::Relaxed); });
    let settings = Settings { output_device: Some("unavailable-test-device".into()), ..Default::default() };
    speak("This is an announcement", &ResolvedVoice::Local { speaker: None }, "opencode", None, &AtomicU16::new(75), &cancelled, &settings, None).unwrap();
    assert!(!cancelled.load(Ordering::Relaxed), "Speech must finish without timing out");
}

#[test]
fn capture_activity_keeps_hidden_meetings_muted_and_probe_failures_fail_closed() {
    assert_eq!(meeting_evidence(Ok(true), Ok(false)), Ok(true));
    assert_eq!(meeting_evidence(Ok(false), Ok(true)), Ok(true));
    assert_eq!(meeting_evidence(Ok(false), Ok(false)), Ok(false));
    assert!(meeting_evidence(Err("Capture unavailable".into()), Ok(false)).is_err());
    assert!(meeting_evidence(Ok(false), Err("Controls unavailable".into())).is_err());
}

#[test]
fn speech_gate_waits_until_released() {
    let gate = Arc::new(SpeechGate::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let (started_tx, started_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let worker_gate = gate.clone();
    let worker_cancelled = cancelled.clone();
    std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        finished_tx.send(worker_gate.wait(&worker_cancelled)).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(finished_rx.recv_timeout(Duration::from_millis(100)).is_err());
    gate.release();
    assert!(finished_rx.recv_timeout(Duration::from_secs(1)).unwrap());
}

#[test]
fn speech_gate_cancellation_unblocks_waiting_audio() {
    let gate = Arc::new(SpeechGate::new());
    let cancelled = Arc::new(AtomicBool::new(false));
    let (started_tx, started_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let worker_gate = gate.clone();
    let worker_cancelled = cancelled.clone();
    std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        finished_tx.send(worker_gate.wait(&worker_cancelled)).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    cancelled.store(true, Ordering::Relaxed);
    gate.cancel();
    assert!(!finished_rx.recv_timeout(Duration::from_secs(1)).unwrap());
}

#[test]
fn interference_audio_has_silent_edges_and_a_faded_envelope() {
    let data = std::env::temp_dir()
        .join("opencode")
        .join(format!("herald-signal-{}", std::process::id()));
    std::fs::create_dir_all(&data).unwrap();
    let signal = Signal::new(&data);
    let wav = std::fs::read(&signal.path).unwrap();
    let samples: Vec<_> = wav[44..]
        .chunks_exact(2)
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    assert_eq!(samples.len(), 10400);
    assert_eq!(samples[0], 0);
    assert_eq!(*samples.last().unwrap(), 0);
    let edge: i32 = samples[..100].iter().map(|v| i32::from(*v).abs()).sum();
    let middle: i32 = samples[4500..5500]
        .iter()
        .map(|v| i32::from(*v).abs())
        .sum();
    assert!(middle > edge * 20);
    let peak = samples.iter().map(|value| i32::from(*value).abs()).max().unwrap();
    let rms = (samples.iter().map(|value| f64::from(*value).powi(2)).sum::<f64>() / samples.len() as f64).sqrt();
    assert!(peak > 8000 && peak < 16000, "Static should be audible without clipping: {peak}");
    assert!(rms > 2000.0, "Static should have audible average volume: {rms}");
    drop(signal);
    std::fs::remove_dir_all(data).unwrap();
}
