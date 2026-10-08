[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$Bundle = $PSScriptRoot,
    [string]$InstallDirectory = (Join-Path $env:LOCALAPPDATA 'Programs/herald'),
    [string]$ClaudeConfigDirectory,
    [string]$OpenCodeConfigDirectory,
    [string]$ProgramsDirectory = [Environment]::GetFolderPath('Programs'),
    [switch]$SkipHostRegistration,
    [switch]$NoStart,
    [switch]$ReloadOpenCode
)

$ErrorActionPreference = 'Stop'
$PluginId = 'herald@herald-local'
. "$PSScriptRoot/shortcut.ps1"

function Assert-NoLinks {
    param([string]$Path)
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        if (Test-Path -LiteralPath $current) {
            if ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Links are not allowed: $current" }
        }
        $current = Split-Path $current -Parent
    }
}

function Get-PayloadFiles {
    param([string]$Directory)
    Assert-NoLinks $Directory
    $visit = {
        param([IO.DirectoryInfo]$Current)
        foreach ($item in $Current.GetFileSystemInfos()) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Links are not allowed: $($item.FullName)" }
            if ($item -is [IO.DirectoryInfo]) { & $visit $item } else { $item }
        }
    }
    & $visit ([IO.DirectoryInfo]::new([IO.Path]::GetFullPath($Directory)))
}

function Assert-SafePayloadPath {
    param([string]$Path)
    if (-not $Path -or $Path.Contains('\') -or $Path -match '[:\x00-\x1f]' -or $Path.StartsWith('/')) { throw "Unsafe payload path: $Path" }
    foreach ($part in $Path.Split('/')) {
        if (-not $part -or $part -in @('.', '..') -or $part -match '[<>"|?*]' -or $part -match '[. ]$' -or $part -match '^(?i:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)') { throw "Unsafe payload path: $Path" }
    }
}

function Get-PayloadHash {
    param([string]$Path)
    $stream = [IO.File]::OpenRead($Path)
    try { [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant() }
    finally { $stream.Dispose() }
}

function Test-Bundle {
    param([string]$Directory)
    $Directory = [IO.Path]::GetFullPath($Directory)
    $actual = @(Get-PayloadFiles $Directory)
    $inventory = [Collections.Generic.Dictionary[string, IO.FileInfo]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($file in $actual) { $inventory.Add([IO.Path]::GetRelativePath($Directory, $file.FullName).Replace('\', '/'), $file) }
    $manifestPath = Join-Path $Directory 'bundle-manifest.json'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $architecture = switch ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) { 'X64' { 'x64' } 'Arm64' { 'arm64' } default { throw 'Unsupported Windows architecture' } }
    if ($manifest.schemaVersion -ne 1 -or $manifest.name -ne 'herald' -or $manifest.platform -ne 'win32' -or $manifest.arch -ne $architecture -or $manifest.version -notmatch '^\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$') { throw 'Unsupported bundle manifest or architecture' }
    $paths = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($file in $manifest.files) {
        Assert-SafePayloadPath $file.path
        if ($file.path -match '(?i)/bin/(onnxruntime|sherpa-onnx-c-api)\.dll$|/resources/tts/') { throw 'Offline voice must be installed from Settings, not included in the bundle' }
        if ($file.path -eq 'bundle-manifest.json' -or -not $paths.Add($file.path) -or $file.sha256 -notmatch '^[a-f0-9]{64}$' -or $file.size -lt 0) { throw 'Invalid or duplicate manifest entry' }
        $path = Join-Path $Directory $file.path
        if (-not $inventory.ContainsKey($file.path) -or $inventory[$file.path].Length -ne $file.size -or (Get-PayloadHash $path) -ne $file.sha256) { throw "Payload verification failed: $($file.path)" }
    }
    foreach ($required in @('install.ps1', 'register-opencode.mjs', 'package.json', 'opencode-plugin/index.js', 'opencode-plugin/tui.js', 'claude-plugin/.claude-plugin/plugin.json', 'claude-plugin/.claude-plugin/marketplace.json', 'claude-plugin/hooks/hooks.json', 'claude-plugin/hooks/register.ts', "native-announcer/bin/herald-win32-$architecture.exe", "claude-plugin/native-announcer/bin/herald-win32-$architecture.exe")) {
        if (-not $paths.Contains($required)) { throw "Required runtime file missing: $required" }
    }
    foreach ($obsolete in @('claude-plugin/native-announcer/bin/node.exe', 'claude-plugin/native-announcer/licenses/node-LICENSE', 'claude-plugin/scripts/bridge.mjs', 'claude-plugin/scripts/runtime.mjs', 'claude-plugin/scripts/session-title.mjs')) {
        if ($paths.Contains($obsolete) -or (Test-Path -LiteralPath (Join-Path $Directory $obsolete))) { throw "Obsolete Node runtime file is present: $obsolete" }
    }
    if (-not $paths.Contains('shortcut.ps1')) { throw 'Required runtime file missing: shortcut.ps1' }
    foreach ($entry in @('index.ts', 'tui.ts')) { if (-not $paths.Contains($entry)) { throw "Required OpenCode entry missing: $entry" } }
    foreach ($prefix in @('native-announcer', 'claude-plugin/native-announcer')) {
        foreach ($name in @('characters.json')) {
            if (-not $paths.Contains("$prefix/resources/$name")) { throw "Required runtime asset missing: $prefix/resources/$name" }
        }
        $characters = Get-Content -LiteralPath (Join-Path $Directory "$prefix/resources/characters.json") -Raw | ConvertFrom-Json -AsHashtable
        if (-not $characters.Count) { throw 'Character library is empty' }
        foreach ($character in $characters.Values) {
            Assert-SafePayloadPath $character.animationPath
            if (-not $paths.Contains("$prefix/resources/$($character.animationPath)")) { throw 'Character animation is missing' }
        }
    }
    if (-not $paths.Contains('claude-plugin/.claude-plugin/packaged.json')) { throw 'Claude packaged runtime marker is missing' }
    if (-not $paths.Contains('claude-plugin/hooks/hooks.json')) { throw 'Required Claude runtime missing: claude-plugin/hooks/hooks.json' }
    if ($paths.Contains('claude-plugin/.claude-plugin/development.json')) { throw 'Development fallback markers cannot be installed' }
    $package = Get-Content -LiteralPath (Join-Path $Directory 'package.json') -Raw | ConvertFrom-Json -AsHashtable
    $plugin = Get-Content -LiteralPath (Join-Path $Directory 'claude-plugin/.claude-plugin/plugin.json') -Raw | ConvertFrom-Json
    $marketplace = Get-Content -LiteralPath (Join-Path $Directory 'claude-plugin/.claude-plugin/marketplace.json') -Raw | ConvertFrom-Json
    if ($package.name -ne $manifest.name -or $package.version -ne $manifest.version -or $package.exports['.'] -ne './opencode-plugin/index.js' -or $package.exports['./tui'] -ne './opencode-plugin/tui.js' -or $plugin.name -ne $manifest.name -or $plugin.version -ne $manifest.version -or $marketplace.name -ne 'herald-local') { throw 'Runtime package identity does not match the bundle' }
    if ($package.dependencies.Count) { throw 'Plugin runtime dependencies must be compiled into the bundle' }
    if ($actual.Count -ne $paths.Count + 1) { throw 'Bundle contains unlisted files' }
    foreach ($file in $actual) {
        $relative = [IO.Path]::GetRelativePath($Directory, $file.FullName).Replace('\', '/')
        if ($relative -ne 'bundle-manifest.json' -and -not $paths.Contains($relative)) { throw "Unlisted payload: $relative" }
    }
    $manifest
}

function Remove-DeploymentDirectory {
    param([string]$Directory)
    if (-not (Test-Path -LiteralPath $Directory)) { return }
    Assert-NoLinks $Directory
    $remove = {
        param([IO.DirectoryInfo]$Current)
        foreach ($item in $Current.GetFileSystemInfos()) {
            if ($item -is [IO.DirectoryInfo] -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { & $remove $item }
            else { $item.Delete() }
        }
        $Current.Delete()
    }
    & $remove ([IO.DirectoryInfo]::new([IO.Path]::GetFullPath($Directory)))
}

function Install-Payload {
    param([string]$Source, [string]$Root, [switch]$LockHeld)
    $manifest = Test-Bundle $Source
    Assert-NoLinks $Root
    $hash = (Get-FileHash -LiteralPath (Join-Path $Source 'bundle-manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant()
    $destination = Join-Path $Root "versions/$($manifest.version)-$($manifest.arch)-$hash"
    New-Item -ItemType Directory -Path $Root -Force | Out-Null
    Assert-NoLinks (Join-Path $Root '.install.lock')
    $lock = if (-not $LockHeld) { [IO.File]::Open((Join-Path $Root '.install.lock'), 'OpenOrCreate', 'ReadWrite', 'None') }
    $stage = Join-Path $Root ('.herald-stage-' + [guid]::NewGuid())
    $backup = Join-Path $Root ('.herald-backup-' + [guid]::NewGuid())
    try {
        if (Test-Path -LiteralPath $destination) {
            Assert-NoLinks $destination
            $ownership = Join-Path $destination 'bundle-manifest.json'
            if (-not (Test-Path -LiteralPath $ownership) -or (Get-FileHash -LiteralPath $ownership).Hash.ToLowerInvariant() -ne $hash) { throw "Cannot confirm installation ownership: $destination" }
            try { Test-Bundle $destination | Out-Null; return $destination } catch { }
            Get-PayloadFiles $destination | Out-Null
        }
        New-Item -ItemType Directory -Path $stage | Out-Null
        foreach ($file in @($manifest.files.path) + @('bundle-manifest.json')) {
            $target = Join-Path $stage $file
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($target)) | Out-Null
            [IO.File]::Copy((Join-Path $Source $file), $target, $false)
        }
        Test-Bundle $stage | Out-Null
        New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
        Assert-NoLinks (Split-Path $destination -Parent)
        if (Test-Path -LiteralPath $destination) { Move-Item -LiteralPath $destination -Destination $backup }
        try { Move-Item -LiteralPath $stage -Destination $destination } catch {
            if (Test-Path -LiteralPath $backup) { Move-Item -LiteralPath $backup -Destination $destination }
            throw
        }
        Remove-DeploymentDirectory $backup
        $destination
    } finally {
        Remove-DeploymentDirectory $stage
        if ($lock) { $lock.Dispose() }
    }
}

function Invoke-Checked {
    param([string]$Command, [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed with exit code $LASTEXITCODE" }
}

function Test-OpenCodeRuntime {
    param([string]$OpenCodeCommand, [int]$TimeoutMilliseconds = 10000)
    $start = [Diagnostics.ProcessStartInfo]::new((Join-Path $PSHOME 'pwsh.exe'))
    $start.UseShellExecute = $false
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardOutputEncoding = [Text.Encoding]::UTF8
    $start.ArgumentList.Add('-NoProfile')
    $start.ArgumentList.Add('-NonInteractive')
    $start.ArgumentList.Add('-Command')
    $start.ArgumentList.Add('$ErrorActionPreference = "Stop"; & $env:HERALD_OPENCODE --version; exit $LASTEXITCODE')
    $start.Environment['BUN_BE_BUN'] = '1'
    $start.Environment['HERALD_OPENCODE'] = $OpenCodeCommand
    $probe = [Diagnostics.Process]::Start($start)
    try {
        $output = $probe.StandardOutput.ReadToEndAsync()
        $errors = $probe.StandardError.ReadToEndAsync()
        $probe.StandardInput.Close()
        if (-not $probe.WaitForExit($TimeoutMilliseconds)) { $probe.Kill($true); $probe.WaitForExit(); throw 'OpenCode runtime probe exceeded its deadline' }
        if ($probe.ExitCode -ne 0) { throw "OpenCode does not support BUN_BE_BUN=1: $($errors.GetAwaiter().GetResult())" }
        $output.GetAwaiter().GetResult() | Out-Null
    } finally { $probe.Dispose() }
}

function Get-ClaudeInstallations {
    param([string]$ConfigDirectory)
    $registry = Join-Path $ConfigDirectory 'plugins/installed_plugins.json'
    if (-not (Test-Path -LiteralPath $registry)) { return @() }
    $installed = Get-Content -LiteralPath $registry -Raw | ConvertFrom-Json -AsHashtable
    return @($installed.plugins[$PluginId] | Where-Object { $_.scope -eq 'user' })
}

function Enable-ClaudePlugin {
    $plugins = (& claude plugin list --json) | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect enabled Claude plugins' }
    if (@($plugins | Where-Object { $_.id -eq $PluginId -and $_.scope -eq 'user' -and $_.enabled }).Count) { return }
    Invoke-Checked 'claude' @('plugin', 'enable', $PluginId, '--scope', 'user')
}

function Get-ClaudeMarketplaceSource {
    param([string]$ConfigDirectory)
    $registry = Join-Path $ConfigDirectory 'plugins/known_marketplaces.json'
    Assert-NoLinks $registry
    if (-not (Test-Path -LiteralPath $registry)) { return }
    $entry = (Get-Content -LiteralPath $registry -Raw | ConvertFrom-Json -AsHashtable)['herald-local']
    if (-not $entry) { return }
    $source = $entry.source.path
    if (-not $source) { throw 'Cannot confirm ownership of existing Claude marketplace' }
    Assert-NoLinks $source
    foreach ($identity in @(@{ file = 'marketplace.json'; name = 'herald-local' }, @{ file = 'plugin.json'; name = 'herald' })) {
        $manifest = Join-Path $source ".claude-plugin/$($identity.file)"
        Assert-NoLinks $manifest
        if (-not (Test-Path -LiteralPath $manifest) -or (Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json).name -ne $identity.name) { throw 'Cannot confirm ownership of existing Claude marketplace source' }
    }
    [IO.Path]::GetFullPath($source)
}

function Assert-ClaudeInstallation {
    param([string]$InstallPath, [string]$ConfigDirectory)
    $cache = [IO.Path]::GetFullPath((Join-Path $ConfigDirectory 'plugins/cache')) + [IO.Path]::DirectorySeparatorChar
    $destination = [IO.Path]::GetFullPath($InstallPath)
    Assert-NoLinks $destination
    if (-not $destination.StartsWith($cache, [StringComparison]::OrdinalIgnoreCase)) { throw "Refusing to replace a plugin outside the Claude cache: $destination" }
    $manifest = Join-Path $destination '.claude-plugin/plugin.json'
    Assert-NoLinks $manifest
    if (-not (Test-Path -LiteralPath $manifest) -or (Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json).name -ne 'herald') { throw "Cannot confirm plugin ownership: $destination" }
}

function Get-OwnedAnnouncerDirectories {
    param([string]$ConfigDirectory)
    $source = Get-ClaudeMarketplaceSource $ConfigDirectory
    if ($source) {
        Join-Path $source 'native-announcer'
        $parent = Split-Path $source -Parent
        $package = Join-Path $parent 'package.json'
        if ((Test-Path -LiteralPath $package) -and (Get-Content -LiteralPath $package -Raw | ConvertFrom-Json).name -eq 'herald') { Join-Path $parent 'native-announcer' }
    }
    foreach ($installation in @(Get-ClaudeInstallations $ConfigDirectory)) {
        Assert-ClaudeInstallation $installation.installPath $ConfigDirectory
        Join-Path $installation.installPath 'native-announcer'
    }
}

function Stop-OwnedAnnouncers {
    param([string[]]$Directories)
    $prefixes = @($Directories | ForEach-Object { [IO.Path]::GetFullPath($_).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar })
    if (-not $prefixes.Count) { return }
    foreach ($process in Get-CimInstance Win32_Process -Filter "Name LIKE 'herald%.exe'") {
        if (-not $process.ExecutablePath -or -not @($prefixes | Where-Object { $process.ExecutablePath.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) }).Count) { continue }
        if ($process.CommandLine -match '(?:^|[\s"])--settings(?:$|[\s"])') { continue }
        $handle = Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue
        if (-not $handle -or $handle.HasExited -or $handle.Path -ne $process.ExecutablePath) { continue }
        $binary = $handle.Path
        $handle.Kill()
        if (-not $handle.WaitForExit(10000)) { throw "Announcer process $($process.ProcessId) did not stop" }
        $runtime = @($prefixes | Where-Object { $binary.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) })[0]
        @{ binary = $binary; assets = Join-Path $runtime 'resources' }
    }
}

function Deploy-ClaudeFiles {
    param([string]$Source, [string]$InstallPath, [string]$ConfigDirectory, [string]$Announcer, [Collections.Generic.List[object]]$Backups)
    $destination = [IO.Path]::GetFullPath($InstallPath)
    Assert-ClaudeInstallation $destination $ConfigDirectory
    $runtime = if ($Announcer) { $Announcer } else { Join-Path $Source 'native-announcer' }
    Get-PayloadFiles $Source | Out-Null
    Get-PayloadFiles $runtime | Out-Null
    foreach ($required in @('.claude-plugin/plugin.json', 'hooks/hooks.json', 'hooks/register.ts')) {
        if (-not (Test-Path -LiteralPath (Join-Path $Source $required))) { throw "Missing Claude runtime: $required" }
    }
    $parent = Split-Path $destination -Parent
    $stage = Join-Path $parent ('.herald-stage-' + [guid]::NewGuid())
    $backup = Join-Path $parent ('.herald-backup-' + [guid]::NewGuid())
    try {
        New-Item -ItemType Directory -Path $stage | Out-Null
        foreach ($name in @('.claude-plugin', 'hooks')) { Copy-Item -LiteralPath (Join-Path $Source $name) -Destination (Join-Path $stage $name) -Recurse -Force }
        Copy-Item -LiteralPath $runtime -Destination (Join-Path $stage 'native-announcer') -Recurse -Force
        Move-Item -LiteralPath $destination -Destination $backup
        try { Move-Item -LiteralPath $stage -Destination $destination } catch { Move-Item -LiteralPath $backup -Destination $destination; throw }
        if ($null -ne $Backups) { $Backups.Add(@{ destination = $destination; backup = $backup }) }
        else { Remove-DeploymentDirectory $backup }
    } finally { Remove-DeploymentDirectory $stage }
}

function Install-SettingsShortcut {
    param([string]$Binary, [string]$ProgramsDirectory = [Environment]::GetFolderPath('Programs'))
    Assert-NoLinks $ProgramsDirectory
    New-Item -ItemType Directory -Path $ProgramsDirectory -Force | Out-Null
    $path = Join-Path $ProgramsDirectory 'Herald settings.lnk'
    Assert-NoLinks $path
    if (Test-Path -LiteralPath $path) {
        $shortcut = Read-NativeShortcut $path
        if ($shortcut.Arguments -ne '--settings' -or (Split-Path $shortcut.TargetPath -Leaf) -notlike 'herald*.exe') { throw 'An unrelated shortcut already uses the Herald settings name' }
        $appRoot = Split-Path (Split-Path (Split-Path $shortcut.TargetPath -Parent) -Parent) -Parent
        $packagePath = Join-Path $appRoot 'package.json'
        Assert-NoLinks $packagePath
        if (-not (Test-Path -LiteralPath $packagePath) -or (Get-Content -LiteralPath $packagePath -Raw | ConvertFrom-Json).name -ne 'herald') { throw 'Cannot confirm ownership of the existing settings shortcut' }
    }
    $target = [IO.Path]::GetFullPath($Binary)
    Write-NativeShortcut $path $target '--settings' (Split-Path $target -Parent)
}

function Register-OpenCodeBundle {
    param([string]$App, [string[]]$Configs, [scriptblock]$BeforeWrite, [scriptblock]$AfterWrite, [string]$OpenCodeCommand)
    $commandName = if ($OpenCodeCommand) { $OpenCodeCommand } else { 'opencode' }
    $OpenCodeCommand = @(Get-Command $commandName -CommandType Application,ExternalScript -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $OpenCodeCommand) { throw 'OpenCode is required to register the plugin' }
    $locked = [Collections.Generic.List[object]]::new()
    $completed = $false
    try {
        foreach ($config in $Configs) {
            Assert-NoLinks $config
            $exists = Test-Path -LiteralPath $config
            $stream = [IO.File]::Open($config, $(if ($exists) { 'Open' } else { 'CreateNew' }), 'ReadWrite', 'None')
            $entry = @{ config = $config; stream = $stream; bytes = [byte[]]@(); written = $false; existed = $exists }
            $locked.Add($entry)
            if (-not $exists) {
                $initial = [Text.Encoding]::UTF8.GetBytes("{}`n")
                $stream.Write($initial)
                $stream.Flush($true)
                $stream.Position = 0
            }
            $memory = [IO.MemoryStream]::new()
            try { $stream.CopyTo($memory); $entry.bytes = $memory.ToArray() } finally { $memory.Dispose() }
            $reader = [IO.StreamReader]::new([IO.MemoryStream]::new($entry.bytes), [Text.UTF8Encoding]::new($false, $true), $true)
            try { $entry.text = $reader.ReadToEnd(); $entry.encoding = $reader.CurrentEncoding } finally { $reader.Dispose() }
            $preamble = $entry.encoding.GetPreamble()
            $entry.preamble = if ($preamble.Length -and $entry.bytes.Length -ge $preamble.Length -and [Convert]::ToHexString($entry.bytes[0..($preamble.Length - 1)]) -eq [Convert]::ToHexString($preamble)) { $preamble } else { [byte[]]@() }
        }
        $request = @{ installed = $App; documents = @($locked | ForEach-Object { @{ config = $_.config; text = $_.text } }) } | ConvertTo-Json -Depth 100 -Compress
        $start = [Diagnostics.ProcessStartInfo]::new((Join-Path $PSHOME 'pwsh.exe'))
        $start.UseShellExecute = $false
        $start.RedirectStandardInput = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
        $start.StandardOutputEncoding = [Text.Encoding]::UTF8
        $start.ArgumentList.Add('-NoProfile')
        $start.ArgumentList.Add('-NonInteractive')
        $start.ArgumentList.Add('-Command')
        $start.ArgumentList.Add('$ErrorActionPreference = "Stop"; & $env:HERALD_OPENCODE $env:HERALD_RENDERER --render; exit $LASTEXITCODE')
        $start.Environment['BUN_BE_BUN'] = '1'
        $start.Environment['HERALD_OPENCODE'] = $OpenCodeCommand
        $start.Environment['HERALD_RENDERER'] = Join-Path $App 'register-opencode.mjs'
        $node = [Diagnostics.Process]::Start($start)
        try {
            $output = $node.StandardOutput.ReadToEndAsync()
            $errors = $node.StandardError.ReadToEndAsync()
            $node.StandardInput.Write($request)
            $node.StandardInput.Close()
            if (-not $node.WaitForExit(30000)) { $node.Kill($true); $node.WaitForExit(); throw 'OpenCode registration renderer exceeded its deadline' }
            if ($node.ExitCode -ne 0) { throw "OpenCode registration rejected: $($errors.GetAwaiter().GetResult())" }
            $updates = @($output.GetAwaiter().GetResult() | ConvertFrom-Json)
        } finally { $node.Dispose() }
        if ($updates.Count -ne $locked.Count) { throw 'Invalid registration renderer response' }
        if ($BeforeWrite) { & $BeforeWrite }
        for ($index = 0; $index -lt $locked.Count; $index++) {
            $entry = $locked[$index]
            $update = $updates[$index]
            if ($update.config -ne $entry.config -or $update.text -cne $entry.text -or $null -eq $update.result) { throw 'Invalid registration renderer document' }
            if ($update.result -ceq $entry.text) { continue }
            $bytes = [byte[]]($entry.preamble + $entry.encoding.GetBytes($update.result))
            $entry.written = $true
            $entry.stream.Position = 0
            $entry.stream.Write($bytes)
            $entry.stream.SetLength($bytes.Length)
            $entry.stream.Flush($true)
        }
        if ($AfterWrite) { & $AfterWrite }
        $completed = $true
    } catch {
        foreach ($entry in $locked) {
            if (-not $entry.written) { continue }
            try {
                $entry.stream.Position = 0
                $entry.stream.Write($entry.bytes)
                $entry.stream.SetLength($entry.bytes.Length)
                $entry.stream.Flush($true)
            } catch { Write-Warning "Could not restore OpenCode configuration: $_" }
        }
        throw
    } finally {
        foreach ($entry in $locked) {
            $entry.stream.Dispose()
            if (-not $completed -and -not $entry.existed) { Remove-Item -LiteralPath $entry.config -Force }
        }
    }
}

function Register-ClaudeBundle {
    param([string]$App, [string]$ConfigDirectory, [Collections.Generic.List[object]]$Backups)
    Assert-NoLinks $ConfigDirectory
    $source = Join-Path $App 'claude-plugin'
    $oldSource = Get-ClaudeMarketplaceSource $ConfigDirectory
    if ($oldSource -and $oldSource -eq [IO.Path]::GetFullPath($source)) { $source = $null }
    if ($source) { Invoke-Checked 'claude' @('plugin', 'marketplace', 'add', $source, '--scope', 'user') }
    if (-not @(Get-ClaudeInstallations $ConfigDirectory).Count) { Invoke-Checked 'claude' @('plugin', 'install', $PluginId, '--scope', 'user') }
    $installed = @(Get-ClaudeInstallations $ConfigDirectory)
    if (-not $installed.Count) { throw 'Claude did not report a user installation' }
    foreach ($installation in $installed) { Deploy-ClaudeFiles (Join-Path $App 'claude-plugin') $installation.installPath $ConfigDirectory -Backups $Backups }
    Enable-ClaudePlugin
}

function Register-BundleHosts {
    param([string]$App, [string[]]$Configs, [string]$ClaudeConfigDirectory, [string]$Binary, [string]$ProgramsDirectory, [scriptblock]$BeforeRegistration, [scriptblock]$AfterRegistration, [string]$OpenCodeCommand)
    $files = @('settings.json', 'plugins/known_marketplaces.json', 'plugins/installed_plugins.json') | ForEach-Object { Join-Path $ClaudeConfigDirectory $_ }
    $files += Join-Path $ProgramsDirectory 'Herald settings.lnk'
    $snapshots = @($files | ForEach-Object {
        Assert-NoLinks $_
        @{ path = $_; bytes = if (Test-Path -LiteralPath $_) { [IO.File]::ReadAllBytes($_) } else { $null } }
    })
    $previousCaches = @(Get-ClaudeInstallations $ClaudeConfigDirectory | ForEach-Object { [IO.Path]::GetFullPath($_.installPath) })
    $backups = [Collections.Generic.List[object]]::new()
    try {
        Register-OpenCodeBundle $App $Configs {
            if ($BeforeRegistration) { & $BeforeRegistration }
            Register-ClaudeBundle $App $ClaudeConfigDirectory -Backups $backups
        } {
            Install-SettingsShortcut $Binary $ProgramsDirectory
            if ($AfterRegistration) { & $AfterRegistration }
        } -OpenCodeCommand $OpenCodeCommand
    } catch {
        $failure = $_
        $newCaches = @(Get-ClaudeInstallations $ClaudeConfigDirectory | Where-Object { [IO.Path]::GetFullPath($_.installPath) -notin $previousCaches })
        foreach ($installation in $newCaches) {
            Assert-ClaudeInstallation $installation.installPath $ClaudeConfigDirectory
            Remove-DeploymentDirectory $installation.installPath
        }
        for ($index = $backups.Count - 1; $index -ge 0; $index--) {
            $entry = $backups[$index]
            if ($entry.destination -in $previousCaches) {
                Assert-ClaudeInstallation $entry.destination $ClaudeConfigDirectory
                Remove-DeploymentDirectory $entry.destination
                Move-Item -LiteralPath $entry.backup -Destination $entry.destination
            } else { Remove-DeploymentDirectory $entry.backup }
        }
        foreach ($snapshot in $snapshots) {
            if ($null -eq $snapshot.bytes) {
                if (Test-Path -LiteralPath $snapshot.path) { Remove-Item -LiteralPath $snapshot.path -Force }
            } else { [IO.File]::WriteAllBytes($snapshot.path, $snapshot.bytes) }
        }
        throw $failure
    }
    foreach ($entry in $backups) { Remove-DeploymentDirectory $entry.backup }
}

if ($MyInvocation.InvocationName -eq '.') { return }
if (-not $IsWindows -or $PSVersionTable.PSVersion.Major -lt 7) { throw 'PowerShell 7 on Windows is required' }
Test-Bundle $Bundle | Out-Null
if (-not $PSCmdlet.ShouldProcess($InstallDirectory, 'Install the verified Herald bundle and selected host registrations')) { return }
Assert-NoLinks $InstallDirectory
New-Item -ItemType Directory -Path $InstallDirectory -Force | Out-Null
Assert-NoLinks (Join-Path $InstallDirectory '.install.lock')
$installationLock = [IO.File]::Open((Join-Path $InstallDirectory '.install.lock'), 'OpenOrCreate', 'ReadWrite', 'None')
try {
$app = Install-Payload $Bundle $InstallDirectory -LockHeld
$manifest = Test-Bundle $app
$binary = Join-Path $app "native-announcer/bin/herald-win32-$($manifest.arch).exe"
if (-not $SkipHostRegistration) {
    if (-not $ClaudeConfigDirectory) { $ClaudeConfigDirectory = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } else { Join-Path $HOME '.claude' } }
    if (-not $OpenCodeConfigDirectory) { $OpenCodeConfigDirectory = if ($env:XDG_CONFIG_HOME) { Join-Path $env:XDG_CONFIG_HOME 'opencode' } else { Join-Path $HOME '.config/opencode' } }
    Get-Command claude -ErrorAction Stop | Out-Null
    $openCodeCommand = @(Get-Command opencode -CommandType Application,ExternalScript -ErrorAction Stop | Select-Object -First 1).Source
    if (-not $openCodeCommand) { throw 'OpenCode is required to register the plugin' }
    Test-OpenCodeRuntime $openCodeCommand
    Assert-NoLinks $OpenCodeConfigDirectory
    New-Item -ItemType Directory -Path $OpenCodeConfigDirectory -Force | Out-Null
    $configs = @(@('opencode.json', 'opencode.jsonc') | ForEach-Object { Join-Path $OpenCodeConfigDirectory $_ } | Where-Object { Test-Path -LiteralPath $_ })
    if (-not $configs.Count) { $configs = @(Join-Path $OpenCodeConfigDirectory 'opencode.jsonc') }
    foreach ($config in $configs) { Assert-NoLinks $config }
    $oldConfig = $env:CLAUDE_CONFIG_DIR
    $stopped = @()
    try {
        $env:CLAUDE_CONFIG_DIR = $ClaudeConfigDirectory
        Register-BundleHosts $app $configs $ClaudeConfigDirectory $binary $ProgramsDirectory {
            $script:stopped = @(Stop-OwnedAnnouncers @(Get-OwnedAnnouncerDirectories $ClaudeConfigDirectory))
        } {
            if ($ReloadOpenCode) { Invoke-Checked 'opencode' @('api', 'post', '/api/location/reload') }
            else { Write-Warning 'No explicit OpenCode reload was requested. OpenCode may automatically watch configuration changes. Restart Claude sessions to load the installed hooks.' }
        } -OpenCodeCommand $openCodeCommand
    } catch {
        foreach ($previous in $stopped) {
            if (Test-Path -LiteralPath $previous.binary) {
                try { Start-Process -FilePath $previous.binary -ArgumentList @('--assets', ('"' + $previous.assets + '"')) -WindowStyle Hidden | Out-Null }
                catch { Write-Warning "Could not restore announcer playback: $_" }
            }
        }
        throw
    } finally { $env:CLAUDE_CONFIG_DIR = $oldConfig }
}
if (-not $NoStart) { Start-Process -FilePath $binary -ArgumentList @('--assets', ('"' + (Join-Path $app 'native-announcer/resources') + '"')) -WindowStyle Hidden | Out-Null }
Write-Output $app
} finally { $installationLock.Dispose() }
