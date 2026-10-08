import argparse
import base64
import ctypes
import json
import math
import os
import shutil
import subprocess
import tempfile
import time
import wave
from ctypes import wintypes
from pathlib import Path


def last_message(history, session_id=None):
    result = None
    with Path(history).open(encoding="utf-8") as records:
        for line in records:
            try:
                message = json.loads(line)
            except json.JSONDecodeError:
                continue
            if isinstance(message, dict) and isinstance(message.get("text"), str) and (session_id is None or message.get("sessionID") == session_id):
                result = message
    if result is None:
        raise RuntimeError("No displayed announcement was saved for this session")
    return result


def window_for_process(user32, process_id):
    handles = []
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)

    @callback_type
    def inspect(handle, parameter):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(handle, ctypes.byref(owner))
        if owner.value == process_id:
            handles.append(handle)
        return True

    user32.EnumWindows(inspect, 0)
    return next((handle for handle in handles if user32.IsWindowVisible(handle)), None)


def synthesize(message, output, settings):
    payload = json.dumps({"text": message["text"], "source": message.get("source", "opencode"), "output": str(output), "voices": settings.get("voices", {})})
    encoded_payload = base64.b64encode(payload.encode("utf-8")).decode()
    script = f"""$ErrorActionPreference = 'Stop'
$data = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded_payload}')) | ConvertFrom-Json
$voice = New-Object -ComObject SAPI.SpVoice
$stream = New-Object -ComObject SAPI.SpFileStream
try {{
    $names = if ($data.source -eq 'claude') {{ @('Mark', 'David') }} else {{ @('David', 'Mark') }}
    if ($data.voices.($data.source)) {{ $names = @($data.voices.($data.source)) }}
    $tokens = $voice.GetVoices()
    $chosen = $null
    foreach ($name in $names) {{
        for ($index = 0; $index -lt $tokens.Count; $index++) {{
            $token = $tokens.Item($index)
            if ($token.GetAttribute('Name').IndexOf($name, [StringComparison]::OrdinalIgnoreCase) -ge 0) {{ $chosen = $token; break }}
        }}
        if ($chosen) {{ break }}
    }}
    if ($chosen) {{ $voice.Voice = $chosen }}
    $voice.Rate = if ($data.source -eq 'claude') {{ 1 }} else {{ -1 }}
    $pitch = if ($data.source -eq 'claude') {{ 1 }} else {{ -2 }}
    $stream.Format.Type = 22
    $stream.Open($data.output, 3, $false)
    $voice.AudioOutputStream = $stream
    $text = [Security.SecurityElement]::Escape($data.text)
    $voice.Speak('<pitch absmiddle="' + $pitch + '">' + $text + '</pitch>', 8) | Out-Null
}} finally {{
    $stream.Close()
    [Runtime.InteropServices.Marshal]::FinalReleaseComObject($voice) | Out-Null
    [Runtime.InteropServices.Marshal]::FinalReleaseComObject($stream) | Out-Null
}}
"""
    encoded = base64.b64encode(script.encode("utf-16le")).decode()
    subprocess.run(["powershell.exe", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], check=True, timeout=60)
    with wave.open(str(output)) as audio:
        duration = audio.getnframes() / audio.getframerate()
    if duration <= 0:
        raise RuntimeError("Speech synthesis produced an empty file")
    return duration


def video_timeline(frames, speech_seconds, announcement_seconds, speech_start=0.65, padding_seconds=0.5):
    if not frames:
        raise RuntimeError("The announcer did not render any frames")
    if speech_start + speech_seconds > announcement_seconds:
        raise RuntimeError("The narration extends beyond the rendered announcement")
    if frames[-1]["elapsed"] >= announcement_seconds:
        raise RuntimeError("The final frame is outside the announcement duration")
    speech_delay = padding_seconds + speech_start
    duration = announcement_seconds + padding_seconds * 2
    lines = ["ffconcat version 1.0", "file 'background.png'", "option framerate 1000", f"duration {padding_seconds + frames[0]['elapsed']:.9f}"]
    for index, frame in enumerate(frames):
        end = frames[index + 1]["elapsed"] if index + 1 < len(frames) else announcement_seconds
        lines.extend([f"file '{frame['file']}'", "option framerate 1000", f"duration {max(0.001, end - frame['elapsed']):.9f}"])
    lines.extend(["file 'background.png'", "option framerate 1000", f"duration {padding_seconds:.9f}", "file 'background.png'", "option framerate 1000"])
    return "\n".join(lines) + "\n", speech_delay, duration


def interference_timing(frames, padding_seconds=0.5):
    closing_start = next((frame["closingStart"] for frame in frames if frame.get("closingStart") is not None), None)
    if closing_start is None:
        raise RuntimeError("The closing interference transition was not recorded")
    return padding_seconds, padding_seconds + closing_start


def playback_seconds(text, speech_seconds):
    return max(10, 3 + len(text.split()) * 0.4, speech_seconds + 0.65) + 0.65


def audio_gain(settings):
    volume = settings.get("volume", 100)
    if not isinstance(volume, (int, float)) or not 0 <= volume <= 100:
        raise ValueError("Announcer volume must be between 0 and 100")
    return 0 if volume == 0 else 10 ** ((-60 + 0.6 * volume) / 20)


def main():
    parser = argparse.ArgumentParser()
    messages = parser.add_mutually_exclusive_group(required=True)
    messages.add_argument("--text")
    messages.add_argument("--last", action="store_true")
    parser.add_argument("--session")
    parser.add_argument("--history", type=Path)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("Announcement recording requires Windows")
    from PIL import Image

    root = Path(__file__).resolve().parents[1]
    runtime_data = Path(os.environ.get("HERALD_DATA", str(Path(os.environ["LOCALAPPDATA"]) / "herald")))
    history = args.history or runtime_data / "history.jsonl"
    message = last_message(history, args.session) if args.last else {"text": args.text, "title": "Herald videos", "source": "opencode"}
    settings = json.loads((runtime_data / "settings.json").read_text()) if (runtime_data / "settings.json").exists() else {}
    gain = audio_gain(settings)
    temporary_root = Path(os.environ["LOCALAPPDATA"]) / "Temp/opencode"
    if args.binary:
        binary = args.binary.resolve()
    else:
        build = temporary_root / "herald-build"
        subprocess.run(["cargo", "build", "--release", "--locked", "--manifest-path", str(root / "native-announcer/Cargo.toml"), "--target-dir", str(build)], check=True, timeout=180)
        binary = build / "release/herald.exe"
    user32 = ctypes.windll.user32
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="herald-recording-", dir=temporary_root) as directory:
        temporary = Path(directory)
        data = temporary / "data"
        inbox = data / "inbox"
        inbox.mkdir(parents=True)
        (data / "settings.json").write_text(json.dumps({"nightStart": 0, "nightEnd": 24}))
        assets = root / "native-announcer/resources"
        if args.last:
            source_video = Path(message["video"])
            assets = temporary / "assets"
            (assets / "videos").mkdir(parents=True)
            shutil.copyfile(source_video, assets / "videos" / source_video.name)
        speech_seconds = synthesize(message, temporary / "speech.wav", settings)
        playback = playback_seconds(message["text"], speech_seconds)
        frames_directory = temporary / "frames"
        environment = dict(os.environ, HERALD_DATA=str(data))
        app = subprocess.Popen([str(binary), "--isolated", "--assets", str(assets), "--capture-frames", str(frames_directory), "--capture-speech-seconds", str(speech_seconds), "--test-seconds", str(math.ceil(playback + 15)), "--report", str(temporary / "report.json")], env=environment)
        try:
            last_presence = 0

            def report_presence():
                nonlocal last_presence
                now = time.monotonic()
                if now - last_presence < 2:
                    return
                last_presence = now
                value = {"type": "presence", "clientID": "recording", "sessionIDs": ["screen-recording"], "at": int(time.time() * 1000)}
                path = inbox / f"presence-{time.time_ns()}.tmp"
                path.write_text(json.dumps(value), encoding="utf-8")
                path.rename(path.with_suffix(".json"))

            report_presence()
            notification = {"type": "notify", "id": "screen-recording", "sessionID": "screen-recording", "completed": 1, "text": message["text"], "title": message.get("title", ""), "character": message.get("source", "opencode"), "emotion": "neutral"}
            command = inbox / "message.tmp"
            command.write_text(json.dumps(notification), encoding="utf-8")
            command.rename(command.with_suffix(".json"))
            deadline = time.monotonic() + playback + 15
            handle = None
            while time.monotonic() < deadline:
                report_presence()
                handle = window_for_process(user32, app.pid)
                if handle:
                    break
                if app.poll() is not None:
                    raise RuntimeError("Announcer exited before rendering the message")
                time.sleep(0.02)
            if not handle:
                raise RuntimeError("Notification did not appear")
            while user32.IsWindowVisible(handle) and time.monotonic() < deadline:
                report_presence()
                if app.poll() is not None:
                    raise RuntimeError("Announcer exited before completing the message")
                time.sleep(0.03)
            if not user32.PostMessageW(handle, 0x0010, 0, 0):
                raise RuntimeError("Could not close the isolated recording window")
            app.wait(timeout=10)
            report = json.loads((temporary / "report.json").read_text())
            if app.returncode or report["shown"] != 1 or report["finished"] != 1 or report["decodedVideoFrames"] < 2:
                raise RuntimeError("The announcement did not render completely")
            frames = [json.loads(line) for line in (frames_directory / "timeline.jsonl").read_text().splitlines()]
            timeline, speech_delay, duration = video_timeline(frames, speech_seconds, report["durations"][0])
            opening_delay, closing_delay = interference_timing(frames)
            interference = data / "interference-v4.wav"
            if not interference.exists():
                raise RuntimeError("The announcer did not generate its interference sound")
            (frames_directory / "frames.ffconcat").write_text(timeline, encoding="utf-8")
            preview = next((frame for frame in frames if frame["elapsed"] >= 2), frames[-1])
            with Image.open(frames_directory / preview["file"]) as image:
                image.save(args.output.with_suffix(".png"))
                Image.new("RGB", image.size, "#202020").save(frames_directory / "background.png")
            subprocess.run([
                "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i", str(frames_directory / "frames.ffconcat"), "-i", str(temporary / "speech.wav"), "-i", str(interference),
                "-filter_complex", f"[0:v]fps=30,pad=ceil(iw/2)*2:ceil(ih/2)*2[v];[1:a]adelay={round(speech_delay*1000)}:all=1,apad[speech];[2:a]asplit=2[opening][closing];[opening]adelay={round(opening_delay*1000)}:all=1[start];[closing]adelay={round(closing_delay*1000)}:all=1[end];[speech][start][end]amix=inputs=3:normalize=0,volume={gain}[a]",
                "-map", "[v]", "-map", "[a]", "-t", f"{duration:.9f}", "-c:v", "libx264", "-crf", "16", "-pix_fmt", "yuv420p", "-c:a", "aac", "-movflags", "+faststart", str(args.output)
            ], check=True, timeout=120)
            report.update({"narrationGenerated": True, "speechDelaySeconds": speech_delay, "speechDurationSeconds": speech_seconds, "durationSeconds": duration, "beforeSeconds": 0.5, "afterSeconds": 0.5, "announcementDisappearsSeconds": duration - 0.5, "background": "#202020", "interferenceIncluded": True, "closingInterferenceSeconds": closing_delay, "interferenceGain": 1})
            args.output.with_suffix(".json").write_text(json.dumps({"original": message, "recording": last_message(data / "history.jsonl"), "verification": report}, indent=2), encoding="utf-8")
            print(args.output, flush=True)
        finally:
            if app.poll() is None:
                app.terminate()
                app.wait(timeout=10)


if __name__ == "__main__":
    main()
