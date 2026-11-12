import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def history(data):
    path = data / "history.jsonl"
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()] if path.exists() else []


def main():
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", default=str(root / "native-announcer/target/debug/herald.exe"))
    parser.add_argument("--assets", default=str(root / "native-announcer/resources"))
    parser.add_argument("--evidence")
    args = parser.parse_args()
    binary = Path(args.binary).resolve(strict=True)
    assets = Path(args.assets).resolve(strict=True)
    evidence = Path(args.evidence).resolve() if args.evidence else Path(tempfile.mkdtemp(prefix="herald-restart-evidence-"))
    if args.evidence:
        evidence.mkdir(parents=True, exist_ok=False)
    data = Path(tempfile.mkdtemp(prefix="herald-restart-data-"))
    env = dict(os.environ, HERALD_DATA=str(data))
    settings = {"quietMode": True, "scheduleEnabled": False, "volume": 0, "useGpu": False}
    (data / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
    (evidence / "settings.json").write_text(json.dumps(settings), encoding="utf-8")
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    processes = []
    actions = []

    def record(message):
        actions.append(message)
        (evidence / "actions.txt").write_text("\n".join(actions) + "\n", encoding="utf-8")
        print(message, flush=True)

    def bridge(command):
        result = subprocess.run(
            [str(binary), "--bridge", "--assets", str(assets)],
            input=json.dumps(command), text=True, encoding="utf-8", capture_output=True,
            env=env, timeout=10, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
        if result.returncode:
            raise RuntimeError(f"Bridge exited {result.returncode}: {result.stderr}")

    def run(name, sessions, messages):
        report_path = evidence / f"{name}-report.json"
        with (evidence / f"{name}-runtime.log").open("w", encoding="utf-8") as log:
            process = subprocess.Popen(
                [str(binary), "--isolated", "--assets", str(assets), "--test-seconds", "12",
                 "--report", str(report_path), "--snapshot", str(evidence / f"{name}-render.png")],
                env=env, stdout=log, stderr=log,
            )
            processes.append(process)
            (evidence / f"{name}-instance.json").write_text(json.dumps({
                "pid": process.pid, "started": time.time(), "binary": str(binary),
                "sha256": binary_hash, "data": str(data), "entry": "production bridge transport",
            }, indent=2), encoding="utf-8")
            record(f"Launch {name} with owned PID {process.pid}")
            started = time.monotonic()
            sent = False
            while process.poll() is None:
                if time.monotonic() - started > 22:
                    raise RuntimeError(f"{name} exceeded its deadline")
                errors = data / "errors.log"
                if errors.exists() and errors.stat().st_size:
                    raise RuntimeError(errors.read_text(encoding="utf-8"))
                bridge({"type": "presence", "clientID": "restart-verification", "sessionIDs": sessions,
                        "at": int(time.time() * 1000)})
                if not sent and time.monotonic() - started >= 2:
                    if not (data / "inbox").is_dir():
                        raise RuntimeError(f"{name} did not create its inbox")
                    record(f"Doctor passed for {name}")
                    for message in messages:
                        bridge(message)
                    sent = True
                time.sleep(0.5)
            if process.returncode:
                raise RuntimeError(f"{name} exited {process.returncode}")
        if hashlib.sha256(binary.read_bytes()).hexdigest() != binary_hash:
            raise RuntimeError("Binary changed during verification")
        for filename in ["history.jsonl", "queue.json", "timing.log", "errors.log"]:
            if (data / filename).exists():
                shutil.copy2(data / filename, evidence / f"{name}-{filename}")
        return read_json(report_path)

    try:
        record(f"Binary SHA256 {binary_hash}")
        now = int(time.time() * 1000)
        shown = {"type": "notify", "id": "shown", "sessionID": "codex:shown", "completed": now,
                 "title": "Shown before restart", "text": "The completed task remains visible. " * 30,
                 "character": "codex", "characterID": "chimera", "emotion": "neutral"}
        pending = {"type": "notify", "id": "pending", "sessionID": "codex:pending", "completed": now,
                   "title": "Pending before restart", "text": "The pending task survived restart.",
                   "character": "codex", "characterID": "chimera", "emotion": "neutral"}
        (evidence / "notifications.json").write_text(json.dumps([shown, pending], indent=2), encoding="utf-8")
        first = run("before-restart", ["codex:shown"], [shown, pending])
        if first["shown"] != 1 or first["finished"] != 0 or [row["id"] for row in history(data)] != ["shown"]:
            raise AssertionError("The first process did not exit while the announcement was visible")
        if not first["passiveWindow"] or not first["focusChecked"] or not first["focusUnchanged"]:
            raise AssertionError("The first process did not preserve focus")
        if "pending" not in [row["id"] for row in read_json(data / "queue.json")]:
            raise AssertionError("The unshown notification was not persisted")
        record("PASS: exited while shown; unshown notification persisted")
        second = run("after-restart", ["codex:shown", "codex:pending"], [])
        ids = [row["id"] for row in history(data)]
        if ids.count("shown") != 1:
            raise AssertionError(f"NO_REPLAY failed: shown announcement appeared {ids.count('shown')} times")
        record("PASS: shown announcement did not replay")
        if ids.count("pending") != 1 or second["shown"] != 1 or second["finished"] != 1:
            raise AssertionError("The pending notification did not render and finish once after restart")
        if not second["passiveWindow"] or not second["focusChecked"] or not second["focusUnchanged"]:
            raise AssertionError("The restarted process did not preserve focus")
        if read_json(data / "queue.json"):
            raise AssertionError("The completed queue did not drain")
        if not (evidence / "before-restart-render.png").exists() or not (evidence / "after-restart-render.png").exists():
            raise AssertionError("Rendered evidence is missing")
        record("PASS: pending notification rendered once after restart; queue drained")
        (evidence / "proof.json").write_text(json.dumps({"shownCount": ids.count("shown"),
            "pendingCount": ids.count("pending"), "queueEmpty": True}, indent=2), encoding="utf-8")
    except Exception as error:
        record(f"FAIL: {error}")
        (evidence / "failure.txt").write_text(str(error), encoding="utf-8")
        raise
    finally:
        for process in processes:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
        shutil.rmtree(data)
        (evidence / "cleanup.json").write_text(json.dumps({"scratchRemoved": not data.exists(),
            "processExited": all(process.poll() is not None for process in processes)}, indent=2), encoding="utf-8")
        print(f"Evidence: {evidence}", flush=True)


if __name__ == "__main__":
    main()
