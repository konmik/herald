# herald

A desktop herald that announces completed AI tasks with animated characters and spoken summaries. Works with OpenCode and Claude. Tasks lasting at least one minute trigger an announcement when they finish.

It waits for background jobs, stays above other windows without taking focus, and works while the current session is open. New messages reset the timer. Speech is muted during meetings. Quiet hours default to 22:00–08:00 and can be changed or disabled.

## Announcement generation

Forking the existing conversation is a critical requirement for every announcement. Subagents stay silent; announce the final main-task result after background work finishes. Append the announcement prompt to the main conversation's existing context without changing the original conversation. Claude uses `$.model.fork`; OpenCode uses `ctx.session.generate` with the existing session ID. Never replace this with a standalone request built only from a report.

## Layout

- `native-announcer/`: shared Rust code, executable in `bin/`, runtime videos in `resources/`. New library characters keep their source PNG in `resources/portraits/` and silent MP4 in `resources/videos/`; both belong in Git.
- `claude-plugin/`, `opencode-plugin/`: integration code and tests only; both use the shared announcer.
- `development_tools/`: read [the tool index](development_tools/index.md) before choosing build, deployment, generation, recording, verification or profiling tools.
- `generated-assets/`: unpublished videos; excluded from Git.

Save every character image in the single `native-announcer/resources/portraits/` folder, including source images. Use descriptive filenames instead of per-character image folders. Scratch recordings and screenshots belong in `temp/`; published README media belongs in `.github/assets/`.

## Deployment

For playback, settings, audio, or plugin-completion changes, read `.claude/skills/verify-herald/SKILL.md` and verify each affected entry point. Run `pnpm run check` after code changes.

When asked to rebuild and deploy on Windows, run `pnpm run deploy:plugins` from the repository root. It checks both plugins, builds and verifies a release bundle, then installs it under `%LOCALAPPDATA%/Programs/herald/versions/<version>-<arch>-<payloadHash>`. Claude receives a physical plugin cache copy; OpenCode points to the installed package. The default Claude profile is `CLAUDE_CONFIG_DIR`, otherwise `.claude`. The installer restarts the announcer with the installed bundle's videos, leaving the checkout executable unchanged. Restart Claude sessions to load new hooks. Reload OpenCode only when requested with `pnpm run deploy:plugins -ReloadOpenCode`; this reloads all loaded locations and cancels pending permissions and forms. Use `pnpm run deploy:plugins -WhatIf` for a dry run.

When asked to rebuild and deploy on macOS, run `pnpm run deploy:macos` from the repository root. It runs `pnpm run check`, builds a release binary and installs ad-hoc signed `/Applications/Herald.app` (announcer, kept out of the Dock) and `/Applications/Herald Settings.app`. The plugin payload is inside `Herald.app/Contents/Resources/herald/`; Claude receives a physical plugin cache copy and OpenCode points to that payload when `opencode` is installed. Claude and OpenCode configuration is backed up to `temp/deploy-backups/` first. The script restarts the announcer from `/Applications` and leaves Settings windows open. Open Settings with `open "/Applications/Herald Settings.app"` or from Launchpad/Spotlight. Use `--skip-checks` only when the failures are known and reported, and `--dry-run` to print the plan.

## Generate herald video

1. Read [the generation requirements](development_tools/index.md#generation-requirements). Match the original portrait in `native-announcer/resources/portraits`.
2. Create mono 16 kHz speech outside the project: “I bring news for your attention. Listen as I deliver this announcement.” Fit it into four seconds. Silence produces idle mouths.
3. Verify ComfyUI/CUDA. Stop the previous job before replacing it.
4. Use the Wan speech-to-video graph in `development_tools/generate_assets.py`: 256×256, 65 frames, 20 steps. Short prompt: enthusiastic announcement, preserved identity/costume/scroll/framing, fixed camera/lighting.
5. After latent prepend/decode, select 64 frames from index 4; encode silent at 16 fps. Verify 256×256, four seconds/64 frames and visible mouth movement before publishing.
6. Publish to `native-announcer/resources` only when requested. Remove temporary inputs, job files, caches and intermediates; close only the server you started.

Unpublished generated assets stay out of Git; requested library portraits and videos are tracked. Save no prompts or graphs; recovery files contain only job IDs/fingerprints. Cleanup preserves requested assets and runtime files, and deletes obsolete build output rather than merely ignoring it.
