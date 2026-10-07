param(
    [string]$Binary = (Join-Path (Split-Path $PSScriptRoot -Parent) 'native-announcer/target/debug/civilized-announcer.exe'),
    [string]$Evidence = (Join-Path (Split-Path $PSScriptRoot -Parent) ('temp/verification/characters-' + [guid]::NewGuid())),
    [switch]$SettingsOnly,
    [switch]$VideoPickerOnly,
    [switch]$CharacterPromptsOnly
)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CivilizedCharacterTest {
    public struct Rect { public int Left, Top, Right, Bottom; }
    public delegate bool ChildCallback(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr window, ChildCallback callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumWindows(ChildCallback callback, IntPtr parameter);
    [DllImport("user32.dll", EntryPoint = "GetClassNameW", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr window, StringBuilder name, int length);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongW")] public static extern int GetWindowLong(IntPtr window, int index);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll", EntryPoint = "FindWindowW", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string className, string title);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll", EntryPoint = "PostMessageW")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)] public static extern IntPtr SetText(IntPtr window, uint message, IntPtr wparam, string text);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)] public static extern IntPtr ReadText(IntPtr window, uint message, IntPtr length, StringBuilder text);
    public static IntPtr FindOwnedDialog(uint process, string title) {
        var result = IntPtr.Zero;
        EnumWindows((window, parameter) => {
            GetWindowThreadProcessId(window, out var owner);
            if (owner != process) return true;
            var text = new StringBuilder(256);
            ReadText(window, 0xD, new IntPtr(text.Capacity), text);
            if (text.ToString() == title) { result = window; return false; }
            return true;
        }, IntPtr.Zero);
        return result;
    }
    public static IntPtr FindClass(IntPtr parent, string className) {
        var result = IntPtr.Zero;
        EnumChildWindows(parent, (window, parameter) => {
            var name = new StringBuilder(128);
            GetClassName(window, name, name.Capacity);
            if (name.ToString() == className) { result = window; return false; }
            return true;
        }, IntPtr.Zero);
        return result;
    }
    public static string Describe(IntPtr parent) {
        var result = new StringBuilder();
        EnumChildWindows(parent, (window, parameter) => {
            var name = new StringBuilder(128);
            var text = new StringBuilder(4096);
            GetClassName(window, name, name.Capacity);
            ReadText(window, 0xD, new IntPtr(text.Capacity), text);
            result.AppendLine($"{GetDlgCtrlID(window)} {name} {text}");
            return true;
        }, IntPtr.Zero);
        return result.ToString();
    }
    public static string PickerFolder(IntPtr parent) {
        var result = "";
        EnumChildWindows(parent, (window, parameter) => {
            var text = new StringBuilder(32768);
            ReadText(window, 0xD, new IntPtr(text.Capacity), text);
            if (text.ToString().StartsWith("Address: ")) { result = text.ToString().Substring(9); return false; }
            return true;
        }, IntPtr.Zero);
        return result;
    }
}
'@
$root = Split-Path $PSScriptRoot -Parent
if (Test-Path $Evidence) { throw 'Choose a new evidence directory.' }
New-Item -ItemType Directory -Path $Evidence -Force | Out-Null
$binaryHash = (Get-FileHash $Binary -Algorithm SHA256).Hash
$temporary = Join-Path $env:LOCALAPPDATA ('Temp/opencode/characters-test-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temporary | Out-Null
$data = Join-Path $temporary 'data'
New-Item -ItemType Directory -Path $data | Out-Null
$addressFile = Join-Path $temporary 'address.txt'
$requestsFile = Join-Path $temporary 'requests.jsonl'
$settingsPath = Join-Path $data 'settings.json'
$video = Join-Path $temporary 'test-character.mp4'
$configuredDefault = 'configured-default'
$manualVoiceId = 'own-voice_123'
$process = $null
$server = $null
$playback = $null
function Wait-Until {
    param([scriptblock]$Condition, [string]$Failure, [int]$Seconds = 15)
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while (-not (& $Condition)) {
        if ([DateTime]::UtcNow -ge $deadline) { throw $Failure }
        Start-Sleep -Milliseconds 50
    }
}
function Get-Control {
    param([int]$Id)
    $control = [CivilizedCharacterTest]::GetDlgItem($process.MainWindowHandle, $Id)
    if ($control -eq [IntPtr]::Zero) { throw "Missing character control $Id" }
    return $control
}
function Send-Control {
    param([int]$Id, [uint32]$Message, [long]$Wparam = 0, [long]$Lparam = 0)
    return [CivilizedCharacterTest]::SendMessage((Get-Control $Id), $Message, [IntPtr]$Wparam, [IntPtr]$Lparam).ToInt64()
}
function Set-Control {
    param([int]$Id, [string]$Value)
    if ($Id -eq 218) { $Value = $Value.Replace("`r`n", "`n").Replace("`n", "`r`n") }
    if ($Id -eq 216) {
        Send-Control $Id 0xB1 0 -1 | Out-Null
        [CivilizedCharacterTest]::SetText((Get-Control $Id), 0xC2, [IntPtr]1, $Value) | Out-Null
    } else {
        [CivilizedCharacterTest]::SetText((Get-Control $Id), 0xC, [IntPtr]::Zero, $Value) | Out-Null
    }
    Add-Content (Join-Path $Evidence 'actions.txt') "Set control $Id to $Value"
}
function Read-Control {
    param([int]$Id)
    $buffer = [Text.StringBuilder]::new(4096)
    [CivilizedCharacterTest]::ReadText((Get-Control $Id), 0xD, [IntPtr]4096, $buffer) | Out-Null
    if ($Id -eq 218) { return $buffer.ToString().Replace("`r`n", "`n") }
    return $buffer.ToString()
}
function Open-Settings {
    $script:process = [Diagnostics.Process]::Start($script:settingsInfo)
    Wait-Until { $process.Refresh(); $process.HasExited -or $process.MainWindowHandle -ne [IntPtr]::Zero } 'Settings window did not open.'
    if ($process.HasExited) { throw "Settings exited with $($process.ExitCode)." }
    if ((Get-FileHash $Binary -Algorithm SHA256).Hash -ne $binaryHash) { throw 'The settings binary changed during verification.' }
    if ((Read-Control 107) -ne 'Apply' -or (Read-Control 108) -ne 'Close') { throw 'Settings readiness controls are missing.' }
    foreach ($obsolete in @(204, 311)) {
        if ([CivilizedCharacterTest]::GetDlgItem($process.MainWindowHandle, $obsolete) -ne [IntPtr]::Zero) { throw "Removed description control $obsolete remains." }
    }
    @{ pid = $process.Id; started = $process.StartTime.ToUniversalTime().ToString('o'); binary = $settingsInfo.FileName; sha256 = $binaryHash; scratch = $temporary } | ConvertTo-Json | Set-Content (Join-Path $Evidence 'instance.json')
    Add-Content (Join-Path $Evidence 'actions.txt') 'Opened the owned settings window and checked its binary and controls.'
}
function Select-SettingsPage([int]$Index) {
    Send-Control 200 0x100 0x24 | Out-Null
    Send-Control 200 0x101 0x24 | Out-Null
    for ($step = 0; $step -lt $Index; $step++) {
        Send-Control 200 0x100 0x28 | Out-Null
        Send-Control 200 0x101 0x28 | Out-Null
    }
    if ((Send-Control 200 0x188) -ne $Index) { throw 'The settings sidebar did not select the requested page.' }
    Add-Content (Join-Path $Evidence 'actions.txt') "Selected settings page $Index through the native sidebar."
}
function Select-CharactersPage {
    Select-SettingsPage 0
    Wait-Until { [CivilizedCharacterTest]::IsWindowVisible((Get-Control 201)) } 'Characters page did not show its list.'
}
function Pick-Animation {
    param([string]$ExpectedFolder)
    Add-Content (Join-Path $Evidence 'actions.txt') "Opening picker for $(Read-Control 206), visible $([CivilizedCharacterTest]::IsWindowVisible((Get-Control 206))), enabled $([CivilizedCharacterTest]::IsWindowEnabled((Get-Control 206)))"
    [CivilizedCharacterTest]::PostMessage((Get-Control 206), 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    $script:picker = [IntPtr]::Zero
    Wait-Until {
        $candidate = [CivilizedCharacterTest]::FindOwnedDialog([uint32]$process.Id, 'Choose character animation')
        $owner = [uint32]0
        if ($candidate -ne [IntPtr]::Zero) { [CivilizedCharacterTest]::GetWindowThreadProcessId($candidate, [ref]$owner) | Out-Null }
        if ($owner -eq $process.Id) { $script:picker = $candidate; return $true }
        return $false
    } 'The native animation file picker did not open.'
    Wait-Until { [CivilizedCharacterTest]::FindClass($picker, 'SysTreeView32') -ne [IntPtr]::Zero } 'The file picker did not finish loading its navigation pane.'
    [CivilizedCharacterTest]::Describe($picker) | Set-Content (Join-Path $Evidence 'picker-initial-controls.txt')
    if ($ExpectedFolder) {
        $script:folder = ''
        Wait-Until {
            $script:folder = [CivilizedCharacterTest]::PickerFolder($picker)
            return $script:folder.Length -gt 0 -and [IO.Path]::GetFullPath($script:folder) -eq [IO.Path]::GetFullPath($ExpectedFolder)
        } "The video picker did not navigate to '$ExpectedFolder'."
        if ([IO.Path]::GetFullPath($folder) -ne [IO.Path]::GetFullPath($ExpectedFolder)) { throw "The video picker opened at '$folder' instead of '$ExpectedFolder'." }
        Add-Content (Join-Path $Evidence 'actions.txt') "Video picker opened at $folder."
    }
    $filename = [CivilizedCharacterTest]::GetDlgItem($picker, 1148)
    if ($filename -eq [IntPtr]::Zero) { $filename = [CivilizedCharacterTest]::GetDlgItem($picker, 1152) }
    if ($filename -eq [IntPtr]::Zero) { throw 'The animation picker has no filename control.' }
    $edit = [CivilizedCharacterTest]::FindClass($filename, 'Edit')
    if ($edit -ne [IntPtr]::Zero) { $filename = $edit }
    [CivilizedCharacterTest]::SetText($filename, 0xC, [IntPtr]::Zero, $video) | Out-Null
    $typed = [Text.StringBuilder]::new(4096)
    [CivilizedCharacterTest]::ReadText($filename, 0xD, [IntPtr]4096, $typed) | Out-Null
    if ($typed.ToString() -ne $video) { throw "The file picker did not accept the filename. It contains '$typed'." }
    [CivilizedCharacterTest]::Describe($picker) | Set-Content (Join-Path $Evidence 'picker-controls.txt')
    $open = [CivilizedCharacterTest]::GetDlgItem($picker, 1)
    if ($open -eq [IntPtr]::Zero) { throw 'The animation picker has no Open button.' }
    [CivilizedCharacterTest]::SetForegroundWindow($picker) | Out-Null
    [CivilizedCharacterTest]::SendMessage($open, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Wait-Until { (Read-Control 206) -eq $video } 'The file picker did not select the animation.'
    Add-Content (Join-Path $Evidence 'actions.txt') 'Selected the animation through the native Browse dialog.'
}
function Delete-Character {
    param([bool]$Confirm = $true)
    [CivilizedCharacterTest]::PostMessage((Get-Control 214), 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    $script:confirmation = [IntPtr]::Zero
    Wait-Until {
        $candidate = [CivilizedCharacterTest]::FindWindow('#32770', 'Delete character')
        $owner = [uint32]0
        if ($candidate -ne [IntPtr]::Zero) { [CivilizedCharacterTest]::GetWindowThreadProcessId($candidate, [ref]$owner) | Out-Null }
        if ($owner -eq $process.Id) { $script:confirmation = $candidate; return $true }
        return $false
    } 'Delete did not show its confirmation dialog.'
    $default = [CivilizedCharacterTest]::SendMessage($confirmation, 0x400, [IntPtr]::Zero, [IntPtr]::Zero).ToInt64() -band 0xFFFF
    if ($default -ne 7) { throw 'Delete confirmation must default to No.' }
    $button = [CivilizedCharacterTest]::GetDlgItem($confirmation, $(if ($Confirm) { 6 } else { 7 }))
    [CivilizedCharacterTest]::SendMessage($button, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Wait-Until { -not [CivilizedCharacterTest]::IsWindowVisible($confirmation) } 'Delete confirmation did not close.'
}
try {
    & ffmpeg -hide_banner -loglevel error -f lavfi -i 'color=c=green:s=256x256:r=16:d=4' -an -c:v libx264 -pix_fmt yuv420p $video
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the test animation.' }
    $serverInfo = [Diagnostics.ProcessStartInfo]::new('node')
    $serverInfo.UseShellExecute = $false
    $serverInfo.ArgumentList.Add((Join-Path $PSScriptRoot 'voice-api-fixture.mjs'))
    $serverInfo.ArgumentList.Add($addressFile)
    $serverInfo.ArgumentList.Add($requestsFile)
    $server = [Diagnostics.Process]::Start($serverInfo)
    Wait-Until { Test-Path $addressFile } 'Voice API fixture did not start.'
    $baseUrl = Get-Content $addressFile -Raw
    $legacySettings = '{"quietMode":false,"scheduleEnabled":false,"volume":35,"voices":{"claude":"Mark"},"characters":{"hatted-herald-01":{"name":"Crimson Court Herald","voiceDescription":"Legacy description retained until Apply.","animationPath":"videos/hatted-herald-01.mp4"}}}'
    Set-Content $settingsPath $legacySettings -Encoding utf8NoBOM
    $startupBytes = [IO.File]::ReadAllBytes($settingsPath)
    $settingsInfo = [Diagnostics.ProcessStartInfo]::new([IO.Path]::GetFullPath($Binary))
    $settingsInfo.UseShellExecute = $false
    $settingsInfo.Environment['CIVILIZED_AGENT_DATA'] = $data
    $settingsInfo.Environment['CIVILIZED_AGENT_TTS'] = Join-Path $root 'native-announcer/resources/tts/kitten-nano-en-v0_8-int8'
    $settingsInfo.Environment['ELEVENLABS_API_KEY'] = 'wrong-environment-key'
    $settingsInfo.Environment['ELEVENLABS_API_BASE_URL'] = $baseUrl
    $settingsInfo.ArgumentList.Add('--settings')
    $settingsInfo.ArgumentList.Add('--assets')
    $settingsInfo.ArgumentList.Add((Join-Path $root 'native-announcer/resources'))
    Open-Settings
    if ($CharacterPromptsOnly) {
        Select-CharactersPage
        $firstPrompt = "Speak as a herald.`nReport the result explicitly and briefly."
        $secondPrompt = "Speak as a robot.`nReport the result in one sentence."
        Send-Control 201 0x186 0 | Out-Null
        [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
        if (-not [CivilizedCharacterTest]::IsWindowVisible((Get-Control 218))) { throw 'The character prompt editor is not visible.' }
        $promptClass = [Text.StringBuilder]::new(64)
        [CivilizedCharacterTest]::GetClassName((Get-Control 218), $promptClass, $promptClass.Capacity) | Out-Null
        if ($promptClass.ToString() -ne 'Edit') { throw 'The character prompt control is not an editable text box.' }
        Set-Control 218 $firstPrompt
        Send-Control 201 0x186 1 | Out-Null
        [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
        Set-Control 218 $secondPrompt
        Send-Control 201 0x186 0 | Out-Null
        [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
        if ((Read-Control 218) -cne $firstPrompt) { throw 'Switching characters lost the first prompt draft.' }
        Send-Control 107 0xF5 | Out-Null
        $saved = Get-Content $settingsPath -Raw | ConvertFrom-Json
        $firstId = $saved.selectedCharacter
        if ($saved.characters.$firstId.summaryPrompt -cne $firstPrompt) { throw 'Apply did not save the first character prompt.' }
        Send-Control 201 0x186 1 | Out-Null
        [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
        if ((Read-Control 218) -cne $secondPrompt) { throw 'Switching characters lost the second prompt draft.' }
        Send-Control 107 0xF5 | Out-Null
        $saved = Get-Content $settingsPath -Raw | ConvertFrom-Json
        $secondId = $saved.selectedCharacter
        if ($firstId -eq $secondId -or $saved.characters.$secondId.summaryPrompt -cne $secondPrompt) { throw 'Apply did not save separate character prompts.' }
        Copy-Item $settingsPath (Join-Path $Evidence 'settings-after.json')
        $applied = Get-Content $settingsPath -Raw
        Set-Control 218 'Discard this prompt.'
        Send-Control 108 0xF5 | Out-Null
        if (-not $process.WaitForExit(5000)) { throw 'Character prompt settings did not close.' }
        if ((Get-Content $settingsPath -Raw) -cne $applied) { throw 'Close saved an unapplied character prompt.' }
        Open-Settings
        Select-CharactersPage
        if ((Read-Control 218) -cne $secondPrompt) { throw 'Reopening did not restore the second character prompt.' }
        Send-Control 201 0x186 0 | Out-Null
        [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
        if ((Read-Control 218) -cne $firstPrompt) { throw 'Reopening did not restore the first character prompt.' }
        Send-Control 108 0xF5 | Out-Null
        if (-not $process.WaitForExit(5000)) { throw 'Reopened character prompt settings did not close.' }
        @{ passed = $true; characterPromptsOnly = $true; firstCharacter = $firstId; secondCharacter = $secondId } | ConvertTo-Json | Set-Content (Join-Path $Evidence 'result.json')
        Write-Output 'PASS: separate character prompts, draft switching, Apply, discard and reopen.'
        return
    }
    if ($VideoPickerOnly) {
        Select-CharactersPage
        Send-Control 201 0x186 0 | Out-Null
        [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
        Pick-Animation -ExpectedFolder (Split-Path (Read-Control 206) -Parent)
        Pick-Animation -ExpectedFolder (Split-Path $video -Parent)
        Send-Control 108 0xF5 | Out-Null
        if (-not $process.WaitForExit(5000)) { throw 'Video picker settings did not close.' }
        @{ passed = $true; videoPickerOnly = $true } | ConvertTo-Json | Set-Content (Join-Path $Evidence 'result.json')
        Write-Output 'PASS: video picker starts in the current bundled and custom video folders.'
        return
    }
    if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($settingsPath)) -cne [Convert]::ToBase64String($startupBytes)) { throw 'Opening Settings rewrote the legacy settings file.' }
    Select-SettingsPage 3
    if ((Read-Control 118) -notmatch 'key') { throw 'Voice limits must explain that an ElevenLabs key is required.' }
    if ((Read-Control 120) -ne 'Open My Voices' -or -not [CivilizedCharacterTest]::IsWindowVisible((Get-Control 120)) -or -not [CivilizedCharacterTest]::IsWindowEnabled((Get-Control 120))) { throw 'Speech service must offer My Voices without requiring an API key.' }
    Set-Control 114 'character-ui-test-key'
    Send-Control 119 0xF5 | Out-Null
    Wait-Until { (Read-Control 118) -match '8 (left|remaining)' } "Voice limits did not show the account's eight available slots. $(Read-Control 118)"
    if ((Read-Control 118) -notmatch '2.*10' -or (Read-Control 118) -notmatch '62 (left|remaining)') { throw 'Voice limits must distinguish remaining slots from remaining additions or edits.' }
    @{ usage = (Read-Control 118) } | ConvertTo-Json | Set-Content (Join-Path $Evidence 'voice-usage-before.json')
    Set-Control 114 'invalid-fixture-key'
    Send-Control 119 0xF5 | Out-Null
    Wait-Until { (Read-Control 118) -match '401|unavailable|denied' } "An invalid key did not show unavailable limits. $(Read-Control 118)"
    if ((Read-Control 118) -match '8 (left|remaining)|invalid-fixture-key|character-ui-test-key') { throw 'A changed key must not retain old account counts or expose the key.' }
    Set-Control 114 ''
    if ((Read-Control 118) -notmatch 'key') { throw 'Clearing the key must clear the previous account usage.' }
    Set-Control 114 'missing-permission-test-key'
    Send-Control 119 0xF5 | Out-Null
    Wait-Until { (Read-Control 118) -match 'User read|user_read|permission' } "Account access errors must explain the required read permission. $(Read-Control 118)"
    Set-Control 114 'character-ui-test-key'
    Send-Control 119 0xF5 | Out-Null
    Wait-Until { (Read-Control 118) -match '8 (left|remaining)' } 'Refreshing voice limits did not recover after correcting the API key.'
    Set-Control 114 'unknown-limit-test-key'
    Send-Control 119 0xF5 | Out-Null
    Wait-Until { (Read-Control 118) -match 'unknown' } "A missing account operation limit must be shown as unknown. $(Read-Control 118)"
    if ((Read-Control 118) -match 'unlimited') { throw 'A missing limit must not be interpreted as unlimited.' }
    Set-Control 114 'slow-usage-test-key'
    Send-Control 119 0xF5 | Out-Null
    Wait-Until { (Get-Content $requestsFile -Raw) -match '"usageAccount":"slow"' } 'The delayed account request did not start.'
    Set-Control 114 'character-ui-test-key'
    Send-Control 119 0xF5 | Out-Null
    Wait-Until { (Read-Control 118) -match '8 (left|remaining)' } 'The new account request did not replace the delayed one.'
    Wait-Until { (Get-Content $requestsFile -Raw) -match '"event":"usage-response"' } 'The superseded account response did not finish.'
    Start-Sleep -Milliseconds 250
    if ((Read-Control 118) -notmatch '8 (left|remaining)') { throw 'A response for an old key overwrote the current account usage.' }
    Set-Control 121 $configuredDefault
    $beforeDefaultPreview = Get-Content $settingsPath -Raw
    Select-SettingsPage 1
    Send-Control 112 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Preview finished.' } "Audio preview with the configured default did not finish. $(Read-Control 109)"
    Wait-Until { (Get-Content $requestsFile -Raw) -match [regex]::Escape('/v1/text-to-speech/configured-default?output_format=pcm_16000') } 'Audio preview did not request the configured default voice ID.'
    if ((Get-Content $settingsPath -Raw) -ne $beforeDefaultPreview) { throw 'Audio preview saved the unapplied default voice draft.' }
    Select-SettingsPage 3
    if ([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'The Speech service page must not show character fields.' }
    Select-CharactersPage
    $assets = Join-Path $root 'native-announcer/resources'
    Add-Content (Join-Path $Evidence 'actions.txt') "Bundled count $(Send-Control 201 0x18B), editor visible $([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203)))"
    if ([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'The preinstalled character catalog selected a character unexpectedly.' }
    Send-Control 201 0x186 0 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    if ((Read-Control 216) -ne '') { throw 'The bundled character did not display a blank voice ID.' }
    $sample = 'I bring news for your attention. Listen as I deliver this announcement. Your work is ready, and every check has passed.'
    $beforeBundledRequests = @(Get-Content $requestsFile).Count
    Send-Control 210 0xF5 | Out-Null
    if (-not [CivilizedCharacterTest]::IsWindowEnabled((Get-Control 216))) { throw 'Voice ID editing must remain enabled during preview.' }
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "Bundled voice example did not finish. $(Read-Control 109)"
    if ((Read-Control 216) -ne '') { throw 'Default voice preview changed the blank character voice ID.' }
    $bundledRequests = @(Get-Content $requestsFile | Select-Object -Skip $beforeBundledRequests | ForEach-Object { $_ | ConvertFrom-Json })
    if ($bundledRequests.Count -ne 1 -or $bundledRequests[0].url -cne '/v1/text-to-speech/configured-default?output_format=pcm_16000' -or $bundledRequests[0].body.text -cne $sample) { throw 'A blank bundled voice ID must preview the configured default without creating a voice.' }
    if (([CivilizedCharacterTest]::GetWindowLong((Get-Control 216), -16) -band 0x800) -ne 0) { throw 'The voice ID field must be editable.' }
    if (-not [CivilizedCharacterTest]::IsWindowEnabled((Get-Control 216))) { throw 'Voice ID editing must be enabled after preview.' }
    $idBounds = [CivilizedCharacterTest+Rect]::new()
    $actionBounds = [CivilizedCharacterTest+Rect]::new()
    [CivilizedCharacterTest]::GetWindowRect((Get-Control 216), [ref]$idBounds) | Out-Null
    [CivilizedCharacterTest]::GetWindowRect((Get-Control 210), [ref]$actionBounds) | Out-Null
    if ($idBounds.Right -le $idBounds.Left -or $idBounds.Bottom -le $idBounds.Top -or $idBounds.Bottom -ge $actionBounds.Top -or $actionBounds.Top - $idBounds.Bottom -gt 32) { throw 'The voice ID and actions must have usable, compact, separate rows.' }
    @{ voiceId = (Read-Control 216); idBounds = $idBounds; actionBounds = $actionBounds } | ConvertTo-Json -Depth 3 | Set-Content (Join-Path $Evidence 'voice-id-controls.json')
    if ((Get-Content $settingsPath -Raw) -match 'installedBundledCharacters') { throw 'Previewing a bundled character saved unapplied installation state.' }
    foreach ($obsolete in @(204, 205, 207, 208, 209, 211, 212, 213, 215, 311)) {
        if ([CivilizedCharacterTest]::GetDlgItem($process.MainWindowHandle, $obsolete) -ne [IntPtr]::Zero) { throw "Obsolete character control $obsolete remains." }
    }
    if ((Read-Control 210) -ne 'Play voice example' -or (Read-Control 214) -ne 'Delete' -or (Read-Control 202) -ne 'New') { throw 'Character actions do not match the simplified interface.' }
    Send-Control 202 0xF5 | Out-Null
    if (-not [CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'New did not show the character editor.' }
    if ((Read-Control 216) -ne '') { throw 'A new character must start with a blank voice ID.' }
    Set-Control 203 'Test herald'
    Pick-Animation
    $before = Get-Content $settingsPath -Raw
    $before | Set-Content (Join-Path $Evidence 'settings-before.json')
    Send-Control 210 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "Voice example did not finish. $(Read-Control 109)"
    if ((Read-Control 216) -ne '') { throw 'Preview must not assign a voice ID to the custom character.' }
    if ((Get-Content $settingsPath -Raw) -ne $before) { throw 'Playing a voice saved unapplied settings.' }
    Send-Control 210 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "Repeated default voice example did not finish. $(Read-Control 109)"
    Select-SettingsPage 3
    Send-Control 119 0xF5 | Out-Null
    Wait-Until { (Read-Control 118) -match '8 (left|remaining)' } "Voice previews changed available voice slots. $(Read-Control 118)"
    if ((Read-Control 118) -notmatch '62 (left|remaining)') { throw 'Voice previews consumed additions or edits.' }
    @{ usage = (Read-Control 118) } | ConvertTo-Json | Set-Content (Join-Path $Evidence 'voice-usage-after.json')
    Set-Control 114 ''
    Select-CharactersPage
    Send-Control 210 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "The no-key voice example did not play with Kitten CPU. $(Read-Control 109)"
    Select-SettingsPage 3
    Set-Control 114 'character-ui-test-key'
    Select-CharactersPage
    $beforeManual = Get-Content $settingsPath -Raw
    $beforeManualRequests = @(Get-Content $requestsFile).Count
    Set-Control 216 "  $manualVoiceId  "
    if ((Read-Control 216) -cne "  $manualVoiceId  " -or (Send-Control 216 0xB0) -ne (("  $manualVoiceId  ".Length -shl 16) -bor "  $manualVoiceId  ".Length)) { throw 'Editing the voice ID rewrote its text or moved its caret.' }
    Set-Control 203 'Renamed herald'
    Pick-Animation
    if ((Read-Control 216).Trim() -cne $manualVoiceId) { throw 'Name or video editing lost the manual voice ID.' }
    Set-Control 203 'Test herald'
    $draftIndex = Send-Control 201 0x188
    Send-Control 201 0x186 ($draftIndex + 1) | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    Send-Control 201 0x186 $draftIndex | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    if ((Read-Control 216) -cne $manualVoiceId) { throw 'Character switching lost the manual voice ID.' }
    Select-SettingsPage 1
    Select-CharactersPage
    if ((Read-Control 216).Trim() -cne $manualVoiceId) { throw 'Page navigation lost the manual voice ID draft.' }
    Send-Control 210 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "Manual voice preview did not finish. $(Read-Control 109)"
    $manualRequests = @(Get-Content $requestsFile | Select-Object -Skip $beforeManualRequests | ForEach-Object { $_ | ConvertFrom-Json })
    if ($manualRequests.Count -ne 1 -or $manualRequests[0].url -cne "/v1/text-to-speech/${manualVoiceId}?output_format=pcm_16000" -or $manualRequests[0].body.text -ne $sample) { throw 'Manual voice preview must use the exact trimmed ID without design or creation requests.' }
    if ((Get-Content $settingsPath -Raw) -ne $beforeManual) { throw 'Manual voice preview persisted its draft before Apply.' }
    if ((Read-Control 216) -cne $manualVoiceId) { throw 'Preview changed the manual voice ID.' }
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if ($settings.defaultVoiceId -ne $configuredDefault) { throw 'Apply did not persist the configured default voice ID.' }
    $characterId = $settings.selectedCharacter
    if (-not $characterId) { throw 'The chosen character was not activated.' }
    $character = $settings.characters.$characterId
    $serialized = $character | ConvertTo-Json -Depth 8 -Compress
    if ($character.name -ne 'Test herald' -or $serialized -match 'sampleText|voiceDescription' -or $character.voice.voiceId -cne $manualVoiceId -or $serialized -notmatch [regex]::Escape('test-character.mp4')) { throw "Character did not persist its name, entered voice ID, and video without legacy prose. $serialized" }
    if ((Get-Content $settingsPath -Raw) -match 'voiceDescription') { throw 'Apply did not drop legacy voice descriptions.' }
    if ($settings.voices.claude -ne 'Mark') { throw 'Applying a character lost the existing local voice setting.' }
    Send-Control 202 0xF5 | Out-Null
    Set-Control 203 'Second character'
    Set-Control 206 $video
    Send-Control 201 0x186 0 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if ($settings.selectedCharacter -ne $characterId -or $settings.defaultVoiceId -ne $configuredDefault) { throw 'Apply did not preserve the chosen character and configured default voice ID.' }
    Select-SettingsPage 1
    Send-Control 107 0xF5 | Out-Null
    if ([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'Apply on the Audio page exposed character controls.' }
    Select-CharactersPage
    $saved = Get-Content $settingsPath -Raw
    $saved | Set-Content (Join-Path $Evidence 'settings-applied.json')
    if ($saved -match 'character-ui-test-key|audio_base_64|"previews"|sampleText|voiceDescription|voice_slots_used|voice_limit|voice_add_edit_counter|max_voice_add_edits') { throw 'Settings contain plaintext credentials, transient previews, account usage, or legacy prose.' }
    Set-Control 203 'Unapplied name'
    Set-Control 216 'unapplied-voice'
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000)) { throw 'Settings did not close.' }
    if ((Get-Content $settingsPath -Raw) -ne $saved) { throw 'Close saved an unapplied character edit.' }
    Open-Settings
    Select-CharactersPage
    if ((Read-Control 203) -ne 'Test herald' -or (Read-Control 206) -ne $video) { throw 'The character did not survive reopening.' }
    if ((Read-Control 216) -cne $manualVoiceId) { throw 'The saved manual voice ID did not survive reopening or Close saved the discarded ID.' }
    @{ name = (Read-Control 203); animation = (Read-Control 206); voiceId = (Read-Control 216); selected = (Send-Control 201 0x188) } | ConvertTo-Json | Set-Content (Join-Path $Evidence 'reopened-controls.json')
    $beforeInvalid = Get-Content $settingsPath -Raw
    $beforeInvalidRequests = @(Get-Content $requestsFile).Count
    Set-Control 216 'bad/id?'
    Send-Control 210 0xF5 | Out-Null
    if ((Read-Control 109) -ne 'ElevenLabs voice ID is invalid.') { throw 'Malformed manual voice ID was not rejected before preview.' }
    [CivilizedCharacterTest]::PostMessage((Get-Control 107), 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    $script:validation = [IntPtr]::Zero
    Wait-Until {
        $candidate = [CivilizedCharacterTest]::FindWindow('#32770', 'Civilized Agent settings')
        $owner = [uint32]0
        if ($candidate -ne [IntPtr]::Zero) { [CivilizedCharacterTest]::GetWindowThreadProcessId($candidate, [ref]$owner) | Out-Null }
        if ($owner -eq $process.Id) { $script:validation = $candidate; return $true }
        return $false
    } 'Apply did not reject the malformed voice ID.'
    $validationControls = [CivilizedCharacterTest]::Describe($validation)
    $validationControls | Set-Content (Join-Path $Evidence 'invalid-voice-id-dialog.txt')
    if ($validationControls -notmatch 'ElevenLabs voice ID is invalid') { throw 'Apply showed an unexpected validation error.' }
    [CivilizedCharacterTest]::SendMessage([CivilizedCharacterTest]::GetDlgItem($validation, 2), 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Wait-Until { -not [CivilizedCharacterTest]::IsWindowVisible($validation) } 'Voice ID validation dialog did not close.'
    if ((Get-Content $settingsPath -Raw) -ne $beforeInvalid -or @(Get-Content $requestsFile).Count -ne $beforeInvalidRequests) { throw 'Malformed ID validation persisted settings or called the voice service.' }
    Set-Control 216 ''
    Send-Control 107 0xF5 | Out-Null
    $cleared = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if ($cleared.characters.$characterId.voice) { throw 'Clearing the manual voice ID did not save the default local voice.' }
    Copy-Item $settingsPath (Join-Path $Evidence 'settings-cleared.json')
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000)) { throw 'Cleared voice settings did not close.' }
    Open-Settings
    Select-CharactersPage
    if ((Read-Control 216) -ne '') { throw 'Cleared voice ID did not survive reopening.' }
    $beforeClearedPreview = Get-Content $settingsPath -Raw
    $beforeClearedRequests = @(Get-Content $requestsFile).Count
    Send-Control 210 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "Cleared voice did not preview the default. $(Read-Control 109)"
    $clearedRequests = @(Get-Content $requestsFile | Select-Object -Skip $beforeClearedRequests | ForEach-Object { $_ | ConvertFrom-Json })
    if ($clearedRequests.Count -ne 1 -or $clearedRequests[0].url -cne '/v1/text-to-speech/configured-default?output_format=pcm_16000' -or $clearedRequests[0].body.text -cne $sample -or (Read-Control 216) -ne '' -or (Get-Content $settingsPath -Raw) -ne $beforeClearedPreview) { throw 'Clearing must restore default previews without persisting or assigning a voice.' }
    Set-Control 216 $manualVoiceId
    Send-Control 107 0xF5 | Out-Null
    Send-Control 201 0x186 1 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    if ((Read-Control 203) -ne 'Second character') { throw 'The list did not select the second character.' }
    Delete-Character $false
    if ((Read-Control 203) -ne 'Second character') { throw 'Cancelling Delete changed the selected character.' }
    Delete-Character
    if ([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'Delete activated another character unexpectedly.' }
    Send-Control 201 0x186 0 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    if ((Read-Control 203) -ne 'Test herald') { throw 'The remaining custom character could not be selected after deletion.' }
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if ($settings.selectedCharacter -ne $characterId) { throw 'Character deletion did not preserve the chosen character.' }
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000)) { throw 'Reopened settings did not close.' }
    if (-not $SettingsOnly) {
        $reportPath = Join-Path $temporary 'report.json'
        $playbackInfo = [Diagnostics.ProcessStartInfo]::new([IO.Path]::GetFullPath($Binary))
        $playbackInfo.UseShellExecute = $false
        foreach ($key in @('CIVILIZED_AGENT_DATA', 'CIVILIZED_AGENT_TTS', 'ELEVENLABS_API_KEY', 'ELEVENLABS_API_BASE_URL')) { $playbackInfo.Environment[$key] = $settingsInfo.Environment[$key] }
        foreach ($argument in @('--isolated', '--assets', (Join-Path $root 'native-announcer/resources'), '--test-seconds', '20', '--report', $reportPath, '--snapshot', (Join-Path $Evidence 'render.png'))) { $playbackInfo.ArgumentList.Add($argument) }
        $playback = [Diagnostics.Process]::Start($playbackInfo)
        $inbox = Join-Path $data 'inbox'
        New-Item -ItemType Directory -Path $inbox -Force | Out-Null
        Start-Sleep -Seconds 3
        $notification = @{ type = 'notify'; id = 'character-runtime'; sessionID = 'character-runtime-session'; completed = 1; text = 'The native voice adviser uses the selected character for this announcement.'; title = 'Custom character verification'; character = 'opencode'; emotion = 'neutral' }
        $notification | ConvertTo-Json | Set-Content (Join-Path $Evidence 'notification.json') -Encoding utf8NoBOM
        $notification | ConvertTo-Json | Set-Content (Join-Path $inbox 'notify.tmp') -Encoding utf8NoBOM
        Move-Item (Join-Path $inbox 'notify.tmp') (Join-Path $inbox 'notify.json')
        $deadline = [DateTime]::UtcNow.AddSeconds(25)
        $lastPresence = [DateTime]::MinValue
        while (-not $playback.WaitForExit(100)) {
            if ([DateTime]::UtcNow -gt $deadline) { throw 'Custom character playback did not finish.' }
            if (([DateTime]::UtcNow - $lastPresence).TotalSeconds -ge 2) {
                $presence = @{ type = 'presence'; clientID = 'character-verification'; sessionIDs = @('character-runtime-session'); at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() }
                $presence | ConvertTo-Json | Set-Content (Join-Path $inbox 'presence.tmp') -Encoding utf8NoBOM
                Move-Item (Join-Path $inbox 'presence.tmp') (Join-Path $inbox ('presence-' + [guid]::NewGuid() + '.json'))
                $lastPresence = [DateTime]::UtcNow
            }
        }
        if ($playback.ExitCode -ne 0) { throw "Custom character playback exited with $($playback.ExitCode)." }
        $report = Get-Content $reportPath -Raw | ConvertFrom-Json
        Copy-Item $reportPath (Join-Path $Evidence 'playback-report.json')
        if ($report.selectedVideos.Count -ne 1 -or [IO.Path]::GetFullPath($report.selectedVideos[0]) -ne [IO.Path]::GetFullPath($video) -or $report.decodedVideoFrames -lt 16 -or $report.speechStarted -ne 1) { throw 'Playback did not decode the chosen animation and start speech.' }
        if (-not $report.passiveWindow -or -not $report.focusUnchanged -or $report.finished -ne 1) { throw 'The character did not finish without taking focus.' }
    } else {
        Add-Content (Join-Path $Evidence 'actions.txt') 'Skipped normal announcement playback. This run verifies settings controls and voice examples only.'
    }
    $requests = @(Get-Content $requestsFile | ForEach-Object { $_ | ConvertFrom-Json })
    $design = @($requests | Where-Object { $_.url -like '/v1/text-to-voice/design*' })
    $create = @($requests | Where-Object { $_.url -eq '/v1/text-to-voice' })
    $defaultRequests = @($requests | Where-Object { $_.url -like '/v1/text-to-speech/configured-default*' })
    $audioExamples = @($defaultRequests | Where-Object { $_.body.text -ceq 'This is an announcement' })
    $characterExamples = @($defaultRequests | Where-Object { $_.body.text -ceq $sample })
    if ($defaultRequests.Count -ne 5 -or $audioExamples.Count -ne 1 -or $characterExamples.Count -ne 4 -or @($defaultRequests | Where-Object { $_.url -cne '/v1/text-to-speech/configured-default?output_format=pcm_16000' }).Count -ne 0) { throw 'Audio and blank-ID character previews did not use the exact configured default request path.' }
    if ($design.Count -ne 0 -or $create.Count -ne 0) { throw 'Character actions must never design or create cloud voices.' }
    if (-not $SettingsOnly) {
        $speech = @($requests | Where-Object { $_.url -ceq "/v1/text-to-speech/${manualVoiceId}?output_format=pcm_16000" -and $_.body.text -eq $notification.text })
        if ($speech.Count -ne 1 -or $speech[0].url -notmatch 'output_format=pcm_16000' -or $speech[0].body.model_id -ne 'eleven_flash_v2_5' -or $speech[0].body.text -ne $notification.text) { throw 'Runtime speech did not use the custom character voice.' }
    }
    if ((Test-Path (Join-Path $data 'errors.log')) -and (Get-Item (Join-Path $data 'errors.log')).Length -gt 0) { throw 'The custom character logged an error or used local speech.' }
    Copy-Item $requestsFile (Join-Path $Evidence 'requests.jsonl')
    Open-Settings
    Select-CharactersPage
    Delete-Character
    if ([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'Deleting the final custom character activated another character unexpectedly.' }
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if ($settings.selectedCharacter) { throw 'Deleting the final custom character did not clear the saved selection.' }
    Send-Control 201 0x186 0 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    Delete-Character
    if ([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'Deleting a bundled character activated another character unexpectedly.' }
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if ($settings.characters.'hatted-herald-01') { throw 'Deleting a bundled character did not persist its tombstone.' }
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000)) { throw 'Character settings did not close.' }
    Open-Settings
    Select-CharactersPage
    if ([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'Reopening selected a character unexpectedly.' }
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000)) { throw 'Reopened character settings did not close.' }
    if (Test-Path (Join-Path $data 'history.jsonl')) { Copy-Item (Join-Path $data 'history.jsonl') (Join-Path $Evidence 'history.jsonl') }
    @{ passed = $true; settingsOnly = [bool]$SettingsOnly; runtimePlaybackVerified = -not $SettingsOnly } | ConvertTo-Json | Set-Content (Join-Path $Evidence 'result.json')
    Add-Content (Join-Path $Evidence 'actions.txt') 'Verified voice IDs and limits, preinstalled and custom profiles, Apply, Close, reopen, tombstones, and voice example requests.'
    Write-Output 'Voice IDs and limits, default previews without creation, legacy removal, deletion, tombstones, Apply, Close, and reopen passed.'
    Write-Output "Evidence saved to $Evidence"
} catch {
    $_ | Out-String | Set-Content (Join-Path $Evidence 'failure.txt')
    if (Test-Path (Join-Path $data 'errors.log')) { Copy-Item (Join-Path $data 'errors.log') (Join-Path $Evidence 'errors.log') }
    if ($process -and -not $process.HasExited -and $process.MainWindowHandle -ne [IntPtr]::Zero) { Read-Control 109 | Set-Content (Join-Path $Evidence 'status.txt') }
    if ($picker -and [CivilizedCharacterTest]::IsWindowVisible($picker)) { [CivilizedCharacterTest]::Describe($picker) | Set-Content (Join-Path $Evidence 'picker-after.txt') }
    throw
} finally {
    foreach ($child in @($process, $playback, $server)) {
        if ($child -and -not $child.HasExited) { $child.Kill(); $child.WaitForExit() }
    }
    Remove-Item -LiteralPath $temporary -Recurse -Force
    @{ scratchRemoved = -not (Test-Path $temporary); processExited = @($process, $playback, $server | Where-Object { $_ -and -not $_.HasExited }).Count -eq 0 } | ConvertTo-Json | Set-Content (Join-Path $Evidence 'cleanup.json')
}
