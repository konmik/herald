# Audio output selection

Users choose an audio output, refresh device discovery, and retain an unavailable selection while using the system default.

## Sub-features

- Unavailable saved devices remain visible and survive Apply.
- Refresh preserves the selected unavailable device.
- System default clears the explicit device selection.
- Available devices route announcement and preview sound to that device.

## How to get to it (user POV)

Open settings and use the output dropdown, `Refresh devices`, and Apply. Preview sound with `Play example`.

## Driving it with Win32

Preconditions: baseline; no real device changes required for system-default proof.

- **Unavailable device, refresh, system default.** Run `pwsh -NoProfile -File .cursor/skills/verify-civilized-agent/scripts/verify.ps1 -Feature Output -Evidence temp/verification/output-proof`. The seeded missing device appears as `Selected device unavailable (using system default)`. Refresh and Apply preserve its ID in `settings-unavailable-device.json`. Selecting `System default` and Apply saves a null output device. Reopening displays the default selection.
- **Physical output.** For routing changes, select an actual named device in an isolated settings window and play an example; capture loopback output or ask a human to confirm that device. Record the selected label and saved device ID. The helper does not prove physical routing.

## Gotchas

- Device list indices vary across machines. Index zero is the stable `System default` entry; never assume another index names a particular speaker.
- An unavailable selection is expected to remain saved, not silently disappear.
- Keep audio evidence separate from the user's personal sessions.
