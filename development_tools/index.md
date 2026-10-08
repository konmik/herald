# Development tools

Run tools from the repository root. Package-script entry points are in [package.json](../package.json); invoke them with `pnpm run <script>`. Windows scripts use PowerShell 7.

## Build and deploy

| Tool | Purpose |
| --- | --- |
| [setup-checks.ps1](setup-checks.ps1) | Install locked JavaScript dependencies and the Python test environment. |
| [check.ps1](check.ps1) | Run lint, plugin tests, type checks, asset checks, tooling tests and Rust tests. |
| [build-announcer.mjs](build-announcer.mjs) | Build the native executable and copy it into the checkout's `native-announcer/bin/`. |
| [install-linux-menu.mjs](install-linux-menu.mjs) | Install the Herald Settings application entry; Linux native builds run this automatically. |
| [build-bundle.ps1](build-bundle.ps1) | Build or repackage a Windows release ZIP without replacing the checkout executable. |
| [deploy-plugins.ps1](deploy-plugins.ps1) | Check, build, verify and install a release bundle. Changes host registrations and restarts the announcer. |
| [bundle/](bundle/) | Installer, OpenCode registration and Start menu shortcut helpers. |
| [bundle-licenses.ps1](bundle-licenses.ps1), [licenses/](licenses/) | Collect bundled JavaScript dependency licenses. |
| [install-directory.mjs](install-directory.mjs) | Replace a staged installation directory with rollback on failure. |

See [installation details](../INSTALLATION.md) for bundle behavior and deployment options.

## Characters and speech

| Tool | Purpose |
| --- | --- |
| [generate_character.py](generate_character.py) | Generate and publish one portrait and talking video through ComfyUI. |
| [generate_assets.py](generate_assets.py) | Shared portrait/video graphs and asset generation, publishing and cleanup commands. |
| [asset_library.py](asset_library.py) | Consolidate portraits and remove non-library intermediates. Cleanup deletes files. |
| [install-models.py](install-models.py) | Download Wan model files into the configured ComfyUI installation. |
| [prepare-tts.mjs](prepare-tts.mjs) | Download and verify the local Kitten voice model for development. |
| [record-announcement.py](record-announcement.py) | Record isolated playback with SAPI narration and a plain background. |
| [record-samples.ps1](record-samples.ps1) | Generate Windows speech samples and viseme timing data. |

For character generation, read [the generation skill](../.claude/skills/generate-character/SKILL.md). For configured voices and actual desktop recordings, read [the recording skill](../.agents/skills/record-announcement/SKILL.md); the recording helper alone does not provide those.

### Generation requirements

Generation needs a local [ComfyUI](https://github.com/comfyanonymous/ComfyUI) installation with CUDA, a compatible NVIDIA GPU, and its Python environment. Install the development Python dependencies from [requirements-checks.txt](requirements-checks.txt); model downloads also need `huggingface-hub`. FFmpeg and FFprobe must be on PATH.

Set `COMFYUI_DIRECTORY` or pass `--comfy-dir` to the generation and model-installation scripts. The default is `~/ComfyUI`; no machine-specific installation is assumed. The helper uses ComfyUI at `http://127.0.0.1:8188`, and can start it from `<ComfyUI directory>/venv` if it is not already running.

[install-models.py](install-models.py) lists and downloads the Wan speech-to-video models. Portrait generation additionally uses `flux1-dev-fp8.safetensors`, `clip_l.safetensors`, `t5xxl_fp8_e4m3fn.safetensors` and `ae.safetensors`. Download these separately under their model licenses. On Windows, automatic narration uses Microsoft David Desktop; supply `--audio` to `generate_character.py` when that voice is unavailable or on another platform.

## Verification and profiling

Read [the verification skill](../.claude/skills/verify-herald/SKILL.md) before playback, settings, audio or host-completion checks. Its helpers preserve evidence and keep the live announcer separate.

| Tool | Purpose |
| --- | --- |
| [verify-announcer.ps1](verify-announcer.ps1) | Checkout-binary playback checks. |
| [verify-settings.ps1](verify-settings.ps1) | Settings persistence, speech models, audio previews and single-instance checks. |
| [verify-settings-layout.ps1](verify-settings-layout.ps1) | Settings layout checks. |
| [verify-settings-theme.ps1](verify-settings-theme.ps1) | Settings theme checks. |
| [verify-settings-status.ps1](verify-settings-status.ps1) | Settings status display checks. |
| [verify-characters.ps1](verify-characters.ps1) | Character settings, video picker, prompts, text editing and playback checks. |
| [verify-announcement-audio.py](verify-announcement-audio.py) | Capture output audio and check announcement/preview timing. |
| [verify-linux-playback.py](verify-linux-playback.py) | Check Hyprland desktop transparency, sustained animation, focus and captured output audio with an isolated demo. |
| [voice-api-fixture.mjs](voice-api-fixture.mjs) | Local speech API fixture for deterministic tests. |
| [host-verification/](host-verification/) | Real Claude/OpenCode lifecycle drivers and proof capture. These tests make model requests. |
| [benchmark-tts.ps1](benchmark-tts.ps1) | Measure local voice loading and synthesis across thread counts. |
| [profile-plugin.ts](profile-plugin.ts) | Measure memory during 10,000 simulated completion cycles. |

## Tooling tests

- [test-tools.ps1](test-tools.ps1): Python `test_*.py` suites in this directory; dependencies are listed in [requirements-checks.txt](requirements-checks.txt).
- [test-bundle.ps1](test-bundle.ps1), [test-deploy-plugins.ps1](test-deploy-plugins.ps1): bundle and deployment checks using test fixtures.
- [test-verification-process.ps1](test-verification-process.ps1): verification process ownership checks.
- [tests/](tests/): JavaScript asset and registration tests.
