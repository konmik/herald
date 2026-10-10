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
| [deploy-macos.mjs](deploy-macos.mjs) | `pnpm run deploy:macos`: check, build a release payload, install ad-hoc signed `/Applications/Herald.app` (announcer) and `/Applications/Herald Settings.app`, register the Claude and OpenCode plugins and restart the announcer. Tested by [tests/macos-app.test.mjs](tests/macos-app.test.mjs). |
| [bundle/](bundle/) | Installer, OpenCode registration and Start menu shortcut helpers. |
| [bundle-licenses.ps1](bundle-licenses.ps1), [licenses/](licenses/) | Collect bundled JavaScript dependency licenses. |
| [install-directory.mjs](install-directory.mjs) | Replace a staged installation directory with rollback on failure. |

See [installation details](../INSTALLATION.md) for bundle behavior and deployment options.

On macOS, `pnpm run deploy:macos` mirrors the Windows deployment. `Herald.app` runs the announcer outside the Dock (`LSUIElement`) and carries the Windows-equivalent payload in `Contents/Resources/herald/` (OpenCode package, Claude marketplace source with its own `native-announcer/`, `bundle-manifest.json`). `Herald Settings.app` is the Settings entry, like the Linux menu item: the same executable opens Settings when it runs as `Contents/MacOS/Herald Settings`, and a bundled executable reads assets from `Contents/Resources`. Both are signed with `codesign --force --deep -s -` and verified with `codesign --verify --deep --strict`. An application with another bundle identifier is never replaced. The script stops only announcers started from the checkout, an earlier install or the Claude plugin, backs up the Claude and OpenCode configuration to `temp/deploy-backups/macos-<time>/` before registering, and starts the announcer with `open -g /Applications/Herald.app`. Options: `--skip-checks`, `--skip-host-registration`, `--no-start`, `--reload-opencode`, `--applications <dir>`, `--claude-config <dir>`, `--opencode-config <dir>` and `--dry-run`. `CLAUDE_CONFIG_DIR` is passed to `claude` only when chosen, because Claude then also keeps its `.claude.json` in that folder. OpenCode registration is skipped when `opencode` is not on `PATH`. Ad-hoc signatures change on every build, so macOS asks again for Automation and Accessibility (meeting detection) after redeploying, and Gatekeeper (`spctl`) rejects the apps on other Macs.

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
| [verify-entrance-lightning.py](verify-entrance-lightning.py) | Check bottom/right screen-edge strikes, video-first impact, outline propagation, sporadic holding arcs and the swallowing exit in real PNG scenes from `announce.ps1 -CaptureFrames`; pass the evidence `frames/` directory. |
| [preview-lightning.ps1](preview-lightning.ps1) | Preview `-Style original`, `storm`, `electric` or `plasma` silently with the isolated debug announcer and captured frames. Leaves the installed effect and saved settings unchanged. |
| [verify-linux-playback.py](verify-linux-playback.py) | Check Hyprland desktop transparency, sustained animation, focus and captured output audio with an isolated demo. |
| [verify-macos-settings.py](verify-macos-settings.py) | Launch the debug settings window on macOS with disposable data, drive it through Accessibility plus real pointer and keyboard events, and verify every page, controls, previews, Lightning, Characters, layout, keyboard, Apply, Close, reopening and single-instance activation. |
| [verify-linux-settings.py](verify-linux-settings.py) | Launch installed graphical settings on Hyprland, drive its AT-SPI controls, and verify navigation, Apply, draft discard, and reopening. |
| [voice-api-fixture.mjs](voice-api-fixture.mjs) | Local speech API fixture for deterministic tests. |
| [host-verification/](host-verification/) | Real Claude/OpenCode lifecycle drivers and proof capture. These tests make model requests. |
| [benchmark-tts.ps1](benchmark-tts.ps1) | Measure local voice loading and synthesis across thread counts. |
| [benchmark-settings-memory.ps1](benchmark-settings-memory.ps1) | Compare release settings-process working set and private bytes with isolated data and alternating runs. |
| [profile-plugin.ts](profile-plugin.ts) | Measure memory during 10,000 simulated completion cycles. |

`pnpm run test:settings` drives GPUI controls through Windows UI Automation. Sliders and dropdowns use normal pointer actions on the owned controls when GPUI Kit's accessibility actions are unsupported. It checks navigation, single-instance activation, draft persistence, Close/reopen, silent previews, unavailable output devices and System default. It does not play audible speech or install a voice model.

Use `pwsh -NoProfile -File development_tools/verify-settings-gpui.ps1 -Feature Lightning -Binary native-announcer/target/debug/herald.exe` for the inline Lightning preview. It samples entrance, holding and exit, checks character motion and containment, drives presets and all five sliders, verifies draft feedback without saved-byte changes, Apply, Close discard, reopening, off-tab worker teardown and compact keyboard reveal. It checks silence and that no extra native window opens. Evidence survives cleanup. Linux native Lightning playback and settings are unverified on Windows.

`pnpm run test:settings:linux` launches the installed application entry on Hyprland and drives the graphical settings window through AT-SPI. It requires Python GObject bindings with the Atspi typelib, `gio`, `hyprctl`, and `grim`. It temporarily enables the accessibility bus and restores its original state during cleanup. It captures each page, verifies Apply and Close, and reopens the saved settings. Its disposable data does not change your saved settings or stop the live announcer. Evidence remains in `temp/verification/`.

`pnpm run test:settings:macos` drives the debug executable (`native-announcer/target/debug/herald`; override with `--binary`) on an interactive macOS desktop. It needs Python 3 with pyobjc (`pyobjc-framework-Quartz`, `pyobjc-framework-ApplicationServices`) and Pillow from [requirements-checks.txt](requirements-checks.txt), `ffmpeg` on `PATH` for video and the Lightning preview, and **Accessibility** plus **Screen Recording** permission for the application that runs the command (Terminal, an IDE or an agent host). Grant both in System Settings > Privacy & Security and restart that application. `pnpm run test:settings:macos:preflight` reports exactly which permission is missing and for which application; the full run stops with exit code 2 before launching anything when one is missing. Use `--feature settings|controls|keyboard|layout|lightning|characters` for one area. GPUI controls are found by accessibility identifier or label; sliders and dropdowns fall back to real pointer and keyboard events when no AX action is exposed. Keyboard checks use Command+S rather than Ctrl+S and refuse to type unless the owned window is frontmost. The helper uses unique `HERALD_DATA` under `/tmp/opencode`, keeps previews at volume 0, never installs voice models, never stops the live announcer, and writes `proof.json`, `actions.json`, `cleanup.json`, window screenshots and AX snapshots to a unique `temp/verification/macos-settings-*/` folder.

`pnpm run test:settings:layout:linux` also needs the Gtk 3 typelib. It opens a disposable workspace and a tiling peer, checks a tiled launch, resizes through 420×650, 420×360, and 1100×700, then returns to tiling. It checks all seven pages for horizontal overflow, reachable navigation and footer controls, and retained draft values. Cleanup closes its peer and restores the original workspace. An optional `--scroll-helper <path>` accepts a Wayland pointer helper taking x, y, screen width, screen height, and wheel steps. Without that helper, the proof records wheel scrolling as unverified.

`pnpm run test:settings:keyboard:linux` also requires `wtype` and the Gtk 3 typelib. It drives the installed app with real keyboard events and verifies focus, page shortcuts, checkbox and slider edits, Apply, Escape, and multiline text. It uses the same disposable-data and workspace cleanup as layout verification.

Idle settings and zero-volume previews must not load the offline speech runtime. Audible previews load the model on demand and release it when playback finishes or is cancelled; each local preview pays the model-loading cost. The background announcer preloads and retains its speech cache only when no ElevenLabs key is configured and speech is not muted. With remote speech configured, the local model loads only if remote speech fails and is released after fallback playback. Font enumeration reads format signatures, not complete font files.

Compare two preserved release executables with `pnpm run benchmark:settings -Baseline <old.exe> -Treatment <new.exe> -Evidence temp/verification/settings-memory`. The defaults are five alternating runs per executable, five seconds of warmup and ten seconds of sampling. The results include per-run working-set and private-byte medians, ranges, raw samples, executable hashes and cleanup evidence. Working set is resident RAM; private bytes are committed private memory. GPU memory is excluded. `complete` means all measurements finished, not that a difference is statistically significant.

Use `-TtsDirectory <model-directory>` to run both variants with the same alternate local model. Omitting it uses the prepared model under `native-announcer/resources/tts/`. Runtime errors make the measurement inconclusive; a missing model is not a valid substitute for installed-model measurements.

The raw benchmark also records committed address space by private, mapped and image regions, plus thread counts and thread-start modules. These are diagnostic snapshots after sampling, not resident-memory measurements. Image and mapped regions can be shared, so their committed sizes must not be added to the private-byte counter or presented as RAM use.

## Tooling tests

- [test-tools.ps1](test-tools.ps1): Python `test_*.py` suites in this directory; dependencies are listed in [requirements-checks.txt](requirements-checks.txt).
- [test-bundle.ps1](test-bundle.ps1), [test-deploy-plugins.ps1](test-deploy-plugins.ps1): bundle and deployment checks using test fixtures.
- [test-verification-process.ps1](test-verification-process.ps1): verification process ownership checks.
- [tests/](tests/): JavaScript asset and registration tests.
