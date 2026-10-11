[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$ClaudeConfigDirectory,
    [string]$OpenCodeConfigDirectory,
    [string]$InstallDirectory = (Join-Path $env:LOCALAPPDATA 'Programs/herald'),
    [switch]$ReloadOpenCode
)

$ErrorActionPreference = 'Stop'
$Repository = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot/bundle/install.ps1" -ClaudeConfigDirectory $ClaudeConfigDirectory -OpenCodeConfigDirectory $OpenCodeConfigDirectory -InstallDirectory $InstallDirectory -ReloadOpenCode:$ReloadOpenCode
if ($MyInvocation.InvocationName -eq '.') { return }
if (-not $PSCmdlet.ShouldProcess($InstallDirectory, 'Test, build a release bundle and install Herald')) { return }
Push-Location $Repository
try {
    Invoke-Checked 'npm' @('run', 'check')
    Invoke-Checked 'claude' @('plugin', 'validate', (Join-Path $Repository 'claude-plugin'))
    $zip = & "$PSScriptRoot/build-bundle.ps1"
    $zip = @($zip)[-1]
    Invoke-Checked 'pwsh' @('-NoProfile', '-File', (Join-Path $Repository '.claude/skills/verify-herald/scripts/install-bundle.ps1'), '-Archive', $zip)
    $extract = Join-Path $Repository ('temp/bundles/.herald-extract-' + [guid]::NewGuid())
    try {
        Expand-Archive -LiteralPath $zip -DestinationPath $extract
        & "$extract/install.ps1" -InstallDirectory $InstallDirectory -ClaudeConfigDirectory $ClaudeConfigDirectory -OpenCodeConfigDirectory $OpenCodeConfigDirectory -ReloadOpenCode:$ReloadOpenCode
    } finally { Remove-DeploymentDirectory $extract }
} finally { Pop-Location }
