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

Use `development_tools/record-announcement.py --help` for its current arguments. The helper requires Windows SAPI, FFmpeg, Rust and a Python environment containing `pillow`. It builds a separate release binary outside the repository; `--binary` can select an existing binary with `--isolated`, `--capture-frames` and `--capture-speech-seconds`. Capture duration follows the complete speech file, including slower configured voices.

This machine's recording environment is `%LOCALAPPDATA%/Temp/opencode/civilized-recording-venv/Scripts/python.exe`. If absent, create that virtual environment with Python 3.13 and install `pillow` into it. Keep dependencies and temporary frames outside the repository.

The helper renders an isolated announcer without stopping or deploying the live one. Its preview is muted. It synthesizes the complete narration directly into a WAV using the integration's SAPI voice, rate and pitch, then combines that file with timestamped rendered frames. Neither desktop content nor system audio enters the recording. This produces a replay, not a capture of the original playback.

Done when Pillow imports, FFmpeg works and the recording binary builds.

## 3. Capture

Save the deliverables under `./temp` with a descriptive, versioned filename, such as `last-announcement-v1.mp4`. For another recording of the same message, increment the highest existing version and keep the earlier videos. In PowerShell, for the current session (replace `v1` with the next unused version):

```powershell
& "$env:LOCALAPPDATA/Temp/opencode/civilized-recording-venv/Scripts/python.exe" development_tools/record-announcement.py --last --session $env:OPENCODE_SESSION_ID --output ./temp/last-announcement-v1.mp4
```

For supplied text, replace `--last --session ...` with `--text 'The exact message.'`. For a particular saved history, use `--history`.

The helper preserves the last message's video by placing only that video in its isolated library. It exports the announcer's rendered pixels against a plain background and aligns speech with the end of the opening transition. It includes 500 ms of empty background before appearance and after disappearance, rather than holding the final announcement frame. It mixes the announcer's own interference sound at the opening and closing transition times, with increased volume for the recording. Temporary frames and audio are removed on exit. It also saves a PNG preview and JSON verification beside the MP4.

Done when the helper exits successfully and the MP4, preview and verification exist under `./temp`.

## 4. Verify and deliver

Inspect the preview for the complete text, selected character and plain background. Use FFprobe to confirm H.264 video, AAC audio, nonzero duration and constant 30 fps. Check audio volume with FFmpeg `volumedetect`.

Read the JSON verification: the recorded text and character must match the original; `narrationGenerated` must be true and `finished` must be 1. Require `beforeSeconds` and `afterSeconds` to be 0.5. Speech must finish before `announcementDisappearsSeconds`. Inspect frames inside both padding intervals for empty background and frames near the closing transition for continued animation. The isolated preview's `speechStarted` is 0 because narration comes from the complete WAV instead.

Require `interferenceIncluded` to be true and check both transition audio segments for nonzero volume.

Done when the recording is visibly correct, narration and both interference sounds are present and verification matches. Return the MP4 path; describe reconstructed messages as replays, not recovered originals.
