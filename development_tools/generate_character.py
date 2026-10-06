import argparse
import base64
import json
import os
import re
import secrets
import shutil
import subprocess
import tempfile
import time
import wave
from contextlib import contextmanager
from fractions import Fraction
from pathlib import Path

from PIL import Image

if __package__:
    from . import generate_assets as generator
else:
    import generate_assets as generator


SPEECH = "I bring news for your attention. Listen as I deliver this announcement."
ROOT = Path(__file__).resolve().parents[1]


def validate_name(name):
    if not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", name) or len(name) > 64:
        raise ValueError("Use a name of at most 64 lowercase letters, digits and hyphens")
    if name.upper() in {"CON", "PRN", "AUX", "NUL", *(f"COM{i}" for i in range(1, 10)), *(f"LPT{i}" for i in range(1, 10))}:
        raise ValueError("That name is reserved by Windows")
    return name


def temp_root():
    if os.name == "nt":
        return Path(os.environ["LOCALAPPDATA"]) / "Temp" / "opencode"
    return Path(tempfile.gettempdir())


def get_json(route):
    response = generator.requests.get(generator.API + route, timeout=10)
    response.raise_for_status()
    return response.json()


def ensure_idle():
    queue = get_json("/queue")
    if queue.get("queue_running") or queue.get("queue_pending"):
        raise RuntimeError("ComfyUI is busy; wait for the existing job to finish")


@contextmanager
def comfy_server(comfy):
    process = None
    try:
        try:
            stats = get_json("/system_stats")
        except generator.requests.ConnectionError:
            python = comfy / ("venv/Scripts/python.exe" if os.name == "nt" else "venv/bin/python")
            process = subprocess.Popen(
                [str(python), "main.py", "--listen", "127.0.0.1", "--port", "8188"],
                cwd=comfy, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
            )
            deadline = time.monotonic() + 180
            while True:
                if process.poll() is not None:
                    raise RuntimeError("ComfyUI exited before becoming ready")
                try:
                    stats = get_json("/system_stats")
                    break
                except generator.requests.ConnectionError:
                    if time.monotonic() >= deadline:
                        raise TimeoutError("ComfyUI did not become ready within three minutes")
                    time.sleep(2)
        if not any(device.get("type") == "cuda" for device in stats.get("devices", [])):
            raise RuntimeError("ComfyUI did not report a CUDA device")
        ensure_idle()
        yield
    finally:
        if process is not None:
            process.terminate()
            try:
                process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)


def speech_audio(work):
    if os.name != "nt":
        raise RuntimeError("Supply --audio with the full announcement phrase on this platform")
    raw = work / "speech.wav"
    script = f"""$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
$speech = [System.Speech.Synthesis.SpeechSynthesizer]::new()
try {{
    $speech.SelectVoice('Microsoft David Desktop')
    $speech.SetOutputToWaveFile('{str(raw).replace("'", "''")}')
    $speech.Speak('{SPEECH}')
}} finally {{ $speech.Dispose() }}
"""
    encoded = base64.b64encode(script.encode("utf-16le")).decode()
    subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], check=True, timeout=60)
    return raw


def prepare_audio(source, output):
    metadata = json.loads(subprocess.check_output(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "json", str(source)], timeout=30,
    ))
    duration = float(metadata["format"]["duration"])
    if duration <= 0:
        raise ValueError("Speech audio is empty")
    tempo = duration / 4
    filters = []
    while tempo < 0.5:
        filters.append("atempo=0.5")
        tempo *= 2
    while tempo > 2:
        filters.append("atempo=2")
        tempo /= 2
    filters.extend([f"atempo={tempo}", "apad", "atrim=duration=4"])
    subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(source), "-af", ",".join(filters), "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", str(output)],
        check=True, timeout=60,
    )
    with wave.open(str(output)) as audio:
        if (audio.getnchannels(), audio.getframerate(), audio.getsampwidth(), audio.getnframes()) != (1, 16000, 2, 64000):
            raise RuntimeError("Expected four seconds of mono 16 kHz PCM speech")


def verify_portrait(path):
    with Image.open(path) as image:
        if image.size != (256, 256):
            raise RuntimeError(f"Expected native 256×256 portrait, got {image.size}")


def verify_video(path):
    data = json.loads(subprocess.check_output(
        ["ffprobe", "-v", "error", "-count_frames", "-show_streams", "-show_format", "-of", "json", str(path)], timeout=60,
    ))
    streams = data["streams"]
    if len(streams) != 1 or streams[0]["codec_type"] != "video":
        raise RuntimeError("Expected a silent video with exactly one stream")
    video = streams[0]
    if (video["width"], video["height"], Fraction(video["r_frame_rate"]), int(video["nb_read_frames"])) != (256, 256, 16, 64):
        raise RuntimeError("Expected 256×256 video with 64 frames at 16 fps")
    if abs(float(data["format"]["duration"]) - 4) > 0.05:
        raise RuntimeError("Expected a four-second video")


def prepare_video(source, output):
    subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(source), "-map", "0:v:0", "-c:v", "copy", "-an", "-map_metadata", "-1", "-movflags", "+faststart", str(output)],
        check=True, timeout=60,
    )


def copy_new(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    with Path(source).open("rb") as input_file:
        with destination.open("xb") as output:
            try:
                shutil.copyfileobj(input_file, output)
            except BaseException:
                output.close()
                destination.unlink()
                raise


def cancel_owned_jobs(work):
    for path in work.glob("*-job.json"):
        prompt_id = json.loads(path.read_text())["prompt_id"]
        queue = get_json("/queue")
        if any(item[1] == prompt_id for item in queue.get("queue_pending", [])):
            generator.requests.post(generator.API + "/queue", json={"delete": [prompt_id]}, timeout=10).raise_for_status()
        if any(item[1] == prompt_id for item in queue.get("queue_running", [])):
            generator.requests.post(generator.API + "/interrupt", json={"prompt_id": prompt_id}, timeout=10).raise_for_status()
            deadline = time.monotonic() + 60
            while any(item[1] == prompt_id for item in get_json("/queue").get("queue_running", [])):
                if time.monotonic() >= deadline:
                    raise TimeoutError("Owned ComfyUI job did not stop; temporary files were retained")
                time.sleep(1)


def generate(name, description, comfy, audio=None, resume=False, seed=None):
    validate_name(name)
    portrait = ROOT / "native-announcer/resources/portraits" / f"{name}.png"
    video = ROOT / "native-announcer/resources/videos" / f"{name}.mp4"
    if video.exists() or (portrait.exists() and not resume):
        raise FileExistsError("Character already exists; use a new name, or --resume for a portrait without a video")
    if resume and not portrait.exists():
        raise FileNotFoundError("--resume requires the saved portrait")
    if audio is not None and not audio.is_file():
        raise FileNotFoundError(audio)
    for tool in ("ffmpeg", "ffprobe"):
        if not shutil.which(tool):
            raise RuntimeError(f"{tool} is required")
    scratch = temp_root()
    scratch.mkdir(parents=True, exist_ok=True)
    lock = scratch / "civilized-character-generation.lock"
    with lock.open("x"):
        pass
    work = Path(tempfile.mkdtemp(prefix="civilized-character-", dir=scratch))
    previous = generator.ASSETS, generator.COMFY, generator.RUNTIME_ASSETS
    generator.ASSETS, generator.COMFY, generator.RUNTIME_ASSETS = work, comfy, work
    label = work.name
    outputs = []
    clean = False
    try:
        speech = work / "announcement.wav"
        prepare_audio(audio or speech_audio(work), speech)
        with comfy_server(comfy):
            try:
                if portrait.exists():
                    verify_portrait(portrait)
                    reference = work / "portraits" / f"{label}.png"
                    reference.parent.mkdir(parents=True)
                    shutil.copyfile(portrait, reference)
                else:
                    prompt = f"{description.strip()}. Realistic photographed character, muted colors, tightly framed head and shoulders, entire head and headwear visible, direct eye contact, eyes open, mouth closed, plain dark charcoal background, soft even studio lighting, square 256×256 composition, no text or watermark."
                    reference, source = generator.portrait(label, size=256, steps=20, prompt_override=prompt, seed=seed if seed is not None else secrets.randbits(48), retain_native=True)
                    outputs.append(source)
                    verify_portrait(reference)
                    copy_new(reference, portrait)
                ensure_idle()
                result, source = generator.video(
                    "neutral", label, size=256, frames=65, steps=20, fps=16, output_frames=64,
                    seed=seed if seed is not None else secrets.randbits(48), audio_path=speech, publish=False,
                    prompt="Enthusiastically delivering an announcement with visible natural mouth movement. Preserve the reference identity, costume, props and framing. Fixed camera and lighting.",
                )
                outputs.append(source)
                stripped = work / "library.mp4"
                prepare_video(result, stripped)
                verify_video(stripped)
                copy_new(stripped, video)
            finally:
                cancel_owned_jobs(work)
                clean = True
                generator.release_idle_models()
        return portrait, video
    finally:
        try:
            if clean or not list(work.glob("*-job.json")):
                for path in outputs:
                    path.unlink(missing_ok=True)
                output_directory = comfy / "output" / "civilized" / label
                if output_directory.exists():
                    shutil.rmtree(output_directory)
                for filename in (f"civilized-{label}-portrait.png", f"civilized-{label}-neutral.wav"):
                    (comfy / "input" / filename).unlink(missing_ok=True)
                shutil.rmtree(work)
        finally:
            generator.ASSETS, generator.COMFY, generator.RUNTIME_ASSETS = previous
            lock.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description="Generate and publish one character portrait and talking video")
    parser.add_argument("name", type=validate_name)
    parser.add_argument("--description", required=True)
    parser.add_argument("--comfy-dir", type=Path, default=generator.COMFY)
    parser.add_argument("--audio", type=Path, help="Speech recording of the full announcement phrase; normalized to four seconds")
    parser.add_argument("--resume", action="store_true", help="Keep the saved portrait and generate its missing video")
    parser.add_argument("--seed", type=int)
    args = parser.parse_args()
    if not args.description.strip():
        parser.error("--description must not be empty")
    try:
        for path in generate(args.name, args.description, args.comfy_dir, args.audio, args.resume, args.seed):
            print(path)
    except (OSError, RuntimeError, ValueError, generator.requests.RequestException, subprocess.SubprocessError) as error:
        parser.exit(1, f"{error}\n")


if __name__ == "__main__":
    main()
