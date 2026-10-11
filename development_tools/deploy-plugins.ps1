[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$ClaudeConfigDirectory,
    [string]$OpenCodeConfigDirectory,
    [string]$PiAgentDirectory,
    [string]$InstallDirectory = (Join-Path $env:LOCALAPPDATA 'Programs/herald'),
    [switch]$ReloadOpenCode
)

$ErrorActionPreference = 'Stop'
$Repository = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot/bundle/install.ps1" -ClaudeConfigDirectory $ClaudeConfigDirectory -OpenCodeConfigDirectory $OpenCodeConfigDirectory -PiAgentDirectory $PiAgentDirectory -InstallDirectory $InstallDirectory -ReloadOpenCode:$ReloadOpenCode
if ($MyInvocation.InvocationName -eq '.') { return }
if (-not $PSCmdlet.ShouldProcess($InstallDirectory, 'Test, build a release bundle and install Herald')) { return }
Push-Location $Repository
try {
    Invoke-Checked 'pnpm' @('run', 'check')
    Invoke-Checked 'claude' @('plugin', 'validate', (Join-Path $Repository 'claude-plugin'))
    $zip = & "$PSScriptRoot/build-bundle.ps1"
    $zip = @($zip)[-1]
    Invoke-Checked 'pwsh' @('-NoProfile', '-File', (Join-Path $Repository '.claude/skills/verify-herald/scripts/install-bundle.ps1'), '-Archive', $zip)
    $extract = Join-Path $Repository ('temp/bundles/.herald-extract-' + [guid]::NewGuid())
    try {
        Expand-Archive -LiteralPath $zip -DestinationPath $extract
        & "$extract/install.ps1" -InstallDirectory $InstallDirectory -ClaudeConfigDirectory $ClaudeConfigDirectory -OpenCodeConfigDirectory $OpenCodeConfigDirectory -PiAgentDirectory $PiAgentDirectory -ReloadOpenCode:$ReloadOpenCode
    } finally { Remove-DeploymentDirectory $extract }
} finally { Pop-Location }
