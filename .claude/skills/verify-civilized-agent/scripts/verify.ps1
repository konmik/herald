param(
    [ValidateSet('Settings', 'Quiet', 'Output', 'Preview')][string]$Feature = 'Settings',
    [string]$Evidence = ('temp/verification/' + [guid]::NewGuid()),
    [string]$AppDirectory,
    [string]$ClaudePluginDirectory,
    [ValidateSet('OpenCode', 'Claude')][string]$Runtime = 'OpenCode',
    [switch]$Audible
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../../..'))
$binary = Join-Path $root 'native-announcer/target/debug/civilized-announcer.exe'
if ($AppDirectory) {
    $AppDirectory = [IO.Path]::GetFullPath($AppDirectory)
    $runtimeDirectory = if ($Runtime -eq 'Claude') { Join-Path $AppDirectory 'claude-plugin' } else { $AppDirectory }
    if ($Runtime -eq 'Claude' -and $ClaudePluginDirectory) { $runtimeDirectory = [IO.Path]::GetFullPath($ClaudePluginDirectory) }
    $architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    if ($architecture -notin @('x64', 'arm64')) { throw 'Unsupported Windows architecture' }
    $binary = Join-Path $runtimeDirectory "native-announcer/bin/civilized-announcer-win32-$architecture.exe"
}
$evidencePath = [IO.Path]::GetFullPath($Evidence, $root)
$scratch = Join-Path $env:LOCALAPPDATA ('Temp/opencode/civilized-verify-' + [guid]::NewGuid())
$process = $null
$transcribing = $false
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CivilizedVerify {
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", EntryPoint="SendMessageW", CharSet=CharSet.Unicode)] public static extern IntPtr SetText(IntPtr window, uint message, IntPtr wparam, string text);
    [DllImport("user32.dll", EntryPoint="SendMessageW", CharSet=CharSet.Unicode)] public static extern IntPtr ReadText(IntPtr window, uint message, IntPtr length, StringBuilder text);
}
'@
function Control([int]$Id) {
    $handle = [CivilizedVerify]::GetDlgItem($process.MainWindowHandle, $Id)
    if ($handle -eq [IntPtr]::Zero) { throw "Missing control $Id" }
    return $handle
}
function Send-Control([int]$Id, [uint32]$Message, [long]$Wparam = 0, [long]$Lparam = 0) {
    return [CivilizedVerify]::SendMessageW((Control $Id), $Message, [IntPtr]$Wparam, [IntPtr]$Lparam).ToInt64()
}
function Read-Control([int]$Id) {
    $buffer = [Text.StringBuilder]::new(512)
    [CivilizedVerify]::ReadText((Control $Id), 0xD, [IntPtr]512, $buffer) | Out-Null
    return $buffer.ToString()
}
function Select-Page([int]$Index) {
    Send-Control 200 0x100 0x24 | Out-Null
    Send-Control 200 0x101 0x24 | Out-Null
    for ($step = 0; $step -lt $Index; $step++) {
        Send-Control 200 0x100 0x28 | Out-Null
        Send-Control 200 0x101 0x28 | Out-Null
    }
    if ((Send-Control 200 0x188) -ne $Index) { throw 'The settings sidebar did not select the requested page' }
}
function Snapshot([string]$Name) {
    $state = [ordered]@{ title = $process.MainWindowTitle; page = (Send-Control 200 0x188); quiet = (Send-Control 101 0xF0); schedule = (Send-Control 102 0xF0); start = (Read-Control 103); end = (Read-Control 104); volume = (Send-Control 105 0x400); outputIndex = (Send-Control 106 0x147); output = (Read-Control 106); apply = (Read-Control 107); close = (Read-Control 108); status = (Read-Control 109); preview = (Read-Control 112); model = (Send-Control 113 0x147) }
    $state | ConvertTo-Json | Set-Content (Join-Path $evidencePath "$Name.json") -Encoding utf8NoBOM
    return $state
}
function Launch {
    $script:process = [Diagnostics.Process]::Start($info)
    @{ feature = $Feature; binary = $binary; sha256 = $binaryHash; pid = $process.Id; started = $process.StartTime.ToUniversalTime().ToString('o'); data = $scratch } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'instance.json')
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 50
        $process.Refresh()
        if ($process.HasExited) { throw "Settings exited early: $($process.ExitCode)" }
    } while ($process.MainWindowHandle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
    if ($process.MainWindowHandle -eq [IntPtr]::Zero) { throw 'Settings window did not open' }
    Doctor
}
function Doctor {
    $process.Refresh()
    if ($process.HasExited -or $process.Path -ne $binary -or $process.MainWindowTitle -ne 'Civilized Agent settings') { throw 'Wrong or missing settings instance' }
    if ((Read-Control 107) -ne 'Apply' -or (Read-Control 108) -ne 'Close') { throw 'Settings controls are not ready' }
    if ((Get-FileHash $binary).Hash -ne $binaryHash) { throw 'Binary changed during verification' }
    if (Test-Path (Join-Path $scratch 'errors.log')) { throw (Get-Content (Join-Path $scratch 'errors.log') -Raw) }
    Write-Output "Doctor passed: PID $($process.Id), $binaryHash, isolated data $scratch"
}
function Close-Settings {
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000) -or $process.ExitCode -ne 0) { throw 'Close did not exit cleanly' }
}
try {
    if (-not (Test-Path $binary)) { throw "Announcer executable is missing at $binary" }
    New-Item -ItemType Directory -Path $evidencePath, $scratch -Force | Out-Null
    Start-Transcript -Path (Join-Path $evidencePath 'actions.txt') | Out-Null
    $transcribing = $true
    $binaryHash = (Get-FileHash $binary).Hash
    $settingsPath = Join-Path $scratch 'settings.json'
    @{ quietMode = $false; scheduleEnabled = $true; quietStart = 1320; quietEnd = 480; volume = 0; outputDevice = 'unavailable-verification-device'; voices = @{ claude = 'Mark' } } | ConvertTo-Json | Set-Content $settingsPath -Encoding utf8NoBOM
    Copy-Item $settingsPath (Join-Path $evidencePath 'settings-before.json')
    $info = [Diagnostics.ProcessStartInfo]::new($binary)
    $info.UseShellExecute = $false
    $info.Environment['CIVILIZED_AGENT_DATA'] = $scratch
    if ($AppDirectory) {
        $info.Environment.Remove('CIVILIZED_AGENT_TTS') | Out-Null
        $info.ArgumentList.Add('--assets')
        $info.ArgumentList.Add((Join-Path $runtimeDirectory 'native-announcer/resources'))
    } else {
        $info.Environment['CIVILIZED_AGENT_TTS'] = Join-Path $root 'native-announcer/resources/tts/kitten-nano-en-v0_8-int8'
    }
    $info.ArgumentList.Add('--settings')
    Write-Output "Launch: $binary --settings; feature=$Feature; audible=$Audible"
    Launch
    $before = Snapshot 'controls-before'
    if ($Feature -eq 'Output') {
        Write-Output 'Check unavailable device uses system default; Refresh devices; Apply.'
        if ($before.output -ne 'Selected device unavailable (using system default)') { throw 'Unavailable device did not use system default' }
        Send-Control 111 0xF5 | Out-Null
        Send-Control 107 0xF5 | Out-Null
        if ((Get-Content $settingsPath -Raw | ConvertFrom-Json).outputDevice -ne 'unavailable-verification-device') { throw 'Apply lost unavailable device' }
        Copy-Item $settingsPath (Join-Path $evidencePath 'settings-unavailable-device.json')
        Write-Output 'Choose System default; Apply.'
        Send-Control 106 0x14E 0 | Out-Null
        Send-Control 107 0xF5 | Out-Null
        if ($null -ne (Get-Content $settingsPath -Raw | ConvertFrom-Json).outputDevice) { throw 'System default was not saved' }
    } elseif ($Feature -eq 'Preview') {
        $saved = Get-Content $settingsPath -Raw
        Write-Output 'At zero volume, click Play example.'
        Send-Control 112 0xF5 | Out-Null
        if ((Read-Control 109) -ne 'Preview is silent at 0% volume.') { throw 'Zero volume preview was not silent' }
        Snapshot 'controls-silent' | Out-Null
        if ($Audible) {
            Write-Output 'Choose System default and 35% volume; Play example; Stop example.'
            Send-Control 106 0x14E 0 | Out-Null
            Send-Control 105 0x405 1 35 | Out-Null
            Send-Control 112 0xF5 | Out-Null
            if ((Read-Control 112) -ne 'Stop example') { throw 'Preview did not start' }
            Snapshot 'controls-playing' | Out-Null
            Send-Control 112 0xF5 | Out-Null
            if ((Read-Control 109) -ne 'Preview stopped.') { throw 'Preview did not stop' }
        }
        if ((Get-Content $settingsPath -Raw) -ne $saved) { throw 'Preview saved unapplied settings' }
    } else {
        Write-Output 'Set quiet hours to 22:30 and 08:15; choose 35% volume and ElevenLabs v4; Apply.'
        Select-Page 2
        Send-Control 101 0xF5 | Out-Null
        Send-Control 102 0xF5 | Out-Null
        Send-Control 102 0xF5 | Out-Null
        [CivilizedVerify]::SetText((Control 103), 0xC, [IntPtr]::Zero, '22:30') | Out-Null
        [CivilizedVerify]::SetText((Control 104), 0xC, [IntPtr]::Zero, '08:15') | Out-Null
        Select-Page 1
        Send-Control 105 0x405 1 35 | Out-Null
        Select-Page 3
        Send-Control 113 0x14E 1 | Out-Null
        [CivilizedVerify]::SendMessageW($process.MainWindowHandle, 0x111, [IntPtr](113 + 65536), (Control 113)) | Out-Null
        Snapshot 'controls-draft' | Out-Null
        Send-Control 107 0xF5 | Out-Null
        $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
        if ((Read-Control 109) -ne 'Saved. Changes apply to the next announcement.') { throw 'Apply did not report saved state' }
        if (-not $settings.quietMode -or -not $settings.scheduleEnabled -or $settings.quietStart -ne 1350 -or $settings.quietEnd -ne 495 -or $settings.volume -ne 35 -or $settings.speechModel -ne 'eleven_v4' -or $settings.voices.claude -ne 'Mark') { throw 'Applied settings did not persist' }
    }
    Snapshot 'controls-after' | Out-Null
    Copy-Item $settingsPath (Join-Path $evidencePath 'settings-after.json')
    $applied = Get-Content $settingsPath -Raw
    Write-Output 'Change volume to 15% without Apply; Close; confirm disk unchanged.'
    Select-Page 1
    Send-Control 105 0x405 1 15 | Out-Null
    Close-Settings
    if ((Get-Content $settingsPath -Raw) -ne $applied) { throw 'Close saved an unapplied edit' }
    Write-Output 'Reopen settings; read controls from the new window.'
    Launch
    $reopened = Snapshot 'controls-reopened'
    $persisted = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if ($reopened.volume -ne $persisted.volume -or $reopened.quiet -ne [int]$persisted.quietMode -or $reopened.schedule -ne [int]$persisted.scheduleEnabled) { throw 'Saved state did not survive reopening' }
    if ($Feature -eq 'Settings' -or $Feature -eq 'Quiet') {
        Select-Page 2
        Snapshot 'controls-reopened-quiet' | Out-Null
        if ((Read-Control 103) -ne '22:30' -or (Read-Control 104) -ne '08:15') { throw 'Saved schedule did not survive reopening' }
        Select-Page 3
        Snapshot 'controls-reopened-speech' | Out-Null
        if ((Send-Control 113 0x147) -ne 1) { throw 'Saved speech model did not survive reopening' }
    }
    if ($Feature -eq 'Output' -and ($reopened.outputIndex -ne 0 -or $reopened.output -ne 'System default')) { throw 'Default output did not survive reopening' }
    Close-Settings
    @{ passed = $true; feature = $Feature; audible = [bool]$Audible; exitCode = $process.ExitCode } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'result.json')
    Write-Output "PASS: $Feature; evidence: $evidencePath"
} catch {
    if (Test-Path $evidencePath) { $_ | Out-String | Set-Content (Join-Path $evidencePath 'failure.txt') }
    throw
} finally {
    if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    if (Test-Path $scratch) {
        if (Test-Path (Join-Path $scratch 'errors.log')) { Copy-Item (Join-Path $scratch 'errors.log') (Join-Path $evidencePath 'errors.log') }
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
    if (Test-Path $evidencePath) { @{ scratchRemoved = -not (Test-Path $scratch); processExited = (-not $process -or $process.HasExited) } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'cleanup.json') }
    if ($transcribing) { Stop-Transcript | Out-Null }
}
