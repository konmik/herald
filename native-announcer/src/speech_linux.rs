use super::SpeechPlayback;
use crate::characters::ResolvedVoice;
use crate::settings::Settings;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;

pub fn espeak_program() -> Option<&'static str> {
    ["espeak-ng", "espeak"].into_iter().find(|program| {
        Command::new(program).arg("--version").output().is_ok_and(|output| output.status.success())
    })
}

pub(super) fn speak(text: &str, voice: &ResolvedVoice, source_character: &str, local_speaker: Option<&str>,
    volume: &AtomicU16, cancelled: &Arc<AtomicBool>, settings: &Settings, playback: Option<&SpeechPlayback<'_>>) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) || volume.load(Ordering::Relaxed) == 0 || text.trim().is_empty() { return Ok(()); }
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
            Ok(samples) => return play(samples, 16000, volume, cancelled, settings, playback),
            Err(crate::elevenlabs::SpeechError::Cancelled) => return Ok(()),
            Err(error) => crate::state::log(&super::data_directory(), format!("ElevenLabs speech unavailable; using eSpeak: {error}")),
        }
    }
    if cancelled.load(Ordering::Relaxed) { return Ok(()); }
    let preferred = match voice {
        ResolvedVoice::Local { speaker } => speaker.as_deref().or(local_speaker),
        ResolvedVoice::ElevenLabs { .. } => local_speaker,
    };
    let program = espeak_program().ok_or("Offline speech requires espeak-ng or espeak")?;
    let output = Command::new(program).args(["--stdout", "-v",
        preferred.unwrap_or(if source_character == "claude" { "en-us+m2" } else { "en-us+m3" }),
        "-s", if source_character == "claude" { "180" } else { "160" }, "--", text])
        .output().map_err(|error| error.to_string())?;
    if !output.status.success() { return Err(format!("eSpeak synthesis failed: {}", String::from_utf8_lossy(&output.stderr).trim())); }
    let (samples, rate) = decode_espeak(&output.stdout)?;
    play(samples, rate, volume, cancelled, settings, playback)
}

fn play(samples: Vec<i16>, rate: u32, volume: &AtomicU16, cancelled: &AtomicBool,
    settings: &Settings, playback: Option<&SpeechPlayback<'_>>) -> Result<(), String> {
    if samples.is_empty() || cancelled.load(Ordering::Relaxed) { return Ok(()); }
    if settings.silent_sound_seconds > 0 {
        let silence = vec![0; rate as usize * settings.silent_sound_seconds as usize];
        crate::audio::play_pcm(&silence, rate, volume, settings.output_device.as_deref(), cancelled)?;
    }
    if let Some(playback) = playback { if !playback.begin(cancelled) { return Ok(()); } }
    crate::audio::play_pcm(&samples, rate, volume, settings.output_device.as_deref(), cancelled)
}

fn decode_espeak(wav: &[u8]) -> Result<(Vec<i16>, u32), String> {
    if wav.len() < 44 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" { return Err("Invalid eSpeak audio".into()); }
    let mut offset = 12;
    let mut rate = None;
    while offset + 8 <= wav.len() {
        let size = u32::from_le_bytes(wav[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let start = offset + 8;
        if &wav[offset..offset + 4] == b"data" {
            let rate = rate.ok_or("eSpeak audio has no PCM format")?;
            // eSpeak's stdout header declares an open-ended data chunk.
            let data = &wav[start..start + size.min(wav.len() - start)];
            if !data.len().is_multiple_of(2) { return Err("Invalid eSpeak PCM length".into()); }
            return Ok((data.chunks_exact(2).map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]])).collect(), rate));
        }
        let end = start.checked_add(size).filter(|end| *end <= wav.len()).ok_or("Truncated eSpeak audio")?;
        if &wav[offset..offset + 4] == b"fmt " {
            let format = &wav[start..end];
            if format.len() < 16 || format[..2] != [1, 0] || format[2..4] != [1, 0] || format[14..16] != [16, 0] {
                return Err("eSpeak audio must be mono 16-bit PCM".into());
            }
            let sample_rate = u32::from_le_bytes(format[4..8].try_into().unwrap());
            if !(8000..=192000).contains(&sample_rate) { return Err("Invalid eSpeak sample rate".into()); }
            rate = Some(sample_rate);
        }
        offset = end + size % 2;
    }
    Err("eSpeak audio has no PCM data".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_espeak_streamed_wav_at_its_actual_sample_rate() {
        let mut wav = b"RIFF\xff\xff\xff\x7fWAVEfmt \x10\0\0\0\x01\0\x01\0\x22\x56\0\0\x44\xac\0\0\x02\0\x10\0data\xff\xff\xff\x7f".to_vec();
        wav.extend([0, 0, 1, 0, 255, 255]);
        assert_eq!(decode_espeak(&wav).unwrap(), (vec![0, 1, -1], 22050));
    }

    #[test]
    fn rejects_non_pcm_espeak_audio() {
        let wav = b"RIFF\xff\xff\xff\x7fWAVEfmt \x10\0\0\0\x03\0\x01\0\x22\x56\0\0\x44\xac\0\0\x02\0\x10\0data\0\0\0\0";
        assert_eq!(decode_espeak(wav).unwrap_err(), "eSpeak audio must be mono 16-bit PCM");
    }

    #[test]
    fn muted_speech_does_not_require_a_voice_or_output_device() {
        let mut settings = Settings::default();
        settings.output_device = Some("herald-nonexistent-output-device".into());
        let voice = ResolvedVoice::Local { speaker: Some("herald-nonexistent-voice".into()) };
        assert_eq!(speak("Muted preview", &voice, "opencode", None, &AtomicU16::new(0),
            &Arc::new(AtomicBool::new(false)), &settings, None), Ok(()));
    }
}
