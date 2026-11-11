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
fn mouth_pose_interpolates_at_the_played_sample_including_the_short_final_block() {
    let poses = (0..3).map(|index| Pose {
        artic: HumanArtic::from_array(&[index as f64; 17]),
        f0: 100.0 + 100.0 * index as f64,
        level: index as f32 * 0.25,
    }).collect();
    let utterance = Utterance { samples: vec![0; POSE_HOP + POSE_HOP / 2], poses };
    for (position, fraction) in [(POSE_HOP / 2, 0.5), (POSE_HOP, 1.0), (POSE_HOP + POSE_HOP / 4, 1.5)] {
        let pose = utterance.pose(position as u64);
        assert!(pose.artic.to_array().iter().all(|value| (*value - fraction).abs() < 1e-9));
        assert_eq!(pose.f0, 100.0 + 100.0 * fraction);
        assert_eq!(pose.level, fraction as f32 * 0.25);
    }
    assert_eq!(utterance.pose(0).artic, Pose::neutral().artic);
    assert_eq!(utterance.pose(utterance.samples.len() as u64).artic, Pose::neutral().artic);
}

#[test]
fn capture_and_live_render_the_same_pose_after_delayed_frames() {
    let utterance = speech();
    let playback = Arc::new(Playback::new(Arc::clone(&utterance)));
    let mut live = Head::new();
    live.attach(Arc::clone(&playback));
    let mut capture = Head::new();
    capture.attach_capture(Arc::clone(&utterance), Duration::from_millis(650));
    for milliseconds in [0, 300, 650, 701, 900, 1200, 1800, 2400, 10_000] {
        let elapsed = Duration::from_millis(milliseconds);
        let expected = ((milliseconds.saturating_sub(650) * u64::from(SAMPLE_RATE)) / 1000).min(utterance.samples.len() as u64);
        playback.position.store(expected, Ordering::Release);
        live.advance(elapsed);
        capture.advance(elapsed);
        assert_eq!(capture.sample_position(), expected);
        assert_eq!(capture.frame(), live.frame());
    }
}

#[test]
fn displayed_sample_position_stays_with_the_rendered_frame() {
    let playback = Arc::new(Playback::new(speech()));
    let mut head = Head::new();
    head.attach(Arc::clone(&playback));
    playback.position.store(4800, Ordering::Release);
    head.advance(Duration::from_secs(1));
    playback.position.store(9600, Ordering::Release);
    assert_eq!(head.sample_position(), 4800);
    head.advance(Duration::from_secs(2));
    assert_eq!(head.sample_position(), 9600);
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
