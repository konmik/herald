---
name: record-announcement
description: Record a Herald announcement with its character voice and desktop background. Use when asked to record the last played message or a supplied message.
---

# Record an announcement

## 1. Select the message

Work from this repository on Windows. For the last message, read `history.jsonl` in `HERALD_DATA`, or `%LOCALAPPDATA%/herald` when that variable is unset. Match `sessionID` to `OPENCODE_SESSION_ID` when the user means the current OpenCode session. If that session has no saved announcement, ask before choosing another session. Preserve the saved text, title and video unless the user overrides them.

An older message without a history entry cannot be recovered exactly. Explain that gap before replaying a reconstructed result. Any newly generated announcement must fork the existing conversation, as required by the repository's `AGENTS.md`.

For a supplied message, use its exact text with `--text` instead of `--last`. Resolve a requested character against the saved character settings and library. Otherwise use the saved announcement's character, or the selected character for supplied text. Resolve its configured voice provider, voice ID and speech model from the runtime settings. Use the integration's fallback only when the character has no voice override.

Use a supplied title unchanged; otherwise preserve the saved announcement's title or use the current session title. Change the actual session name only when asked. A recording title must not silently reuse an unrelated session's title.

Done when the exact text, title, character video and voice settings are identified.

## 2. Prepare recording

Read `development_tools/record-announcement.py --help` and inspect its implementation before running it. Use an existing deployed binary through `--binary` when it supports `--isolated`, `--capture-frames` and `--capture-speech-seconds`. Otherwise build a separate binary outside the repository. Leave the live announcer and its deployment unchanged.

Use a Python 3.13 virtual environment outside the repository with `pillow` installed. Install provider dependencies there only when needed. Keep adapters, dependencies, temporary frames and audio outside the repository.

The helper's SAPI narration and plain-background rendering do not satisfy a configured ElevenLabs voice or desktop capture. If the current helper lacks these options, use a temporary adapter around its existing playback and timing functions. Keep the repository helper unchanged unless the user asks to change it.

Synthesize the complete narration with the resolved provider, voice and model, retaining configured speech settings. For ElevenLabs, use the saved API key and the production request format for the selected model; decrypt protected credentials locally without printing or saving them. Do not silently substitute SAPI when a configured provider fails. If the user supplies audio, use the complete file at its original speed instead of synthesizing replacement speech.

Done when Pillow imports, FFmpeg works, a compatible binary exists and complete narration is available using the requested voice or supplied audio.

## 3. Capture

Save the MP4, PNG preview and JSON verification under `./temp` with a descriptive, versioned filename. For another recording of the same message, increment the highest existing version and keep earlier videos. Use `--last --session <id>` for saved messages, `--text 'The exact message.'` for supplied text, or `--history` for a particular history file; pass only arguments supported by the helper.

Launch an isolated, muted announcer with disposable settings and presence. Put only the chosen video in its isolated library and select that character explicitly. Preserve the live settings and process.

Capture the actual desktop with FFmpeg `gdigrab`; a plain background or wallpaper composite is not desktop capture. Start before the announcement appears and continue after it disappears. Use DPI-aware physical window bounds to crop the announcer plus desktop padding: add 15% of the original window width on the left and 15% of its height above, rounded to pixels. Preserve the original content size. Record a user-specified padding override instead when supplied. Check that the requested crop fits on screen.

Align captured desktop timestamps with announcement appearance and the recorded transition timings. Keep 500 ms of unobstructed desktop before appearance and after disappearance. Align the complete narration with the end of the opening transition; playback must last through the full narration, with no trimming, speeding up or fixed four-second limit. Mix the announcer's own interference sound at both transitions, with increased volume for the recording. Capture no system audio.

Close only the isolated announcer and screen recorder started for this recording. Remove their temporary frames, desktop footage and audio on success, failure or cancellation.

Done when both owned processes have exited, scratch files are removed and the MP4, preview and verification exist under `./temp`.

## 4. Verify and deliver

Inspect the preview for the complete message, requested character, actual desktop background and correct top/left padding. Check the displayed session title against the requested title; report any clipping rather than silently shortening it. Use FFprobe to confirm H.264 video, AAC audio, nonzero duration and constant 30 fps. Check audio volume with FFmpeg `volumedetect`.

Read the JSON verification: the recorded text, title and character must match the resolved request, and `finished` must be 1. Record the actual voice provider, voice ID, speech model, desktop capture, physical window bounds, padding fraction and pixel amounts. For synthesized speech, require `narrationGenerated` to be true; for supplied audio, record its source and full duration instead. Require `beforeSeconds` and `afterSeconds` to be 0.5. Speech must finish before `announcementDisappearsSeconds`. Inspect frames inside both padding intervals for unobstructed desktop and frames near the closing transition for continued animation. The isolated preview's `speechStarted` is 0 because narration is mixed from the complete audio file.

Require `interferenceIncluded` to be true and check both transition audio segments for nonzero volume.

Done when the recording is visibly correct, the complete requested narration and both interference sounds are present, verification matches and the live announcer remains running. Return the MP4 path. Describe this as a recorded replay, not a recovered capture of the original announcement.
