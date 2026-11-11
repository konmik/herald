import argparse
import array
from datetime import datetime
import json
import math
from pathlib import Path
import subprocess
import tempfile
import time
import uuid
import wave

def speech_envelope(path):
    with wave.open(str(path), "rb") as recording:
        if recording.getsampwidth() != 2:
            raise AssertionError("Expected 16-bit PCM speech")
        samples = array.array("h", recording.readframes(recording.getnframes()))
        window = recording.getframerate() // 100 * recording.getnchannels()
    return [math.sqrt(sum(value * value for value in samples[offset:offset + window]) / window)
            for offset in range(0, len(samples) - window + 1, window)]


def verify_chimera_audio(recording_path, reference_path):
    import numpy as np

    reference = speech_envelope(reference_path)
    recorded = speech_envelope(recording_path)
    if not reference:
        raise AssertionError("CHIMERA reference speech is empty")
    mean = sum(reference) / len(reference)
    centered = [value - mean for value in reference]
    reference_energy = sum(value * value for value in centered)
    if reference_energy <= 0 or max(reference) < 10:
        raise AssertionError("CHIMERA reference speech is silent")
    best = (-1.0, 0, 0.0)
    count = len(reference)
    for offset in range(len(recorded) - count + 1):
        candidate = recorded[offset:offset + count]
        candidate_mean = sum(candidate) / count
        energy = sum((value - candidate_mean) ** 2 for value in candidate)
        if energy <= 0:
            continue
        correlation = sum(a * b for a, b in zip(centered, candidate)) / math.sqrt(reference_energy * energy)
        if correlation > best[0]:
            best = correlation, offset / 100, max(candidate)
    def waveform(path):
        with wave.open(str(path), "rb") as recording:
            samples = np.frombuffer(recording.readframes(recording.getnframes()), dtype="<i2")
            return recording.getframerate(), np.diff(samples.reshape(-1, recording.getnchannels()).mean(axis=1))

    rate, samples = waveform(recording_path)
    reference_rate, expected = waveform(reference_path)
    if rate != reference_rate or len(samples) < len(expected):
        raise AssertionError("CHIMERA output and reference need matching sample rates and a complete recording")
    expected -= expected.mean()
    size = 1 << (len(samples) + len(expected) - 2).bit_length()
    correlation = np.fft.irfft(np.fft.rfft(samples, size) * np.fft.rfft(expected[::-1], size), size)[len(expected) - 1:len(samples)]
    squares = np.concatenate(([0], np.cumsum(samples * samples)))
    energy = squares[len(expected):] - squares[:-len(expected)]
    normalized = correlation / np.sqrt(np.maximum(1, energy) * np.sum(expected * expected))
    offset = int(np.argmax(normalized))
    proof = {"correlation": float(normalized[offset]), "envelopeCorrelation": best[0], "speechAtSeconds": offset / rate, "peakRms": best[2], "reference": str(reference_path)}
    if proof["correlation"] < 0.6 or best[0] < 0.5 or best[2] < 10:
        raise AssertionError(f"CHIMERA speech was not found in actual output: {proof}")
    return proof


def audio_activity(path):
    with wave.open(str(path), "rb") as recording:
        rate = recording.getframerate()
        channels = recording.getnchannels()
        samples = array.array("h", recording.readframes(recording.getnframes()))
    window = rate // 100 * channels
    threshold = max(1, max(abs(value) for value in samples) / 200)
    audible = []
    for offset in range(0, len(samples) - window + 1, window):
        block = samples[offset:offset + window]
        audible.append(max(abs(value) for value in block) > threshold)
    return audible


def static_gap(audible):
    start = next((index for index, value in enumerate(audible) if value), None)
    if start is None:
        raise AssertionError("No static or speech captured on the default output")
    if len(audible) < start + 100:
        raise AssertionError("Recording ends before the speech handoff can be checked")
    for index in range(start + 30, min(start + 80, len(audible) - 10)):
        if not any(audible[index:index + 10]):
            speech = next((position for position in range(index + 10, len(audible)) if audible[position]), None)
            if speech is None:
                raise AssertionError("No speech captured after static")
            return (speech - index) / 100
    if not any(audible[start + 65:start + 100]):
        raise AssertionError("No speech captured at the end of the opening static")
    return 0.0


def opening_gap(path):
    return static_gap(audio_activity(path))


def first_audio_time(audible, started_at, after, before):
    start = max(0, int((after - started_at) * 100))
    end = min(len(audible), int((before - started_at) * 100) + 1)
    index = next((index for index in range(start, end) if audible[index]), None)
    if index is None:
        raise AssertionError("No audio captured for the requested playback")
    return started_at + index / 100


def verify_announcement_order(path, evidence, delay_seconds, started_at):
    requests = [json.loads(line) for line in (evidence / "speech-api-requests.jsonl").read_text(encoding="utf-8-sig").splitlines()]
    if len(requests) != 1 or requests[0]["method"] != "POST":
        raise AssertionError("Expected one fixture speech request")
    history = [json.loads(line) for line in (evidence / "history.jsonl").read_text(encoding="utf-8-sig").splitlines()]
    if len(history) != 1:
        raise AssertionError("Expected one shown notification")
    requested_at = requests[0]["at"] / 1000
    shown_at = datetime.fromisoformat(history[0]["shownAt"].replace("Z", "+00:00")).timestamp()
    audible = audio_activity(path)
    sound_at = first_audio_time(audible, started_at, requested_at - 0.1, started_at + len(audible) / 100)
    if shown_at < requested_at + delay_seconds - 0.2:
        raise AssertionError("Notification appeared before the silent delay completed")
    if sound_at < requested_at + delay_seconds - 0.2:
        raise AssertionError("Static played before the silent delay completed")
    if abs(sound_at - shown_at) > 0.4:
        raise AssertionError("Notification and opening static did not start together")
    gap = static_gap(audible)
    if gap >= 0.35:
        raise AssertionError(f"Static-to-speech silence {gap:.2f}s exceeds 0.35s")
    return {"delaySeconds": delay_seconds, "requestAt": requested_at, "shownAt": shown_at, "staticAt": sound_at, "staticToSpeechGap": gap}


def verify_preview_order(path, evidence, delay_seconds, started_at):
    events = [json.loads(line) for line in (evidence / "preview-events.jsonl").read_text(encoding="utf-8-sig").splitlines()]
    if [event["control"] for event in events] != [210, 112, 210, 112]:
        raise AssertionError("Expected both preview buttons for character and default voices")
    audible = audio_activity(path)
    proof = []
    for event in events:
        requested_at = event["requestAt"] / 1000
        finished_at = event["finishedAt"] / 1000
        sound_at = first_audio_time(audible, started_at, requested_at, finished_at)
        if sound_at < requested_at + delay_seconds - 0.2:
            raise AssertionError(f"Preview control {event['control']} played before the silent delay completed")
        gap = None
        if event["control"] == 112:
            start = max(0, int((requested_at - started_at) * 100))
            end = min(len(audible), int((finished_at - started_at) * 100) + 1)
            gap = static_gap(audible[start:end])
            if gap >= 0.35:
                raise AssertionError(f"Example static-to-speech silence {gap:.2f}s exceeds 0.35s")
        proof.append({"control": event["control"], "requestAt": requested_at, "soundAt": sound_at, "staticToSpeechGap": gap})
    return {"delaySeconds": delay_seconds, "previews": proof}


def main():
    parser = argparse.ArgumentParser(epilog="Capture requires PyAudioWPatch. CHIMERA waveform checks also require numpy.")
    parser.add_argument("--evidence", default=str(Path(tempfile.gettempdir()) / f"herald/verification/audio-{uuid.uuid4()}"))
    parser.add_argument("--check", type=Path)
    parser.add_argument("--delay-seconds", type=int, choices=range(11), default=0)
    parser.add_argument("--settings-previews", action="store_true")
    parser.add_argument("--app-directory", type=Path)
    parser.add_argument("--chimera-reference", type=Path)
    args = parser.parse_args()
    if args.chimera_reference and args.settings_previews:
        parser.error("CHIMERA waveform verification requires announcement playback")
    recording_path = args.check
    if recording_path is None:
        import pyaudiowpatch as audio

        root = Path(__file__).resolve().parent.parent
        evidence = root / args.evidence
        evidence.mkdir(parents=True, exist_ok=False)
        recording_path = evidence / "loopback.wav"
        with audio.PyAudio() as device:
            output = device.get_default_wasapi_loopback()
            rate = int(output["defaultSampleRate"])
            channels = output["maxInputChannels"]
            with device.open(format=audio.paInt16, channels=channels, rate=rate, input=True, input_device_index=output["index"], frames_per_buffer=1024, start=False) as stream:
                helper = "verify.ps1" if args.settings_previews else "announce.ps1"
                command = ["pwsh", "-NoProfile", "-File", str(root / ".claude/skills/verify-herald/scripts" / helper)]
                if args.settings_previews:
                    command.extend(["-Feature", "VoicePreview", "-Audible"])
                elif args.chimera_reference:
                    command.extend(["-Speech", "-CharacterId", "chimera", "-CaptureFrames", "-Volume", "100"])
                else:
                    command.extend(["-Speech", "-SpeechFixture"])
                command.extend(["-Evidence", str(evidence / "playback"), "-SilentSoundSeconds", str(args.delay_seconds)])
                if args.app_directory:
                    command.extend(["-AppDirectory", str(args.app_directory)])
                    if args.settings_previews:
                        command.append("-SkipLocalPreview")
                started_at = time.time()
                stream.start_stream()
                (evidence / "capture.json").write_text(json.dumps({"startedAt": started_at, "sampleRate": rate, "channels": channels}), encoding="utf-8")
                process = subprocess.Popen(command, cwd=root)
                try:
                    with wave.open(str(recording_path), "wb") as recording:
                        recording.setnchannels(channels)
                        recording.setsampwidth(2)
                        recording.setframerate(rate)
                        while process.poll() is None:
                            recording.writeframes(stream.read(1024, exception_on_overflow=False))
                    if process.returncode:
                        raise RuntimeError(f"Playback verification exited {process.returncode}")
                finally:
                    if process.poll() is None:
                        try:
                            process.wait(timeout=55)
                        except subprocess.TimeoutExpired:
                            subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], check=True, timeout=10)
                            process.wait(timeout=10)
    if args.chimera_reference:
        proof = verify_chimera_audio(recording_path, args.chimera_reference)
        if args.check is None:
            (evidence / "chimera-audio.json").write_text(json.dumps(proof, indent=2), encoding="utf-8")
        print(f"PASS: CHIMERA speech captured on the actual output, correlation {proof['correlation']:.3f}")
        return
    if args.check is None:
        verifier = verify_preview_order if args.settings_previews else verify_announcement_order
        proof = verifier(recording_path, evidence / "playback", args.delay_seconds, started_at)
        (evidence / "order.json").write_text(json.dumps(proof, indent=2), encoding="utf-8")
        print("PASS: selected silence precedes presentation; static is followed immediately by speech")
        return
    gap = opening_gap(recording_path)
    if gap >= 0.35:
        raise AssertionError(f"FAIL: static-to-speech silence {gap:.2f}s exceeds 0.35s")
    else:
        print("PASS: no static-to-speech silence exceeds 0.35s")


if __name__ == "__main__":
    main()
