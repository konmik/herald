param([string]$Evidence = ('temp/verification/' + [guid]::NewGuid()), [string]$AppDirectory, [string]$ClaudePluginDirectory, [ValidateSet('OpenCode', 'Claude')][string]$Runtime = 'OpenCode', [switch]$Speech, [switch]$Meeting, [switch]$Quiet, [string]$AppearanceSettings, [switch]$SummaryTitle, [ValidateRange(0, 10)][int]$SilentSoundSeconds = 0, [switch]$SpeechFixture, [switch]$CaptureFrames)
$ErrorActionPreference = 'Stop'
$expectedTitle = if ($SummaryTitle) { 'Checks passed' } else { 'Verification session' }
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../../..'))
$evidencePath = [IO.Path]::GetFullPath($Evidence, $root)
$scratch = Join-Path $env:LOCALAPPDATA ('Temp/opencode/herald-announcement-' + [guid]::NewGuid())
$binary = Join-Path $root 'native-announcer/target/debug/herald.exe'
$assets = Join-Path $root 'native-announcer/resources'
$toolNode = 'node'
if ($AppDirectory) {
    $AppDirectory = [IO.Path]::GetFullPath($AppDirectory)
    $runtimeDirectory = if ($Runtime -eq 'Claude') { Join-Path $AppDirectory 'claude-plugin' } else { $AppDirectory }
    if ($Runtime -eq 'Claude' -and $ClaudePluginDirectory) { $runtimeDirectory = [IO.Path]::GetFullPath($ClaudePluginDirectory) }
    $architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    if ($architecture -notin @('x64', 'arm64')) { throw 'Unsupported Windows architecture' }
    $binary = Join-Path $runtimeDirectory "native-announcer/bin/herald-win32-$architecture.exe"
    $assets = Join-Path $runtimeDirectory 'native-announcer/resources'
}
$process = $null
$server = $null
$transcribing = $false
$oldData = $env:HERALD_DATA
. (Join-Path $PSScriptRoot 'process.ps1')
if (Test-Path -LiteralPath $evidencePath) { throw 'Use a new evidence directory' }
New-Item -ItemType Directory -Path $evidencePath | Out-Null
function Send-Bridge($Message) {
    $start = [Diagnostics.ProcessStartInfo]::new($binary)
    $start.UseShellExecute = $false
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardOutputEncoding = [Text.Encoding]::UTF8
    foreach ($argument in @('--bridge', '--assets', $assets)) { $start.ArgumentList.Add($argument) }
    $bridgeProcess = [Diagnostics.Process]::Start($start)
    try {
        $output = $bridgeProcess.StandardOutput.ReadToEndAsync()
        $errors = $bridgeProcess.StandardError.ReadToEndAsync()
        $bridgeProcess.StandardInput.Write(($Message | ConvertTo-Json -Compress))
        $bridgeProcess.StandardInput.Close()
        if (-not $bridgeProcess.WaitForExit(10000)) { $bridgeProcess.Kill($true); $bridgeProcess.WaitForExit(); throw 'Bridge exceeded its deadline' }
        $output.GetAwaiter().GetResult() | Out-Null
        if ($bridgeProcess.ExitCode -ne 0) { throw "Bridge exited $($bridgeProcess.ExitCode): $($errors.GetAwaiter().GetResult())" }
    } finally { $bridgeProcess.Dispose() }
}
try {
    New-Item -ItemType Directory -Path $scratch | Out-Null
    Start-Transcript -Path (Join-Path $evidencePath 'actions.txt') | Out-Null
    $transcribing = $true
    $env:HERALD_DATA = $scratch
    $settings = @{ quietMode = (-not $Speech -or [bool]$Quiet); scheduleEnabled = $false; volume = 35; useGpu = $false }
    if ($PSBoundParameters.ContainsKey('SilentSoundSeconds')) { $settings.silentSoundSeconds = $SilentSoundSeconds }
    if ($SpeechFixture) {
        $addressFile = Join-Path $scratch 'speech-api-address.txt'
        $requestsFile = Join-Path $evidencePath 'speech-api-requests.jsonl'
        $serverInfo = [Diagnostics.ProcessStartInfo]::new($toolNode)
        $serverInfo.UseShellExecute = $false
        foreach ($argument in @((Join-Path $root 'development_tools/voice-api-fixture.mjs'), $addressFile, $requestsFile)) { $serverInfo.ArgumentList.Add($argument) }
        $server = [Diagnostics.Process]::Start($serverInfo)
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (-not (Test-Path $addressFile)) {
            if ($server.HasExited -or [DateTime]::UtcNow -ge $deadline) { throw 'Speech fixture did not start.' }
            Start-Sleep -Milliseconds 50
        }
        $settings.elevenlabsApiKey = 'character-ui-test-key'
        $settings.speechModel = 'eleven_v4'
    }
    if ($AppearanceSettings) {
        $appearance = Get-Content -LiteralPath $AppearanceSettings -Raw | ConvertFrom-Json
        $settings.announcementBodyFont = $appearance.announcementBodyFont
        $settings.announcementTitleFont = $appearance.announcementTitleFont
    }
    $settings | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $scratch 'settings.json') -Encoding utf8NoBOM
    Copy-Item (Join-Path $scratch 'settings.json') (Join-Path $evidencePath 'settings.json')
    if ($Meeting) { @{ active = $true; updated = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() / 1000 } | ConvertTo-Json | Set-Content (Join-Path $scratch 'meeting.json') -Encoding utf8NoBOM }
    $info = [Diagnostics.ProcessStartInfo]::new($binary)
    $info.UseShellExecute = $false
    $info.Environment['HERALD_DATA'] = $scratch
    if ($SpeechFixture) { $info.Environment['ELEVENLABS_API_BASE_URL'] = Get-Content $addressFile -Raw }
    if ($AppDirectory) { $info.Environment.Remove('HERALD_TTS') | Out-Null }
    else { $info.Environment['HERALD_TTS'] = Join-Path $root 'native-announcer/resources/tts/kitten-nano-en-v0_8-int8' }
    foreach ($argument in @('--isolated', '--assets', $assets, '--test-seconds', '35', '--report', (Join-Path $evidencePath 'report.json'), '--snapshot', (Join-Path $evidencePath 'render.png'))) { $info.ArgumentList.Add($argument) }
    if ($CaptureFrames) {
        $info.ArgumentList.Add('--capture-frames')
        $info.ArgumentList.Add((Join-Path $evidencePath 'frames'))
    }
    Write-Output "Launch: $binary --isolated; data=$scratch; speech=$Speech; meeting=$Meeting; quiet=$Quiet"
    $process = [Diagnostics.Process]::Start($info)
    $hash = (Get-FileHash $binary).Hash
    @{ pid = $process.Id; started = $process.StartTime.ToUniversalTime().ToString('o'); binary = $binary; sha256 = $hash; data = $scratch; entry = 'production bridge transport' } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'instance.json')
    $started = [DateTime]::UtcNow
    $queued = $false
    while (-not $process.WaitForExit(1000)) {
        if (-not (Test-OwnedPlaybackProcess $process $binary (Join-Path $scratch 'errors.log'))) { break }
        Send-Bridge @{ type = 'presence'; clientID = 'verification'; sessionIDs = @('claude:verification'); at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() }
        if (-not $queued -and ([DateTime]::UtcNow - $started).TotalSeconds -ge 3) {
            if ($process.MainWindowHandle -eq [IntPtr]::Zero -and -not (Test-Path (Join-Path $scratch 'inbox'))) { throw 'Announcer not ready' }
            Write-Output "Doctor passed: owned PID $($process.Id), SHA256 $hash, isolated inbox"
            $message = @{ type = 'notify'; id = 'verification-result'; sessionID = 'claude:verification'; completed = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds(); title = 'Verification session'; text = 'The verification task passed.'; character = 'claude'; emotion = 'neutral' }
            if ($SummaryTitle) { $message.text = ' Checks passed | The verification task passed. ' }
            $message | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'notification.json')
            Write-Output 'Send notification through the production bridge; keep session presence alive.'
            Send-Bridge $message
            $queued = $true
        }
        if (([DateTime]::UtcNow - $started).TotalSeconds -gt 45) { throw 'Announcer exceeded its deadline' }
    }
    if ($process.ExitCode -ne 0) { throw "Announcer exited $($process.ExitCode)" }
    if (Test-Path (Join-Path $scratch 'errors.log')) { throw (Get-Content (Join-Path $scratch 'errors.log') -Raw) }
    $report = Get-Content (Join-Path $evidencePath 'report.json') -Raw | ConvertFrom-Json
    if ($report.shown -ne 1 -or $report.finished -ne 1 -or -not $report.passiveWindow -or -not $report.focusChecked -or -not $report.focusUnchanged -or $report.decodedVideoFrames -le 0 -or $report.sessionTitles[0] -ne $expectedTitle) { throw 'Notification did not render, complete, or preserve focus' }
    $shouldSpeak = $Speech -and -not $Meeting -and -not $Quiet
    if (($shouldSpeak -and $report.speechStarted -ne 1) -or (-not $shouldSpeak -and ($report.speechStarted -ne 0 -or $report.mutedAnnouncements -ne 1))) { throw 'Speech policy failed' }
    $history = Get-Content (Join-Path $scratch 'history.jsonl') | ForEach-Object { $_ | ConvertFrom-Json }
    if (@($history).Count -ne 1 -or $history.id -ne 'verification-result' -or $history.text -ne 'The verification task passed.' -or $history.title -ne $expectedTitle) { throw 'Shown history does not match notification' }
    Copy-Item (Join-Path $scratch 'history.jsonl') (Join-Path $evidencePath 'history.jsonl')
    Copy-Item (Join-Path $scratch 'queue.json') (Join-Path $evidencePath 'queue.json')
    if (@(Get-Content (Join-Path $evidencePath 'queue.json') -Raw | ConvertFrom-Json).Count -ne 0) { throw 'Completed notification remained queued' }
    if (-not (Test-Path (Join-Path $evidencePath 'render.png'))) { throw 'No rendered evidence' }
    Write-Output "PASS: announcement transport and desktop playback; evidence: $evidencePath"
} catch {
    if (Test-Path $evidencePath) { $_ | Out-String | Set-Content (Join-Path $evidencePath 'failure.txt') }
    throw
} finally {
    if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    if ($server -and -not $server.HasExited) { $server.Kill(); $server.WaitForExit() }
    if (Test-Path $scratch) {
        if (Test-Path (Join-Path $scratch 'errors.log')) { Copy-Item (Join-Path $scratch 'errors.log') (Join-Path $evidencePath 'errors.log') }
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
    $env:HERALD_DATA = $oldData
    if (Test-Path $evidencePath) { @{ scratchRemoved = -not (Test-Path $scratch); processExited = (-not $process -or $process.HasExited); serverExited = (-not $server -or $server.HasExited) } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'cleanup.json') }
    if ($transcribing) { Stop-Transcript | Out-Null }
}
