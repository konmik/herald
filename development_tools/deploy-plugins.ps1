[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$ClaudeConfigDirectory,
    [switch]$ReloadOpenCode
)

$ErrorActionPreference = 'Stop'
$Repository = Split-Path $PSScriptRoot -Parent
$PluginId = 'civilized-agent@civilized-agent-local'

function Invoke-Checked {
    param([string]$Command, [string[]]$Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed with exit code $LASTEXITCODE" }
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

function Remove-DeploymentDirectory {
    param([string]$Directory)
    $shared = Join-Path $Directory 'native-announcer'
    if (Test-Path -LiteralPath $shared) {
        $item = Get-Item -LiteralPath $shared
        if ($item.LinkType) { Remove-Item -LiteralPath $shared -Force }
    }
    if (Test-Path -LiteralPath $Directory) { Remove-Item -LiteralPath $Directory -Recurse -Force }
}

function Deploy-ClaudeFiles {
    param([string]$Source, [string]$InstallPath, [string]$ConfigDirectory, [string]$Announcer)
    $cache = [IO.Path]::GetFullPath((Join-Path $ConfigDirectory 'plugins/cache')) + [IO.Path]::DirectorySeparatorChar
    $destination = [IO.Path]::GetFullPath($InstallPath)
    if (-not $destination.StartsWith($cache, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to replace a plugin outside the Claude cache: $destination"
    }
    $manifest = Join-Path $destination '.claude-plugin/plugin.json'
    if (-not (Test-Path -LiteralPath $manifest) -or (Get-Content -LiteralPath $manifest -Raw | ConvertFrom-Json).name -ne 'civilized-agent') {
        throw "Cannot confirm ownership of the installed plugin: $destination"
    }
    $parent = Split-Path $destination -Parent
    $stage = Join-Path $parent ('.civilized-stage-' + [guid]::NewGuid())
    $backup = Join-Path $parent ('.civilized-backup-' + [guid]::NewGuid())
    $moved = $false
    try {
        New-Item -ItemType Directory -Path (Join-Path $stage '.claude-plugin') -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $Source '.claude-plugin/plugin.json') -Destination (Join-Path $stage '.claude-plugin/plugin.json')
        foreach ($directory in @('hooks', 'scripts')) {
            Copy-Item -LiteralPath (Join-Path $Source $directory) -Destination (Join-Path $stage $directory) -Recurse
        }
        New-Item -ItemType Junction -Path (Join-Path $stage 'native-announcer') -Target $Announcer | Out-Null
        Move-Item -LiteralPath $destination -Destination $backup
        $moved = $true
        Move-Item -LiteralPath $stage -Destination $destination
    } catch {
        if ($moved -and -not (Test-Path -LiteralPath $destination)) {
            Move-Item -LiteralPath $backup -Destination $destination
        }
        throw
    } finally {
        Remove-DeploymentDirectory $stage
    }
    Remove-DeploymentDirectory $backup
    Write-Output "Claude plugin deployed: $destination"
}

function Stop-OwnedAnnouncers {
    param([string[]]$Directories)
    $prefixes = @($Directories | ForEach-Object { [IO.Path]::GetFullPath($_).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar })
    foreach ($process in Get-CimInstance Win32_Process -Filter "Name LIKE 'civilized-announcer%.exe'") {
        if (-not $process.ExecutablePath) { continue }
        $owned = @($prefixes | Where-Object { $process.ExecutablePath.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0
        if ($owned) {
            $handle = Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue
            if ($handle) {
                Stop-Process -Id $process.ProcessId -Force
                if (-not $handle.WaitForExit(10000)) { throw "Announcer process $($process.ProcessId) did not stop" }
            }
        }
    }
}

if ($MyInvocation.InvocationName -eq '.') { return }
if (-not $IsWindows -or $PSVersionTable.PSVersion.Major -lt 7) { throw 'Run this deployment script with PowerShell 7 on Windows' }
if (-not $ClaudeConfigDirectory) {
    $ClaudeConfigDirectory = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR }
        elseif (Test-Path -LiteralPath (Join-Path $HOME '.claude-whg')) { Join-Path $HOME '.claude-whg' }
        else { Join-Path $HOME '.claude' }
}
$ClaudeConfigDirectory = [IO.Path]::GetFullPath($ClaudeConfigDirectory)
if (-not $PSCmdlet.ShouldProcess("Claude ($ClaudeConfigDirectory) and OpenCode", 'Test, rebuild and deploy Civilized Agent')) { return }
foreach ($command in @('node', 'bun', 'cargo', 'claude', 'opencode')) { Get-Command $command -ErrorAction Stop | Out-Null }

$oldClaudeConfig = $env:CLAUDE_CONFIG_DIR
$oldCargoTarget = $env:CARGO_TARGET_DIR
$env:CLAUDE_CONFIG_DIR = $ClaudeConfigDirectory
$env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'Temp/opencode/civilized-announcer-build'
$announcer = Join-Path $Repository 'native-announcer'
$source = Join-Path $Repository 'claude-plugin'
$header = 'x-opencode-directory:' + $Repository
$restartAnnouncer = $false
$binary = $null
Push-Location $Repository
try {
    $plugins = (& opencode api get /api/plugin -H $header) | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect OpenCode plugins' }
    $plugin = @($plugins.data | Where-Object { $_.id -eq 'civilized-agent' })
    if ($plugin.Count -ne 1 -or $plugin[0].source.type -ne 'local' -or -not ([IO.Path]::GetFullPath($plugin[0].source.path)).StartsWith($Repository + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'OpenCode must already be configured to load Civilized Agent from this repository'
    }
    Invoke-Checked 'bun' @('test', './opencode-plugin/tests')
    Invoke-Checked 'claude' @('plugin', 'test', $source)
    Invoke-Checked 'claude' @('plugin', 'validate', $source)
    Invoke-Checked 'cargo' @('test', '--locked', '-j', '6', '--manifest-path', (Join-Path $announcer 'Cargo.toml'))
    $architecture = & node -p 'process.arch'
    if ($LASTEXITCODE -ne 0) { throw 'Could not determine the announcer architecture' }
    $binary = Join-Path $announcer ("bin/civilized-announcer-win32-$architecture.exe")
    $installed = @(Get-ClaudeInstallations $ClaudeConfigDirectory)
    Stop-OwnedAnnouncers (@($Repository) + @($installed | ForEach-Object { $_.installPath }))
    $restartAnnouncer = $true
    Invoke-Checked 'node' @((Join-Path $PSScriptRoot 'build-announcer.mjs'), '--release')
    if (-not $installed.Count) {
        Invoke-Checked 'claude' @('plugin', 'marketplace', 'add', $source, '--scope', 'user')
        Invoke-Checked 'claude' @('plugin', 'install', $PluginId, '--scope', 'user')
    }
    $installed = @(Get-ClaudeInstallations $ClaudeConfigDirectory)
    if (-not $installed.Count) { throw 'Claude did not report a user installation for Civilized Agent' }
    foreach ($installation in $installed) {
        Deploy-ClaudeFiles $source $installation.installPath $ClaudeConfigDirectory $announcer
    }
    Enable-ClaudePlugin
    if (-not (Test-Path -LiteralPath $binary)) { throw 'Built announcer binary is missing' }
    Start-Process -FilePath $binary -ArgumentList @('--assets', ('"' + (Join-Path $announcer 'resources') + '"')) -WindowStyle Hidden | Out-Null
    $restartAnnouncer = $false
    Write-Output 'OpenCode plugin deployed through its existing repository registration.'
    if ($ReloadOpenCode) {
        Invoke-Checked 'opencode' @('api', 'post', '/api/location/reload')
        Write-Output 'OpenCode locations reloaded.'
    } else {
        Write-Output 'OpenCode was not reloaded. Use -ReloadOpenCode to apply code changes to loaded locations; this cancels pending permissions and forms.'
    }
    Write-Output 'Restart Claude sessions to load the deployed hooks.'
} finally {
    if ($restartAnnouncer -and $binary -and (Test-Path -LiteralPath $binary)) {
        try {
            Start-Process -FilePath $binary -ArgumentList @('--assets', ('"' + (Join-Path $announcer 'resources') + '"')) -WindowStyle Hidden | Out-Null
        } catch {
            Write-Warning "Could not restore announcer playback: $_"
        }
    }
    Pop-Location
    $env:CLAUDE_CONFIG_DIR = $oldClaudeConfig
    $env:CARGO_TARGET_DIR = $oldCargoTarget
}
