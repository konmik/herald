---
name: record-announcement
description: Record a Civilized Agent announcement as a narrated video. Use when asked to record the last played message or make a video from a supplied message.
---

# Record an announcement

## 1. Select the message

Work from this repository on Windows. For the last message, read `history.jsonl` in `CIVILIZED_AGENT_DATA`, or `%LOCALAPPDATA%/CivilizedAgent` when that variable is unset. Match `sessionID` to `OPENCODE_SESSION_ID` when the user means the current OpenCode session. Use the saved text and selected character unchanged.

An older message without a history entry cannot be recovered exactly. Explain that gap before replaying a reconstructed result. Any newly generated announcement must fork the existing conversation, as required by the repository's `AGENTS.md`.

For a supplied message, use its exact text with `--text` instead of `--last`.

Done when the exact text, session and saved video are identified, or the user has supplied the text.

## 2. Prepare recording

Use `development_tools/record-announcement.py --help` for its current arguments. The helper requires FFmpeg, the deployed native announcer with `--isolated`, and a Python environment containing `numpy`, `soundcard`, `soundfile` and `pillow`.

This machine's recording environment is `%LOCALAPPDATA%/Temp/opencode/civilized-recording-venv/Scripts/python.exe`. If absent, create that virtual environment with Python 3.13 and install those four packages into it. Keep dependencies and temporary captures outside the repository.

The helper records an isolated announcer without stopping the live one. It enables narration outside quiet hours only in its temporary settings; meeting detection remains active. Record with no competing system audio, since WASAPI captures the default speaker's output. If the deployed binary lacks `--isolated`, obtain authorization to rebuild and deploy before continuing.

Done when the dependencies import, FFmpeg works and no meeting or competing playback is active.

## 3. Capture

Save the deliverables under `./temp` with a descriptive, unused filename. In PowerShell, for the current session:

```powershell
& "$env:LOCALAPPDATA/Temp/opencode/civilized-recording-venv/Scripts/python.exe" development_tools/record-announcement.py --last --session $env:OPENCODE_SESSION_ID --output ./temp/last-announcement.mp4
```

For supplied text, replace `--last --session ...` with `--text 'The exact message.'`. For a particular saved history, use `--history`.

The helper preserves the last message's video by placing only that video in its isolated library. It starts screen and audio capture before sending the message, records its own process's window, and crops the final MP4 to that window. Temporary full-desktop footage and audio are removed on exit. It also saves a PNG preview and JSON verification beside the MP4.

Done when the helper exits successfully and the MP4, preview and verification exist under `./temp`.

## 4. Verify and deliver

Inspect the preview for the complete text, selected character and any private background visible around the window. Use FFprobe to confirm H.264 video, AAC audio, nonzero duration and constant 30 fps. Check audio volume with FFmpeg `volumedetect`.

Read the JSON verification: the recorded text and character must match the original; `speechStarted` and `finished` must both be 1, with zero `mutedAnnouncements`. The helper rejects muted or unfinished narration rather than exporting other system audio as the announcement.

Done when the recording is visibly correct, narration is present and verification matches. Return the MP4 path; describe reconstructed messages as replays, not recovered originals.
