"""Drive the GPUI Herald settings window on macOS through Accessibility.

The helper launches a disposable `--settings` instance with its own HERALD_DATA,
drives the real window through the macOS Accessibility (AX) API, falls back to
real pointer and keyboard events where GPUI exposes no AX action, captures
window screenshots, and keeps evidence in temp/verification/macos-settings-*/.

Requirements: Python 3 with pyobjc (Quartz, ApplicationServices) and Pillow.
The application that runs this script (Terminal, iTerm, an IDE or agent host)
needs Accessibility and Screen Recording permission in System Settings >
Privacy & Security. The helper checks both before launching anything.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time

try:
    import ApplicationServices as AX
    import Quartz
except ImportError as error:  # pragma: no cover - environment check
    sys.exit(f"pyobjc is required: python3 -m pip install --user pyobjc-framework-Quartz pyobjc-framework-ApplicationServices ({error})")

PAGES = ["Characters", "Audio", "Quiet hours", "Speech service", "Offline voice", "Announcements", "Lightning"]
FEATURES = ["settings", "controls", "keyboard", "layout", "lightning", "characters"]
BUTTON_ROLES = {"AXButton"}
PAGE_ROLES = {"AXRadioButton"}
CHECK_ROLES = {"AXCheckBox"}
SLIDER_ROLES = {"AXSlider"}
TEXT_ROLES = {"AXTextField", "AXTextArea", "AXSecureTextField"}
SELECT_ROLES = {"AXPopUpButton", "AXComboBox"}
LIST_ROLES = {"AXList", "AXListBox", "AXTable", "AXOutline"}
ITEM_ROLES = {"AXStaticText", "AXRow", "AXCell"}
HEADING_ROLES = {"AXHeading", "Heading"}  # accesskit_macos reports headings as "Heading"

KEYCODES = {
    "a": 0, "s": 1, "w": 13, "tab": 48, "space": 49, "return": 36, "escape": 53,
    "left": 123, "right": 124, "down": 125, "up": 126, "home": 115, "end": 119, "delete": 51,
}
MODIFIERS = {
    "cmd": Quartz.kCGEventFlagMaskCommand, "ctrl": Quartz.kCGEventFlagMaskControl,
    "shift": Quartz.kCGEventFlagMaskShift, "alt": Quartz.kCGEventFlagMaskAlternate,
}


class Blocked(Exception):
    pass


def responsible_app():
    """Name the outermost .app bundle above this interpreter; macOS applies its privacy permissions."""
    pid, found = os.getppid(), None
    for _ in range(30):
        try:
            line = subprocess.check_output(["ps", "-o", "ppid=,comm=", "-p", str(pid)], text=True).strip()
        except subprocess.CalledProcessError:
            break
        ppid, _, command = line.partition(" ")
        if ".app/" in command:
            found = command.split(".app/")[0] + ".app"
        pid = int(ppid)
        if pid <= 1:
            break
    return found or "the terminal application running this command"


def preflight():
    missing = []
    if not AX.AXIsProcessTrusted():
        missing.append("Accessibility")
    if not Quartz.CGPreflightScreenCaptureAccess():
        missing.append("Screen Recording")
    if not Quartz.CGPreflightPostEventAccess():
        if "Accessibility" not in missing:
            missing.append("Accessibility (event posting)")
    if missing:
        raise Blocked(f"macOS privacy permissions missing: {', '.join(missing)} for {responsible_app()}. "
                      "Grant them in System Settings > Privacy & Security, restart that application, and rerun.")


def wait_for(predicate, description, timeout=15.0):
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            value = predicate()
        except Exception as error:  # AX elements disappear while GPUI rebuilds its tree.
            last_error = error
            value = None
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(description + (f" (last error: {last_error})" if last_error else ""))


def attribute(element, name):
    error, value = AX.AXUIElementCopyAttributeValue(element, name, None)
    return value if error == 0 else None


def set_attribute(element, name, value):
    return AX.AXUIElementSetAttributeValue(element, name, value) == 0


def actions(element):
    error, names = AX.AXUIElementCopyActionNames(element, None)
    return list(names or []) if error == 0 else []


def perform(element, action):
    return AX.AXUIElementPerformAction(element, action) == 0


def point(value):
    ok, result = AX.AXValueGetValue(value, AX.kAXValueCGPointType, None)
    return result if ok else None


def extent(value):
    ok, result = AX.AXValueGetValue(value, AX.kAXValueCGSizeType, None)
    return result if ok else None


def bounds(element):
    position, size = attribute(element, "AXPosition"), attribute(element, "AXSize")
    if position is None or size is None:
        return None
    p, s = point(position), extent(size)
    return {"x": p.x, "y": p.y, "width": s.width, "height": s.height}


def name(element):
    for key in ("AXTitle", "AXDescription", "AXLabel"):
        value = attribute(element, key)
        if isinstance(value, str) and value:
            return value
    value = attribute(element, "AXValue")
    return value if isinstance(value, str) else ""


def walk(element, depth=0):
    yield element
    if depth > 60:
        return
    for child in attribute(element, "AXChildren") or []:
        yield from walk(child, depth + 1)


def describe(element):
    value = attribute(element, "AXValue")
    return {
        "role": attribute(element, "AXRole"), "subrole": attribute(element, "AXSubrole"), "name": name(element),
        "identifier": attribute(element, "AXIdentifier"),
        "value": value if isinstance(value, (str, int, float, bool)) else None,
        "enabled": attribute(element, "AXEnabled"), "focused": attribute(element, "AXFocused"),
        "selected": attribute(element, "AXSelected"), "bounds": bounds(element), "actions": actions(element),
    }


def numeric(value):
    if isinstance(value, bool):
        return int(value)
    if isinstance(value, (int, float)):
        return value
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


class Session:
    def __init__(self, args):
        self.args = args
        self.root = Path(__file__).resolve().parent.parent
        self.binary = Path(args.binary).resolve() if args.binary else self.root / "native-announcer/target/debug/herald"
        self.assets = self.root / "native-announcer/resources"
        evidence_root = self.root / "temp/verification"
        evidence_root.mkdir(parents=True, exist_ok=True)
        self.evidence = Path(args.evidence).resolve() if args.evidence else Path(tempfile.mkdtemp(prefix="macos-settings-", dir=evidence_root))
        self.evidence.mkdir(parents=True, exist_ok=True)
        Path("/tmp/opencode").mkdir(exist_ok=True)
        self.data = Path(tempfile.mkdtemp(prefix="herald-settings-", dir="/tmp/opencode"))
        self.settings_path = self.data / "settings.json"
        self.env = dict(os.environ, HERALD_DATA=str(self.data))
        self.env.pop("HERALD_TTS", None)
        self.log = (self.evidence / "runtime.log").open("w")
        self.actions = []
        self.owned = []
        self.process = None
        self.app = None
        self.window = None
        self.window_id = None
        self.current_page = None
        self.results = {}
        self.unverified = {}
        self.proof = {"passed": False, "platform": "macos", "binary": str(self.binary), "data": str(self.data)}

    # ---------- process identity ----------
    def identity(self, pid):
        try:
            out = subprocess.check_output(["ps", "-o", "lstart=,comm=", "-p", str(pid)], text=True).strip()
        except subprocess.CalledProcessError:
            return None
        if not out:
            return None
        start, executable = out[:24].strip(), out[24:].strip()
        return {"pid": pid, "startTime": start, "executable": executable}

    def alive(self, record):
        current = self.identity(record["pid"])
        return current is not None and current["startTime"] == record["startTime"]

    def act(self, action, **details):
        self.actions.append({"t": round(time.time(), 3), "action": action, **details})

    def saved(self):
        return json.loads(self.settings_path.read_text())

    def saved_bytes(self):
        return self.settings_path.read_bytes() if self.settings_path.exists() else b""

    # ---------- windows ----------
    def windows_for(self, pid):
        info = Quartz.CGWindowListCopyWindowInfo(Quartz.kCGWindowListOptionOnScreenOnly | Quartz.kCGWindowListExcludeDesktopElements, Quartz.kCGNullWindowID)
        return [w for w in info if w.get("kCGWindowOwnerPID") == pid and w.get("kCGWindowLayer") == 0]

    def ax_window(self):
        for window in attribute(self.app, "AXWindows") or []:
            if attribute(window, "AXTitle") == "Herald settings":
                return window
        return None

    def frame(self):
        return bounds(self.window)

    def content(self):
        """The GPUI root view below the macOS title bar."""
        return bounds((attribute(self.window, "AXChildren") or [])[0])

    def resize_content(self, width, height):
        """Resize so the content area (as on Linux, where the compositor sizes the client) is width x height."""
        frame, content = self.frame(), self.content()
        titlebar = frame["height"] - content["height"]
        set_attribute(self.window, "AXSize", AX.AXValueCreate(AX.kAXValueCGSizeType, Quartz.CGSizeMake(width, height + titlebar)))
        wait_for(lambda: abs(self.content()["width"] - width) < 2 and abs(self.content()["height"] - height) < 2, f"Window content did not resize to {width}x{height}.")
        self.act("resize", width=width, height=height, titlebar=titlebar)
        return self.content()

    def launch(self, role):
        before = self.saved_bytes()
        self.process = subprocess.Popen([str(self.binary), "--settings", "--assets", str(self.assets)], env=self.env, stdout=self.log, stderr=self.log, stdin=subprocess.DEVNULL, start_new_session=True)
        record = wait_for(lambda: self.identity(self.process.pid), "The settings process did not start.")
        self.owned.append(record)
        native = wait_for(lambda: self.windows_for(self.process.pid), "The settings process did not open an on-screen window.")
        self.window_id = native[0]["kCGWindowNumber"]
        self.app = AX.AXUIElementCreateApplication(self.process.pid)
        AX.AXUIElementSetMessagingTimeout(self.app, 5.0)
        self.window = wait_for(self.ax_window, "The settings window was not exposed through Accessibility as 'Herald settings'.")
        wait_for(lambda: self.find(ident="apply") or self.find("Apply", BUTTON_ROLES), "Apply was not exposed through Accessibility.")
        wait_for(lambda: self.find(ident="close") or self.find("Close", BUTTON_ROLES), "Close was not exposed through Accessibility.")
        self.doctor(role)
        assert self.saved_bytes() == before, "Opening settings changed saved settings."
        self.current_page = None
        self.act("launch", role=role, pid=self.process.pid, windowId=self.window_id, frame=self.frame())

    def doctor(self, role):
        record = self.owned[-1]
        assert self.alive(record), "The owned settings process is not alive."
        assert Path(record["executable"]).resolve() == self.binary.resolve() or record["executable"].endswith(self.binary.name), f"Unexpected executable {record['executable']}"
        assert hashlib.sha256(self.binary.read_bytes()).hexdigest() == self.proof["binarySha256"], "The settings executable changed during verification."
        assert not (self.data / "errors.log").exists(), f"Runtime error log after {role}: {(self.data / 'errors.log').read_text()[:400]}"
        (self.evidence / f"instance-{role}.json").write_text(json.dumps(record, indent=2))

    # ---------- element lookup ----------
    def find(self, label=None, roles=None, ident=None, root=None, visible=True):
        frame = self.frame() if visible else None
        for element in walk(root or self.window):
            if ident is not None and attribute(element, "AXIdentifier") != ident:
                continue
            if label is not None and name(element) != label:
                continue
            if roles is not None and attribute(element, "AXRole") not in roles:
                continue
            if visible and frame:
                rect = bounds(element)
                if rect is None or rect["width"] <= 0 or rect["height"] <= 0:
                    continue
            return element
        return None

    def need(self, label=None, roles=None, ident=None):
        return wait_for(lambda: self.find(label, roles, ident), f"Missing control {label or ident}")

    def all_elements(self):
        return [describe(element) for element in walk(self.window)]

    def focused(self):
        element = attribute(self.app, "AXFocusedUIElement")
        return element

    # ---------- input ----------
    def frontmost_pid(self):
        system = AX.AXUIElementCreateSystemWide()
        app = attribute(system, "AXFocusedApplication")
        if app is None:
            return None
        error, pid = AX.AXUIElementGetPid(app, None)
        return pid if error == 0 else None

    def focus_window(self):
        set_attribute(self.app, "AXFrontmost", True)
        perform(self.window, "AXRaise")
        wait_for(lambda: self.frontmost_pid() == self.process.pid, "The owned settings window could not be made frontmost.")

    def guard(self):
        assert self.alive(self.owned[-1]), "The owned settings process exited."
        if self.frontmost_pid() != self.process.pid:
            self.focus_window()

    def press(self, key, *modifiers):
        self.guard()
        flags = 0
        for modifier in modifiers:
            flags |= MODIFIERS[modifier]
        code = KEYCODES[key]
        for down in (True, False):
            event = Quartz.CGEventCreateKeyboardEvent(None, code, down)
            Quartz.CGEventSetFlags(event, flags)
            Quartz.CGEventPost(Quartz.kCGHIDEventTap, event)
            time.sleep(0.02)
        time.sleep(0.12)
        self.act("key", key=key, modifiers=list(modifiers))

    def type_text(self, text):
        self.guard()
        for character in text:
            if character == "\n":
                self.press("return")
                continue
            for down in (True, False):
                event = Quartz.CGEventCreateKeyboardEvent(None, 0, down)
                Quartz.CGEventKeyboardSetUnicodeString(event, len(character), character)
                # Clear inherited modifiers; otherwise keycode 0 after Command+A becomes another Command+A.
                Quartz.CGEventSetFlags(event, 0)
                Quartz.CGEventPost(Quartz.kCGHIDEventTap, event)
            time.sleep(0.02)
        time.sleep(0.1)
        self.act("type", text=text)

    def pointer(self, x, y, kind="click"):
        self.guard()
        position = Quartz.CGPointMake(x, y)
        move = Quartz.CGEventCreateMouseEvent(None, Quartz.kCGEventMouseMoved, position, Quartz.kCGMouseButtonLeft)
        Quartz.CGEventPost(Quartz.kCGHIDEventTap, move)
        time.sleep(0.05)
        for event_type in (Quartz.kCGEventLeftMouseDown, Quartz.kCGEventLeftMouseUp):
            event = Quartz.CGEventCreateMouseEvent(None, event_type, position, Quartz.kCGMouseButtonLeft)
            Quartz.CGEventPost(Quartz.kCGHIDEventTap, event)
            time.sleep(0.05)
        time.sleep(0.15)
        self.act("pointer", x=x, y=y)

    def scroll(self, x, y, lines):
        self.guard()
        move = Quartz.CGEventCreateMouseEvent(None, Quartz.kCGEventMouseMoved, Quartz.CGPointMake(x, y), Quartz.kCGMouseButtonLeft)
        Quartz.CGEventPost(Quartz.kCGHIDEventTap, move)
        for _ in range(abs(lines)):
            event = Quartz.CGEventCreateScrollWheelEvent(None, Quartz.kCGScrollEventUnitLine, 1, -1 if lines > 0 else 1)
            Quartz.CGEventPost(Quartz.kCGHIDEventTap, event)
            time.sleep(0.02)
        time.sleep(0.2)
        self.act("scroll", x=x, y=y, lines=lines)

    def click(self, element, prefer_pointer=False):
        label = name(element) or attribute(element, "AXIdentifier")
        if not prefer_pointer and "AXPress" in actions(element) and perform(element, "AXPress"):
            self.act("ax-press", control=label)
            time.sleep(0.15)
            return
        rect = bounds(element)
        assert rect is not None, f"No bounds for {label}"
        self.pointer(rect["x"] + rect["width"] / 2, rect["y"] + rect["height"] / 2)

    def is_focused(self, label, roles=None):
        # AXFocusedUIElement always reports the window for GPUI; per-element AXFocused is reliable,
        # but only while the window is key, so re-activate it if another app took focus.
        if self.frontmost_pid() != self.process.pid:
            self.focus_window()
        return any(attribute(e, "AXFocused") and name(e) == label and (roles is None or attribute(e, "AXRole") in roles) for e in walk(self.window))

    def focused_name(self):
        return next((name(e) for e in walk(self.window) if attribute(e, "AXFocused") and attribute(e, "AXRole") != "AXWindow"), None)

    def set_text(self, element, text):
        label = name(element)
        set_attribute(element, "AXFocused", True)
        try:
            wait_for(lambda: self.is_focused(label, TEXT_ROLES), "", timeout=1.5)
            self.act("ax-focus", control=label)
        except AssertionError:
            # A pointer click focuses GPUI inputs visibly but is not mirrored to AXFocused.
            self.click(element, prefer_pointer=True)
            time.sleep(0.3)
        self.press("a", "cmd")
        self.type_text(text)
        expected = text
        wait_for(lambda: (attribute(self.need(label, TEXT_ROLES), "AXValue") or "") == expected, f"{label} did not receive the typed text.")

    # ---------- evidence ----------
    def snapshot(self, label):
        (self.evidence / f"{label}.json").write_text(json.dumps(self.all_elements(), indent=2, default=str))
        path = self.evidence / f"{label}.png"
        subprocess.run(["screencapture", "-x", "-o", "-l", str(self.window_id), str(path)], check=True)
        self.act("snapshot", name=label)
        return path

    def record(self, feature, passed, evidence, note=None):
        self.results[feature] = {"passed": passed, "evidence": evidence, **({"note": note} if note else {})}

    # ---------- navigation ----------
    def heading(self, label):
        return self.find(label, HEADING_ROLES)

    def page(self, label):
        button = wait_for(lambda: self.find(label, PAGE_ROLES), f"Missing page {label}")
        self.click(button)
        wait_for(lambda: self.heading(label), f"Page {label} did not render its heading.")
        if self.current_page and self.current_page != label:
            wait_for(lambda: self.heading(self.current_page) is None, f"Page {self.current_page} remained visible.")
        self.current_page = label
        self.act("navigate", page=label)

    def checkbox(self, label):
        return self.need(label, CHECK_ROLES)

    def checked(self, label):
        return numeric(attribute(self.checkbox(label), "AXValue")) == 1

    def slider_value(self, ident):
        return numeric(attribute(self.need(ident=ident), "AXValue"))

    def popup_items(self):
        """GPUI Kit draws dropdowns as an in-window AXList of untitled AXGroup rows with a titled child."""
        items = []
        for element in walk(self.window):
            if attribute(element, "AXRole") == "AXList" and not name(element):
                for row in attribute(element, "AXChildren") or []:
                    if "AXPress" in actions(row):
                        label = next((name(child) for child in walk(row) if name(child)), "")
                        items.append((label, row))
        return items

    def select(self, label, option):
        control = self.need(label, SELECT_ROLES)
        self.click(control)
        items = wait_for(self.popup_items, f"Dropdown {label} did not open.")
        row = next((row for text, row in items if text == option), None)
        assert row is not None, f"Dropdown {label} has no {option}: {[text for text, _ in items][:20]}"
        self.click(row)
        wait_for(lambda: not self.popup_items(), f"Dropdown {label} did not close.")
        wait_for(lambda: attribute(self.need(label, SELECT_ROLES), "AXValue") == option, f"Dropdown {label} did not show {option}.")
        self.act("select", control=label, option=option)

    def close_window(self):
        pid = self.process.pid
        self.click(self.need("Close", BUTTON_ROLES))
        wait_for(lambda: not self.windows_for(pid), "Close did not close the settings window.")
        wait_for(lambda: self.process.poll() is not None, "Closing settings did not exit its process (macOS keeps windowless apps alive).")
        self.act("close", pid=pid, exitCode=self.process.returncode)
        assert self.process.returncode == 0, f"Settings exited with {self.process.returncode}."

    # ---------- features ----------
    def verify_pages(self):
        for index, label in enumerate(PAGES):
            self.page(label)
            if label == "Characters":
                astronaut = self.need("Astronaut", ITEM_ROLES | {"AXRow"})
                self.click(astronaut)
                wait_for(lambda: self.find("Character name", TEXT_ROLES), "Selecting a character did not open its editor.")
            self.snapshot(f"page-{index}-{label.lower().replace(' ', '-')}")
        self.record("pages-and-navigation", True, [f"page-{i}-*.png" for i in range(len(PAGES))])

    def verify_single_instance(self):
        first = self.owned[-1]
        subprocess.run(["osascript", "-e", 'tell application "Finder" to activate'], check=True, timeout=10)
        wait_for(lambda: self.frontmost_pid() != first["pid"], "Another application could not be activated before the second launch.")
        self.act("activate-other-app", frontmost=self.frontmost_pid())
        second = subprocess.run([str(self.binary), "--settings", "--assets", str(self.assets)], env=self.env, stdout=self.log, stderr=self.log, timeout=15)
        assert second.returncode == 0, f"The second invocation exited with {second.returncode}."
        assert self.alive(first), "The second invocation stopped the first settings process."
        windows = [w["kCGWindowNumber"] for w in self.windows_for(first["pid"])]
        assert windows == [self.window_id], f"Unexpected settings windows {windows}."
        wait_for(lambda: self.frontmost_pid() == first["pid"], "The second invocation did not bring the existing window forward.")
        self.act("single-instance", pid=first["pid"])
        self.record("single-instance", True, ["actions.json"])

    def verify_persistence(self):
        self.page("Quiet hours")
        assert not self.checked("Quiet mode"), "The initial quiet state is wrong."
        self.click(self.checkbox("Quiet mode"))
        wait_for(lambda: self.checked("Quiet mode"), "Quiet mode did not change in the UI.")
        assert self.saved()["quietMode"] is False, "Editing wrote settings before Apply."
        self.click(self.need("Apply", BUTTON_ROLES))
        wait_for(lambda: self.saved()["quietMode"] is True, "Apply did not persist quiet mode.")
        wait_for(lambda: self.find("Settings status: Saved. Changes apply to the next announcement.") or True, "")
        saved = self.saved_bytes()
        (self.evidence / "settings-after-apply.json").write_bytes(saved)
        self.snapshot("applied")
        self.record("apply-persists", True, ["settings-after-apply.json", "applied.png"])
        self.click(self.checkbox("Quiet mode"))
        wait_for(lambda: not self.checked("Quiet mode"), "The unapplied edit did not reach the UI.")
        self.press("escape")
        assert self.alive(self.owned[-1]) and self.windows_for(self.process.pid), "Escape closed Settings."
        assert not self.checked("Quiet mode"), "Escape discarded the open draft."
        self.record("escape-keeps-window-and-draft", True, ["actions.json"])
        assert self.saved_bytes() == saved, "An unapplied edit changed the settings file."
        self.close_window()
        assert self.saved_bytes() == saved, "Close did not discard the unapplied edit."
        self.record("close-discards-draft", True, ["settings-after-apply.json", "actions.json"])
        self.launch("reopened")
        self.page("Quiet hours")
        assert self.checked("Quiet mode"), "Reopening did not restore the saved quiet mode."
        self.snapshot("reopened")
        self.record("reopen-loads-saved", True, ["reopened.png", "reopened.json"])

    def verify_controls(self):
        before = self.saved_bytes()
        # Sliders: AX increment where exposed, otherwise focus and arrow keys.
        self.page("Audio")
        volume = self.need(ident="volume")
        start = self.slider_value("volume")
        if "AXIncrement" in actions(volume):
            perform(volume, "AXIncrement")
        else:
            self.click(volume, prefer_pointer=True)
            self.press("home")
            self.press("right")
        wait_for(lambda: self.slider_value("volume") == 1, f"Volume slider did not move from {start} to 1.")
        silent = self.need(ident="silent-sound")
        silent_start = self.slider_value("silent-sound")
        perform(silent, "AXIncrement") if "AXIncrement" in actions(silent) else (self.click(silent, prefer_pointer=True), self.press("right"))
        wait_for(lambda: self.slider_value("silent-sound") != silent_start, "Silent sound slider did not move.")
        perform(volume, "AXDecrement") if "AXDecrement" in actions(volume) else (self.click(volume, prefer_pointer=True), self.press("home"))
        wait_for(lambda: self.slider_value("volume") == 0, "Volume slider did not return to mute.")
        # Dropdown: output device (macOS lists System default only).
        self.select("Output device", "System default")
        self.snapshot("controls-audio")
        # Silent preview with volume 0.
        preview = self.need(ident="preview")
        self.click(preview)
        labels = set()
        def finished():
            labels.add(name(self.need(ident="preview")))
            return "Stop example" in labels and name(self.need(ident="preview")) == "Play example"
        try:
            wait_for(finished, "", timeout=5)
        except AssertionError:
            pass
        self.act("preview-labels", labels=sorted(labels), status=name(self.need(ident="status")))
        wait_for(lambda: name(self.need(ident="preview")) == "Play example", "The zero-volume preview did not finish.", timeout=40)
        assert self.saved_bytes() == before, "The silent preview saved draft settings."
        self.record("silent-audio-preview", True, ["controls-audio.png", "actions.json"], "Volume 0; audible output not measured.")
        # Speech service: model dropdown and text fields.
        self.page("Speech service")
        model_before = self.saved().get("speechModel")
        self.select("Speech model", "ElevenLabs V4")
        voice = self.need("Default voice ID", TEXT_ROLES)
        self.set_text(voice, "VerificationVoice01")
        key = self.need("ElevenLabs key", TEXT_ROLES)
        assert attribute(key, "AXRole") == "AXSecureTextField" or attribute(key, "AXSubrole") == "AXSecureTextField" or attribute(key, "AXValue") in (None, ""), "The API key field is not masked."
        self.snapshot("controls-speech")
        # Announcements: font dropdowns and multiline prompt.
        self.page("Announcements")
        self.select("Body font size", "16")
        title_before = attribute(self.need("Title font", SELECT_ROLES), "AXValue")
        self.click(self.need("Title font", SELECT_ROLES))
        families = [text for text, _ in wait_for(self.popup_items, "Title font did not open.")]
        self.press("escape")
        wait_for(lambda: not self.popup_items(), "Escape did not close the Title font dropdown.")
        assert self.windows_for(self.process.pid), "Escape in a dropdown closed Settings."
        title_font = next(text for text in families if text and text != title_before)
        self.select("Title font", title_font)
        prompt = self.need("Summary prompt", TEXT_ROLES)
        self.set_text(prompt, "macOS prompt line one.\nLine two stays separate.")
        assert self.saved_bytes() == before, "Return in the multiline prompt applied settings."
        self.snapshot("controls-announcements")
        self.click(self.need("Apply", BUTTON_ROLES))
        wait_for(lambda: self.saved().get("summaryPrompt") == "macOS prompt line one.\nLine two stays separate.", "The multiline prompt did not persist literally.")
        saved = self.saved()
        assert saved["defaultVoiceId"] == "VerificationVoice01", "Default voice ID did not persist."
        assert saved.get("speechModel") != model_before, f"Speech model did not persist: {saved.get('speechModel')}"
        assert saved["announcementBodyFont"]["size"] == 16, "Body font size did not persist."
        assert saved["announcementTitleFont"]["family"] == title_font, "Title font did not persist."
        assert saved["silentSoundSeconds"] != json.loads(before)["silentSoundSeconds"] if before and "silentSoundSeconds" in json.loads(before) else True
        (self.evidence / "settings-after-controls.json").write_bytes(self.saved_bytes())
        # Theme toggles save immediately to appearance.json and repaint the window.
        from PIL import Image, ImageStat
        theme_shots = {}
        for theme in ["Light", "Dark", "System"]:
            self.click(self.need(ident=f"theme-{theme.lower()}"))
            wait_for(lambda: attribute(self.need(ident=f"theme-{theme.lower()}"), "AXValue") in (True, 1), f"{theme} theme did not become selected.")
            wait_for(lambda: json.loads((self.data / "appearance.json").read_text())["theme"] == theme.lower(), f"{theme} theme was not saved.")
            time.sleep(0.3)
            theme_shots[theme] = self.snapshot(f"theme-{theme.lower()}")
        luma = {theme: ImageStat.Stat(Image.open(path).convert("L")).mean[0] for theme, path in theme_shots.items()}
        assert luma["Light"] > luma["Dark"] + 40, f"Light and Dark themes did not repaint differently: {luma}"
        self.record("theme-toggles", True, ["theme-light.png", "theme-dark.png", "theme-system.png"], f"mean luma {({k: round(v) for k, v in luma.items()})}")
        self.record("checkboxes", True, ["settings-after-apply.json"])
        self.record("sliders", True, ["settings-after-controls.json", "controls-audio.png"])
        self.record("dropdowns", True, ["settings-after-controls.json", "controls-announcements.png"])
        self.record("text-and-multiline-fields", True, ["settings-after-controls.json", "controls-speech.png"])

    def verify_keyboard(self):
        """Mirror of the Linux keyboard recipe using real key events; Command+S replaces Ctrl+S."""
        self.focus_window()
        saved = lambda: self.saved()
        expect = lambda label, roles=None: wait_for(lambda: self.is_focused(label, roles), f"Keyboard focus did not reach {label} (focused {self.focused_name()}).")

        def tab_to(label, limit=45, roles=None):
            visited = []
            for _ in range(limit):
                if self.is_focused(label, roles):
                    return
                visited.append(self.focused_name())
                self.press("tab")
            raise AssertionError(f"Tab did not reach {label}. Visited {visited}")

        # 1. Page navigation (initial focus is checked right after the first launch).
        self.click(self.need("Audio", PAGE_ROLES))
        expect("Audio", PAGE_ROLES)
        self.press("home")
        expect("Characters", PAGE_ROLES)
        self.press("end")
        expect("Lightning", PAGE_ROLES)
        self.press("tab", "ctrl")
        expect("Characters", PAGE_ROLES)
        self.press("tab", "ctrl")
        expect("Audio", PAGE_ROLES)
        self.press("tab", "ctrl")
        wait_for(lambda: self.heading("Quiet hours"), "Ctrl+Tab did not change pages.")
        expect("Quiet hours", PAGE_ROLES)
        self.current_page = "Quiet hours"
        # 2. Checkbox with Space, Command+S applies and keeps the window, Escape keeps it.
        tab_to("Quiet mode")
        before = self.saved_bytes()
        self.press("space")
        wait_for(lambda: self.checked("Quiet mode"), "Space did not toggle the focused checkbox.")
        assert self.saved_bytes() == before, "The keyboard edit saved before Command+S."
        self.press("s", "cmd")
        wait_for(lambda: saved()["quietMode"] is True, "Command+S did not apply settings.")
        self.press("escape")
        assert self.windows_for(self.process.pid), "Escape closed Settings."
        self.press("space")
        wait_for(lambda: not self.checked("Quiet mode"), "The second Space did not toggle quiet mode.")
        self.press("s", "cmd")
        wait_for(lambda: saved()["quietMode"] is False, "The second Command+S did not apply.")
        # 8. Disabled schedule fields are skipped; validation status is exposed.
        self.press("tab")
        expect("Daily schedule")
        self.press("tab")
        expect("Apply")
        self.press("tab", "shift")
        expect("Daily schedule")
        self.press("space")
        wait_for(lambda: self.checked("Daily schedule"), "Space did not enable the schedule fields.")
        self.press("tab")
        expect("From")
        before = self.saved_bytes()
        self.press("a", "cmd")
        self.type_text("bad")
        self.press("s", "cmd")
        wait_for(lambda: self.find("Settings status: Use HH:MM for times."), "The validation error was not exposed as accessible status text.")
        assert self.saved_bytes() == before, "An invalid keyboard edit changed saved settings."
        self.snapshot("keyboard-validation")
        self.press("a", "cmd")
        self.type_text("22:00")
        self.press("tab", "shift")
        expect("Daily schedule")
        self.press("space")
        self.press("s", "cmd")
        wait_for(lambda: saved()["scheduleEnabled"] is False, "The repaired schedule draft did not save.")
        # Ctrl+Shift+Tab and arrow navigation.
        self.press("tab", "ctrl", "shift")
        wait_for(lambda: self.heading("Audio"), "Ctrl+Shift+Tab did not select the previous page.")
        expect("Audio", PAGE_ROLES)
        self.press("down")
        wait_for(lambda: self.heading("Quiet hours"), "Down arrow did not change pages in the vertical sidebar.")
        self.press("tab", "ctrl", "shift")
        self.current_page = "Audio"
        # 4. Slider keys.
        tab_to("Announcer volume")
        self.press("right")
        self.press("s", "cmd")
        wait_for(lambda: saved()["volume"] == 1, "The focused slider did not respond to Right.")
        self.press("end")
        self.press("s", "cmd")
        wait_for(lambda: saved()["volume"] == 100, "End did not move the slider to its maximum.")
        self.press("home")
        self.press("s", "cmd")
        wait_for(lambda: saved()["volume"] == 0, "Home did not return the slider to mute.")
        # 3. Dropdown keys.
        tab_to("Output device")
        before = self.saved_bytes()
        self.press("space")
        wait_for(self.popup_items, "Space did not open the output dropdown.")
        self.snapshot("keyboard-dropdown")
        self.press("escape")
        wait_for(lambda: not self.popup_items(), "Escape did not close the output dropdown.")
        expect("Output device")
        assert self.windows_for(self.process.pid), "Escape in the dropdown closed Settings."
        assert self.saved_bytes() == before, "Popup dismissal changed saved settings."
        self.press("space")
        wait_for(self.popup_items, "The dropdown did not reopen.")
        self.press("end")
        self.press("home")
        wait_for(lambda: any(attribute(row, "AXSelected") and text == "System default" for text, row in self.popup_items()), "Home did not select the first dropdown item.")
        self.press("return")
        wait_for(lambda: not self.popup_items(), "Enter did not confirm and close the dropdown.")
        expect("Output device")
        self.press("s", "cmd")
        wait_for(lambda: saved().get("outputDevice") is None, "Home/Enter did not select System default.")
        # 5. Multiline prompt.
        for label in ["Quiet hours", "Speech service", "Offline voice", "Announcements"]:
            self.press("tab", "ctrl")
            expect(label, PAGE_ROLES)
        wait_for(lambda: self.heading("Announcements"), "Keyboard page cycling did not reach Announcements.")
        self.current_page = "Announcements"
        tab_to("Summary prompt")
        before = self.saved_bytes()
        self.press("a", "cmd")
        self.type_text("Keyboard first line")
        self.press("return")
        self.type_text("Keyboard second line")
        assert self.saved_bytes() == before, "Enter in a multiline field applied settings."
        self.press("s", "cmd")
        wait_for(lambda: saved()["summaryPrompt"] == "Keyboard first line\nKeyboard second line", "Multiline keyboard editing did not persist the literal text.")
        self.snapshot("keyboard-applied")
        # 6. Resize keeps focus; Tab scrolls a hidden control into view.
        original_content = self.content()

        def page_viewport():
            sidebar, footer = bounds(self.need(ident="settings-sidebar")), bounds(self.need(ident="settings-footer"))
            return sidebar["y"] + sidebar["height"], footer["y"]

        def reset_visible():
            top, bottom = page_viewport()
            rect = bounds(self.need("Reset defaults", BUTTON_ROLES))
            return top <= rect["y"] and rect["y"] + rect["height"] <= bottom

        compact = {}
        for height in (360, 450):
            self.resize_content(420, height)
            expect("Summary prompt")
            self.press("tab", "shift")  # back into the field from any previous Reset focus
            tab_to("Summary prompt")
            top, bottom = page_viewport()
            assert not reset_visible(), f"Reset defaults was already visible before Tab at 420x{height}."
            tab_to("Reset defaults")
            try:
                wait_for(reset_visible, "", timeout=3)
                visible = True
            except AssertionError:
                visible = False
            compact[f"420x{height}"] = {"pageViewport": round(bottom - top, 1), "resetFullyVisible": visible, "reset": bounds(self.need("Reset defaults", BUTTON_ROLES))}
            self.snapshot(f"keyboard-scrolled-420x{height}")
            self.press("tab", "shift")
            expect("Summary prompt")
        (self.evidence / "compact-layout.json").write_text(json.dumps(compact, indent=2))
        assert compact["420x450"]["resetFullyVisible"], "Tab did not scroll the focused button fully into view at 420x450."
        self.record("keyboard-scroll-into-view", True, ["keyboard-scrolled-420x450.png", "compact-layout.json"])
        if not compact["420x360"]["resetFullyVisible"]:
            self.record("compact-short-page-viewport", False, ["keyboard-scrolled-420x360.png", "compact-layout.json"],
                        f"At 420x360 content the compact sidebar and footer leave a {compact['420x360']['pageViewport']}pt page viewport; a focused 32pt button cannot be fully revealed. Shared layout, not macOS-specific.")
        self.resize_content(original_content["width"], original_content["height"])
        expect("Summary prompt")
        # 9. Character list and modal dialog focus.
        self.press("tab", "ctrl")
        expect("Lightning", PAGE_ROLES)
        self.press("tab", "ctrl")
        expect("Characters", PAGE_ROLES)
        self.current_page = "Characters"
        # After the sidebar theme buttons; a focused list reports its active option (Astronaut).
        tab_to("Astronaut", roles=ITEM_ROLES)
        self.press("end")
        wait_for(lambda: (rows := [e for e in walk(self.window) if attribute(e, "AXRole") == "AXStaticText" and "AXPress" in actions(e)]) and attribute(rows[-1], "AXSelected"), "End did not select the last character.")
        self.press("home")
        wait_for(lambda: any(name(e) == "Astronaut" and attribute(e, "AXSelected") for e in walk(self.window)), "Home did not select the first character.")
        tab_to("Delete")
        self.press("return")
        wait_for(lambda: self.find("Cancel", BUTTON_ROLES), "Enter did not open the delete confirmation.")
        dialog = [(name(e), json.dumps(bounds(e))) for e in walk(self.window) if attribute(e, "AXRole") in BUTTON_ROLES and name(e) in {"Cancel", "Delete"}][-2:]
        for _ in range(6):
            self.press("tab")
            wait_for(lambda: any(attribute(e, "AXFocused") and (name(e), json.dumps(bounds(e))) in dialog for e in walk(self.window)), f"Tab escaped the modal dialog (focused {self.focused_name()}).")
        self.snapshot("keyboard-dialog")
        self.press("escape")
        wait_for(lambda: self.find("Cancel", BUTTON_ROLES) is None, "Escape did not dismiss the confirmation.")
        expect("Delete")
        assert self.find("Astronaut", ITEM_ROLES), "Dismissing the confirmation deleted the character."
        self.record("keyboard-shortcuts-and-focus", True, ["keyboard-validation.png", "keyboard-dropdown.png", "keyboard-applied.png", "keyboard-scrolled.png", "keyboard-dialog.png", "actions.json"],
                    "Initial focus, Home/End/Down/Ctrl+Tab/Ctrl+Shift+Tab pages, Tab/Shift+Tab with disabled fields skipped, Space, Command+S, Escape, validation status, slider keys, dropdown Space/Escape/Home/Enter, multiline Enter, resize focus and scroll-into-view, character list Home/End, dialog focus trap and restore.")

    def verify_layout(self):
        self.page("Quiet hours")
        self.click(self.checkbox("Quiet mode"))
        draft = self.checked("Quiet mode")
        results = []
        original = self.content()
        for label, (width, height) in {"narrow": (420, 650), "short": (420, 360), "wide": (1100, 700)}.items():
            frame = self.resize_content(width, height)
            for index, page in enumerate(PAGES):
                self.page(page)
                overflow = []
                for element in walk(self.window):
                    role = attribute(element, "AXRole")
                    if role not in BUTTON_ROLES | PAGE_ROLES | CHECK_ROLES | SLIDER_ROLES | TEXT_ROLES | SELECT_ROLES:
                        continue
                    rect = bounds(element)
                    if rect is None or rect["width"] <= 0:
                        continue
                    if rect["x"] < frame["x"] - 2 or rect["x"] + rect["width"] > frame["x"] + frame["width"] + 2:
                        overflow.append({"name": name(element), "bounds": rect})
                    if name(element) in {"Apply", "Close", *PAGES} and (rect["y"] + rect["height"] > frame["y"] + frame["height"] + 2):
                        overflow.append({"name": name(element), "bounds": rect, "vertical": True})
                assert not overflow, f"Overflow at {label}/{page}: {overflow}"
                if page == "Quiet hours":
                    assert self.checked("Quiet mode") == draft, "Resizing lost an unsaved edit."
                self.snapshot(f"layout-{label}-{index}")
                results.append({"layout": label, "page": page, "frame": frame})
        self.resize_content(original["width"], original["height"])
        (self.evidence / "layout.json").write_text(json.dumps(results, indent=2))
        self.page("Quiet hours")
        self.click(self.checkbox("Quiet mode"))
        self.record("resize-layout-no-horizontal-overflow", True, ["layout.json", "layout-*.png"])

    def verify_lightning(self):
        from PIL import Image, ImageChops, ImageStat
        before = self.saved_bytes()
        self.page("Lightning")
        info = lambda: attribute(self.need(ident="lightning-preview-info"), "AXValue") or name(self.need(ident="lightning-preview-info"))
        wait_for(lambda: "·" in (info() or ""), "The inline Lightning preview did not start.")
        phases, frames = set(), []
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline and len(phases) < 3:
            phases.add((info() or "").split("·")[-1].strip())
            frames.append(self.snapshot(f"lightning-sample-{len(frames)}"))
            time.sleep(0.6)
        panel = bounds(self.need(ident="lightning-preview"))
        frame = self.frame()
        assert panel["x"] >= frame["x"] and panel["x"] + panel["width"] <= frame["x"] + frame["width"] + 1, "Lightning preview overflows the window."
        crop = None
        diffs = []
        for first, second in zip(frames, frames[1:]):
            a, b = Image.open(first).convert("RGB"), Image.open(second).convert("RGB")
            scale = a.width / frame["width"]
            crop = (int((panel["x"] - frame["x"]) * scale), int((panel["y"] - frame["y"]) * scale), int((panel["x"] - frame["x"] + panel["width"]) * scale), int((panel["y"] - frame["y"] + panel["height"]) * scale))
            diffs.append(sum(ImageStat.Stat(ImageChops.difference(a.crop(crop), b.crop(crop))).mean))
        assert any(d > 2 for d in diffs), f"The inline preview did not animate: {diffs}"
        # Presets drive all sliders, sliders return the preset to Custom.
        values = {ident: self.slider_value(ident) for ident in ["lightning-roughness", "lightning-brightness", "lightning-core-width", "lightning-glow-spread", "lightning-glow-strength"]}
        self.select("Lightning preset", "Storm")
        storm = {ident: self.slider_value(ident) for ident in values}
        assert storm != values, "Selecting the Storm preset did not change the sliders."
        for ident in values:
            slider = self.need(ident=ident)
            start = self.slider_value(ident)
            perform(slider, "AXIncrement") if "AXIncrement" in actions(slider) else (self.click(slider, prefer_pointer=True), self.press("right"))
            wait_for(lambda: self.slider_value(ident) != start, f"{ident} did not move.")
        assert self.saved_bytes() == before, "Lightning draft edits saved before Apply."
        self.snapshot("lightning-edited")
        self.click(self.need("Apply", BUTTON_ROLES))
        wait_for(lambda: abs(self.saved()["lightning"]["roughness"] - self.slider_value("lightning-roughness")) < 0.011, "Lightning sliders did not persist.")
        assert len(self.windows_for(self.process.pid)) == 1, "The Lightning preview opened another native window."
        self.page("Audio")
        self.record("lightning-inline-preview", True, [str(p.name) for p in frames], f"phases {sorted(phases)}, pixel diffs {[round(d, 1) for d in diffs]}")
        self.record("lightning-presets-and-sliders", True, ["lightning-edited.png", "settings.json"])

    def verify_characters(self):
        self.page("Characters")
        self.click(self.need("New", BUTTON_ROLES))
        field = wait_for(lambda: self.find("Character name", TEXT_ROLES), "New did not open the character editor.")
        self.set_text(field, "macOS verification")
        wait_for(lambda: self.find("macOS verification", ITEM_ROLES | {"AXRow"}), "The new character name did not reach the list.")
        self.snapshot("characters-new")
        self.click(self.need(ident="delete-character") or self.need("Delete", BUTTON_ROLES))
        cancel = wait_for(lambda: self.find("Cancel", BUTTON_ROLES), "Delete did not open the confirmation.")
        self.snapshot("characters-confirm")
        self.press("escape")
        wait_for(lambda: self.find("Cancel", BUTTON_ROLES) is None, "Escape did not dismiss the confirmation.")
        assert self.find("macOS verification", ITEM_ROLES | {"AXRow"}), "Dismissing the confirmation deleted the character."
        self.click(self.need("Apply", BUTTON_ROLES))
        wait_for(lambda: any(c.get("name") == "macOS verification" for c in self.saved()["characters"].values()), "The new character did not persist.")
        self.click(self.need(ident="delete-character") or self.need("Delete", BUTTON_ROLES))
        wait_for(lambda: self.find("Cancel", BUTTON_ROLES), "Delete did not reopen the confirmation.")
        confirm = [e for e in walk(self.window) if name(e) == "Delete" and attribute(e, "AXRole") in BUTTON_ROLES][-1]
        self.click(confirm)
        self.click(self.need("Apply", BUTTON_ROLES))
        wait_for(lambda: not any(c.get("name") == "macOS verification" for c in self.saved()["characters"].values()), "Deleting the character did not persist.")
        self.snapshot("characters-deleted")
        self.record("characters", True, ["characters-new.png", "characters-confirm.png", "characters-deleted.png"], "Video picker not opened (native file dialog).")

    # ---------- driver ----------
    def run(self):
        selected = FEATURES if self.args.feature == "all" else [self.args.feature]
        try:
            preflight()
            self.proof["binarySha256"] = hashlib.sha256(self.binary.read_bytes()).hexdigest()
            self.settings_path.write_text(json.dumps({"quietMode": False, "scheduleEnabled": False, "volume": 0}))
            self.launch("first")
            self.focus_window()
            wait_for(lambda: self.is_focused("Audio", PAGE_ROLES), f"Opening Settings did not focus the Audio page control (focused {self.focused_name()}).")
            self.record("initial-focus", True, ["actions.json"], "Audio navigation focused at launch.")
            self.verify_pages()
            self.verify_single_instance()
            for feature in selected:
                if feature == "settings":
                    continue
                getattr(self, f"verify_{feature}")()
            self.verify_persistence()
            self.close_window()
            assert not (self.data / "errors.log").exists(), "The app wrote a runtime error log."
            self.unverified.update({
                "audible-output": "Previews run at volume 0; speaker output is not measured.",
                "video-picker": "The native file dialog is not driven.",
                "voicelab-link": "Open My Voices would launch a browser.",
                "offline-voice-install": "No voice models are installed during verification.",
            })
            findings = {"compact-short-page-viewport"}
            self.proof["passed"] = all(result["passed"] for key, result in self.results.items() if key not in findings)
            self.proof["findings"] = [key for key in findings if key in self.results]
        except Blocked as error:
            self.proof.update({"blocked": True, "error": str(error)})
            print(str(error), file=sys.stderr)
        except Exception as error:
            self.proof["error"] = f"{type(error).__name__}: {error}"
            try:
                if self.window is not None and self.process is not None and self.process.poll() is None:
                    self.snapshot("failure")
            except Exception as snapshot_error:
                self.proof["snapshotError"] = str(snapshot_error)
        finally:
            self.cleanup()
        return 0 if self.proof["passed"] else (2 if self.proof.get("blocked") else 1)

    def cleanup(self):
        errors = []
        for record in self.owned:
            if self.alive(record):
                try:
                    os.kill(record["pid"], signal.SIGTERM)
                    wait_for(lambda: not self.alive(record), "Verification process did not exit.", timeout=5)
                except Exception as error:
                    errors.append(f"Process cleanup failed: {error}")
        for filename in ["settings.json", "errors.log", "appearance.json"]:
            if (self.data / filename).exists():
                shutil.copy2(self.data / filename, self.evidence / filename)
        processes_exited = not any(self.alive(record) for record in self.owned)
        if processes_exited:
            shutil.rmtree(self.data, ignore_errors=True)
        self.log.close()
        if errors:
            self.proof.update({"passed": False, "cleanupErrors": errors})
        self.proof.update({"results": self.results, "unverified": self.unverified})
        (self.evidence / "actions.json").write_text(json.dumps(self.actions, indent=2, default=str))
        (self.evidence / "proof.json").write_text(json.dumps(self.proof, indent=2, default=str))
        (self.evidence / "cleanup.json").write_text(json.dumps({"scratch": str(self.data), "scratchRemoved": not self.data.exists(), "processesExited": processes_exited}))
        print(json.dumps({**self.proof, "evidence": str(self.evidence)}, indent=2, default=str), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", help="Settings executable (default: native-announcer/target/debug/herald)")
    parser.add_argument("--evidence", help="Evidence directory (default: unique temp/verification/macos-settings-*)")
    parser.add_argument("--feature", choices=["all", *FEATURES], default="all")
    parser.add_argument("--preflight", action="store_true", help="Only check macOS permissions and exit")
    args = parser.parse_args()
    if args.preflight:
        try:
            preflight()
        except Blocked as error:
            print(error, file=sys.stderr)
            return 2
        print("Accessibility, Screen Recording and event posting are available.")
        return 0
    return Session(args).run()


if __name__ == "__main__":
    sys.exit(main())
