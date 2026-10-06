param(
    [ValidateSet('OpenCode', 'Claude')][string]$HostName = 'OpenCode',
    [ValidateSet('Background', 'Subagent', 'CancelRestart')][string]$Scenario = 'Background',
    [string]$Model,
    [string]$Evidence = ('temp/verification/hosts-' + [guid]::NewGuid())
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../../..'))
$evidencePath = [IO.Path]::GetFullPath((Join-Path $root $Evidence))
$scratch = Join-Path $env:LOCALAPPDATA ('Temp/opencode/civilized-host-' + [guid]::NewGuid())
$binary = Join-Path $root 'native-announcer/target/debug/civilized-announcer.exe'
if (-not (Test-Path -LiteralPath $binary)) { throw 'Build the debug announcer before host verification' }
if ($HostName -eq 'OpenCode' -and (-not $Model -or -not $Model.Contains('/'))) { throw 'Specify -Model provider/model for OpenCode; this test makes real model requests' }
if (Test-Path -LiteralPath $evidencePath) { throw 'Use a new evidence directory' }
New-Item -ItemType Directory -Path $evidencePath | Out-Null
$native = $null
$driver = $null
$oldData = $env:CIVILIZED_AGENT_DATA
$transcribing = $false
try {
    Start-Transcript -Path (Join-Path $evidencePath 'actions.txt') | Out-Null
    $transcribing = $true
    New-Item -ItemType Directory -Path "$scratch/data", "$scratch/config/opencode", "$scratch/state", "$scratch/claude-profile" -Force | Out-Null
    $env:CIVILIZED_AGENT_DATA = "$scratch/data"
    @{ quietMode = $true; scheduleEnabled = $false } | ConvertTo-Json | Set-Content "$scratch/data/settings.json" -Encoding utf8NoBOM
    $proof = Join-Path $evidencePath 'host-proof.jsonl'
    $marker = 'civilized-context-' + [guid]::NewGuid()
    if ($HostName -eq 'OpenCode') {
        @{
            '$schema' = 'https://opencode.ai/config.json'
            snapshots = $false
            plugins = @(@{ package = $root; options = @{ minimumSeconds = 0 } }, @{ package = (Join-Path $root 'development_tools/host-verification/opencode') })
            permissions = @(@{ action = '*'; resource = '*'; effect = 'deny' }, @{ action = 'shell'; resource = 'pwsh *'; effect = 'allow' }, @{ action = 'subagent'; resource = '*'; effect = 'allow' })
        } | ConvertTo-Json -Depth 6 | Set-Content "$scratch/config/opencode/opencode.json" -Encoding utf8NoBOM
    } else {
        $profile = if ($env:CLAUDE_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR } elseif (Test-Path "$HOME/.claude-whg/.credentials.json") { "$HOME/.claude-whg" } else { "$HOME/.claude" }
        if (Test-Path "$profile/.credentials.json") { Copy-Item -LiteralPath "$profile/.credentials.json" -Destination "$scratch/claude-profile/.credentials.json" }
    }
    $launch = [Diagnostics.ProcessStartInfo]::new($binary)
    $launch.UseShellExecute = $false
    $launch.Environment['CIVILIZED_AGENT_DATA'] = "$scratch/data"
    $launch.Environment['CIVILIZED_AGENT_TTS'] = Join-Path $root 'native-announcer/resources/tts/kitten-nano-en-v0_8-int8'
    foreach ($argument in @('--isolated', '--assets', (Join-Path $root 'native-announcer/resources'), '--test-seconds', '360', '--report', (Join-Path $evidencePath 'report.json'), '--snapshot', (Join-Path $evidencePath 'render.png'))) { $launch.ArgumentList.Add($argument) }
    $native = [Diagnostics.Process]::Start($launch)
    @{ pid = $native.Id; started = $native.StartTime.ToUniversalTime().ToString('o'); binary = $binary; sha256 = (Get-FileHash $binary).Hash; data = "$scratch/data"; host = $HostName; scenario = $Scenario } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'instance.json')
    $run = [Diagnostics.ProcessStartInfo]::new((Get-Command bun).Source)
    $run.UseShellExecute = $false
    $run.WorkingDirectory = $root
    $run.Environment['CIVILIZED_AGENT_DATA'] = "$scratch/data"
    $run.Environment['CIVILIZED_AGENT_BINARY'] = $binary
    $run.Environment['CIVILIZED_AGENT_EXTERNAL_COMPANION'] = '1'
    $run.Environment['CIVILIZED_AGENT_HOST_PROOF'] = $proof
    $run.Environment['CIVILIZED_AGENT_HOST_MARKER'] = $marker
    $run.Environment['XDG_STATE_HOME'] = "$scratch/state"
    $run.Environment['XDG_CONFIG_HOME'] = "$scratch/config"
    $run.Environment['CLAUDE_CONFIG_DIR'] = "$scratch/claude-profile"
    $run.Environment.Remove('CLAUDECODE') | Out-Null
    foreach ($argument in @((Join-Path $root 'development_tools/host-verification/run.ts'), $HostName, $Scenario, $Model, $scratch, $evidencePath, $root, (Get-Command claude).Source)) { $run.ArgumentList.Add([string]$argument) }
    $driver = [Diagnostics.Process]::Start($run)
    $started = [DateTime]::UtcNow
    while (-not $driver.WaitForExit(1000)) {
        if ($native.HasExited) { throw 'Isolated companion exited before the host test finished' }
        $actions = Join-Path $evidencePath 'actions.jsonl'
        if ($HostName -eq 'OpenCode' -and (Test-Path $actions)) {
            $created = Get-Content $actions | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object action -EQ 'created' | Select-Object -First 1
            if ($created) {
                @{ type = 'presence'; clientID = 'host-verification'; sessionIDs = @($created.sessionID); at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() } | ConvertTo-Json -Compress | & node (Join-Path $root 'claude-plugin/scripts/bridge.mjs')
                if ($LASTEXITCODE -ne 0) { throw 'Host verification presence bridge failed' }
            }
        }
        if (([DateTime]::UtcNow - $started).TotalSeconds -gt 300) { throw 'Host verification exceeded five minutes' }
    }
    if ($driver.ExitCode -ne 0) { throw "Host verification exited $($driver.ExitCode)" }
    $result = Get-Content (Join-Path $evidencePath 'host-result.json') -Raw | ConvertFrom-Json
    if (-not $result.passed -or -not $result.sessionsRemoved) { throw 'Host verification or session cleanup failed' }
    Write-Output "PASS: $HostName $Scenario; real host evidence: $evidencePath"
} catch {
    $_ | Out-String | Set-Content (Join-Path $evidencePath 'failure.txt')
    throw
} finally {
    if ($driver -and -not $driver.HasExited) { & taskkill /PID $driver.Id /T /F | Out-Null; $driver.WaitForExit(10000) | Out-Null }
    if ($native -and -not $native.HasExited) { $native.Kill(); $native.WaitForExit() }
    $registration = "$scratch/state/opencode/service.json"
    if (Test-Path -LiteralPath $registration) {
        & bun (Join-Path $root 'development_tools/host-verification/cleanup.ts') $scratch
        if ($LASTEXITCODE -ne 0) { Write-Warning 'Private host server cleanup failed; scratch retained' }
    }
    foreach ($name in @('history.jsonl', 'queue.json', 'errors.log')) {
        if (Test-Path "$scratch/data/$name") { Copy-Item -LiteralPath "$scratch/data/$name" -Destination (Join-Path $evidencePath $name) }
    }
    $serverExited = -not (Test-Path -LiteralPath $registration)
    if ($serverExited -and (Test-Path -LiteralPath $scratch)) { Remove-Item -LiteralPath $scratch -Recurse -Force }
    $env:CIVILIZED_AGENT_DATA = $oldData
    @{ scratchRemoved = -not (Test-Path -LiteralPath $scratch); processExited = (-not $native -or $native.HasExited) -and (-not $driver -or $driver.HasExited); serverExited = $serverExited } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'cleanup.json')
    if ($transcribing) { Stop-Transcript | Out-Null }
}
