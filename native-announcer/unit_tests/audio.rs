use super::*;

#[test]
fn warmup_is_exact_zero_pcm_and_fully_drains_before_activation_and_speech() {
    use std::time::{Duration, Instant};
    let speech = [i16::MIN, -7, 11, i16::MAX];
    for rate in [16000, 24000] {
        for seconds in [0, 1, 2, 10] {
            let now = Instant::now();
            let mut progress = PlaybackProgress::new(speech.len(), rate, Some(Duration::from_secs(seconds)), now);
            let mut buffer = vec![123; rate as usize / 25];
            let mut silence = Vec::new();
            if seconds != 0 {
                loop {
                    let count = progress.fill(&mut buffer, &speech);
                    if count == 0 { break; }
                    silence.extend_from_slice(&buffer[..count]);
                    progress.drained(true, speech.len());
                    assert!(matches!(progress.phase, PlaybackPhase::Warmup { .. }));
                }
                assert_eq!(progress.phase, PlaybackPhase::Warmup { remaining: 0 });
                assert_eq!(progress.fill(&mut buffer, &speech), 0);
                progress.drained(true, speech.len());
                assert_eq!(progress.phase, PlaybackPhase::Warmup { remaining: 0 });
                progress.drained(false, speech.len());
            }
            assert_eq!(silence, vec![0; rate as usize * seconds as usize]);
            assert_eq!(progress.phase, PlaybackPhase::Activation);
            assert_eq!(progress.fill(&mut buffer, &speech), 0);
            progress.activate(true, now + Duration::from_secs(seconds));
            let count = progress.fill(&mut buffer, &speech);
            assert_eq!(&buffer[..count], &speech);
            progress.drained(true, speech.len());
            assert!(matches!(progress.phase, PlaybackPhase::Speech { .. }));
            progress.drained(false, speech.len());
            assert_eq!(progress.phase, PlaybackPhase::Finished);
        }
    }
}

#[test]
fn warmup_has_a_deadline_but_gate_wait_does_not_spend_the_speech_budget() {
    use std::time::{Duration, Instant};
    let now = Instant::now();
    let mut progress = PlaybackProgress::new(16000, 16000, Some(Duration::from_secs(10)), now);
    assert!(!progress.timed_out(now + Duration::from_secs(14)));
    assert!(progress.timed_out(now + Duration::from_secs(15)));
    let mut buffer = vec![1; 160000];
    assert_eq!(progress.fill(&mut buffer, &[7; 16000]), 160000);
    progress.drained(false, 16000);
    assert!(!progress.timed_out(now + Duration::from_secs(120)));
    progress.activate(true, now + Duration::from_secs(120));
    assert!(!progress.timed_out(now + Duration::from_secs(125)));
    assert!(progress.timed_out(now + Duration::from_secs(126)));
}

#[test]
fn rejected_activation_never_queues_speech() {
    let now = std::time::Instant::now();
    let mut progress = PlaybackProgress::new(2, 24000, Some(std::time::Duration::ZERO), now);
    progress.activate(false, now);
    let mut buffer = [0; 2];
    assert_eq!(progress.fill(&mut buffer, &[7, -9]), 0);
    assert_eq!(buffer, [0, 0]);
    assert_eq!(progress.phase, PlaybackPhase::Finished);
}

#[cfg(windows)]
#[test]
#[ignore = "Uses Windows audio output"]
fn real_output_drains_silence_before_activation_and_cancels_warmup() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    for rate in [16000, 24000] {
        let started = Instant::now();
        let start = SpeechStart::new(2, || {
            assert!(started.elapsed() >= Duration::from_millis(1950));
            true
        });
        play_pcm_with_start(&[0; 1600], rate, &AtomicU16::new(0), None, &AtomicBool::new(false), Some(start)).unwrap();
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        stop.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let start = SpeechStart::new(10, || panic!("Cancelled warmup activated presentation"));
    play_pcm_with_start(&[0; 1600], 16000, &AtomicU16::new(0), None, &cancelled, Some(start)).unwrap();
    cancel.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[cfg(windows)]
#[test]
#[ignore = "Uses Windows audio output"]
fn real_output_cancellation_releases_a_waiting_activation() {
    use std::sync::{mpsc, Arc};
    use std::time::{Duration, Instant};
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    let (ready, activated) = mpsc::channel();
    let cancel = std::thread::spawn(move || {
        activated.recv_timeout(Duration::from_secs(2)).unwrap();
        stop.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let start = SpeechStart::new(0, || {
        ready.send(()).unwrap();
        while !cancelled.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    });
    play_pcm_with_start(&[0; 1600], 16000, &AtomicU16::new(0), None, &cancelled, Some(start)).unwrap();
    cancel.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[cfg(windows)]
#[test]
fn empty_and_cancelled_playback_never_opens_output_or_activates() {
    for (samples, cancelled) in [(&[][..], false), (&[7, -9][..], true)] {
        let start = SpeechStart::new(10, || panic!("Empty or cancelled playback activated"));
        assert!(play_pcm_with_start(samples, 0, &AtomicU16::new(100), Some("missing output"), &AtomicBool::new(cancelled), Some(start)).is_ok());
    }
}

#[test]
fn scales_only_announcer_samples_without_clipping() {
    let mut samples = [i16::MIN, -1000, 0, 1000, i16::MAX];
    scale_samples(&mut samples, 100);
    assert_eq!(samples, [i16::MIN, -1000, 0, 1000, i16::MAX]);
    scale_samples(&mut samples, 50);
    assert_eq!(samples, [-1036, -32, 0, 32, 1036]);
    scale_samples(&mut samples, 0);
    assert_eq!(samples, [0; 5]);
}

#[test]
fn reads_speech_wav_chunks_instead_of_assuming_a_fixed_header() {
    let mut wav = b"RIFF\0\0\0\0WAVE".to_vec();
    wav.extend(b"fmt ");
    wav.extend(18u32.to_le_bytes());
    for value in [1u16, 1] { wav.extend(value.to_le_bytes()); }
    for value in [16000u32, 32000] { wav.extend(value.to_le_bytes()); }
    for value in [2u16, 16, 0] { wav.extend(value.to_le_bytes()); }
    wav.extend(b"JUNK");
    wav.extend(1u32.to_le_bytes());
    wav.extend([1, 0]);
    wav.extend(b"data");
    wav.extend(4u32.to_le_bytes());
    wav.extend(123i16.to_le_bytes());
    wav.extend((-123i16).to_le_bytes());
    assert_eq!(decode_wav(&wav).unwrap(), [123, -123]);
    wav.pop();
    assert!(decode_wav(&wav).is_err());
}

#[test]
fn device_selection_uses_stable_ids_and_system_default_when_missing() {
    let devices = vec![OutputDevice { id: "speakers".into(), name: "Speakers".into(), index: 3 }];
    assert_eq!(selected_index(&devices, Some("speakers")), Some(3));
    assert_eq!(selected_index(&devices, Some("unplugged")), None);
    assert_eq!(selected_index(&devices, None), None);
}
