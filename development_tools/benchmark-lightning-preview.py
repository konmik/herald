"""Run the Settings window's inline Lightning preview for a while with HERALD_PREVIEW_LOG, for benchmark-lightning.mjs --live.

Usage: python3 benchmark-lightning-preview.py <herald binary> <assets> <frame log> <seconds>
Needs the same Accessibility permission as verify-macos-settings.py, whose helpers it reuses.
"""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

here = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("verify", here / "verify-macos-settings.py")
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)

binary, assets, log, seconds = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4])
data = tempfile.mkdtemp(prefix="herald-preview-bench-")
env = dict(os.environ, HERALD_DATA=data, HERALD_PREVIEW_LOG=log)
process = subprocess.Popen([binary, "--settings", "--assets", assets], env=env, stdin=subprocess.DEVNULL,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
try:
    app = verify.AX.AXUIElementCreateApplication(process.pid)
    window = verify.wait_for(lambda: next((w for w in (verify.attribute(app, "AXWindows") or []) if verify.attribute(w, "AXTitle") == "Herald settings"), None), "Settings window did not open")

    def find(label, roles):
        for element in verify.walk(window):
            if verify.attribute(element, "AXRole") in roles and verify.name(element) == label:
                return element

    verify.perform(verify.wait_for(lambda: find("Lightning", verify.PAGE_ROLES), "Missing Lightning page"), "AXPress")
    verify.wait_for(lambda: find("Lightning", verify.HEADING_ROLES), "Lightning page did not open")
    # The preview only animates while its window is active.
    subprocess.run(["osascript", "-e", f'tell application "System Events" to set frontmost of (first process whose unix id is {process.pid}) to true'], check=False)
    time.sleep(seconds)
finally:
    process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
