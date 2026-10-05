import argparse
import ctypes
import json
import os
import subprocess
import tempfile
import threading
import time
from pathlib import Path
from ctypes import wintypes


def main():
    ctypes.windll.user32.SetProcessDPIAware()
    parser = argparse.ArgumentParser()
    parser.add_argument("--text", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    import numpy as np
    import soundcard
    import soundfile
    from PIL import ImageGrab

    root = Path(__file__).resolve().parents[1]
    binary = root / "native-announcer/bin/civilized-announcer-win32-x64.exe"
    user32 = ctypes.windll.user32
    user32.FindWindowW.restype = wintypes.HWND
    user32.FindWindowW.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    temporary_root = Path(os.environ["LOCALAPPDATA"]) / "Temp/opencode"
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="herald-screen-", dir=temporary_root) as directory:
        temporary = Path(directory)
        data = temporary / "data"
        inbox = data / "inbox"
        inbox.mkdir(parents=True)
        (data / "settings.json").write_text(json.dumps({"nightStart": 22, "nightEnd": 22}))
        environment = dict(os.environ, CIVILIZED_AGENT_DATA=str(data))
        app = subprocess.Popen([str(binary), "--assets", str(root / "native-announcer/resources"),
                                "--test-seconds", "90", "--report", str(temporary / "report.json")], env=environment)
        stop = threading.Event()
        ready = threading.Event()
        audio_blocks = []
        audio_errors = []
        audio_started = []
        video = None
        audio_thread = None
        try:
            last_presence = 0

            def report_presence():
                nonlocal last_presence
                now = time.monotonic()
                if now - last_presence < 2:
                    return
                last_presence = now
                message = {"type": "presence", "clientID": "recording", "sessionIDs": ["screen-recording"], "at": int(time.time() * 1000)}
                path = inbox / f"presence-{time.time_ns()}.tmp"
                path.write_text(json.dumps(message), encoding="utf-8")
                path.rename(path.with_suffix(".json"))

            def record_audio():
                try:
                    speaker = soundcard.default_speaker()
                    microphone = soundcard.get_microphone(speaker.id, include_loopback=True)
                    with microphone.recorder(samplerate=48000, channels=2, blocksize=1024) as recorder:
                        audio_started.append(time.perf_counter())
                        ready.set()
                        while not stop.is_set():
                            audio_blocks.append(recorder.record(numframes=1024))
                except BaseException as error:
                    audio_errors.append(error)
                    ready.set()

            audio_thread = threading.Thread(target=record_audio)
            audio_thread.start()
            if not ready.wait(15) or audio_errors:
                raise RuntimeError(f"System audio capture failed: {audio_errors}")
            time.sleep(1)
            report_presence()
            notification = {"type": "notify", "id": "screen-recording", "sessionID": "screen-recording",
                            "completed": 1, "text": args.text, "title": "Herald videos", "character": "opencode", "emotion": "neutral"}
            (inbox / "message.json").write_text(json.dumps(notification), encoding="utf-8")
            deadline = time.monotonic() + 45
            handle = None
            while time.monotonic() < deadline:
                report_presence()
                handle = user32.FindWindowW(None, "Civilized Agent")
                if handle and user32.IsWindowVisible(handle):
                    break
                if app.poll() is not None:
                    raise RuntimeError("Announcer exited before showing the message")
                time.sleep(0.02)
            else:
                raise RuntimeError("Notification did not appear")
            rectangle = wintypes.RECT()
            if not user32.GetWindowRect(handle, ctypes.byref(rectangle)):
                raise RuntimeError("Cannot find notification bounds")
            width = rectangle.right - rectangle.left
            height = rectangle.bottom - rectangle.top
            video_started = time.perf_counter()
            video = subprocess.Popen([
                "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "gdigrab",
                "-framerate", "30", "-draw_mouse", "0", "-offset_x", str(rectangle.left),
                "-offset_y", str(rectangle.top), "-video_size", f"{width}x{height}", "-i", "desktop",
                "-c:v", "libx264", "-crf", "0", "-preset", "ultrafast", "-pix_fmt", "yuv444p",
                str(temporary / "screen.mkv")], stdin=subprocess.PIPE)
            preview_saved = False
            while user32.IsWindowVisible(handle) and time.monotonic() < deadline:
                report_presence()
                if video.poll() is not None:
                    raise RuntimeError("Screen recording stopped early")
                if not preview_saved and time.perf_counter() - video_started > 2:
                    ImageGrab.grab(bbox=(rectangle.left, rectangle.top, rectangle.right, rectangle.bottom)).save(args.output.with_suffix(".png"))
                    preview_saved = True
                time.sleep(0.03)
            video.communicate(b"q\n", timeout=15)
            if video.returncode:
                raise RuntimeError("Screen capture failed")
            stop.set()
            audio_thread.join(timeout=10)
            if audio_thread.is_alive() or audio_errors or not audio_blocks:
                raise RuntimeError(f"Audio recording failed: {audio_errors}")
            samples = np.concatenate(audio_blocks)
            soundfile.write(temporary / "audio.wav", samples, 48000, subtype="PCM_16")
            if np.max(np.abs(samples)) < 0.001:
                raise RuntimeError("System audio recording is silent")
            subprocess.run([
                "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(temporary / "screen.mkv"),
                "-ss", str(video_started - audio_started[0]), "-i", str(temporary / "audio.wav"),
                "-map", "0:v:0", "-map", "1:a:0", "-c:v", "libx264", "-crf", "16",
                "-pix_fmt", "yuv420p", "-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:a", "aac",
                "-shortest", "-movflags", "+faststart", str(args.output)], check=True)
            print(args.output, flush=True)
        finally:
            stop.set()
            if video is not None and video.poll() is None:
                video.communicate(b"q\n", timeout=15)
            if audio_thread is not None:
                audio_thread.join(timeout=10)
            if app.poll() is None:
                app.terminate()
                app.wait(timeout=10)


if __name__ == "__main__":
    main()
