# Settings persistence

Users apply announcer preferences, discard unapplied edits, and recover saved preferences when reopening the settings window.

## Sub-features

- Apply saves the current controls and preserves existing voice preferences.
- Close discards later unapplied changes.
- Reopen loads saved settings.
- CPU is the default; the GPU checkbox selects optional GPU speech.

## How to get to it (user POV)

- Open the installed Civilized Agent settings shortcut.
- Run the announcer executable with `--settings`.
- Run `--settings` again for the same data directory to bring the existing window forward.

## Driving it with Win32

Preconditions: baseline from the index; a fresh `temp/verification/settings-proof` folder.

- **Apply and reopen.** Run `pwsh -NoProfile -File .cursor/skills/verify-civilized-agent/scripts/verify.ps1 -Feature Settings -Evidence temp/verification/settings-proof`. The helper clicks quiet/schedule controls, enters `22:30` and `08:15`, sets volume to 35, leaves CPU selected, clicks Apply, and reads the saved file. The status is `Saved. Changes apply to the next announcement.`
- **Discard.** The same run changes volume to 15 without Apply, clicks Close, and verifies the saved bytes are unchanged.
- **Reopen.** A new settings process must load 35% and the saved checkbox values. Compare `controls-reopened.json` with `settings-after.json`; require exit zero and `result.json` passed.
- **Additional entry points.** The installed shortcut and repeated launch are not driven by this helper. For shortcut changes, inspect and launch that actual shortcut in a disposable profile. For single-instance/GPU changes, the existing `development_tools/verify-settings.ps1` supplies the concrete Win32 recipe, but add evidence preservation and retain both process objects before repeating its launch. Mark these entries skipped until driven.

## Gotchas

- Settings is single-instance per canonical data directory, not globally. Default data would drive the user's real settings.
- Apply affects subsequent announcements; this recipe proves settings persistence, not active playback reload.
- Fixture JSON seeds values only; all mutations under test go through controls.
- Preserve evidence after cleanup; installed shortcut launch needs separate proof from direct executable launch.
