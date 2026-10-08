param(
    [Parameter(Mandatory)][string]$AppDirectory,
    [string]$Evidence = ('temp/verification/bundle-' + [guid]::NewGuid()),
    [switch]$Hosts,
    [string]$TestExecutable,
    [string]$ClaudePluginDirectory,
    [string]$Model,
    [string]$ClaudeModel
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../../..'))
$app = [IO.Path]::GetFullPath($AppDirectory)
$evidencePath = [IO.Path]::GetFullPath($Evidence, $root)
if (-not (Test-Path -LiteralPath $app -PathType Container)) { throw 'Specify the installed bundle directory' }
if (Test-Path -LiteralPath $evidencePath) { throw 'Use a new evidence directory' }
New-Item -ItemType Directory -Path $evidencePath | Out-Null
$overrides = @('HERALD_BINARY', 'HERALD_TTS', 'HERALD_EXTERNAL_COMPANION')
$previous = @{}
$results = [Collections.Generic.List[object]]::new()
try {
    foreach ($name in $overrides) {
        $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        [Environment]::SetEnvironmentVariable($name, $null, 'Process')
    }
    foreach ($runtime in @('OpenCode', 'Claude')) {
        foreach ($feature in @('Settings', 'Quiet', 'Output', 'Preview')) {
            $proof = Join-Path $evidencePath "$runtime-$feature"
            & pwsh -NoProfile -File (Join-Path $PSScriptRoot 'verify.ps1') -AppDirectory $app -ClaudePluginDirectory $ClaudePluginDirectory -Runtime $runtime -Feature $feature -Evidence $proof
            if ($LASTEXITCODE -ne 0) { throw "$runtime $feature verification failed" }
            $cleanup = Get-Content -LiteralPath (Join-Path $proof 'cleanup.json') -Raw | ConvertFrom-Json
            if (-not $cleanup.scratchRemoved -or -not $cleanup.processExited) { throw "$runtime $feature cleanup failed" }
            $results.Add(@{ runtime = $runtime; entry = $feature; passed = $true; evidence = $proof })
        }
        $proof = Join-Path $evidencePath "$runtime-announcement"
        & pwsh -NoProfile -File (Join-Path $PSScriptRoot 'announce.ps1') -AppDirectory $app -ClaudePluginDirectory $ClaudePluginDirectory -Runtime $runtime -Evidence $proof
        if ($LASTEXITCODE -ne 0) { throw "$runtime announcement verification failed" }
        $cleanup = Get-Content -LiteralPath (Join-Path $proof 'cleanup.json') -Raw | ConvertFrom-Json
        if (-not $cleanup.scratchRemoved -or -not $cleanup.processExited) { throw "$runtime announcement cleanup failed" }
        $results.Add(@{ runtime = $runtime; entry = 'silent announcement transport'; passed = $true; evidence = $proof })
        if ($TestExecutable) {
            $proof = Join-Path $evidencePath "$runtime-speech-assets"
            & pwsh -NoProfile -File (Join-Path $PSScriptRoot 'speech-assets.ps1') -AppDirectory $app -ClaudePluginDirectory $ClaudePluginDirectory -Runtime $runtime -TestExecutable $TestExecutable -Evidence $proof
            if ($LASTEXITCODE -ne 0) { throw "$runtime silent speech asset verification failed" }
            $cleanup = Get-Content -LiteralPath (Join-Path $proof 'cleanup.json') -Raw | ConvertFrom-Json
            if (-not $cleanup.scratchRemoved -or -not $cleanup.processExited) { throw "$runtime speech asset cleanup failed" }
            $results.Add(@{ runtime = $runtime; entry = 'silent CPU inference via Rust test harness with installed DLLs and model'; passed = $true; evidence = $proof })
        }
        if ($Hosts) {
            $hostModel = if ($runtime -eq 'Claude') { $ClaudeModel } else { $Model }
            foreach ($scenario in @('Background', 'Subagent', 'CancelRestart')) {
                $proof = Join-Path $evidencePath "$runtime-host-$scenario"
                & pwsh -NoProfile -File (Join-Path $PSScriptRoot 'hosts.ps1') -AppDirectory $app -ClaudePluginDirectory $ClaudePluginDirectory -HostName $runtime -Scenario $scenario -Model $hostModel -Evidence $proof
                if ($LASTEXITCODE -ne 0) { throw "$runtime $scenario host verification failed" }
                $cleanup = Get-Content -LiteralPath (Join-Path $proof 'cleanup.json') -Raw | ConvertFrom-Json
                if (-not $cleanup.scratchRemoved -or -not $cleanup.processExited -or -not $cleanup.serverExited) { throw "$runtime $scenario host cleanup failed" }
                $results.Add(@{ runtime = $runtime; entry = "real host $scenario"; passed = $true; evidence = $proof })
            }
        }
    }
    Write-Output "PASS: installed runtime verification. Evidence saved to $evidencePath"
} catch {
    $_ | Out-String | Set-Content -LiteralPath (Join-Path $evidencePath 'failure.txt')
    throw
} finally {
    foreach ($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
    @{ app = $app; results = $results.ToArray(); speechAssetsViaTestHarness = [bool]$TestExecutable; audibleSpeech = 'not tested'; realHosts = [bool]$Hosts } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $evidencePath 'results.json')
}
