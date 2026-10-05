---
name: generate-character
description: Generate a character portrait and talking video, retain both in the repository, and add the video to the shared library.
disable-model-invocation: true
argument-hint: <name> <character description>
---

# Generate character

Run only when the user invokes this skill: `@generate-character <name> <description>` in OpenCode or `/generate-character <name> <description>` in Claude. The invocation authorizes generating one character and publishing it to the shared library, not committing, playing it, or generating extra variants.

1. Use the supplied name and description. Ask for either if missing. Names use lowercase letters, digits and hyphens. Read `~/_admin/image-generation.md` before generation.
2. From the repository root, run one command:

   ```powershell
   C:\ComfyUI\venv\Scripts\python.exe development_tools/generate_character.py <name> --description "<character description>"
   ```

   The helper checks CUDA and an idle ComfyUI queue, starts ComfyUI only if needed, generates a native 256×256 portrait, synthesizes mono 16 kHz speech outside the repository, and animates it with the existing Wan graph. It publishes a silent four-second, 64-frame, 16 fps MP4 without resizing the portrait. It removes its temporary inputs and ComfyUI outputs and closes only a server it started. It preserves an existing portrait after failure; rerun with `--resume` to finish that character. A completed name is never overwritten.
3. Inspect the saved portrait and extract a temporary contact sheet outside the repository:

   ```powershell
   ffmpeg -hide_banner -loglevel error -i native-announcer/resources/videos/<name>.mp4 -vf "fps=4,scale=256:256,tile=4x4" -frames:v 1 -update 1 "$env:LOCALAPPDATA/Temp/opencode/<name>-contact.png"
   ```

   Check that the description matches, the entire head fits, the identity stays stable, and the mouth visibly moves. Remove the contact sheet afterward. Do not play the video unless asked. If visual verification fails, report the problem and remove only the newly published video from the library, keeping the portrait for inspection; ask before regeneration.
4. Confirm the PNG in `native-announcer/resources/portraits/` and the MP4 in `native-announcer/resources/videos/` exist, pass the helper's media checks, and are not ignored by Git. Both are requested repository assets. Save no prompt, graph, audio, or cache in the repository. Report their paths; leave committing to the user.
