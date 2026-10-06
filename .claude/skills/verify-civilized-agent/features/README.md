# Civilized Agent verification map

Read this index before choosing a proof. Each entry point remains separately accountable; a native transport check is not a host-plugin check.

## Baseline preconditions

- Follow Launch in `../SKILL.md`; require an interactive Windows desktop, PowerShell 7, Node and Cargo.
- Use a fresh evidence directory under `temp/verification/`.
- Helpers own unique disposable data directories. Leave the user's companion and host sessions alone.
- Require Doctor before driving and successful cleanup after each attempt, including failures.
- Serialize focus and audio checks. Default runs are silent.

## Driving conventions

Use `pwsh -NoProfile -File .claude/skills/verify-civilized-agent/scripts/verify.ps1` for real native controls, or `announce.ps1` for the production transport boundary. Control IDs are stable Win32 handles, not coordinate guesses. Preserve action transcripts, resulting controls, persisted state and process identity. Additional host paths require a disposable real host session; record unavailable entries as skipped, not passed.

## Features

- [Settings persistence](settings.md): Apply, Close without saving, reopen, CPU/GPU selection.
- [Quiet mode and schedule](quiet.md): immediate mute, daily times, persisted controls, meeting suppression.
- [Audio output](output.md): missing device fallback, refresh, system default, persisted selection.
- [Audio preview](preview.md): silent preview, unsaved audio settings, start/stop, completion.
- [Announcements](announcements.md): real host completions, native rendering, character animation, focus, history, dismissal.
