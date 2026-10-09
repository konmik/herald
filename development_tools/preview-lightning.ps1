param(
    [Parameter(Mandatory)][ValidateSet('original', 'storm', 'electric', 'plasma')][string]$Style,
    [string]$Evidence = ('temp/verification/lightning-' + $Style + '-' + [guid]::NewGuid())
)

$ErrorActionPreference = 'Stop'
$previous = $env:HERALD_LIGHTNING_STYLE
try {
    $env:HERALD_LIGHTNING_STYLE = $Style.ToLowerInvariant()
    & (Join-Path $PSScriptRoot '../.claude/skills/verify-herald/scripts/announce.ps1') -Evidence $Evidence -CaptureFrames
} finally {
    $env:HERALD_LIGHTNING_STYLE = $previous
}
