# herald installation

A desktop herald that announces completed AI tasks with animated characters and spoken summaries.

## Omarchy and Arch Linux

Install the native build and runtime dependencies:

```sh
omarchy pkg add base-devel rust gtk3 at-spi2-core ffmpeg espeak-ng libpulse ttf-liberation
pnpm install --frozen-lockfile
pnpm run build:announcer -- --release
```

The executable is `native-announcer/bin/herald-linux-x64` on x86-64. Keep the checkout and its `native-announcer/resources` directory available.

Linux builds automatically add **Herald Settings** to the application launcher. It opens the graphical settings app. Existing settings are preserved. To install or refresh just the menu entry, run `pnpm run install:linux-menu`.

Settings follows your window manager's tiling policy. Narrow windows use wrapped top navigation and stacked forms. Page content scrolls while Apply and Close remain visible. Do not add a floating rule for `herald-settings` unless you prefer floating settings windows.

### Settings keyboard navigation

Use Tab and Shift+Tab to move through controls. The page navigation is one Tab stop; arrow keys move between pages. Ctrl+Tab and Ctrl+Shift+Tab select the next or previous page. Enter or Space activates a focused button; Space toggles a checkbox. Lists, dropdowns, and sliders use arrow keys and Home/End.

Ctrl+S applies settings without closing the window. Enter in a multiline prompt inserts a newline. Escape dismisses a popup or dialog without closing Settings or discarding its draft. Window management shortcuts remain owned by your desktop, including Omarchy's Super+W and Alt+Tab.

Use `HERALD_DATA`, `HERALD_BINARY` and `HERALD_TTS` for explicit development overrides.

If a cold build exceeds the helper's two-minute limit, run `cargo build --release --locked --manifest-path native-announcer/Cargo.toml` first. Then rerun the helper to copy the executable.

Add the checkout's absolute path to `plugins` in `~/.config/opencode/opencode.json`:

```json
{
  "plugins": [
    {
      "package": "/absolute/path/to/herald",
      "options": { "minimumSeconds": 60 }
    }
  ]
}
```

Preserve existing configuration fields and other plugins. Start a new host session to load the plugin.

For Claude, install a copy of `claude-plugin` with `native-announcer/bin` and `native-announcer/resources` inside that copy. Register the copy with `claude plugin marketplace add <copy-path> --scope user`, then install `herald@herald-local` with `claude plugin install --scope user`. A marketplace cache copy cannot use the checkout's sibling runtime directory.

For Codex, do the same with a copy of `codex-plugin`: put `native-announcer/bin` and `native-announcer/resources` inside the copy, then run `codex plugin marketplace add <copy-path>` and `codex plugin add herald@herald-local`. Both commands edit `~/.codex/config.toml` in place. Codex hooks require Node 22.13 or later on PATH. Node's built-in SQLite provides session locks that release when a hook process exits. Start Codex, open `/hooks` and trust the Herald hooks; Codex skips untrusted plugin hooks. Helper output goes to `PLUGIN_DATA/helpers.log` inside the Codex plugin data directory.

For Pi, register the checkout after building the announcer:

```sh
node development_tools/bundle/register-pi.mjs --installed /absolute/path/to/herald
```

The helper adds `pi-plugin/index.ts` to `extensions` in `~/.pi/agent/settings.json`. `PI_CODING_AGENT_DIR` or `--agent-dir <directory>` selects another profile. It preserves other settings and extensions. Use `--dry-run` to preview without writing, or `--backup-directory <directory>` to save the previous settings. Run `/reload` in Pi or restart Pi to load the extension. Keep the checkout and its shared runtime available. Herald uses the SDK from your installed `@earendil-works/pi-coding-agent` package.

On Omarchy, add this rule to `~/.config/hypr/hyprland.lua`:

```lua
o.window({ title = "^Herald$" }, {
  float = true,
  pin = true,
  no_initial_focus = true,
  no_focus = true,
  no_follow_mouse = true,
  decorate = false,
  no_shadow = true,
  no_blur = true,
  no_anim = true,
  border_size = 0,
  opacity = "1 override 1 override",
  move = { "monitor_w-window_w-20", "monitor_h-window_h-20" },
})

```

Run `hyprctl reload`, then confirm `hyprctl configerrors` reports no errors.

Linux uses eSpeak NG for speech. Open **Herald Settings** to configure quiet hours, volume, and characters. Settings are stored in `~/.local/share/herald/settings.json`. `XDG_DATA_HOME` or `HERALD_DATA` can override the data location.

## Windows bundle

PowerShell 7 is required. Build on Windows with Bun, Node and Cargo matching the machine architecture:

```powershell
pnpm run build:bundle
```

The release ZIP is written to `temp/bundles`. Building reuses the Cargo cache in `native-announcer/target/bundle`; it does not replace the checkout executable or install anything. The voice engine and model are not bundled. The separate build cache keeps release builds away from the running checkout executable.

To repackage an already extracted complete bundle without rebuilding native code or downloading assets:

```powershell
pwsh -NoProfile -File development_tools/build-bundle.ps1 -PayloadDirectory .\extracted-bundle -OutputDirectory .\temp\bundles
```

Repackaging refreshes the installer and its helpers and generates a new manifest in a staging copy. The source bundle is unchanged.

`pnpm run test:bundle` runs unit checks with placeholder native files and mocked Claude commands. It does not prove a working native installation. Before deployment, run `pnpm run test:bundle:installed -- -Archive <release.zip>`. This uses a disposable Claude profile and real host commands, verifies the installed cache and shortcut, removes the extraction folder, boots OpenCode from the installed compiled package, and starts the real native executable. It makes no model requests and leaves the disposable installation available for further runtime checks.

Close any open herald settings windows (currently titled `Herald settings`). Extract the ZIP, then run its installer:

```powershell
pwsh -NoProfile -File .\install.ps1 -WhatIf
pwsh -NoProfile -File .\install.ps1
```

Installation needs neither Cargo, Bun, nor network downloads. Pi registration needs Node on PATH, as does Pi itself. Open Settings > Offline voice and click Install to download the checksum-verified voice engine and model. The tab shows installation status and offers a retry if installation fails. Offline voice is stored with user data and shared by the plugins across upgrades. Without it, ElevenLabs can still provide speech and visual announcements still work. The default Claude profile is `CLAUDE_CONFIG_DIR`, otherwise `.claude`. OpenCode uses `XDG_CONFIG_HOME/opencode`, otherwise `.config/opencode`. Pi uses `PI_CODING_AGENT_DIR`, otherwise `.pi/agent`. Claude, Codex, OpenCode, and Pi are registered when their commands are on PATH. Missing hosts are skipped without changing their configuration.

The Start menu shortcut uses Windows' native Unicode shell-link interface, including when checking an existing shortcut's ownership. PowerShell 7 compiles this helper internally; no separate compiler installation is needed. Local speech uses Windows short filenames for non-ASCII paths. If the volume does not provide those names, install into an ASCII path.

The complete payload is copied to `%LOCALAPPDATA%/Programs/herald/versions/<version>-<arch>-<payloadHash>`. When Claude is installed, its local marketplace points there and its cache receives a separate physical runtime copy. When `codex` is on PATH, the Codex `herald-local` marketplace points to the installed `codex-plugin` and `codex plugin add` copies it into the Codex plugin cache. Codex hooks require Node 22.13 or later on PATH for built-in SQLite session locks. Node is not bundled. When OpenCode is installed, its registration points to that installed version. The Start menu shortcut is installed even when no hosts are present. The extraction folder and checkout can then be removed. User data remains in `%LOCALAPPDATA%/herald`.

When Pi is installed, its user settings point to the installed bundle's absolute `pi-plugin/index.ts` path. The bundle includes Pi's own `.mjs` runtime helpers, including `pi-plugin/bridge.mjs`, but no Pi SDK or plugin tests. Registration replaces only paths whose package identifies the Herald Pi extension. Keep an old registered package available during migration. Unknown paths and unrelated extensions remain unchanged. Registration refuses an active settings lock and recovers a lock whose PID owner is dead. An empty crash lock can be recovered after one minute; unknown owners and unsafe locks are rejected.

Existing owned Claude marketplace registrations migrate with `claude plugin marketplace add` from the installed source. This changes the source without removing installed plugins. Existing cache junctions are replaced without touching their targets. OpenCode migration recognizes local packages by their package name, exports and plugin ID, preserving unrelated entries, comments and options. Keep the old registered package available until migration completes; unknown registrations are not removed.

For a copy-only test installation:

```powershell
pwsh -NoProfile -File .\install.ps1 -InstallDirectory "$env:LOCALAPPDATA/Temp/opencode/herald-test" -SkipHostRegistration -NoStart
```

`-ClaudeConfigDirectory`, `-CodexHome`, `-OpenCodeConfigDirectory`, `-PiAgentDirectory`, and `-ProgramsDirectory` select other targets. Repeat installation verifies the existing payload and repairs owned corrupt runtime files. Installation rejects links, unlisted files, unsafe paths, wrong architectures and checksum failures before copying.

Host registration keeps the previous Claude cache until the other registrations and the shortcut have succeeded. A registration failure restores the previous cache, Claude registry and settings files, Codex `config.toml`, OpenCode configuration, Pi settings, and shortcut. Newly created OpenCode and Pi settings files are removed on rollback. The verified versioned payload remains available for retry.

Restart Claude and Codex sessions after installation, and trust the Herald hooks once in Codex's `/hooks`. OpenCode is explicitly reloaded only with `-ReloadOpenCode`; that reload cancels pending permissions and forms. OpenCode may also watch configuration changes automatically. Installation stops announcers only under verified previous plugin runtime directories, then starts the installed runtime. `-NoStart` suppresses that start.

Run `/reload` in Pi or restart Pi after installation. `-WhatIf` leaves Pi settings unchanged.

`pnpm run deploy:plugins` runs checks, builds the same bundle, then invokes this installer. No updater or uninstaller is included.
