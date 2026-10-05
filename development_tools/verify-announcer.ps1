param([switch]$Speech, [switch]$Meeting)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $root 'native-announcer/bin/civilized-announcer-win32-x64.exe'
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
$info.ArgumentList.Add('--isolated')
$info.ArgumentList.Add('--assets')
$info.ArgumentList.Add((Join-Path $root 'native-announcer/resources'))
$info.ArgumentList.Add('--test-seconds')
$info.ArgumentList.Add('75')
$info.ArgumentList.Add('--report')
$info.ArgumentList.Add($reportPath)
$info.ArgumentList.Add('--snapshot')
$info.ArgumentList.Add((Join-Path $temporary 'preview.png'))
$process = [System.Diagnostics.Process]::Start($info)
$peak = 0
$lastPresence = [DateTimeOffset]::MinValue
while (-not $process.WaitForExit(50)) {
    $now = [DateTimeOffset]::UtcNow
    if (($now - $lastPresence).TotalSeconds -ge 2) {
        $presence = @{ type = 'presence'; clientID = 'verification'; sessionIDs = @('verify-claude', 'verify-opencode'); at = $now.ToUnixTimeMilliseconds() }
        $path = Join-Path $inbox ("presence-$([guid]::NewGuid())")
        $presence | ConvertTo-Json | Set-Content "$path.tmp" -Encoding utf8NoBOM
        Move-Item "$path.tmp" "$path.json"
        $lastPresence = $now
    }
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
$library = Get-ChildItem (Join-Path $root 'native-announcer/resources/videos') -Filter '*.mp4' -ErrorAction SilentlyContinue
$expectedFPS = if ($library) { 16 } else { 8 }
if ($report.videoFPS -ne $expectedFPS) { throw "Character playback must use $expectedFPS frames per second." }
if ($library) {
    $libraryPath = (Resolve-Path (Join-Path $root 'native-announcer/resources/videos')).Path
    if ($report.selectedVideos.Count -ne 2 -or @($report.selectedVideos | Where-Object { (Resolve-Path (Split-Path $_ -Parent)).Path -ne $libraryPath }).Count -ne 0) { throw 'Notifications must select videos from the shared library.' }
}
if ($peak -ge 100MB) { throw 'Announcer exceeded 100 MB.' }
if ($Speech -and $report.speechStarted -ne 2) { throw 'Both characters must speak.' }
if (-not $Speech -and ($report.speechStarted -ne 0 -or $report.mutedAnnouncements -ne 2)) { throw 'Quiet hours must suppress speech only.' }
if (Test-Path (Join-Path $data 'errors.log')) { throw (Get-Content (Join-Path $data 'errors.log') -Raw) }
