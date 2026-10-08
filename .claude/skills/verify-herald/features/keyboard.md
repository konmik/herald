# Settings keyboard navigation

Users navigate and edit Settings without a mouse. The desktop retains its window-management shortcuts. Keyboard actions must preserve the existing Apply and Close behavior.

## Sub-features

- Opening Settings focuses the selected page's navigation control.
- Tab and Shift+Tab traverse enabled controls in visual order. Page navigation is one Tab stop; arrow keys move between pages.
- Ctrl+Tab and Ctrl+Shift+Tab select the next and previous page.
- Enter or Space activates a focused button. Space toggles a checkbox.
- Arrow keys and Home/End navigate lists and dropdowns and adjust sliders. Text fields keep their editing keys.
- Ctrl+S applies settings without closing on Linux and Windows. The shared shortcut mapping uses Command+S on macOS when that platform is supported.
- Enter inserts a newline in multiline prompts. It must not apply settings.
- Escape dismisses an open popup or dialog. It must not close Settings or discard the draft.
- Focus has a visible indicator, survives layout changes, and scrolls into view.
- Modal dialogs contain focus and return it to the opening control when dismissed.
- Fields and sliders have accessible names. Status and validation errors are available to assistive technology.
- The app leaves Super shortcuts, Alt+Tab, and macOS Command+Tab to the desktop.

## How to get to it

- Open the installed Herald Settings launcher.
- Run the installed announcer with `--settings`.
- Launch it again with the same data directory and continue using the existing window.

## Linux automated checks

This entry uses an interactive Hyprland desktop, not the index's Windows baseline. Build and install the checkout executable with `pnpm run build:announcer`. Require Python GObject bindings with the Atspi and Gtk 3 typelibs, `wtype`, `gio`, `hyprctl`, and `grim`.

Run `pnpm run test:settings:keyboard:linux` from the repository root. The helper launches the installed desktop entry with disposable settings data in its own workspace. It sends real keyboard events and checks:

1. Initial Audio-page focus, next/previous-page shortcuts, navigation Home/End and wraparound, arrow navigation, and Tab/Shift+Tab traversal.
2. Space changes quiet mode without saving. Ctrl+S persists the change, keeps the window open, and can save the reversed change.
3. Escape leaves Settings open. Space visibly opens the output dropdown; Escape dismisses it and restores its focus without changing the saved file. Reopening, End, Home, and Enter select System default and close the popup.
4. Right increments the volume slider by one; End sets it to 100; Home returns it to zero.
5. Typing two literal lines with Enter changes only the draft until Ctrl+S. The saved `summaryPrompt` must equal `Keyboard first line\nKeyboard second line`.
6. Resizing to 420×360 retains the focused Summary prompt input. Tab scrolls the previously hidden Reset defaults button into view. Shift+Tab returns to the prompt; returning to tiling retains focus.
7. The existing settings checks still verify Apply, Close without saving, reopening, and single-instance behavior.
8. Tab skips disabled schedule fields. Enabling the schedule makes From reachable. An invalid time exposes `Settings status: Use HH:MM for times.` without changing saved settings; correcting the field allows Apply.
9. Character-list End and Home select its last and first items. Enter on Delete opens the confirmation. Repeated Tab stays on its Cancel/Delete controls; Escape dismisses it, restores Delete focus, and leaves the character intact.

Require exit zero and `proof.json` with `keyboardVerified`, `keyboardResizeFocusPreserved`, `keyboardScrolledIntoView`, `keyboardValidationStatusVerified`, `keyboardDisabledControlsSkipped`, `keyboardCharacterListVerified`, and `keyboardDialogFocusVerified` true. Check the literal persisted JSON and the focused-control snapshots, including `keyboard-applied.png`, `keyboard-scrolled.png`, and `keyboard-dialog.png`. Evidence remains in a unique `temp/verification/linux-settings-*/` directory. Require cleanup to report `scratchRemoved`, `processesExited`, and `accessibilityRestored` true.

## Additional platform and control checks

The automated path above does not prove every control or platform. Test these separately and retain an action transcript and screenshots:

- On Linux and Windows, complete an edit, Ctrl+S, close, and reopen using only the keyboard. Test both sidebar and compact navigation, disabled controls, list boundaries, and dropdown selection with arrows and Enter.
- Check Up/Down character selection, Space activation of buttons, text-editing shortcuts, and visible focus throughout traversal.
- Open the video picker from the keyboard. Verify modal focus containment, Escape dismissal, and focus restoration. The helper covers the delete confirmation separately. Do not delete a user character during verification.
- Trigger a validation error using disposable data. Confirm that its accessible text updates and can be read by a screen reader. Check meaningful names for fields and sliders.
- Verify that the desktop's window-switching, workspace, tiling, and close shortcuts still work. Omarchy's stock bindings include Super+W, Super+arrows, Super+T, and Alt+Tab.
- On macOS, first establish a supported settings build. Then verify Command+S, the system keyboard-navigation preference, native dialogs, and Command+Tab. Until that prerequisite is met, record macOS as unavailable rather than passed.

Windows and macOS results are separate from a Linux pass. Do not infer file-picker focus, spoken screen-reader announcements, or complete list/dropdown behavior from the automated subset.

## Gotchas

- Guard every keyboard and text injection with the owned window's PID and current focus. Stop instead of typing when another application has focus.
- Use unique `HERALD_DATA`; never drive the user's saved settings or close their open draft.
- Keep verification volume muted before preview actions. Do not trigger remote speech, voice refresh, or model installation as part of keyboard checks.
- Restore the original workspace and accessibility-bus state. Close only helper-owned processes and preserve evidence after failures.
- Existing Win32 verification helpers do not prove GPUI keyboard behavior. Use the actual graphical app and native keyboard input.
