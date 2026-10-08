# herald

A desktop herald that announces completed AI tasks with animated characters and spoken summaries. Works with OpenCode and Claude. Tasks lasting at least one minute trigger an announcement when they finish.

It waits for background jobs, stays above other windows without taking focus, and works while the current session is open. New messages reset the timer. Speech is muted during meetings and 22:00–08:00.

## Announcement generation

Forking the existing conversation is a critical requirement for every announcement, including subagent completions. Append the announcement prompt to the existing context without changing the original conversation. Claude uses `$.model.fork`; its fork uses the main conversation, with the completed task's report appended. OpenCode uses `ctx.session.generate` with the existing session ID. Never replace this with a standalone request built only from a report.

## Layout

- `native-announcer/`: shared Rust code, executable in `bin/`, runtime videos in `resources/`. New library characters keep their source PNG in `resources/portraits/` and silent MP4 in `resources/videos/`; both belong in Git.
- `claude-plugin/`, `opencode-plugin/`: integration code and tests only; both use the shared announcer.
- `development_tools/`: build, generation and verification scripts.
- `generated-assets/`: unpublished videos; excluded from Git.

Save every character image in the single `native-announcer/resources/portraits/` folder, including source images. Use descriptive filenames instead of per-character image folders. Showcase recordings and screenshots belong in `temp/`.

## Deployment

For playback, settings, audio, or plugin-completion changes, read `.claude/skills/verify-civilized-agent/SKILL.md` and verify each affected entry point. Run `npm run check` after code changes.

When asked to rebuild and deploy on Windows, run `npm run deploy:plugins` from the repository root. It tests both plugins, rebuilds and deploys the shared executable to `native-announcer/bin/civilized-announcer-win32-<arch>.exe`, deploys the plugins to the Claude WHG profile and the existing OpenCode registration, and restarts the announcer with videos from `native-announcer/resources/videos/`. Restart Claude sessions to load new hooks. Reload OpenCode only when requested with `npm run deploy:plugins -- -ReloadOpenCode`; this reloads all loaded locations and cancels pending permissions and forms. Use `-- -WhatIf` for a dry run.

## Generate herald video

1. Read `~/_admin/image-generation.md`. Match the original portrait in `native-announcer/resources/portraits`.
2. Create mono 16 kHz speech outside the project: “I bring news for your attention. Listen as I deliver this announcement.” Fit it into four seconds. Silence produces idle mouths.
3. Verify ComfyUI/CUDA. Stop the previous job before replacing it.
4. Use the Wan speech-to-video graph in `development_tools/generate_assets.py`: 256×256, 65 frames, 20 steps. Short prompt: enthusiastic announcement, preserved identity/costume/scroll/framing, fixed camera/lighting.
5. After latent prepend/decode, select 64 frames from index 4; encode silent at 16 fps. Verify 256×256, four seconds/64 frames and visible mouth movement before publishing.
6. Publish to `native-announcer/resources` only when requested. Remove temporary inputs, job files, caches and intermediates; close only the server you started.

Unpublished generated assets stay out of Git; requested library portraits and videos are tracked. Save no prompts or graphs; recovery files contain only job IDs/fingerprints. Cleanup preserves requested assets and runtime files, and deletes obsolete build output rather than merely ignoring it.
