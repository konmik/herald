import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, Gio, GLib


def accessibility_status(connection):
    return connection.call_sync("org.a11y.Bus", "/org/a11y/bus", "org.freedesktop.DBus.Properties", "Get", GLib.Variant("(ss)", ("org.a11y.Status", "IsEnabled")), GLib.VariantType("(v)"), Gio.DBusCallFlags.NONE, -1, None).unpack()[0]


def enable_accessibility(connection, enabled):
    connection.call_sync("org.a11y.Bus", "/org/a11y/bus", "org.freedesktop.DBus.Properties", "Set", GLib.Variant("(ssv)", ("org.a11y.Status", "IsEnabled", GLib.Variant("b", enabled))), None, Gio.DBusCallFlags.NONE, -1, None)


def wait_for(predicate, description):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        try:
            value = predicate()
        except GLib.GError as error:
            if "Unknown object" not in error.message:
                raise
            value = None
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(description)


def clients():
    return json.loads(subprocess.check_output(["hyprctl", "-j", "clients"]))


def identity(pid):
    try:
        return {"pid": pid, "startTime": Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[19], "executable": str(Path(f"/proc/{pid}/exe").resolve())}
    except FileNotFoundError:
        return None


def settings_processes(binary, data):
    records = []
    for process in Path("/proc").iterdir():
        if not process.name.isdigit():
            continue
        try:
            record = identity(int(process.name))
            if record is not None and record["executable"] == str(binary) and f"HERALD_DATA={data}".encode() in (process / "environ").read_bytes().split(b"\0"):
                records.append(record)
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
    return records


def walk(node):
    yield node
    for index in range(node.get_child_count()):
        child = node.get_child_at_index(index)
        if child is not None:
            yield from walk(child)


def application(pid):
    desktop = Atspi.get_desktop(0)
    for index in range(desktop.get_child_count()):
        candidate = desktop.get_child_at_index(index)
        if candidate.get_process_id() == pid:
            return candidate
    return None


def control(app, name, role):
    roles = {role}
    if role == Atspi.Role.PUSH_BUTTON and name in {"Characters", "Audio", "Quiet hours", "Speech service", "Offline voice", "Announcements"}:
        roles.add(Atspi.Role.PAGE_TAB)
    return next((node for node in walk(app) if node.get_name() == name and node.get_role() in roles and node.get_state_set().contains(Atspi.StateType.SHOWING) and node.get_state_set().contains(Atspi.StateType.VISIBLE)), None)


def click(node):
    action = node.get_action_iface()
    assert action is not None and action.get_n_actions() > 0, f"No user action for {node.get_name()}"
    assert action.do_action(0), f"Could not activate {node.get_name()}"


def checked(node):
    return node.get_state_set().contains(Atspi.StateType.CHECKED)


def bounds(node):
    rect = node.get_component_iface().get_extents(Atspi.CoordType.SCREEN)
    return {"x": rect.x, "y": rect.y, "width": rect.width, "height": rect.height}


def dispatch_window(window, operation, arguments):
    code = f'for _, w in ipairs(hl.get_windows()) do if w.pid == {window["pid"]} then hl.dispatch(hl.dsp.window.{operation}({{window = w, {arguments}}})); return end end; error("Owned verification window is missing")'
    result = subprocess.run(["hyprctl", "eval", code], check=True, capture_output=True, text=True)
    assert result.stdout.strip() == "ok", result.stdout + result.stderr


def layout_bounds(app, window):
    width, height = window["size"]
    controls = []
    roles = {Atspi.Role.PUSH_BUTTON, Atspi.Role.PAGE_TAB, Atspi.Role.ENTRY, Atspi.Role.COMBO_BOX, Atspi.Role.CHECK_BOX, Atspi.Role.SLIDER}
    for node in walk(app):
        if node.get_role() not in roles or not node.get_state_set().contains(Atspi.StateType.VISIBLE):
            continue
        rect = bounds(node)
        name = node.get_name()
        assert rect["width"] > 0 and rect["height"] > 0, f"Collapsed control {name}"
        assert rect["x"] >= -2 and rect["x"] + rect["width"] <= width + 2, f"Horizontal overflow at {name}: {rect} in {width}px"
        if name in {"Apply", "Close", "Characters", "Audio", "Quiet hours", "Speech service", "Offline voice", "Announcements"}:
            assert rect["y"] >= -2 and rect["y"] + rect["height"] <= height + 2, f"Unreachable navigation or footer control {name}: {rect} in {height}px"
        controls.append({"name": name, "role": node.get_role_name(), "id": node.get_accessible_id(), "bounds": rect})
    return controls


def snapshot(app, window, evidence, name):
    nodes = [{"name": node.get_name(), "role": node.get_role_name(), "states": [int(state) for state in node.get_state_set().get_states()]} for node in walk(app)]
    (evidence / f"{name}.json").write_text(json.dumps(nodes, indent=2))
    window = next(item for item in clients() if item["pid"] == window["pid"])
    x, y = window["at"]
    width, height = window["size"]
    subprocess.run(["grim", "-g", f"{x},{y} {width}x{height}", str(evidence / f"{name}.png")], check=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--desktop-entry", default=str(Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / "applications/herald-settings.desktop"))
    parser.add_argument("--layout", action="store_true")
    parser.add_argument("--keyboard", action="store_true")
    parser.add_argument("--scroll-helper", help="Wayland pointer helper accepting x, y, screen width, screen height, and wheel steps")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    binary = root / "native-announcer/bin" / f"herald-linux-{'arm64' if os.uname().machine == 'aarch64' else 'x64'}"
    original_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    previous_accessibility = accessibility_status(connection)
    evidence_root = root / "temp/verification"
    evidence_root.mkdir(parents=True, exist_ok=True)
    evidence = Path(tempfile.mkdtemp(prefix="linux-settings-", dir=evidence_root))
    log = (evidence / "runtime.log").open("w")
    data = Path(tempfile.mkdtemp(prefix="herald-settings-", dir="/tmp/opencode"))
    settings_path = data / "settings.json"
    env = dict(os.environ, HERALD_DATA=str(data))
    owned = []
    actions = []
    pages = ["Characters", "Audio", "Quiet hours", "Speech service", "Offline voice", "Announcements"]
    proof = {"passed": False, "binary": str(binary), "binarySha256": original_hash, "desktopEntry": args.desktop_entry}
    current_page = None
    window = app = None
    peer = None
    original_workspace = None
    layout_results = []

    def track_processes():
        for record in settings_processes(binary, data):
            if record not in owned:
                owned.append(record)

    def new_window(before):
        track_processes()
        return next((item for item in clients() if item["address"] not in before and item["mapped"] and any(record["pid"] == item["pid"] for record in owned)), None)

    def launch():
        before = {item["address"] for item in clients()}
        subprocess.run(["gio", "launch", args.desktop_entry], env=env, stdout=log, stderr=log, check=True)
        window = wait_for(lambda: new_window(before), "The installed launcher did not open a graphical settings window.")
        pid = window["pid"]
        executable = Path(f"/proc/{pid}/exe").resolve()
        assert executable == binary, f"Unexpected settings executable {executable}"
        assert window["title"] == "Herald settings", f"Unexpected graphical window {window['title']}"
        app = wait_for(lambda: application(pid), "The settings window was not exposed through AT-SPI.")
        wait_for(lambda: control(app, "Apply", Atspi.Role.PUSH_BUTTON), "Apply was not available.")
        actions.append({"action": "launch", "pid": pid, "window": window})
        return window, app

    def page(app, label):
        nonlocal current_page
        button = wait_for(lambda: control(app, label, Atspi.Role.PUSH_BUTTON), f"Missing page {label}")
        click(button)
        wait_for(lambda: control(app, label, Atspi.Role.HEADING), f"Page {label} did not render.")
        if current_page is not None and current_page != label:
            wait_for(lambda: control(app, current_page, Atspi.Role.HEADING) is None, f"Page {current_page} remained visible.")
        current_page = label
        actions.append({"action": "navigate", "page": label})

    def close(app, window):
        click(control(app, "Close", Atspi.Role.PUSH_BUTTON))
        wait_for(lambda: not any(item["pid"] == window["pid"] for item in clients()), "Close did not close the settings window.")
        wait_for(lambda: not Path(f"/proc/{window['pid']}").exists(), "Closing settings did not exit its process.")
        actions.append({"action": "close", "pid": window["pid"]})

    def verify_layout(app, window):
        page(app, "Quiet hours")
        click(control(app, "Quiet mode", Atspi.Role.CHECK_BOX))
        for layout in ["tile", "narrow", "short", "wide", "tile-again"]:
            if layout in {"narrow", "short", "wide"}:
                if not next(item for item in clients() if item["pid"] == window["pid"])["floating"]:
                    dispatch_window(window, "float", '')
                width, height = {"narrow": (420, 650), "short": (420, 360), "wide": (1100, 700)}[layout]
                dispatch_window(window, "resize", f'x = {width}, y = {height}, relative = false')
                wait_for(lambda: next((item for item in clients() if item["pid"] == window["pid"] and item["size"] == [width, height]), None), "The test window did not reach the requested size.")
            if layout == "tile-again":
                dispatch_window(window, "float", '')
            current = next(item for item in clients() if item["pid"] == window["pid"])
            if "tile" in layout:
                assert not current["floating"], "Settings forced itself out of the tiled layout."
            for index, label in enumerate(pages):
                page(app, label)
                if label == "Characters":
                    click(control(app, "Astronaut", Atspi.Role.LIST_ITEM))
                if label == "Quiet hours":
                    assert checked(control(app, "Quiet mode", Atspi.Role.CHECK_BOX)), "Live resizing lost an unsaved edit."
                controls = layout_bounds(app, current)
                snapshot(app, current, evidence, f"{layout}-page-{index}")
                if layout == "short" and label == "Announcements" and args.scroll_helper:
                    reset = control(app, "Reset defaults", Atspi.Role.PUSH_BUTTON)
                    monitor = next(item for item in json.loads(subprocess.check_output(["hyprctl", "-j", "monitors"])) if item["id"] == current["monitor"])
                    subprocess.run([args.scroll_helper, str(current["at"][0] + current["size"][0] - 20), str(current["at"][1] + 180), str(monitor["width"]), str(monitor["height"]), "35"], check=True)
                    wait_for(lambda: 0 <= bounds(reset)["y"] < bounds(control(app, "Apply", Atspi.Role.PUSH_BUTTON))["y"], "The bottom control was not reachable after scrolling.")
                    snapshot(app, current, evidence, "short-scrolled")
                layout_results.append({"layout": layout, "page": label, "size": current["size"], "floating": current["floating"], "controls": controls})
        page(app, "Quiet hours")
        click(control(app, "Quiet mode", Atspi.Role.CHECK_BOX))
        assert json.loads(settings_path.read_text())["quietMode"] is False, "Resizing saved an unapplied edit."

    def verify_keyboard(app, window):
        def focused():
            return next((node for node in walk(app) if node.get_state_set().contains(Atspi.StateType.FOCUSED) and node.get_role() != Atspi.Role.FRAME), None)

        def press(key, *modifiers):
            active = json.loads(subprocess.check_output(["hyprctl", "-j", "activewindow"]))
            assert active.get("pid") == window["pid"], "Keyboard verification lost its owned window focus."
            command = ["wtype"]
            for modifier in modifiers:
                command.extend(["-M", modifier])
            command.extend(["-k", key])
            for modifier in reversed(modifiers):
                command.extend(["-m", modifier])
            subprocess.run(command, check=True)
            time.sleep(0.1)
            actions.append({"action": "key", "key": key, "modifiers": modifiers})

        def type_text(text):
            active = json.loads(subprocess.check_output(["hyprctl", "-j", "activewindow"]))
            assert active.get("pid") == window["pid"], "Text verification lost its owned window focus."
            subprocess.run(["wtype", "-d", "20", text], check=True)
            time.sleep(0.1)
            actions.append({"action": "type", "text": text})

        def expect_focus(name):
            def matches():
                node = focused()
                return node if node is not None and node.get_name() == name else None
            return wait_for(matches, f"Keyboard focus did not reach {name}.")

        def tab_to(name):
            visited = []
            for _ in range(45):
                node = focused()
                if node is not None:
                    visited.append(node.get_name())
                    if node.get_name() == name:
                        return node
                press("Tab")
                time.sleep(0.05)
            raise AssertionError(f"Tab did not reach {name}. Visited {visited}")

        expect_focus("Audio")
        press("Home")
        expect_focus("Characters")
        press("End")
        expect_focus("Announcements")
        press("Tab", "ctrl")
        expect_focus("Characters")
        press("Tab", "ctrl")
        expect_focus("Audio")
        press("Tab", "ctrl")
        wait_for(lambda: control(app, "Quiet hours", Atspi.Role.HEADING), "Ctrl+Tab did not change pages.")
        expect_focus("Quiet hours")
        press("Tab")
        expect_focus("Quiet mode")
        press("space")
        wait_for(lambda: checked(control(app, "Quiet mode", Atspi.Role.CHECK_BOX)), "Space did not toggle the focused checkbox.")
        assert json.loads(settings_path.read_text())["quietMode"] is False, "The keyboard edit saved before Apply."
        press("s", "ctrl")
        wait_for(lambda: json.loads(settings_path.read_text())["quietMode"] is True, "Ctrl+S did not Apply settings.")
        press("Escape")
        assert any(item["pid"] == window["pid"] for item in clients()), "Escape closed the settings window."
        press("space")
        wait_for(lambda: not checked(control(app, "Quiet mode", Atspi.Role.CHECK_BOX)), "The second Space did not toggle quiet mode.")
        press("s", "ctrl")
        wait_for(lambda: json.loads(settings_path.read_text())["quietMode"] is False, "The second Ctrl+S did not Apply.")
        press("Tab")
        expect_focus("Daily schedule")
        press("Tab")
        expect_focus("Apply")
        press("Tab", "shift")
        expect_focus("Daily schedule")
        press("space")
        wait_for(lambda: checked(control(app, "Daily schedule", Atspi.Role.CHECK_BOX)), "Space did not enable the schedule fields.")
        press("Tab")
        expect_focus("From")
        before = settings_path.read_bytes()
        press("a", "ctrl")
        type_text("bad")
        press("s", "ctrl")
        wait_for(lambda: control(app, "Settings status: Use HH:MM for times.", Atspi.Role.STATUS_BAR), "The validation error was not exposed as accessible status text.")
        assert settings_path.read_bytes() == before, "An invalid keyboard edit changed saved settings."
        press("a", "ctrl")
        type_text("22:00")
        press("Tab", "shift")
        expect_focus("Daily schedule")
        press("space")
        press("s", "ctrl")
        wait_for(lambda: json.loads(settings_path.read_text())["scheduleEnabled"] is False, "The repaired schedule draft did not save.")
        proof["keyboardValidationStatusVerified"] = True
        proof["keyboardDisabledControlsSkipped"] = True
        press("Tab", "ctrl", "shift")
        wait_for(lambda: control(app, "Audio", Atspi.Role.HEADING), "Ctrl+Shift+Tab did not select the previous page.")
        expect_focus("Audio")
        press("Right")
        wait_for(lambda: control(app, "Quiet hours", Atspi.Role.HEADING), "Navigation arrows did not change pages.")
        press("Tab", "ctrl", "shift")
        slider = tab_to("Announcer volume")
        press("Right")
        press("s", "ctrl")
        wait_for(lambda: json.loads(settings_path.read_text())["volume"] == 1, "The focused slider did not respond to Right.")
        press("End")
        press("s", "ctrl")
        wait_for(lambda: json.loads(settings_path.read_text())["volume"] == 100, "End did not move the slider to its maximum.")
        press("Home")
        press("s", "ctrl")
        wait_for(lambda: json.loads(settings_path.read_text())["volume"] == 0, "Home did not return the slider to mute.")
        output = tab_to("Output device")
        before = settings_path.read_bytes()
        press("space")
        wait_for(lambda: any(node.get_role() == Atspi.Role.LIST and node.get_child_count() > 0 for node in walk(app)), "Space did not open the output dropdown.")
        press("Escape")
        wait_for(lambda: not any(node.get_role() == Atspi.Role.LIST for node in walk(app)), "Escape did not close the output dropdown.")
        expect_focus("Output device")
        assert settings_path.read_bytes() == before, "Popup dismissal changed saved settings."
        press("space")
        wait_for(lambda: any(node.get_role() == Atspi.Role.LIST for node in walk(app)), "The dropdown did not reopen.")
        press("End")
        wait_for(lambda: (items := [node for node in walk(app) if node.get_role() == Atspi.Role.LIST_ITEM]) and items[-1].get_state_set().contains(Atspi.StateType.SELECTED), "End did not select the last dropdown item.")
        press("Home")
        wait_for(lambda: (items := [node for node in walk(app) if node.get_role() == Atspi.Role.LIST_ITEM]) and items[0].get_state_set().contains(Atspi.StateType.SELECTED), "Home did not select the first dropdown item.")
        press("Return")
        wait_for(lambda: not any(node.get_role() == Atspi.Role.LIST for node in walk(app)), "Enter did not confirm and close the dropdown.")
        expect_focus("Output device")
        press("s", "ctrl")
        wait_for(lambda: json.loads(settings_path.read_text()).get("outputDevice") is None, "Home did not select System default.")
        for name in ["Quiet hours", "Speech service", "Offline voice", "Announcements"]:
            press("Tab", "ctrl")
            expect_focus(name)
        wait_for(lambda: control(app, "Announcements", Atspi.Role.HEADING), "Keyboard page cycling did not reach Announcements.")
        prompt = tab_to("Summary prompt")
        before = settings_path.read_bytes()
        press("a", "ctrl")
        type_text("Keyboard first line")
        press("Return")
        type_text("Keyboard second line")
        assert settings_path.read_bytes() == before, "Enter in a multiline field applied settings."
        press("s", "ctrl")
        wait_for(lambda: json.loads(settings_path.read_text())["summaryPrompt"] == "Keyboard first line\nKeyboard second line", "Multiline keyboard editing did not persist the literal text.")
        snapshot(app, window, evidence, "keyboard-applied")
        focused_id = focused().get_accessible_id()
        dispatch_window(window, "float", '')
        dispatch_window(window, "resize", 'x = 420, y = 360, relative = false')
        wait_for(lambda: next((item for item in clients() if item["pid"] == window["pid"] and item["size"] == [420, 360]), None), "Keyboard test window did not resize.")
        assert expect_focus("Summary prompt").get_accessible_id() == focused_id, "Resizing replaced the focused input."
        reset_before = bounds(control(app, "Reset defaults", Atspi.Role.PUSH_BUTTON))
        footer_top = min(bounds(node)["y"] for node in walk(app) if node.get_accessible_id() == "status" or node.get_name() in {"Apply", "Close"})
        assert reset_before["y"] + reset_before["height"] > footer_top, "The scroll target was already visible before Tab."
        reset = tab_to("Reset defaults")
        nav_bottom = max(bounds(control(app, name, Atspi.Role.PUSH_BUTTON))["y"] + bounds(control(app, name, Atspi.Role.PUSH_BUTTON))["height"] for name in pages)
        wait_for(lambda: nav_bottom <= bounds(reset)["y"] and bounds(reset)["y"] + bounds(reset)["height"] <= footer_top, "Tab did not scroll the focused bottom button into view.")
        snapshot(app, window, evidence, "keyboard-scrolled")
        press("Tab", "shift")
        expect_focus("Summary prompt")
        dispatch_window(window, "float", '')
        expect_focus("Summary prompt")
        press("Tab", "ctrl")
        expect_focus("Characters")
        press("Tab")
        wait_for(lambda: (node := focused()) is not None and node.get_role() in {Atspi.Role.LIST, Atspi.Role.LIST_BOX}, "Tab did not enter the character list.")
        press("End")
        wait_for(lambda: (items := [node for node in walk(app) if node.get_role() == Atspi.Role.LIST_ITEM]) and items[-1].get_state_set().contains(Atspi.StateType.SELECTED), "End did not select the last character.")
        press("Home")
        wait_for(lambda: any(node.get_name() == "Astronaut" and node.get_state_set().contains(Atspi.StateType.SELECTED) for node in walk(app)), "Home did not select the first character.")
        tab_to("Delete")
        press("Return")
        cancel = wait_for(lambda: control(app, "Cancel", Atspi.Role.PUSH_BUTTON), "Enter did not open the delete confirmation.")
        dialog_delete = next(node for node in reversed(list(walk(app))) if node.get_name() == "Delete" and node.get_role() == Atspi.Role.PUSH_BUTTON)
        dialog_controls = [(node.get_name(), bounds(node)) for node in [cancel, dialog_delete]]
        for _ in range(6):
            press("Tab")
            wait_for(lambda: (node := focused()) is not None and (node.get_name(), bounds(node)) in dialog_controls, "Tab escaped the modal dialog.")
        snapshot(app, window, evidence, "keyboard-dialog")
        press("Escape")
        wait_for(lambda: control(app, "Cancel", Atspi.Role.PUSH_BUTTON) is None, "Escape did not dismiss the confirmation.")
        expect_focus("Delete")
        assert any(node.get_name() == "Astronaut" for node in walk(app)), "Dismissing the confirmation deleted the character."
        proof["keyboardDialogFocusVerified"] = True
        proof["keyboardCharacterListVerified"] = True
        proof["keyboardResizeFocusPreserved"] = True
        proof["keyboardScrolledIntoView"] = True
        proof["keyboardVerified"] = True

    try:
        enable_accessibility(connection, True)
        settings_path.write_text(json.dumps({"quietMode": False, "scheduleEnabled": False, "volume": 0}))
        if args.layout or args.keyboard:
            original_workspace = json.loads(subprocess.check_output(["hyprctl", "-j", "activeworkspace"]))
            subprocess.run(["hyprctl", "eval", f'hl.dispatch(hl.dsp.focus({{workspace = "name:herald-layout-{os.getpid()}"}}))'], check=True, capture_output=True)
            peer = subprocess.Popen(["python3", "-c", 'import gi; gi.require_version("Gtk", "3.0"); from gi.repository import Gtk; window = Gtk.Window(title="Layout verification peer"); window.connect("destroy", Gtk.main_quit); window.show_all(); Gtk.main()'], stdout=log, stderr=log)
            wait_for(lambda: next((item for item in clients() if item["pid"] == peer.pid), None), "The tiling peer did not open.")
        window, app = launch()
        if args.keyboard:
            verify_keyboard(app, window)
        if args.layout:
            verify_layout(app, window)
        for index, label in enumerate(pages):
            page(app, label)
            if label == "Characters":
                click(control(app, "Astronaut", Atspi.Role.LIST_ITEM))
                wait_for(lambda: control(app, "Delete", Atspi.Role.PUSH_BUTTON), "Selecting a character did not open its editor.")
            if label == "Audio":
                click(control(app, "Play example", Atspi.Role.PUSH_BUTTON))
                actions.append({"action": "muted-preview", "screenshot": "page-1.png"})
            snapshot(app, window, evidence, f"page-{index}")
        subprocess.run([str(binary), "--settings", "--assets", str(root / "native-announcer/resources")], env=env, stdout=log, stderr=log, check=True, timeout=15)
        track_processes()
        assert settings_processes(binary, data) == [identity(window["pid"])], "A second invocation left another settings process running."
        assert [item["address"] for item in clients() if any(record["pid"] == item["pid"] for record in owned)] == [window["address"]], "A second invocation opened another settings window."
        actions.append({"action": "single-instance", "pid": window["pid"]})
        page(app, "Quiet hours")
        quiet = control(app, "Quiet mode", Atspi.Role.CHECK_BOX)
        assert quiet is not None and not checked(quiet), "The initial quiet state is wrong."
        click(quiet)
        wait_for(lambda: checked(control(app, "Quiet mode", Atspi.Role.CHECK_BOX)), "Quiet mode did not change in the UI.")
        assert json.loads(settings_path.read_text())["quietMode"] is False, "Editing wrote settings before Apply."
        click(control(app, "Apply", Atspi.Role.PUSH_BUTTON))
        wait_for(lambda: json.loads(settings_path.read_text())["quietMode"] is True, "Apply did not persist quiet mode.")
        saved = settings_path.read_bytes()
        (evidence / "settings-after-apply.json").write_bytes(saved)
        snapshot(app, window, evidence, "applied")
        click(control(app, "Quiet mode", Atspi.Role.CHECK_BOX))
        wait_for(lambda: not checked(control(app, "Quiet mode", Atspi.Role.CHECK_BOX)), "The unapplied edit did not reach the UI.")
        assert settings_path.read_bytes() == saved, "An unapplied edit changed the settings file."
        close(app, window)
        assert settings_path.read_bytes() == saved, "Close did not discard the unapplied edit."
        window, app = launch()
        current_page = None
        page(app, "Quiet hours")
        assert checked(control(app, "Quiet mode", Atspi.Role.CHECK_BOX)), "Reopening did not restore the saved quiet mode."
        snapshot(app, window, evidence, "reopened")
        assert hashlib.sha256(binary.read_bytes()).hexdigest() == original_hash, "The installed executable changed during verification."
        assert not (data / "errors.log").exists(), "The app wrote a runtime error log."
        close(app, window)
        assert not (data / "errors.log").exists(), "Closing settings wrote a runtime error log."
        proof.update({"passed": True, "pages": pages, "characterEditorOpened": True, "mutedPreviewScreenshot": "page-1.png", "singleInstance": True, "applyPersisted": True, "closeDiscardedDraft": True, "reopenLoadedSavedState": True})
        if args.layout:
            proof.update({"tiledLaunch": True, "liveResizeRetainedDraft": True, "layoutCases": len(layout_results), "wheelScrollVerified": args.scroll_helper is not None})
    except Exception as error:
        proof["error"] = str(error)
        try:
            if window is not None and app is not None and any(item["pid"] == window["pid"] for item in clients()):
                snapshot(app, window, evidence, "failure")
        except Exception as snapshot_error:
            proof["snapshotError"] = str(snapshot_error)
        raise
    finally:
        cleanup_errors = []
        try:
            if peer is not None and peer.poll() is None:
                peer.terminate()
                peer.wait(timeout=5)
        except (OSError, subprocess.TimeoutExpired) as error:
            cleanup_errors.append(f"Tiling peer cleanup failed: {error}")
        track_processes()
        for record in owned:
            try:
                if identity(record["pid"]) == record:
                    os.kill(record["pid"], 15)
                    wait_for(lambda: identity(record["pid"]) != record, "Verification process did not exit.")
            except (OSError, AssertionError) as error:
                cleanup_errors.append(f"Process cleanup failed: {error}")
        evidence_copied = True
        for filename in ["settings.json", "errors.log"]:
            try:
                if (data / filename).exists():
                    shutil.copy2(data / filename, evidence / filename)
            except OSError as error:
                evidence_copied = False
                cleanup_errors.append(f"Evidence copy failed: {error}")
        processes_exited = all(identity(record["pid"]) != record for record in owned)
        if processes_exited and evidence_copied:
            try:
                shutil.rmtree(data)
            except OSError as error:
                cleanup_errors.append(f"Scratch cleanup failed: {error}")
        log.close()
        if original_workspace is not None:
            try:
                workspace = str(original_workspace["id"]) if original_workspace["id"] > 0 else "name:" + original_workspace["name"]
                subprocess.run(["hyprctl", "eval", f'hl.dispatch(hl.dsp.focus({{workspace = {json.dumps(workspace)}}}))'], check=True, capture_output=True)
            except Exception as error:
                cleanup_errors.append(f"Workspace restore failed: {error}")
        try:
            enable_accessibility(connection, previous_accessibility)
            assert accessibility_status(connection) == previous_accessibility, "The original accessibility state was not restored."
        except Exception as error:
            cleanup_errors.append(f"Accessibility restore failed: {error}")
        if cleanup_errors:
            proof.update({"passed": False, "cleanupErrors": cleanup_errors})
        (evidence / "actions.json").write_text(json.dumps(actions, indent=2))
        if args.layout:
            (evidence / "layout.json").write_text(json.dumps(layout_results, indent=2))
        (evidence / "proof.json").write_text(json.dumps(proof, indent=2))
        (evidence / "cleanup.json").write_text(json.dumps({"scratch": str(data), "scratchRemoved": not data.exists(), "processesExited": processes_exited, "accessibilityRestored": accessibility_status(connection) == previous_accessibility}))
        print(json.dumps({**proof, "evidence": str(evidence)}, indent=2), flush=True)
        if cleanup_errors:
            raise RuntimeError("; ".join(cleanup_errors))


if __name__ == "__main__":
    main()
