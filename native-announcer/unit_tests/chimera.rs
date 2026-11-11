use super::*;
use std::io::Write;
use std::path::PathBuf;

fn speech() -> Arc<Utterance> {
    static SPEECH: OnceLock<Arc<Utterance>> = OnceLock::new();
    Arc::clone(SPEECH.get_or_init(|| prepare("Hello, world!", &AtomicBool::new(false)).unwrap()))
}

#[test]
fn speech_produces_repeatable_audible_pcm_at_48_khz() {
    let first = speech();
    let second = prepare("Hello, world!", &AtomicBool::new(false)).unwrap();
    assert_eq!(first.sample_rate(), 48_000);
    assert!(first.samples.len() > 48_000);
    assert!(first.samples.len() < 10 * 48_000);
    assert!(first.samples.iter().any(|sample| sample.unsigned_abs() > 100));
    assert!(first.samples.iter().all(|sample| sample.unsigned_abs() <= 26_214));
    assert_eq!(first.samples, second.samples);
}

#[test]
fn head_waits_for_played_samples_and_changes_with_speech() {
    let utterance = speech();
    let playback = Arc::new(Playback::new(Arc::clone(&utterance)));
    let mut head = Head::new();
    let mut idle = Head::new();
    head.attach(Arc::clone(&playback));
    let elapsed = Duration::from_secs(4);
    head.advance(elapsed);
    idle.advance(elapsed);
    assert_eq!(head.sample_position(), 0);
    assert_eq!(head.frame(), idle.frame());
    let neutral = head.frame().clone();
    let moved = (1..utterance.samples.len()).step_by(SAMPLE_RATE as usize / 15).any(|position| {
        playback.position.store(position as u64, Ordering::Release);
        head.advance(elapsed);
        head.frame() != &neutral
    });
    assert!(moved);
    playback.position.store(u64::MAX, Ordering::Release);
    head.advance(elapsed);
    assert_eq!(head.sample_position(), utterance.samples.len() as u64);
    assert_eq!(head.frame(), &neutral);
}

#[test]
fn stopping_playback_closes_the_mouth_and_releases_audio() {
    let utterance = speech();
    let playback = Arc::new(Playback::new(utterance));
    let weak = Arc::downgrade(&playback);
    let mut head = Head::new();
    let neutral = head.frame().clone();
    head.attach(Arc::clone(&playback));
    playback.position.store(SAMPLE_RATE as u64 / 2, Ordering::Release);
    head.advance(Duration::ZERO);
    playback.stop();
    drop(playback);
    head.advance(Duration::ZERO);
    assert!(weak.upgrade().is_none());
    assert_eq!(head.sample_position(), SAMPLE_RATE as u64 / 2);
    assert_eq!(head.frame(), &neutral);
}

#[test]
fn completed_playback_releases_audio() {
    let utterance = speech();
    let playback = Arc::new(Playback::new(Arc::clone(&utterance)));
    let weak = Arc::downgrade(&playback);
    let mut head = Head::new();
    head.attach(Arc::clone(&playback));
    playback.position.store(utterance.samples.len() as u64, Ordering::Release);
    drop(playback);
    head.advance(Duration::ZERO);
    assert!(weak.upgrade().is_none());
    assert_eq!(head.sample_position(), utterance.samples.len() as u64);
}

#[test]
fn invalid_or_cancelled_speech_returns_no_partial_audio() {
    assert!(prepare("Hello", &AtomicBool::new(true)).is_err());
    assert!(prepare(" ", &AtomicBool::new(false)).is_err());
    assert!(prepare("...", &AtomicBool::new(false)).is_err());
    assert!(prepare(&"a".repeat(8_001), &AtomicBool::new(false)).is_err());
    assert!(prepare(&"hello ".repeat(600), &AtomicBool::new(false))
        .err().unwrap().contains("gesture limit"));
}

#[test]
#[ignore]
fn export_assets() {
    let output = PathBuf::from(std::env::var_os("HERALD_CHIMERA_OUTPUT").expect("HERALD_CHIMERA_OUTPUT"));
    assert!(output.is_absolute());
    std::fs::create_dir_all(&output).unwrap();
    let text = std::env::var("HERALD_CHIMERA_TEXT")
        .unwrap_or_else(|_| "I bring news for your attention. Listen as I deliver this announcement.".into());
    let utterance = prepare(&text, &AtomicBool::new(false)).unwrap();
    let mut wav = std::fs::File::create(output.join("speech.wav")).unwrap();
    let data_size = (utterance.samples.len() * 2) as u32;
    wav.write_all(b"RIFF").unwrap();
    wav.write_all(&(data_size + 36).to_le_bytes()).unwrap();
    wav.write_all(b"WAVEfmt ").unwrap();
    wav.write_all(&16u32.to_le_bytes()).unwrap();
    wav.write_all(&1u16.to_le_bytes()).unwrap();
    wav.write_all(&1u16.to_le_bytes()).unwrap();
    wav.write_all(&SAMPLE_RATE.to_le_bytes()).unwrap();
    wav.write_all(&(SAMPLE_RATE * 2).to_le_bytes()).unwrap();
    wav.write_all(&2u16.to_le_bytes()).unwrap();
    wav.write_all(&16u16.to_le_bytes()).unwrap();
    wav.write_all(b"data").unwrap();
    wav.write_all(&data_size.to_le_bytes()).unwrap();
    let pcm: Vec<_> = utterance.samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
    wav.write_all(&pcm).unwrap();
    let mut renderer = head::Head::new(256);
    renderer.draw(&Pose::neutral(), 0.0);
    renderer.frame().save(output.join("neutral.png")).unwrap();
    let fps = 30usize;
    let frame_count = (utterance.samples.len() * fps).div_ceil(SAMPLE_RATE as usize);
    for index in 0..frame_count {
        let position = index as u64 * SAMPLE_RATE as u64 / fps as u64;
        renderer.draw(&utterance.pose(position), index as f64 / fps as f64);
        renderer.frame().save(output.join(format!("frame-{index:04}.png"))).unwrap();
    }
    let manifest = serde_json::json!({
        "sample_count": utterance.samples.len(),
        "sample_rate": SAMPLE_RATE,
        "frame_count": frame_count,
        "fps": fps,
        "width": 256,
        "height": 256,
    });
    std::fs::write(output.join("manifest.json"), serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}
