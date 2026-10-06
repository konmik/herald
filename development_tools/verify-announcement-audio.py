import argparse
import array
from pathlib import Path
import subprocess
import uuid
import wave

def opening_gap(path):
    with wave.open(str(path), "rb") as recording:
        rate = recording.getframerate()
        channels = recording.getnchannels()
        samples = array.array("h", recording.readframes(recording.getnframes()))
    window = rate // 100 * channels
    audible = []
    for offset in range(0, len(samples) - window + 1, window):
        block = samples[offset:offset + window]
        audible.append(max(abs(value) for value in block) > 32768 * 10 ** (-55 / 20))
    start = next((index for index, value in enumerate(audible) if value), None)
    if start is None:
        raise AssertionError("No static or speech captured on the default output")
    for index in range(start, len(audible) - 10):
        if not any(audible[index:index + 10]):
            speech = next((position for position in range(index + 10, len(audible)) if audible[position]), None)
            if speech is None:
                raise AssertionError("No speech captured after static")
            return (speech - index) / 100
    raise AssertionError("Could not identify the static-to-speech handoff")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", default=f"temp/verification/audio-{uuid.uuid4()}")
    parser.add_argument("--check", type=Path)
    args = parser.parse_args()
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
            with device.open(format=audio.paInt16, channels=channels, rate=rate, input=True, input_device_index=output["index"], frames_per_buffer=1024) as stream:
                process = subprocess.Popen([
                    "pwsh", "-NoProfile", "-File",
                    str(root / ".cursor/skills/verify-civilized-agent/scripts/announce.ps1"),
                    "-Speech", "-Evidence", str(evidence),
                ], cwd=root)
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
                        process.terminate()
                        process.wait(timeout=10)
    gap = opening_gap(recording_path)
    if gap >= 0.35:
        raise AssertionError(f"FAIL: static-to-speech silence {gap:.2f}s exceeds 0.35s")
    print(f"PASS: static-to-speech silence {gap:.2f}s is below 0.35s")


if __name__ == "__main__":
    main()
