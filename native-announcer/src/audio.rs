#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

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
        let started = std::time::Instant::now();
        let limit = std::time::Duration::from_secs_f64(samples.len() as f64 / f64::from(sample_rate) + 5.0);
        let mut offset = 0;
        while !cancelled.load(Ordering::Relaxed) {
            for block in &mut blocks {
                if block.prepared && std::ptr::read_volatile(std::ptr::addr_of!(block.header.dwFlags)) & WHDR_DONE != 0 {
                    result = waveOutUnprepareHeader(output, &mut block.header, size);
                    if result != 0 { break; }
                    block.prepared = false;
                }
                if !block.prepared && offset < samples.len() {
                    let count = (samples.len() - offset).min(block.samples.len());
                    block.samples[..count].copy_from_slice(&samples[offset..offset + count]);
                    scale_samples(&mut block.samples[..count], volume.load(Ordering::Relaxed));
                    block.header = WAVEHDR { lpData: windows::core::PSTR(block.samples.as_mut_ptr().cast()), dwBufferLength: (count * 2) as u32, ..WAVEHDR::default() };
                    result = waveOutPrepareHeader(output, &mut block.header, size);
                    if result != 0 { break; }
                    block.prepared = true;
                    result = waveOutWrite(output, &mut block.header, size);
                    if result != 0 { break; }
                    offset += count;
                }
            }
            if result != 0 || (offset == samples.len() && blocks.iter().all(|block| !block.prepared)) { break; }
            if started.elapsed() >= limit { result = 1; break; }
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
    fn device_selection_uses_stable_ids_and_falls_back_when_missing() {
        let devices = vec![OutputDevice { id: "speakers".into(), name: "Speakers".into(), index: 3 }];
        assert_eq!(selected_index(&devices, Some("speakers")), Some(3));
        assert_eq!(selected_index(&devices, Some("unplugged")), None);
        assert_eq!(selected_index(&devices, None), None);
    }

}
