import argparse
import ctypes
import json
import os
import shutil
import subprocess
import tempfile
import threading
import time
from pathlib import Path
from ctypes import wintypes


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


def main():
    parser = argparse.ArgumentParser()
    messages = parser.add_mutually_exclusive_group(required=True)
    messages.add_argument("--text")
    messages.add_argument("--last", action="store_true")
    parser.add_argument("--session")
    parser.add_argument("--history", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt":
        parser.error("Screen recording requires Windows")
    ctypes.windll.user32.SetProcessDPIAware()
    import numpy as np
    import soundcard
    import soundfile
    from PIL import ImageGrab

    root = Path(__file__).resolve().parents[1]
    binary = root / "native-announcer/bin/civilized-announcer-win32-x64.exe"
    history = args.history or Path(os.environ.get("CIVILIZED_AGENT_DATA", str(Path(os.environ["LOCALAPPDATA"]) / "CivilizedAgent"))) / "history.jsonl"
    message = last_message(history, args.session) if args.last else {"text": args.text, "title": "Herald videos", "source": "opencode"}
    user32 = ctypes.windll.user32
    user32.FindWindowW.restype = wintypes.HWND
    user32.FindWindowW.argtypes = [wintypes.LPCWSTR, wintypes.LPCWSTR]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    temporary_root = Path(os.environ["LOCALAPPDATA"]) / "Temp/opencode"
    args.output = args.output.resolve()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="herald-screen-", dir=temporary_root) as directory:
        temporary = Path(directory)
        data = temporary / "data"
        inbox = data / "inbox"
        inbox.mkdir(parents=True)
        (data / "settings.json").write_text(json.dumps({"nightStart": 22, "nightEnd": 22}))
        assets = root / "native-announcer/resources"
        if args.last:
            source_video = Path(message["video"])
            assets = temporary / "assets"
            (assets / "videos").mkdir(parents=True)
            shutil.copyfile(source_video, assets / "videos" / source_video.name)
        environment = dict(os.environ, CIVILIZED_AGENT_DATA=str(data))
        app = subprocess.Popen([str(binary), "--isolated", "--assets", str(assets),
                                "--test-seconds", "90", "--report", str(temporary / "report.json")], env=environment)
        stop = threading.Event()
        ready = threading.Event()
        audio_blocks = []
        audio_errors = []
        audio_started = []
        video = None
        audio_thread = None
        progress_thread = None
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
            time.sleep(2)
            desktop_x, desktop_y = user32.GetSystemMetrics(76), user32.GetSystemMetrics(77)
            desktop_width, desktop_height = user32.GetSystemMetrics(78), user32.GetSystemMetrics(79)
            video_ready = threading.Event()
            video_started = []
            video = subprocess.Popen([
                "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-progress", "pipe:1", "-nostats",
                "-f", "gdigrab", "-framerate", "30", "-draw_mouse", "0",
                "-offset_x", str(desktop_x), "-offset_y", str(desktop_y),
                "-video_size", f"{desktop_width}x{desktop_height}", "-i", "desktop",
                "-c:v", "libx264", "-crf", "0", "-preset", "ultrafast", "-pix_fmt", "yuv444p",
                str(temporary / "screen.mkv")], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

            def video_progress():
                for line in video.stdout:
                    if line.startswith("frame=") and int(line.split("=", 1)[1]) > 0 and not video_ready.is_set():
                        video_started.append(time.perf_counter())
                        video_ready.set()

            progress_thread = threading.Thread(target=video_progress)
            progress_thread.start()
            if not video_ready.wait(15):
                raise RuntimeError("Screen capture did not start")
            report_presence()
            notification = {"type": "notify", "id": "screen-recording", "sessionID": "screen-recording",
                            "completed": 1, "text": message["text"], "title": message.get("title", ""), "character": message.get("source", "opencode"), "emotion": "neutral"}
            command = inbox / "message.tmp"
            command.write_text(json.dumps(notification), encoding="utf-8")
            command.rename(command.with_suffix(".json"))
            deadline = time.monotonic() + 45
            handle = None
            while time.monotonic() < deadline:
                report_presence()
                handle = window_for_process(user32, app.pid)
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
            appeared = time.perf_counter()
            preview_saved = False
            while user32.IsWindowVisible(handle) and time.monotonic() < deadline:
                report_presence()
                if video.poll() is not None:
                    raise RuntimeError("Screen recording stopped early")
                if not preview_saved and time.perf_counter() - appeared > 2:
                    ImageGrab.grab(bbox=(rectangle.left, rectangle.top, rectangle.right, rectangle.bottom)).save(args.output.with_suffix(".png"))
                    preview_saved = True
                time.sleep(0.03)
            video.stdin.write("q\n")
            video.stdin.flush()
            video.wait(timeout=15)
            progress_thread.join(timeout=5)
            if video.returncode:
                raise RuntimeError("Screen capture failed")
            stop.set()
            audio_thread.join(timeout=10)
            if audio_thread.is_alive() or audio_errors or not audio_blocks:
                raise RuntimeError(f"Audio recording failed: {audio_errors}")
            user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
            if not user32.PostMessageW(handle, 0x0010, 0, 0):
                raise RuntimeError("Could not close the isolated recording window")
            app.wait(timeout=10)
            report = json.loads((temporary / "report.json").read_text())
            if app.returncode or report["speechStarted"] != 1 or report["mutedAnnouncements"] or report["finished"] != 1:
                raise RuntimeError("The announcement did not finish with narration; check meeting detection")
            samples = np.concatenate(audio_blocks)
            soundfile.write(temporary / "audio.wav", samples, 48000, subtype="PCM_16")
            if np.max(np.abs(samples)) < 0.001:
                raise RuntimeError("System audio recording is silent")
            subprocess.run([
                "ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-i", str(temporary / "screen.mkv"),
                "-ss", str(max(0, video_started[0] - audio_started[0])), "-i", str(temporary / "audio.wav"),
                "-map", "0:v:0", "-map", "1:a:0", "-c:v", "libx264", "-crf", "16",
                "-pix_fmt", "yuv420p", "-vf", f"crop={width}:{height}:{rectangle.left-desktop_x}:{rectangle.top-desktop_y},pad=ceil(iw/2)*2:ceil(ih/2)*2,fps=30", "-c:a", "aac",
                "-shortest", "-movflags", "+faststart", str(args.output)], check=True)
            print(args.output, flush=True)
            (args.output.with_suffix(".json")).write_text(json.dumps({"original": message, "recording": last_message(data / "history.jsonl"), "verification": report}, indent=2), encoding="utf-8")
        finally:
            stop.set()
            if video is not None and video.poll() is None:
                video.stdin.write("q\n")
                video.stdin.flush()
                video.wait(timeout=15)
            if progress_thread is not None:
                progress_thread.join(timeout=5)
            if audio_thread is not None:
                audio_thread.join(timeout=10)
            if app.poll() is None:
                app.terminate()
                app.wait(timeout=10)


if __name__ == "__main__":
    main()
