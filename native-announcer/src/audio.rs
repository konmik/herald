#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

#[cfg(target_os = "linux")]
#[path = "audio_linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{output_devices, play_noise, play_pcm};

#[cfg(target_os = "macos")]
pub fn output_devices() -> Vec<OutputDevice> { Vec::new() }

#[cfg(target_os = "macos")]
pub fn play_noise(_path: &std::path::Path, _volume: u16, _selected: Option<&str>, _cancelled: &std::sync::atomic::AtomicBool) -> Result<(), String> { Ok(()) }

#[derive(Clone, Debug)]
pub struct OutputDevice {
    pub id: String,
    pub name: String,
    pub index: u32,
}

pub fn selected_index(devices: &[OutputDevice], selected: Option<&str>) -> Option<u32> {
    selected.and_then(|id| devices.iter().find(|device| device.id == id).map(|device| device.index))
}

pub fn scale_samples(samples: &mut [i16], volume: u16) {
    let gain = crate::settings::volume_gain(volume);
    for sample in samples { *sample = (f64::from(*sample) * gain).round() as i16; }
}

#[cfg(target_os = "windows")]
pub fn output_devices() -> Vec<OutputDevice> {
    use windows::Win32::Media::{Audio::*, Multimedia::*};
    let mut devices = Vec::new();
    unsafe {
        for index in 0..waveOutGetNumDevs() {
            let mut caps = WAVEOUTCAPSW::default();
            if waveOutGetDevCapsW(index as usize, &mut caps, std::mem::size_of::<WAVEOUTCAPSW>() as u32) != 0 { continue; }
            let name = caps.szPname;
            let length = name.iter().position(|character| *character == 0).unwrap_or(name.len());
            let name = String::from_utf16_lossy(&name[..length]);
            let handle = HWAVEOUT(index as usize as *mut _);
            let mut bytes = 0u32;
            let mut id = None;
            if waveOutMessage(Some(handle), DRV_QUERYFUNCTIONINSTANCEIDSIZE, &mut bytes as *mut _ as usize, 0) == 0 && (2..=65536).contains(&bytes) {
                let mut buffer = vec![0u16; bytes as usize / 2];
                if waveOutMessage(Some(handle), DRV_QUERYFUNCTIONINSTANCEID, buffer.as_mut_ptr() as usize, bytes as usize) == 0 {
                    let length = buffer.iter().position(|character| *character == 0).unwrap_or(buffer.len());
                    id = Some(String::from_utf16_lossy(&buffer[..length]));
                }
            }
            devices.push(OutputDevice { id: id.filter(|id| !id.is_empty()).unwrap_or_else(|| name.clone()), name, index });
        }
    }
    devices
}

#[cfg(target_os = "windows")]
pub fn device_index(selected: Option<&str>) -> u32 {
    selected_index(&output_devices(), selected).unwrap_or(windows::Win32::Media::Audio::WAVE_MAPPER)
}

#[cfg(target_os = "windows")]
pub fn play_noise(path: &std::path::Path, volume: u16, selected: Option<&str>, cancelled: &AtomicBool) -> Result<(), String> {
    play_wav(path, &AtomicU16::new(volume), selected, cancelled)
}

pub fn decode_wav(wav: &[u8]) -> Result<Vec<i16>, String> {
    if wav.len() < 12 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" { return Err("Invalid announcement audio".into()); }
    let mut offset = 12;
    let mut valid_format = false;
    let mut samples = None;
    while offset + 8 <= wav.len() {
        let size = u32::from_le_bytes(wav[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let start = offset + 8;
        let end = start.checked_add(size).filter(|end| *end <= wav.len()).ok_or("Truncated announcement audio")?;
        let chunk = &wav[start..end];
        match &wav[offset..offset + 4] {
            b"fmt " if chunk.len() >= 16 => {
                valid_format = u16::from_le_bytes(chunk[0..2].try_into().unwrap()) == 1
                    && u16::from_le_bytes(chunk[2..4].try_into().unwrap()) == 1
                    && u32::from_le_bytes(chunk[4..8].try_into().unwrap()) == 16000
                    && u32::from_le_bytes(chunk[8..12].try_into().unwrap()) == 32000
                    && u16::from_le_bytes(chunk[12..14].try_into().unwrap()) == 2
                    && u16::from_le_bytes(chunk[14..16].try_into().unwrap()) == 16;
            }
            b"data" => {
                if !chunk.len().is_multiple_of(2) { return Err("Invalid PCM sample length".into()); }
                samples = Some(chunk.chunks_exact(2).map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]])).collect());
            }
            _ => {}
        }
        offset = end.checked_add(size % 2).ok_or("Invalid audio chunk size")?;
    }
    if !valid_format { return Err("Announcement audio must be mono 16 kHz, 16-bit PCM".into()); }
    samples.ok_or_else(|| "Announcement audio has no PCM data".into())
}

#[cfg(target_os = "windows")]
pub fn play_wav(path: &std::path::Path, volume: &AtomicU16, selected: Option<&str>, cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) { return Ok(()); }
    let wav = std::fs::read(path).map_err(|error| error.to_string())?;
    let samples = decode_wav(&wav)?;
    play_pcm(&samples, 16000, volume, selected, cancelled)
}

#[cfg(target_os = "windows")]
pub fn play_pcm(samples: &[i16], sample_rate: u32, volume: &AtomicU16, selected: Option<&str>, cancelled: &AtomicBool) -> Result<(), String> {
    play_pcm_with_start(samples, sample_rate, volume, selected, cancelled, None)
}

pub(crate) struct SpeechStart<'a> {
    silence: std::time::Duration,
    activate: Box<dyn FnOnce() -> bool + 'a>,
}

impl<'a> SpeechStart<'a> {
    pub(crate) fn new(seconds: u16, activate: impl FnOnce() -> bool + 'a) -> Self {
        Self { silence: std::time::Duration::from_secs(u64::from(seconds)), activate: Box::new(activate) }
    }
}

#[derive(Debug, PartialEq)]
enum PlaybackPhase {
    Warmup { remaining: usize },
    Activation,
    Speech { offset: usize },
    Finished,
}

struct PlaybackProgress {
    phase: PlaybackPhase,
    started: std::time::Instant,
    limit: std::time::Duration,
    speech_limit: std::time::Duration,
}

impl PlaybackProgress {
    fn new(sample_count: usize, sample_rate: u32, silence: Option<std::time::Duration>, now: std::time::Instant) -> Self {
        let speech_limit = std::time::Duration::from_secs_f64(sample_count as f64 / f64::from(sample_rate) + 5.0);
        Self {
            phase: match silence {
                Some(delay) if !delay.is_zero() => PlaybackPhase::Warmup { remaining: (delay.as_secs_f64() * f64::from(sample_rate)).round() as usize },
                Some(_) => PlaybackPhase::Activation,
                None => PlaybackPhase::Speech { offset: 0 },
            },
            started: now,
            limit: silence.map(|delay| delay + std::time::Duration::from_secs(5)).unwrap_or(speech_limit),
            speech_limit,
        }
    }

    fn fill(&mut self, buffer: &mut [i16], speech: &[i16]) -> usize {
        match &mut self.phase {
            PlaybackPhase::Warmup { remaining } => {
                let count = (*remaining).min(buffer.len());
                buffer[..count].fill(0);
                *remaining -= count;
                count
            }
            PlaybackPhase::Speech { offset } => {
                let count = (speech.len() - *offset).min(buffer.len());
                buffer[..count].copy_from_slice(&speech[*offset..*offset + count]);
                *offset += count;
                count
            }
            _ => 0,
        }
    }

    fn drained(&mut self, pending: bool, sample_count: usize) {
        if pending { return; }
        match self.phase {
            PlaybackPhase::Warmup { remaining: 0 } => self.phase = PlaybackPhase::Activation,
            PlaybackPhase::Speech { offset } if offset == sample_count => self.phase = PlaybackPhase::Finished,
            _ => {}
        }
    }

    fn activate(&mut self, allowed: bool, now: std::time::Instant) {
        assert_eq!(self.phase, PlaybackPhase::Activation);
        self.phase = if allowed { PlaybackPhase::Speech { offset: 0 } } else { PlaybackPhase::Finished };
        self.started = now;
        self.limit = self.speech_limit;
    }

    fn timed_out(&self, now: std::time::Instant) -> bool {
        !matches!(self.phase, PlaybackPhase::Activation | PlaybackPhase::Finished) && now.saturating_duration_since(self.started) >= self.limit
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn play_pcm_with_start(samples: &[i16], sample_rate: u32, volume: &AtomicU16, selected: Option<&str>, cancelled: &AtomicBool, mut start: Option<SpeechStart<'_>>) -> Result<(), String> {
    use windows::Win32::Media::Audio::*;
    if cancelled.load(Ordering::Relaxed) || samples.is_empty() { return Ok(()); }
    if !(8000..=192000).contains(&sample_rate) { return Err("Invalid announcement sample rate".into()); }
    let format = WAVEFORMATEX { wFormatTag: 1, nChannels: 1, nSamplesPerSec: sample_rate, nAvgBytesPerSec: sample_rate * 2, nBlockAlign: 2, wBitsPerSample: 16, cbSize: 0 };
    unsafe {
        let mut output = HWAVEOUT::default();
        let index = device_index(selected);
        let mut result = waveOutOpen(Some(&mut output), index, &format, None, None, MIDI_WAVE_OPEN_TYPE(0));
        if result != 0 && index != WAVE_MAPPER {
            result = waveOutOpen(Some(&mut output), WAVE_MAPPER, &format, None, None, MIDI_WAVE_OPEN_TYPE(0));
        }
        if result != 0 { return Err(format!("Could not open announcement audio output: {result}")); }
        #[repr(align(8))]
        struct Block { header: WAVEHDR, samples: Vec<i16>, prepared: bool }
        let mut blocks: Vec<_> = (0..2).map(|_| Box::new(Block { header: WAVEHDR::default(), samples: vec![0; sample_rate as usize / 25], prepared: false })).collect();
        let size = std::mem::size_of::<WAVEHDR>() as u32;
        let mut progress = PlaybackProgress::new(samples.len(), sample_rate, start.as_ref().map(|start| start.silence), std::time::Instant::now());
        while !cancelled.load(Ordering::Relaxed) {
            for block in &mut blocks {
                if cancelled.load(Ordering::Relaxed) { break; }
                if block.prepared && std::ptr::read_volatile(std::ptr::addr_of!(block.header.dwFlags)) & WHDR_DONE != 0 {
                    result = waveOutUnprepareHeader(output, &mut block.header, size);
                    if result != 0 { break; }
                    block.prepared = false;
                }
                if !block.prepared {
                    let count = progress.fill(&mut block.samples, samples);
                    if count == 0 { continue; }
                    scale_samples(&mut block.samples[..count], volume.load(Ordering::Relaxed));
                    block.header = WAVEHDR { lpData: windows::core::PSTR(block.samples.as_mut_ptr().cast()), dwBufferLength: (count * 2) as u32, ..WAVEHDR::default() };
                    result = waveOutPrepareHeader(output, &mut block.header, size);
                    if result != 0 { break; }
                    block.prepared = true;
                    result = waveOutWrite(output, &mut block.header, size);
                    if result != 0 { break; }
                }
            }
            if result != 0 || cancelled.load(Ordering::Relaxed) { break; }
            progress.drained(blocks.iter().any(|block| block.prepared), samples.len());
            if progress.phase == PlaybackPhase::Activation {
                let allowed = start.take().is_some_and(|start| (start.activate)()) && !cancelled.load(Ordering::Relaxed);
                progress.activate(allowed, std::time::Instant::now());
            }
            if progress.phase == PlaybackPhase::Finished { break; }
            if progress.timed_out(std::time::Instant::now()) { result = 1; break; }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        waveOutReset(output);
        for block in &mut blocks {
            if block.prepared { waveOutUnprepareHeader(output, &mut block.header, size); }
        }
        waveOutClose(output);
        if result != 0 { return Err(format!("Could not play announcement audio: {result}")); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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

}
