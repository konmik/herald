import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import wave
import array

from PIL import Image


def hypr(command):
    return json.loads(subprocess.check_output(["hyprctl", "-j", command]))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--silent", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    evidence = Path(tempfile.mkdtemp(prefix="linux-playback-", dir=root / "temp/verification"))
    data = Path(tempfile.mkdtemp(prefix="herald-playback-", dir="/tmp/opencode"))
    env = dict(os.environ, HERALD_DATA=str(data))
    settings = {"scheduleEnabled": False, "volume": 0 if args.silent else 60, "selectedCharacter": "hatted-herald-07"}
    (data / "settings.json").write_text(json.dumps(settings))
    (evidence / "settings.json").write_text(json.dumps(settings))
    before = hypr("activewindow")
    subprocess.run(["grim", "-l", "0", str(evidence / "before.png")], check=True)
    process = recorder = None
    samples = []
    try:
        with (evidence / "runtime.log").open("w") as log, (evidence / "audio.log").open("w") as audio_log:
            if not args.silent:
                sink = subprocess.check_output(["pactl", "get-default-sink"], text=True).strip()
                recorder = subprocess.Popen(["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "warning", "-f", "pulse", "-i", sink + ".monitor", "-t", "20", "-ar", "16000", "-ac", "1", str(evidence / "audio.wav")], stdout=audio_log, stderr=audio_log)
            process = subprocess.Popen([args.binary, "--isolated", "--assets", str(root / "native-announcer/resources"), "--demo", "opencode", "--test-seconds", "18", "--report", str(evidence / "report.json"), "--snapshot", str(evidence / "render.png")], env=env, stdout=log, stderr=log)
            started = time.monotonic()
            while process.poll() is None and time.monotonic() - started < 25:
                clients = hypr("clients")
                window = next((item for item in clients if item["pid"] == process.pid and item["mapped"] and not item["hidden"]), None)
                if window:
                    index = len(samples)
                    shot = evidence / f"desktop-{index:03}.png"
                    subprocess.run(["grim", "-l", "0", str(shot)], check=True)
                    samples.append({"elapsed": time.monotonic() - started, "window": window, "focus": hypr("activewindow").get("address"), "image": shot.name})
                time.sleep(0.25)
            process.wait(timeout=5)
            if recorder:
                recorder.wait(timeout=8)
        for name in ["errors.log", "history.jsonl", "queue.json"]:
            if (data / name).exists():
                shutil.copy2(data / name, evidence / name)
        (evidence / "samples.json").write_text(json.dumps(samples, indent=2))
        report = json.loads((evidence / "report.json").read_text())
        stable = [sample for sample in samples if 2 < sample["elapsed"] < 7]
        background = Image.open(evidence / "before.png").convert("RGB")
        transparent = bool(stable)
        portraits = set()
        for sample in stable:
            desktop = Image.open(evidence / sample["image"]).convert("RGB")
            x, y = sample["window"]["at"]
            width, height = sample["window"]["size"]
            bottom = min(height, desktop.height - y)
            for dx, dy in [(4, 4), (width - 4, 4), (4, bottom - 4)]:
                transparent &= desktop.getpixel((x + dx, y + dy)) == background.getpixel((x + dx, y + dy))
            portraits.add(hashlib.sha256(desktop.crop((x + 192, y + height - 136, x + 312, y + height - 16)).tobytes()).hexdigest())
        audible = args.silent
        if recorder:
            with wave.open(str(evidence / "audio.wav")) as recording:
                pcm = array.array("h", recording.readframes(recording.getnframes()))
            audible = any(abs(value) > 100 for value in pcm)
        proof = {"exit": process.returncode, "transparentDesktopCorners": transparent, "distinctPortraits": len(portraits), "focusUnchanged": bool(samples) and all(sample["focus"] == before.get("address") for sample in samples), "audioCaptured": audible if not args.silent else None, "report": report, "evidence": str(evidence)}
        proof["passed"] = process.returncode == 0 and transparent and len(portraits) >= 8 and proof["focusUnchanged"] and report["decodedVideoFrames"] >= 50 and report["finished"] == 1 and audible and (args.silent or report["speechStarted"] == 1) and not (evidence / "errors.log").exists()
        (evidence / "proof.json").write_text(json.dumps(proof, indent=2))
        print(json.dumps(proof, indent=2), flush=True)
        if not proof["passed"]:
            raise SystemExit(1)
    finally:
        for child in [process, recorder]:
            if child and child.poll() is None:
                child.terminate()
                child.wait(timeout=5)
        shutil.rmtree(data)
        (evidence / "cleanup.json").write_text(json.dumps({"scratchRemoved": not data.exists(), "processExited": process is None or process.poll() is not None, "recorderExited": recorder is None or recorder.poll() is not None}))


if __name__ == "__main__":
    main()
