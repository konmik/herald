param(
    [ValidateSet('Settings', 'Quiet', 'Output', 'Preview', 'VoicePreview', 'Announcements', 'SilentSound')][string]$Feature = 'Settings',
    [string]$Evidence = ('temp/verification/' + [guid]::NewGuid()),
    [string]$AppDirectory,
    [string]$ClaudePluginDirectory,
    [ValidateSet('OpenCode', 'Claude')][string]$Runtime = 'OpenCode',
    [switch]$Audible,
    [ValidateRange(0, 10)][int]$SilentSoundSeconds = 0
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
$server = $null
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
    [DllImport("user32.dll", EntryPoint="PostMessageW")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", EntryPoint="GetClassNameW", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr window, StringBuilder text, int length);
    [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr window, int index);
    [DllImport("user32.dll", EntryPoint="FindWindowW", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string className, string title);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
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
function Post-Control([int]$Id, [uint32]$Message, [long]$Wparam = 0, [long]$Lparam = 0) {
    if (-not [CivilizedVerify]::PostMessage((Control $Id), $Message, [IntPtr]$Wparam, [IntPtr]$Lparam)) { throw "Could not post message $Message to control $Id" }
}
function Read-Control([int]$Id) {
    $control = Control $Id
    $length = [CivilizedVerify]::SendMessageW($control, 0xE, [IntPtr]::Zero, [IntPtr]::Zero).ToInt64()
    if ($length -lt 0) { throw "Could not read control $Id text length" }
    $capacity = [Math]::Max(256, [int]$length + 1)
    while ($true) {
        $buffer = [Text.StringBuilder]::new($capacity)
        $copied = [CivilizedVerify]::ReadText($control, 0xD, [IntPtr]$capacity, $buffer).ToInt64()
        if ($copied -lt ($capacity - 1)) {
            $value = $buffer.ToString()
            if ($Id -eq 134) { $value = $value.Replace("`r`n", "`n") }
            return $value
        }
        if ($capacity -ge 1048576) { throw "Control $Id text exceeded the verification buffer limit" }
        $capacity *= 2
    }
}
function Control-Class([int]$Id) {
    $buffer = [Text.StringBuilder]::new(256)
    [CivilizedVerify]::GetClassName((Control $Id), $buffer, $buffer.Capacity) | Out-Null
    return $buffer.ToString()
}
function Read-Combo-Item([int]$Id, [int]$Index) {
    $length = Send-Control $Id 0x149 $Index
    if ($length -lt 0) { throw "Combo control $Id has no item at index $Index" }
    $capacity = [Math]::Max(1, [int]$length + 1)
    $buffer = [Text.StringBuilder]::new($capacity)
    $copied = [CivilizedVerify]::ReadText((Control $Id), 0x148, [IntPtr]$Index, $buffer).ToInt64()
    if ($copied -lt 0) { throw "Could not read combo control $Id item $Index" }
    return $buffer.ToString()
}
function Read-Combo-Control([int]$Id) {
    $index = Send-Control $Id 0x147
    if ($index -lt 0) { return '' }
    return Read-Combo-Item $Id $index
}
function Notify-Control([int]$Id, [uint32]$Code) {
    $control = Control $Id
    [CivilizedVerify]::SendMessageW($process.MainWindowHandle, 0x111, [IntPtr](([long]$Id) -bor (([long]$Code) -shl 16)), $control) | Out-Null
}
function Set-Control-Text([int]$Id, [string]$Value) {
    $nativeValue = if ($Id -eq 134) { $Value.Replace("`r`n", "`n").Replace("`n", "`r`n") } else { $Value }
    if ([CivilizedVerify]::SetText((Control $Id), 0xC, [IntPtr]::Zero, $nativeValue) -eq [IntPtr]::Zero) { throw "Could not set control $Id" }
    if ((Read-Control $Id) -cne $Value) { throw "Control $Id did not accept its draft text" }
    Notify-Control $Id 0x300
}
function Select-Combo-Index([int]$Id, [int]$Index) {
    $count = Send-Control $Id 0x146
    if ($Index -lt 0 -or $count -le $Index) { throw "Combo control $Id has no item at index $Index" }
    $selected = Send-Control $Id 0x14E $Index
    if ($selected -ne $Index) { throw "Combo control $Id did not select item $Index" }
    Notify-Control $Id 1
}
function Select-Combo-Text([int]$Id, [string]$Value) {
    if ((Control-Class $Id) -notmatch 'COMBOBOX') { throw "Control $Id is not a combo box" }
    $count = Send-Control $Id 0x146
    if ($count -le 0) { throw "Combo control $Id has no items" }
    $match = -1
    for ($index = 0; $index -lt $count; $index++) {
        if ((Read-Combo-Item $Id $index) -ceq $Value) { $match = $index; break }
    }
    if ($match -lt 0) { throw "Combo control $Id does not contain '$Value'" }
    Select-Combo-Index $Id $match
    if ((Read-Combo-Control $Id) -cne $Value) { throw "Combo control $Id did not select '$Value'" }
}
function Read-Announcement-Size([int]$Id) {
    $text = if ((Control-Class $Id) -match 'COMBOBOX') { Read-Combo-Control $Id } else { Read-Control $Id }
    $match = [regex]::Match($text, '^\s*(\d+)\s*(?:pt|px)?\s*$')
    if (-not $match.Success) { throw "Font size control $Id contains '$text'" }
    return [int]$match.Groups[1].Value
}
function Set-Announcement-Size([int]$Id, [int]$Value) {
    if ((Control-Class $Id) -match 'COMBOBOX') {
        $count = Send-Control $Id 0x146
        $match = -1
        for ($index = 0; $index -lt $count; $index++) {
            $item = Read-Combo-Item $Id $index
            if ($item.Trim() -ceq [string]$Value -or $item.Trim() -cmatch "^$Value\s*(?:pt|px)$") { $match = $index; break }
        }
        if ($match -lt 0) { throw "Combo control $Id does not contain font size $Value" }
        Select-Combo-Index $Id $match
    } else {
        Set-Control-Text $Id ([string]$Value)
    }
    if ((Read-Announcement-Size $Id) -ne $Value) { throw "Font size control $Id did not select $Value" }
}
function Set-Invalid-Announcement-Size([int]$Id) {
    if ((Control-Class $Id) -match 'COMBOBOX') {
        $selected = Send-Control $Id 0x14E -1
        if ($selected -ne -1) { throw "Combo control $Id did not clear its invalid selection" }
        Notify-Control $Id 1
    } else {
        Set-Control-Text $Id '0'
    }
}
function Get-Installed-Fonts {
    try { Add-Type -AssemblyName System.Drawing } catch { throw "Could not load the installed font catalog: $($_.Exception.Message)" }
    $families = [System.Drawing.Text.InstalledFontCollection]::new().Families
    $names = @($families | ForEach-Object { $_.Name })
    foreach ($required in @('Consolas', 'Segoe UI')) { if (-not ($names -ccontains $required)) { throw "Required installed font '$required' is unavailable" } }
    return $names
}
function Assert-Announcements-Page {
    foreach ($id in @(130, 131, 132, 133, 134, 135)) {
        $handle = Control $id
        if (-not [CivilizedVerify]::IsWindowVisible($handle)) { throw "Announcements control $id is hidden" }
    }
    if ((Control-Class 130) -notmatch 'COMBOBOX' -or (Control-Class 132) -notmatch 'COMBOBOX') { throw 'Announcement font controls are not non-editable combo boxes' }
    foreach ($id in @(130, 132)) { if (([CivilizedVerify]::GetWindowLongPtr((Control $id), -16).ToInt64() -band 3) -ne 3) { throw "Announcement font control $id is editable" } }
    if ((Read-Announcement-Size 131) -le 0 -or (Read-Announcement-Size 133) -le 0) { throw 'Announcement font sizes are not positive' }
    if ([string]::IsNullOrWhiteSpace((Read-Control 134))) { throw 'The announcement summary prompt is empty' }
}
function Assert-Installed-Announcement-Fonts {
    foreach ($id in @(130, 132)) {
        $selected = Read-Combo-Control $id
        if (-not ($script:installedFonts -ccontains $selected)) { throw "Announcement font control $id selected an uninstalled font '$selected'" }
    }
}
function Assert-Exact-Lf([string]$Name, [string]$Value) {
    if ($Value.Contains("`r")) { throw "$Name contains CR characters" }
    if ($Value -notmatch "`n") { throw "$Name does not contain LF lines" }
}
function Assert-Preserved-Preferences($Before, $After) {
    foreach ($name in @('quietMode', 'scheduleEnabled', 'quietStart', 'quietEnd', 'volume', 'outputDevice', 'speechModel', 'defaultVoiceId')) {
        if ($After.$name -ne $Before.$name) { throw "Apply changed existing preference $name" }
    }
    foreach ($name in @('claude', 'opencode')) {
        if ($After.voices.$name -ne $Before.voices.$name) { throw "Apply changed existing voice preference $name" }
    }
}
function Get-Owned-Settings-Dialog {
    $dialog = [CivilizedVerify]::FindWindow('#32770', 'Civilized Agent settings')
    if ($dialog -eq [IntPtr]::Zero -or -not [CivilizedVerify]::IsWindowVisible($dialog)) { return [IntPtr]::Zero }
    [uint32]$owner = 0
    [CivilizedVerify]::GetWindowThreadProcessId($dialog, [ref]$owner) | Out-Null
    if ($owner -eq $process.Id) { return $dialog }
    return [IntPtr]::Zero
}
function Apply-Invalid-Announcement-Draft([string]$BeforeBytes, [string]$ExpectedStatus, [string]$Description) {
    Post-Control 107 0xF5
    $deadline = [DateTime]::UtcNow.AddSeconds(3)
    $status = ''
    while ([DateTime]::UtcNow -lt $deadline) {
        if ((Get-Owned-Settings-Dialog) -ne [IntPtr]::Zero) { throw "$Description opened a modal dialog" }
        $status = Read-Control 109
        if ($status -match $ExpectedStatus) { break }
        if ((Get-Content $settingsPath -Raw) -ne $BeforeBytes) { throw "$Description changed the saved settings" }
        Start-Sleep -Milliseconds 50
    }
    if ($status -notmatch $ExpectedStatus) { throw "$Description did not report a validation status: '$status'" }
    if ((Get-Content $settingsPath -Raw) -ne $BeforeBytes) { throw "$Description changed the saved settings" }
    Write-Output "$Description rejected without a modal dialog: $status"
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
function Set-Silent-Sound([int]$Seconds) {
    if ((Control-Class 126) -cne 'msctls_trackbar32' -or (Send-Control 126 0x401) -ne 0 -or (Send-Control 126 0x402) -ne 10) { throw 'Silent sound slider must range from 0 to 10 seconds' }
    Send-Control 126 0x100 0x24 | Out-Null
    Send-Control 126 0x101 0x24 | Out-Null
    for ($step = 0; $step -lt $Seconds; $step++) {
        $key = if ($step % 2 -eq 0) { 0x27 } else { 0x22 }
        Send-Control 126 0x100 $key | Out-Null
        Send-Control 126 0x101 $key | Out-Null
        if ((Send-Control 126 0x400) -ne ($step + 1) -or (Read-Control 127) -cne "Silent sound before speech: $($step + 1) seconds") { throw 'Silent sound slider keyboard steps or live label failed' }
    }
    if ((Send-Control 126 0x400) -ne $Seconds -or (Read-Control 127) -cne "Silent sound before speech: $Seconds seconds") { throw 'Silent sound slider did not accept its draft value' }
}
function Snapshot([string]$Name) {
    $state = [ordered]@{ title = $process.MainWindowTitle; page = (Send-Control 200 0x188); quiet = (Send-Control 101 0xF0); schedule = (Send-Control 102 0xF0); start = (Read-Control 103); end = (Read-Control 104); volume = (Send-Control 105 0x400); outputIndex = (Send-Control 106 0x147); output = (Read-Control 106); apply = (Read-Control 107); close = (Read-Control 108); status = (Read-Control 109); preview = (Read-Control 112); voicePreview = (Read-Control 210); voiceId = (Read-Control 216); model = (Send-Control 113 0x147) }
    $bodyFont = [CivilizedVerify]::GetDlgItem($process.MainWindowHandle, 130)
    $silentSoundControl = [CivilizedVerify]::GetDlgItem($process.MainWindowHandle, 126)
    if ($silentSoundControl -ne [IntPtr]::Zero) {
        $state.silentSound = [ordered]@{ position = (Send-Control 126 0x400); minimum = (Send-Control 126 0x401); maximum = (Send-Control 126 0x402); visible = [bool][CivilizedVerify]::IsWindowVisible($silentSoundControl); label = (Read-Control 127) }
    }
    if ($bodyFont -ne [IntPtr]::Zero) {
        $state.announcementBodyFont = [ordered]@{ family = (Read-Combo-Control 130); size = (Read-Announcement-Size 131) }
        $state.announcementTitleFont = [ordered]@{ family = (Read-Combo-Control 132); size = (Read-Announcement-Size 133) }
        $state.summaryPrompt = (Read-Control 134)
        $state.announcementControls = [ordered]@{ bodyFamilyVisible = [bool][CivilizedVerify]::IsWindowVisible($bodyFont); bodySizeVisible = [bool][CivilizedVerify]::IsWindowVisible((Control 131)); titleFamilyVisible = [bool][CivilizedVerify]::IsWindowVisible((Control 132)); titleSizeVisible = [bool][CivilizedVerify]::IsWindowVisible((Control 133)); promptVisible = [bool][CivilizedVerify]::IsWindowVisible((Control 134)); resetVisible = [bool][CivilizedVerify]::IsWindowVisible((Control 135)) }
    }
    $state | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $evidencePath "$Name.json") -Encoding utf8NoBOM
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
    @{ quietMode = $false; scheduleEnabled = $true; quietStart = 1320; quietEnd = 480; volume = 0; outputDevice = 'unavailable-verification-device'; speechModel = 'eleven_flash_v2_5'; defaultVoiceId = 'JBFqnCBsd6RMkjVDRZzb'; voices = @{ claude = 'Mark'; opencode = 'Luna' } } | ConvertTo-Json | Set-Content $settingsPath -Encoding utf8NoBOM
    Copy-Item $settingsPath (Join-Path $evidencePath 'settings-before.json')
    $beforeSettings = Get-Content $settingsPath -Raw | ConvertFrom-Json
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
    if ($Feature -eq 'VoicePreview' -and $Audible) {
        $addressFile = Join-Path $scratch 'voice-api-address.txt'
        $requestsFile = Join-Path $evidencePath 'voice-api-requests.jsonl'
        $serverInfo = [Diagnostics.ProcessStartInfo]::new('node')
        $serverInfo.UseShellExecute = $false
        $serverInfo.ArgumentList.Add((Join-Path $root 'development_tools/voice-api-fixture.mjs'))
        $serverInfo.ArgumentList.Add($addressFile)
        $serverInfo.ArgumentList.Add($requestsFile)
        $server = [Diagnostics.Process]::Start($serverInfo)
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (-not (Test-Path $addressFile)) {
            if ($server.HasExited -or [DateTime]::UtcNow -ge $deadline) { throw 'Voice API fixture did not start.' }
            Start-Sleep -Milliseconds 50
        }
        $info.Environment['ELEVENLABS_API_BASE_URL'] = (Get-Content $addressFile -Raw)
    }
    Write-Output "Launch: $binary --settings; feature=$Feature; audible=$Audible"
    Launch
    if ($Feature -eq 'Announcements') {
        $script:installedFonts = Get-Installed-Fonts
        Select-Page 5
        Assert-Announcements-Page
    }
    $before = Snapshot 'controls-before'
    if ($Feature -eq 'SilentSound') {
        Select-Page 1
        $initial = Snapshot 'controls-silent-sound-default'
        if ($initial.silentSound.position -ne 0 -or $initial.silentSound.minimum -ne 0 -or $initial.silentSound.maximum -ne 10 -or -not $initial.silentSound.visible -or $initial.silentSound.label -cne 'Silent sound before speech: 0 seconds') { throw 'Silent sound slider is not visible on Audio or default zero' }
        Set-Silent-Sound 5
        Snapshot 'controls-silent-sound-five-draft' | Out-Null
        Send-Control 107 0xF5 | Out-Null
        if ((Get-Content $settingsPath -Raw | ConvertFrom-Json).silentSoundSeconds -ne 5) { throw 'Apply did not save five seconds' }
        Copy-Item $settingsPath (Join-Path $evidencePath 'settings-silent-sound-enabled.json')
        Close-Settings
        Launch
        Select-Page 1
        $enabled = Snapshot 'controls-silent-sound-enabled-reopened'
        if ($enabled.silentSound.position -ne 5 -or $enabled.silentSound.label -cne 'Silent sound before speech: 5 seconds') { throw 'Reopen did not restore five seconds' }
        $saved = Get-Content $settingsPath -Raw
        Set-Silent-Sound 10
        Snapshot 'controls-silent-sound-ten-draft' | Out-Null
        Close-Settings
        if ((Get-Content $settingsPath -Raw) -ne $saved) { throw 'Close saved the ten-second draft' }
        Launch
        Select-Page 1
        $discarded = Snapshot 'controls-silent-sound-draft-discarded'
        if ($discarded.silentSound.position -ne 5 -or $discarded.silentSound.label -cne 'Silent sound before speech: 5 seconds') { throw 'Close did not discard the ten-second draft' }
        Set-Silent-Sound 0
        Send-Control 107 0xF5 | Out-Null
        if ((Get-Content $settingsPath -Raw | ConvertFrom-Json).silentSoundSeconds -ne 0) { throw 'Apply did not save zero seconds' }
        Close-Settings
        Launch
        Select-Page 1
        $disabled = Snapshot 'controls-silent-sound-disabled-reopened'
        if ($disabled.silentSound.position -ne 0 -or $disabled.silentSound.label -cne 'Silent sound before speech: 0 seconds') { throw 'Reopen did not restore zero seconds' }
    } elseif ($Feature -eq 'Output') {
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
    } elseif ($Feature -eq 'VoicePreview') {
        $saved = Get-Content $settingsPath -Raw
        Select-Page 1
        Send-Control 105 0x405 1 1 | Out-Null
        Select-Page 2
        Send-Control 101 0xF5 | Out-Null
        Select-Page 0
        Send-Control 201 0x186 0 | Out-Null
        [CivilizedVerify]::SendMessageW($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Control 201)) | Out-Null
        Write-Output 'Enable draft quiet mode without Apply; play the character voice example.'
        Send-Control 210 0xF5 | Out-Null
        Snapshot 'controls-voice-quiet' | Out-Null
        if ((Read-Control 109) -ne 'Preview is silent during quiet hours.' -or (Read-Control 210) -ne 'Play voice example') { throw 'Voice preview ignored draft quiet mode.' }
        Select-Page 1
        Send-Control 112 0xF5 | Out-Null
        if ((Read-Control 109) -ne 'Preview is silent during quiet hours.' -or (Read-Control 112) -ne 'Play example') { throw 'Audio preview ignored draft quiet mode.' }
        Select-Page 2
        Send-Control 101 0xF5 | Out-Null
        [CivilizedVerify]::SetText((Control 103), 0xC, [IntPtr]::Zero, '00:00') | Out-Null
        [CivilizedVerify]::SetText((Control 104), 0xC, [IntPtr]::Zero, '24:00') | Out-Null
        foreach ($button in @(210, 112)) {
            Select-Page $(if ($button -eq 210) { 0 } else { 1 })
            Write-Output "Play example control $button with a draft all-day quiet schedule."
            Send-Control $button 0xF5 | Out-Null
            if ((Read-Control 109) -ne 'Preview is silent during quiet hours.') { throw 'Preview ignored the draft quiet schedule.' }
        }
        Select-Page 2
        Send-Control 102 0xF5 | Out-Null
        Select-Page 1
        Send-Control 105 0x405 1 0 | Out-Null
        foreach ($button in @(210, 112)) {
            Select-Page $(if ($button -eq 210) { 0 } else { 1 })
            Send-Control $button 0xF5 | Out-Null
            $expected = if ($button -eq 210) { 'Voice preview is silent at 0% volume.' } else { 'Preview is silent at 0% volume.' }
            if ((Read-Control 109) -ne $expected) { throw 'Disabling the draft schedule did not restore volume-based previews.' }
        }
        Select-Page 2
        [CivilizedVerify]::SetText((Control 103), 0xC, [IntPtr]::Zero, 'invalid') | Out-Null
        foreach ($button in @(210, 112)) {
            Select-Page $(if ($button -eq 210) { 0 } else { 1 })
            Send-Control $button 0xF5 | Out-Null
            if ((Read-Control 109) -ne 'Use HH:MM for times.') { throw 'Preview did not reject the invalid draft quiet time.' }
        }
        Select-Page 2
        [CivilizedVerify]::SetText((Control 103), 0xC, [IntPtr]::Zero, '00:00') | Out-Null
        if ($Audible) {
            Select-Page 1
            Send-Control 106 0x14E 0 | Out-Null
            Send-Control 105 0x405 1 1 | Out-Null
            Set-Silent-Sound $SilentSoundSeconds
            if ($SilentSoundSeconds -gt 0) {
                Snapshot 'controls-preview-silent-sound-draft' | Out-Null
                foreach ($button in @(210, 112)) {
                    Select-Page $(if ($button -eq 210) { 0 } else { 1 })
                    Send-Control $button 0xF5 | Out-Null
                    $expected = if ($button -eq 210) { 'Voice preview finished.' } else { 'Preview finished.' }
                    $deadline = [DateTime]::UtcNow.AddSeconds(60)
                    while ((Read-Control 109) -ne $expected) {
                        if ([DateTime]::UtcNow -ge $deadline) { throw "Local silent sound preview did not finish. $(Read-Control 109)" }
                        Start-Sleep -Milliseconds 50
                    }
                    Snapshot "controls-local-silent-sound-preview-$button" | Out-Null
                    if ((Get-Content $settingsPath -Raw) -ne $saved) { throw 'Local preview saved unapplied settings.' }
                }
            }
            Select-Page 3
            [CivilizedVerify]::SetText((Control 114), 0xC, [IntPtr]::Zero, 'character-ui-test-key') | Out-Null
            Send-Control 113 0x14E 1 | Out-Null
            [CivilizedVerify]::SetText((Control 121), 0xC, [IntPtr]::Zero, 'draft-default-voice') | Out-Null
            Select-Page 0
            [CivilizedVerify]::SetText((Control 216), 0xC, [IntPtr]::Zero, 'draft-character-voice') | Out-Null
            foreach ($voice in @('draft-character-voice', 'draft-default-voice')) {
                if ($voice -eq 'draft-default-voice') {
                    Select-Page 0
                    [CivilizedVerify]::SetText((Control 216), 0xC, [IntPtr]::Zero, '') | Out-Null
                }
                foreach ($button in @(210, 112)) {
                    Select-Page $(if ($button -eq 210) { 0 } else { 1 })
                    $count = if (Test-Path $requestsFile) { @(Get-Content $requestsFile).Count } else { 0 }
                    Send-Control $button 0xF5 | Out-Null
                    $expected = if ($button -eq 210) { 'Voice preview finished.' } else { 'Preview finished.' }
                    $deadline = [DateTime]::UtcNow.AddSeconds(30 + $SilentSoundSeconds)
                    while ((Read-Control 109) -ne $expected) {
                        if ([DateTime]::UtcNow -ge $deadline) { throw "Preview did not finish. $(Read-Control 109)" }
                        Start-Sleep -Milliseconds 50
                    }
                    $requests = @(Get-Content $requestsFile | Select-Object -Skip $count | ForEach-Object { $_ | ConvertFrom-Json })
                    if ($requests.Count -ne 1 -or $requests[0].url -ne '/v1/text-to-dialogue?output_format=pcm_16000' -or $requests[0].body.model_id -ne 'eleven_v4' -or $requests[0].body.inputs[0].voice_id -ne $voice) { throw 'Preview ignored the draft model or voice.' }
                    Write-Output "Preview control $button finished with draft V4 and $voice."
                    if ($SilentSoundSeconds -gt 0) { Snapshot "controls-remote-silent-sound-preview-$button-$voice" | Out-Null }
                    if ((Get-Content $settingsPath -Raw) -ne $saved) { throw 'Remote preview saved unapplied settings.' }
                }
            }
        }
        if ((Get-Content $settingsPath -Raw) -ne $saved) { throw 'Voice preview saved unapplied settings.' }
        if ($Audible) {
            Select-Page 0
            [CivilizedVerify]::SetText((Control 216), 0xC, [IntPtr]::Zero, 'draft-character-voice') | Out-Null
            Send-Control 107 0xF5 | Out-Null
            $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
            if ($settings.silentSoundSeconds -ne $SilentSoundSeconds) { throw 'Apply did not persist the previewed silent sound seconds.' }
            if (-not $settings.selectedCharacter -or $settings.characters.($settings.selectedCharacter).voice.voiceId -ne 'draft-character-voice' -or $settings.defaultVoiceId -ne 'draft-default-voice' -or $settings.speechModel -ne 'eleven_v4' -or $settings.volume -ne 1 -or $settings.scheduleEnabled -or $settings.quietMode -or $null -ne $settings.outputDevice) { throw 'Apply did not persist the previewed character and audio settings.' }
            Close-Settings
            Launch
            Select-Page 0
            if ((Read-Control 216) -ne 'draft-character-voice') { throw 'The previewed character voice did not survive Apply and reopening.' }
            Write-Output 'Apply persisted the previewed character and audio settings; reopening restored the character voice.'
        }
    } elseif ($Feature -eq 'Preview') {
        $saved = Get-Content $settingsPath -Raw
        Write-Output 'At zero volume, click Play example.'
        Send-Control 112 0xF5 | Out-Null
        if ((Read-Control 109) -ne 'Preview is silent at 0% volume.') { throw 'Zero volume preview was not silent' }
        Snapshot 'controls-silent' | Out-Null
        if ($Audible) {
            Write-Output 'Choose System default and 35% volume; Play example; Stop example.'
            Select-Page 2
            Send-Control 102 0xF5 | Out-Null
            Select-Page 1
            Send-Control 106 0x14E 0 | Out-Null
            Send-Control 105 0x405 1 35 | Out-Null
            Send-Control 112 0xF5 | Out-Null
            if ((Read-Control 112) -ne 'Stop example') { throw 'Preview did not start' }
            Snapshot 'controls-playing' | Out-Null
            Send-Control 112 0xF5 | Out-Null
            if ((Read-Control 109) -ne 'Preview stopped.') { throw 'Preview did not stop' }
        }
        if ((Get-Content $settingsPath -Raw) -ne $saved) { throw 'Preview saved unapplied settings' }
    } elseif ($Feature -eq 'Announcements') {
        $customPrompt = [string]::Join("`n", @('Report the task outcome.', 'Be explicit.', 'Be concise.'))
        $longPrompt = 'Long prompt ' + ('x' * 700) + "`nReport the task outcome.`nBe concise."
        Write-Output 'Select Announcements; reset the default prompt; close without Apply; confirm the reset was discarded.'
        Select-Page 5
        Assert-Announcements-Page
        $resetBytes = Get-Content $settingsPath -Raw
        Send-Control 135 0xF5 | Out-Null
        $defaultPrompt = Read-Control 134
        if ([string]::IsNullOrWhiteSpace($defaultPrompt) -or $defaultPrompt -match '\{\{status\}\}|\{\{report\}\}') { throw 'Reset did not populate the plain editable default prompt' }
        Snapshot 'controls-reset' | Out-Null
        if ((Get-Content $settingsPath -Raw) -ne $resetBytes) { throw 'Reset changed the saved settings before Apply' }
        Set-Control-Text 134 $customPrompt
        Close-Settings
        if ((Get-Content $settingsPath -Raw) -ne $resetBytes) { throw 'Close saved the reset prompt without Apply' }
        Launch
        Select-Page 5
        Assert-Announcements-Page
        Write-Output 'Choose installed Consolas and Segoe UI fonts; set sizes 22 and 12; enter the custom LF prompt.'
        Select-Combo-Text 130 'Consolas'
        Set-Announcement-Size 131 22
        Select-Combo-Text 132 'Segoe UI'
        Set-Announcement-Size 133 12
        Assert-Installed-Announcement-Fonts
        Set-Control-Text 134 $longPrompt
        $longRead = Read-Control 134
        if ($longRead.Length -le 512 -or $longRead -cne $longPrompt) { throw 'Dynamic Read-Control did not return the complete prompt longer than 512 characters' }
        Assert-Exact-Lf 'Long prompt' $longRead
        Snapshot 'controls-long-prompt' | Out-Null
        Write-Output 'Insert a 16384-character prompt with emoji through the edit control and Apply.'
        $unicodePrompt = [char]::ConvertFromUtf32(0x1f600) * 16384
        Set-Control-Text 134 ''
        [CivilizedVerify]::SetText((Control 134), 0xC2, [IntPtr]1, $unicodePrompt) | Out-Null
        if ((Read-Control 134) -cne $unicodePrompt) { throw 'The editor truncated a valid Unicode prompt at the character limit' }
        Send-Control 107 0xF5 | Out-Null
        if ((Get-Content $settingsPath -Raw | ConvertFrom-Json).summaryPrompt -cne $unicodePrompt) { throw 'The maximum-length Unicode prompt was not saved' }
        $unicodeSaved = Get-Content $settingsPath -Raw
        Set-Control-Text 134 ($unicodePrompt + 'x')
        Apply-Invalid-Announcement-Draft $unicodeSaved '(?i)(16384|characters)' 'Oversized Unicode prompt'
        Set-Control-Text 134 $customPrompt
        Assert-Exact-Lf 'Custom prompt' (Read-Control 134)
        if ((Send-Control 134 0xBA) -ne 3) { throw 'The multiline prompt did not display its three separate lines' }
        $beforeInvalid = Get-Content $settingsPath -Raw
        Write-Output 'Reject an invalid body size without saving or showing a modal dialog.'
        Set-Invalid-Announcement-Size 131
        Apply-Invalid-Announcement-Draft $beforeInvalid '(?i)(size|font)' 'Invalid font size'
        Set-Announcement-Size 131 22
        Write-Output 'Reject an empty summary prompt without saving or showing a modal dialog.'
        Set-Control-Text 134 ''
        Apply-Invalid-Announcement-Draft $beforeInvalid '(?i)(empty|prompt)' 'Empty prompt'
        Set-Control-Text 134 $customPrompt
        Assert-Exact-Lf 'Custom prompt' (Read-Control 134)
        Snapshot 'controls-draft' | Out-Null
        Send-Control 107 0xF5 | Out-Null
        $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
        if ((Read-Control 109) -ne 'Saved. Changes apply to the next announcement.') { throw 'Announcement Apply did not report saved state' }
        if ($settings.announcementBodyFont.family -cne 'Consolas' -or [int]$settings.announcementBodyFont.size -ne 22 -or $settings.announcementTitleFont.family -cne 'Segoe UI' -or [int]$settings.announcementTitleFont.size -ne 12 -or $settings.summaryPrompt -cne $customPrompt) { throw 'Applied announcement settings did not persist' }
        Assert-Exact-Lf 'Persisted summary prompt' ([string]$settings.summaryPrompt)
        Assert-Preserved-Preferences $beforeSettings $settings
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
    if ($Feature -eq 'Announcements') {
        Write-Output 'Change body and title sizes and the prompt without Apply; Close; confirm disk unchanged.'
        Select-Page 5
        Assert-Announcements-Page
        Set-Announcement-Size 131 18
        Set-Announcement-Size 133 14
        $unappliedPrompt = [string]::Join("`n", @('Unapplied first line.', 'Summarize the task.', 'Be brief.'))
        Set-Control-Text 134 $unappliedPrompt
        Assert-Exact-Lf 'Unapplied prompt' (Read-Control 134)
        Snapshot 'controls-unapplied' | Out-Null
        Close-Settings
        if ((Get-Content $settingsPath -Raw) -ne $applied) { throw 'Close saved unapplied announcement edits' }
        Write-Output 'Reopen settings; select Announcements and verify the applied values.'
        Launch
        Select-Page 5
        Assert-Announcements-Page
        Assert-Installed-Announcement-Fonts
        $reopened = Snapshot 'controls-reopened'
        $persisted = Get-Content $settingsPath -Raw | ConvertFrom-Json
        if ($reopened.announcementBodyFont.family -cne 'Consolas' -or $reopened.announcementBodyFont.size -ne 22 -or $reopened.announcementTitleFont.family -cne 'Segoe UI' -or $reopened.announcementTitleFont.size -ne 12 -or $reopened.summaryPrompt -cne $customPrompt) { throw 'Reopening did not restore the applied announcement settings' }
        if ($persisted.summaryPrompt -cne $customPrompt) { throw 'Reopening did not preserve the saved summary prompt' }
        Assert-Exact-Lf 'Reopened prompt' $reopened.summaryPrompt
    } else {
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
    }
    Close-Settings
    @{ passed = $true; feature = $Feature; audible = [bool]$Audible; exitCode = $process.ExitCode } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'result.json')
    Write-Output "PASS: $Feature; evidence: $evidencePath"
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
    if (Test-Path $evidencePath) { @{ scratchRemoved = -not (Test-Path $scratch); processExited = (-not $process -or $process.HasExited); serverExited = (-not $server -or $server.HasExited) } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'cleanup.json') }
    if ($transcribing) { Stop-Transcript | Out-Null }
}
