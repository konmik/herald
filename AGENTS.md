# Civilized Agent

A desktop companion for OpenCode and Claude. When a task lasting at least one minute finishes, it displays an animated character and speaks a short summary of the result.

It waits for background jobs, stays above other windows without taking focus, and works while the current session is open. New messages reset the timer. Speech is muted during meetings and 22:00–08:00.

## Layout

- `native-announcer/`: shared Rust code, executable in `bin/`, runtime videos in `resources/`.
- `claude-plugin/`, `opencode-plugin/`: integration code and tests only; both use the shared announcer.
- `development_tools/`: build, generation and verification scripts.
- `generated-assets/`: portraits and unpublished videos; excluded from Git.

## Generate herald video

1. Read `~/_admin/image-generation.md`. Match the original portrait in `generated-assets/character-portraits`.
2. Create mono 16 kHz speech outside the project: “I bring news for your attention. Listen as I deliver this announcement.” Fit it into four seconds. Silence produces idle mouths.
3. Verify ComfyUI/CUDA. Stop the previous job before replacing it.
4. Use the Wan speech-to-video graph in `development_tools/generate_assets.py`: 256×256, 65 frames, 20 steps. Short prompt: enthusiastic announcement, preserved identity/costume/scroll/framing, fixed camera/lighting.
5. After latent prepend/decode, select 64 frames from index 4; encode at 16 fps, export silent at 8 fps. Verify four seconds/32 frames and visible mouth movement before replacing `generated-assets/<character>/neutral.mp4`.
6. Publish to `native-announcer/resources` only when requested. Remove temporary inputs, job files, caches and intermediates; close only the server you started.

Generated assets stay out of Git. Save no prompts or graphs; recovery files contain only job IDs/fingerprints. Cleanup preserves requested assets and runtime files, and deletes obsolete build output rather than merely ignoring it.
