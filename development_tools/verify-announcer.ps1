param([switch]$Speech, [switch]$Meeting)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $root 'claude-plugin/bin/civilized-announcer-win32-x64.exe'
$temporary = Join-Path $env:LOCALAPPDATA "Temp/opencode/civilized-native-$([guid]::NewGuid())"
$data = Join-Path $temporary 'data'
$inbox = Join-Path $data 'inbox'
New-Item -ItemType Directory -Path $inbox -Force | Out-Null
$reportPath = Join-Path $temporary 'report.json'
$settings = if ($Speech -or $Meeting) { @{ nightStart = 22; nightEnd = 22 } } else { @{ nightStart = 0; nightEnd = 24 } }
$settings | ConvertTo-Json | Set-Content (Join-Path $data 'settings.json') -Encoding utf8NoBOM
if ($Meeting) {
    @{ active = $true; updated = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000 } | ConvertTo-Json | Set-Content (Join-Path $data 'meeting.json') -Encoding utf8NoBOM
}
$messages = @(
    @{ type = 'notify'; id = 'native-claude'; sessionID = 'verify-claude'; completed = 1; text = 'The native notification test passed.'; title = 'Claude native notification'; character = 'claude'; emotion = 'neutral' },
    @{ type = 'notify'; id = 'native-opencode'; sessionID = 'verify-opencode'; completed = 1; text = ('word ' * 30).Trim(); title = 'OpenCode native notification'; character = 'opencode'; emotion = 'neutral' }
)
for ($i = 0; $i -lt $messages.Count; $i++) {
    $messages[$i] | ConvertTo-Json | Set-Content (Join-Path $inbox "$i.json") -Encoding utf8NoBOM
}
$info = [System.Diagnostics.ProcessStartInfo]::new($binary)
$info.UseShellExecute = $false
$info.Environment['CIVILIZED_AGENT_DATA'] = $data
$info.ArgumentList.Add('--assets')
$info.ArgumentList.Add((Join-Path $root 'claude-plugin/resources'))
$info.ArgumentList.Add('--test-seconds')
$info.ArgumentList.Add('40')
$info.ArgumentList.Add('--report')
$info.ArgumentList.Add($reportPath)
$info.ArgumentList.Add('--snapshot')
$info.ArgumentList.Add((Join-Path $temporary 'preview.png'))
$process = [System.Diagnostics.Process]::Start($info)
$peak = 0
while (-not $process.WaitForExit(50)) {
    $process.Refresh()
    $peak = [Math]::Max($peak, $process.PeakWorkingSet64)
}
if ($process.ExitCode -ne 0) { throw "Announcer failed with exit code $($process.ExitCode). See $data/errors.log" }
if (-not (Test-Path $reportPath)) { throw 'No report was written. Another announcer may already be running.' }
$report = Get-Content $reportPath -Raw | ConvertFrom-Json
$report | Add-Member -NotePropertyName peakResidentMB -NotePropertyValue ([Math]::Round($peak / 1MB, 2))
$report | Add-Member -NotePropertyName binaryMB -NotePropertyValue ([Math]::Round((Get-Item $binary).Length / 1MB, 2))
$report | ConvertTo-Json -Depth 5
if (-not $report.focusChecked -or -not $report.focusUnchanged) { throw 'Foreground focus changed.' }
if (-not $report.passiveWindow) { throw 'Window must stay topmost, refuse activation, and hide between notifications.' }
if (-not $report.abruptWindowSucceeded -or $report.windowOpacityUpdates -ne $report.shown) { throw 'Window visibility must change abruptly, without opacity animation.' }
if ($report.decodedVideoFrames -lt 100 -or $report.videoLoops -lt 2) { throw 'MP4 videos did not decode and loop.' }
if ($report.shown -ne 2 -or $report.finished -ne 2) { throw 'Both notifications must complete.' }
if ($report.durations[0] -lt 10 -or $report.durations[1] -lt 15) { throw 'Notification duration is too short.' }
if ($report.sessionTitles[0] -ne $messages[0].title -or $report.sessionTitles[1] -ne $messages[1].title) { throw 'Session titles were not displayed.' }
if ($report.animationFrames -lt 2 -or $report.staticFrames -lt 2) { throw 'Animation or static transitions did not render.' }
if ($report.videoFPS -ne 8) { throw 'Character playback must use eight frames per second.' }
if ($peak -ge 100MB) { throw 'Announcer exceeded 100 MB.' }
if ($Speech -and $report.speechStarted -ne 2) { throw 'Both characters must speak.' }
if (-not $Speech -and ($report.speechStarted -ne 0 -or $report.mutedAnnouncements -ne 2)) { throw 'Quiet hours must suppress speech only.' }
if (Test-Path (Join-Path $data 'errors.log')) { throw (Get-Content (Join-Path $data 'errors.log') -Raw) }
