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

Linux builds automatically add **Herald Settings** to the application launcher. It opens the settings file in Omarchy's selected editor, or in `$EDITOR` through `xdg-terminal-exec` on other desktops. Existing settings are preserved. To install or refresh just the menu entry, run `pnpm run install:linux-menu`.

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

Linux uses eSpeak NG for speech. The settings window is Windows-only. Edit `~/.local/share/herald/settings.json` to configure quiet hours, volume, and characters. `XDG_DATA_HOME` or `HERALD_DATA` can override the data location.

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

Installation needs neither Node on PATH, Cargo, Bun, nor network downloads. Open Settings > Offline voice and click Install to download the checksum-verified voice engine and model. The tab shows installation status and offers a retry if installation fails. Offline voice is stored with user data and shared by both plugins across upgrades. Without it, ElevenLabs can still provide speech and visual announcements still work. Host registration uses the installed Claude CLI. The default Claude profile is `CLAUDE_CONFIG_DIR`, otherwise `.claude`. OpenCode uses `XDG_CONFIG_HOME/opencode`, otherwise `.config/opencode`. Both hosts are registered by default.

The Start menu shortcut uses Windows' native Unicode shell-link interface, including when checking an existing shortcut's ownership. PowerShell 7 compiles this helper internally; no separate compiler installation is needed. Local speech uses Windows short filenames for non-ASCII paths. If the volume does not provide those names, install into an ASCII path.

The complete payload is copied to `%LOCALAPPDATA%/Programs/herald/versions/<version>-<arch>-<payloadHash>`. Claude's local marketplace points there and its cache receives a separate physical runtime copy. Node is not bundled; host registration requires the installed Claude and OpenCode CLIs. OpenCode and the Start menu shortcut point to that installed version. The extraction folder and checkout can then be removed. User data remains in `%LOCALAPPDATA%/herald`.

Existing owned Claude marketplace registrations migrate with `claude plugin marketplace add` from the installed source. This changes the source without removing installed plugins. Existing cache junctions are replaced without touching their targets. OpenCode migration recognizes local packages by their package name, exports and plugin ID, preserving unrelated entries, comments and options. Keep the old registered package available until migration completes; unknown registrations are not removed.

For a copy-only test installation:

```powershell
pwsh -NoProfile -File .\install.ps1 -InstallDirectory "$env:LOCALAPPDATA/Temp/opencode/herald-test" -SkipHostRegistration -NoStart
```

`-ClaudeConfigDirectory`, `-OpenCodeConfigDirectory` and `-ProgramsDirectory` select other targets. Repeat installation verifies the existing payload and repairs owned corrupt runtime files. Installation rejects links, unlisted files, unsafe paths, wrong architectures and checksum failures before copying.

Host registration keeps the previous Claude cache until OpenCode registration and the shortcut have succeeded. A registration failure restores the previous cache, Claude registry and settings files, OpenCode configuration and shortcut. A newly created OpenCode config is removed on rollback. The verified versioned payload remains available for retry.

Restart Claude sessions after installation. OpenCode is explicitly reloaded only with `-ReloadOpenCode`; that reload cancels pending permissions and forms. OpenCode may also watch configuration changes automatically. Installation stops announcers only under verified previous plugin runtime directories, then starts the installed runtime. `-NoStart` suppresses that start.

`pnpm run deploy:plugins` runs checks, builds the same bundle, then invokes this installer. No updater or uninstaller is included.
