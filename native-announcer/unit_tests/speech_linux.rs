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
