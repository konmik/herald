# Quiet mode and daily schedule

Users mute speech and static while keeping announcements visible, either immediately or on a daily schedule. Meetings also suppress sound.

## Sub-features

- Quiet mode mutes audio without disabling the visual announcement.
- Daily schedule enables time fields and persists overnight start/end minutes.
- Meeting suppression leaves notifications visible.

## How to get to it (user POV)

- Open settings, choose `Quiet mode (mute speech and static)`, and Apply.
- Choose `Quiet mode on a daily schedule`, enter From/To times, and Apply.
- Receive an announcement during a detected meeting.

## Driving it with Win32

Preconditions: baseline; fresh evidence paths; no concurrent focus check.

- **Persist quiet controls.** Run `pwsh -NoProfile -File .cursor/skills/verify-civilized-agent/scripts/verify.ps1 -Feature Quiet -Evidence temp/verification/quiet-proof`. Reopened quiet and schedule checkboxes are checked; JSON stores `quietStart: 1350`, `quietEnd: 495`, and `quietMode: true`.
- **Visual while muted.** Run `pwsh -NoProfile -File .cursor/skills/verify-civilized-agent/scripts/announce.ps1 -Speech -Quiet -Evidence temp/verification/quiet-playback`. Require one shown/finished notification, zero speech starts, one muted announcement, matching history and a rendered image.
- **Meeting policy boundary.** Run `pwsh -NoProfile -File .cursor/skills/verify-civilized-agent/scripts/announce.ps1 -Speech -Meeting -Evidence temp/verification/meeting-policy`. The fixture uses the existing meeting-status boundary; the same silent/visible assertions apply. It does not prove automatic meeting detection. That entry needs a real meeting and host completion, with privacy-safe evidence.

## Gotchas

- The helper disables schedules for deterministic playback. Schedule persistence is covered; actual clock-boundary muting requires a separate scheduled run, not a claim based on saved JSON.
- Meeting detection can conservatively mute when status is unknown. A speech-start check alone cannot prove sound was heard.
- Preview is an explicit audio action and is not proof of announcement quiet policy.
