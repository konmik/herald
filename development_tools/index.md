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
| [verify-settings.ps1](verify-settings.ps1) | Legacy Win32 baseline checks; does not drive the GPUI settings window. |
| [verify-settings-gpui.ps1](verify-settings-gpui.ps1) | Drive the GPUI settings window through Windows UI Automation and preserve persistence, navigation and screenshot evidence. |
| [verify-settings-layout.ps1](verify-settings-layout.ps1) | Legacy Win32 baseline layout checks. |
| [verify-settings-theme.ps1](verify-settings-theme.ps1) | Legacy Win32 baseline theme checks. |
| [verify-settings-status.ps1](verify-settings-status.ps1) | Legacy Win32 baseline status checks. |
| [verify-characters.ps1](verify-characters.ps1) | Legacy Win32 character controls and playback checks. |
| [verify-announcement-audio.py](verify-announcement-audio.py) | Capture output audio and check announcement/preview timing. |
| [verify-linux-playback.py](verify-linux-playback.py) | Check Hyprland desktop transparency, sustained animation, focus and captured output audio with an isolated demo. |
| [voice-api-fixture.mjs](voice-api-fixture.mjs) | Local speech API fixture for deterministic tests. |
| [host-verification/](host-verification/) | Real Claude/OpenCode lifecycle drivers and proof capture. These tests make model requests. |
| [benchmark-tts.ps1](benchmark-tts.ps1) | Measure local voice loading and synthesis across thread counts. |
| [benchmark-settings-memory.ps1](benchmark-settings-memory.ps1) | Compare release settings-process working set and private bytes with isolated data and alternating runs. |
| [profile-plugin.ts](profile-plugin.ts) | Measure memory during 10,000 simulated completion cycles. |

`pnpm run test:settings` drives GPUI controls through Windows UI Automation. Sliders and dropdowns use normal pointer actions on the owned controls when GPUI Kit's accessibility actions are unsupported. It checks navigation, single-instance activation, draft persistence, Close/reopen, silent previews, unavailable output devices and System default. It does not play audible speech or install a voice model.

Idle settings and zero-volume previews must not load the offline speech runtime. Audible previews load the model on demand and release it when playback finishes or is cancelled; each local preview pays the model-loading cost. The background announcer preloads and retains its speech cache only when no ElevenLabs key is configured and speech is not muted. With remote speech configured, the local model loads only if remote speech fails and is released after fallback playback. Font enumeration reads format signatures, not complete font files.

Compare two preserved release executables with `pnpm run benchmark:settings -Baseline <old.exe> -Treatment <new.exe> -Evidence temp/verification/settings-memory`. The defaults are five alternating runs per executable, five seconds of warmup and ten seconds of sampling. The results include per-run working-set and private-byte medians, ranges, raw samples, executable hashes and cleanup evidence. Working set is resident RAM; private bytes are committed private memory. GPU memory is excluded. `complete` means all measurements finished, not that a difference is statistically significant.

Use `-TtsDirectory <model-directory>` to run both variants with the same alternate local model. Omitting it uses the prepared model under `native-announcer/resources/tts/`. Runtime errors make the measurement inconclusive; a missing model is not a valid substitute for installed-model measurements.

The raw benchmark also records committed address space by private, mapped and image regions, plus thread counts and thread-start modules. These are diagnostic snapshots after sampling, not resident-memory measurements. Image and mapped regions can be shared, so their committed sizes must not be added to the private-byte counter or presented as RAM use.

## Tooling tests

- [test-tools.ps1](test-tools.ps1): Python `test_*.py` suites in this directory; dependencies are listed in [requirements-checks.txt](requirements-checks.txt).
- [test-bundle.ps1](test-bundle.ps1), [test-deploy-plugins.ps1](test-deploy-plugins.ps1): bundle and deployment checks using test fixtures.
- [test-verification-process.ps1](test-verification-process.ps1): verification process ownership checks.
- [tests/](tests/): JavaScript asset and registration tests.
