---
name: verify-herald
description: Verify Herald's Windows desktop announcements and settings after changes to playback, quiet mode, audio previews, persistence, or plugin delivery.
---

# Verify Herald

Read [the feature map](features/README.md), then select the affected entry points. The primary surface is the native Windows desktop companion and settings window. Claude and OpenCode supply completion announcements; they are additional user surfaces, not HTTP services. Run commands from the repository root in PowerShell 7 on an interactive Windows desktop.

## Launch

For local automated checks, run `npm run setup:checks` once, then `npm run check`. Setup installs locked JavaScript dependencies and project-local Python check dependencies. Checks do not deploy, reload sessions, require GitHub, or run paid host tests.

Prepare the CPU speech assets and build once:

```powershell
node development_tools/prepare-tts.mjs
cargo build --locked --manifest-path native-announcer/Cargo.toml
```

Require both exit codes to be zero. Asset preparation downloads a checksum-verified model when missing; it is not a dry-run and does touch the network. It retains runtime assets. Cargo may download dependencies. The debug executable's adjacent `onnxruntime.dll` and `sherpa-onnx-c-api.dll` are needed. There is no auth or seeded user account.

Use the debug executable rather than `npm run build:announcer`: that command builds successfully but then copies over the deployed executable, which can fail with `EBUSY` while the user's companion is running. Verification must leave that companion running. Avoid concurrent builds of the same checkout.

Launch and drive a disposable settings instance:

```powershell
pwsh -NoProfile -File .claude/skills/verify-herald/scripts/verify.ps1 -Feature Settings -Evidence temp/verification/settings-proof
```

The helper starts `native-announcer/target/debug/herald.exe --settings`, waits up to ten seconds for its window, runs Doctor, drives the feature, closes and reopens it, then cleans up. Readiness is the owned `Herald settings` window with `Apply` and `Close` controls.

Announcement launch and drive:

```powershell
pwsh -NoProfile -File .claude/skills/verify-herald/scripts/announce.ps1 -Evidence temp/verification/announcement-proof
```

This starts the same executable with `--isolated --assets native-announcer/resources --test-seconds 35`, an evidence report and render snapshot. Readiness is a live owned process and initialized inbox, followed by rendered history. The helper maintains presence through the production bridge. Playback exits automatically; the helper bounds the run at 45 seconds.

Every helper creates a unique `HERALD_DATA` directory under `$env:LOCALAPPDATA/Temp/opencode` and sets `HERALD_TTS` to the repo's prepared model. Settings windows are single-instance **per data directory**. Isolated playback binds an ephemeral localhost port; normal playback uses shared port 47863. Keep disposable instances separate from the default data directory. Independent scratch state permits side-by-side runs, but desktop focus and audible output are shared: run focus/audio checks one at a time.

## Doctor

The helpers run one read-only Doctor gate before driving: the owned process is alive at the expected executable path, the expected controls or inbox exist, and there is no runtime error log. Settings Doctor also checks that the executable SHA256 has not changed. Each settings reopen repeats it.

If an instance looks wrong, stop the helper and inspect `instance.json`, `failure.txt`, `errors.log`, and `actions.txt` in its evidence folder. Compare the manifest's PID, start time, binary path and hash with `Get-Process -Id` and `Get-FileHash`. Rerun into a new evidence folder only after cleanup. A PID alone does not establish ownership after process reuse.

## Drive

Use the shipped Win32 helper rather than coordinates. It uses the production controls and their normal Apply, Close, Refresh devices and Play example handlers. Exact recipes and assertions live in the feature map:

- [Settings persistence](features/settings.md)
- [Quiet mode and schedule](features/quiet.md)
- [Audio output](features/output.md)
- [Audio preview](features/preview.md)
- [Announcements](features/announcements.md)

`Settings`, `Quiet`, `Output`, and `Preview` are accepted `-Feature` values. Preview is silent by default; `-Audible` deliberately plays sound. Announcement `-Speech` enables sound; `-Speech -Meeting` or `-Speech -Quiet` checks suppression. There is no dry-run flag.

For plugin-generation changes, transport injection is insufficient: exercise each affected real Claude/OpenCode session entry listed in the announcement map. Forking the existing conversation remains required. Do not replace that path with a standalone model request. If a disposable host session cannot be established safely, report that entry as unverified.

## Evidence

Use a new folder under `temp/verification/` for every run. Helpers default to a GUID folder; explicit example paths must be changed when already used. Evidence is local and ignored by Git.

Settings proof includes the action transcript, binary identity, control snapshots before/during/after/reopening, and actual settings files before/after Apply. It must prove Apply persisted values, Close discarded an unapplied edit, and a newly opened user window loaded the persisted state. Reading only the JSON file is insufficient.

Announcement proof includes the exact submitted notification, action transcript, actual rendered PNG, final playback report, shown-message history and drained queue. The PNG is the real renderer's output, not a desktop screenshot; it cannot alone prove stacking or focus. Require the report's passive-window and focus checks, decoded frames, completion count, and matching persisted history. Audio-start counts do not prove audible speaker output; capture loopback audio or obtain human confirmation when sound quality is the feature under test.

Exercise user controls and real host-session paths, not internal settings setters or mocked model generation. Fixture files only establish isolated starting state. The announcement helper injects at the existing production file-transport boundary and verifies the real downstream app; label that scope explicitly. It does not prove completion timing, conversation forking, host presence tracking, subagent generation or dismissal commands. Record every skipped mapped entry point with its unmet precondition. A convenient tested entry point does not cover the others.

## Cleanup

Both helpers use `finally` on success and failure: close or kill only the process object they started, remove its uniquely named scratch data, restore the announcement helper's prior environment, and write `cleanup.json`. Close and reopen are tracked sequentially. Evidence folders are outside scratch and survive teardown.

After every attempt, require `cleanup.json` to report `scratchRemoved: true` and `processExited: true`. Confirm `actions.txt` and the successful feature's proof files still exist. For forced interruption that prevents `finally`, verify the manifest PID **and start time and executable path**, stop only that owned process, and remove only the manifest's unique scratch directory. Keep the evidence. Never stop processes by name, redeploy plugins, reload host sessions, or remove runtime assets for verification cleanup.

## Helpers

- `scripts/verify.ps1`: launch, Doctor, drive native settings, capture control/file evidence, reopen, cleanup. Invoke with `pwsh -NoProfile -File` as above.
- `scripts/announce.ps1`: isolated native playback through the real Claude bridge, presence heartbeat, render/report/history evidence, cleanup. Invoke with `pwsh -NoProfile -File` as above.
- `scripts/hosts.ps1`: real Claude/OpenCode generation, background work, subagents, cancellation/restart and one final main announcement. Recipes and coverage limits are in the announcement map. It sets `HERALD_EXTERNAL_COMPANION=1` with unique data so the plugins use the helper-owned companion rather than starting another process. Host tests are explicit and use real model requests; they are not part of `npm run check`.

PowerShell scripts are executable through `pwsh`; no file association or Unix executable bit is needed. Existing `development_tools/verify-settings.ps1` covers additional speech-model selection, local voice preview and single-instance checks, but deletes its scratch proof. Existing `verify-announcer.ps1` targets the checkout binary and lacks failure cleanup. Neither replaces the evidence-preserving helpers here.
