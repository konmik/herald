param(
    [string]$Binary = (Join-Path (Split-Path $PSScriptRoot -Parent) 'native-announcer/target/debug/civilized-announcer.exe'),
    [string]$Evidence = (Join-Path (Split-Path $PSScriptRoot -Parent) ('temp/verification/characters-' + [guid]::NewGuid())),
    [switch]$SettingsOnly
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
    if ($Id -in @(204, 216)) {
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
    return $buffer.ToString()
}
function Open-Settings {
    $script:process = [Diagnostics.Process]::Start($script:settingsInfo)
    Wait-Until { $process.Refresh(); $process.HasExited -or $process.MainWindowHandle -ne [IntPtr]::Zero } 'Settings window did not open.'
    if ($process.HasExited) { throw "Settings exited with $($process.ExitCode)." }
    if ((Get-FileHash $Binary -Algorithm SHA256).Hash -ne $binaryHash) { throw 'The settings binary changed during verification.' }
    if ((Read-Control 107) -ne 'Apply' -or (Read-Control 108) -ne 'Close') { throw 'Settings readiness controls are missing.' }
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
    [CivilizedCharacterTest]::PostMessage((Get-Control 206), 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    $script:picker = [IntPtr]::Zero
    Wait-Until {
        $candidate = [CivilizedCharacterTest]::FindWindow('#32770', 'Choose character animation')
        $owner = [uint32]0
        if ($candidate -ne [IntPtr]::Zero) { [CivilizedCharacterTest]::GetWindowThreadProcessId($candidate, [ref]$owner) | Out-Null }
        if ($owner -eq $process.Id) { $script:picker = $candidate; return $true }
        return $false
    } 'The native animation file picker did not open.'
    Wait-Until { [CivilizedCharacterTest]::FindClass($picker, 'SysTreeView32') -ne [IntPtr]::Zero } 'The file picker did not finish loading its navigation pane.'
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
    @{ quietMode = $false; scheduleEnabled = $false; volume = 35; voices = @{ claude = 'Mark' } } | ConvertTo-Json | Set-Content $settingsPath -Encoding utf8NoBOM
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
    $catalog = Get-Content (Join-Path $root 'native-announcer/resources/characters.json') -Raw | ConvertFrom-Json
    $assets = Join-Path $root 'native-announcer/resources'
    Add-Content (Join-Path $Evidence 'actions.txt') "Bundled count $(Send-Control 201 0x18B), editor visible $([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203)))"
    if ((Send-Control 201 0x18B) -ne 11 -or [CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'The preinstalled character catalog did not load without selecting a character.' }
    $bundled = $catalog.'hatted-herald-01'
    Send-Control 201 0x186 0 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    if ((Read-Control 203) -ne $bundled.name -or (Read-Control 204) -ne $bundled.voiceDescription -or [IO.Path]::GetFullPath((Read-Control 206)) -ne [IO.Path]::GetFullPath((Join-Path $assets $bundled.animationPath))) { throw 'The bundled character did not display its name, prompt, and relocated animation.' }
    $sample = 'I bring news for your attention. Listen as I deliver this announcement. Your work is ready, and every check has passed.'
    Send-Control 210 0xF5 | Out-Null
    if ([CivilizedCharacterTest]::IsWindowEnabled((Get-Control 216))) { throw 'Voice ID editing must be disabled during voice generation.' }
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "Bundled voice example did not finish. $(Read-Control 109)"
    if ((Read-Control 216) -ne 'saved-generated-0') { throw 'The generated voice ID must appear immediately without Apply.' }
    if (([CivilizedCharacterTest]::GetWindowLong((Get-Control 216), -16) -band 0x800) -ne 0) { throw 'The voice ID field must be editable.' }
    if (-not [CivilizedCharacterTest]::IsWindowEnabled((Get-Control 216))) { throw 'Voice ID editing must be enabled after voice generation.' }
    $idBounds = [CivilizedCharacterTest+Rect]::new()
    $promptBounds = [CivilizedCharacterTest+Rect]::new()
    [CivilizedCharacterTest]::GetWindowRect((Get-Control 216), [ref]$idBounds) | Out-Null
    [CivilizedCharacterTest]::GetWindowRect((Get-Control 204), [ref]$promptBounds) | Out-Null
    if ($idBounds.Right -le $idBounds.Left -or $idBounds.Bottom -le $idBounds.Top -or $idBounds.Bottom -ge $promptBounds.Top -or $promptBounds.Bottom -le $promptBounds.Top) { throw 'The voice ID and description must have usable, separate rows.' }
    @{ voiceId = (Read-Control 216); idBounds = $idBounds; descriptionBounds = $promptBounds } | ConvertTo-Json -Depth 3 | Set-Content (Join-Path $Evidence 'voice-id-controls.json')
    if ((Get-Content $settingsPath -Raw) -match 'installedBundledCharacters') { throw 'Previewing a bundled character saved unapplied installation state.' }
    foreach ($obsolete in @(205, 207, 208, 209, 211, 212, 213, 215)) {
        if ([CivilizedCharacterTest]::GetDlgItem($process.MainWindowHandle, $obsolete) -ne [IntPtr]::Zero) { throw "Obsolete character control $obsolete remains." }
    }
    if ((Read-Control 210) -ne 'Play voice example' -or (Read-Control 214) -ne 'Delete' -or (Read-Control 202) -ne 'New') { throw 'Character actions do not match the simplified interface.' }
    Send-Control 202 0xF5 | Out-Null
    if (-not [CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'New did not show the character editor.' }
    if ((Read-Control 216) -eq 'saved-generated-0') { throw 'A new character must not display the previous character voice ID.' }
    Set-Control 203 'Test herald'
    Set-Control 204 'A warm theatrical herald with a rich British baritone, clear speech, and cheerful urgency.'
    Pick-Animation
    $before = Get-Content $settingsPath -Raw
    $before | Set-Content (Join-Path $Evidence 'settings-before.json')
    Send-Control 210 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "Voice example did not finish. $(Read-Control 109)"
    if ((Read-Control 216) -ne 'saved-generated-0') { throw 'The custom character must show its generated voice ID.' }
    if ((Get-Content $settingsPath -Raw) -ne $before) { throw 'Playing a voice saved unapplied settings.' }
    Send-Control 210 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "Reused voice example did not finish. $(Read-Control 109)"
    Set-Control 204 'An elderly royal herald with a warm weathered baritone, measured pacing, clear speech, and cheerful urgency.'
    if ((Read-Control 216) -eq 'saved-generated-0') { throw 'Changing the voice prompt must clear the invalidated voice ID.' }
    Send-Control 210 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "Changed prompt did not regenerate its voice. $(Read-Control 109)"
    Select-SettingsPage 3
    Send-Control 119 0xF5 | Out-Null
    Wait-Until { (Read-Control 118) -match '5 (left|remaining)' } "Voice limits did not refresh after creating three voices. $(Read-Control 118)"
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
    Select-SettingsPage 1
    Select-CharactersPage
    if ((Read-Control 216).Trim() -cne $manualVoiceId) { throw 'Page navigation lost the manual voice ID draft.' }
    Send-Control 210 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Voice preview finished.' } "Manual voice preview did not finish. $(Read-Control 109)"
    $manualRequests = @(Get-Content $requestsFile | Select-Object -Skip $beforeManualRequests | ForEach-Object { $_ | ConvertFrom-Json })
    if ($manualRequests.Count -ne 1 -or $manualRequests[0].url -cne "/v1/text-to-speech/${manualVoiceId}?output_format=pcm_16000" -or $manualRequests[0].body.text -ne $sample) { throw 'Manual voice preview must use the exact trimmed ID without design or creation requests.' }
    if ((Get-Content $settingsPath -Raw) -ne $beforeManual) { throw 'Manual voice preview persisted its draft before Apply.' }
    Set-Control 204 'A changed prompt invalidates a manually entered voice.'
    if ((Read-Control 216) -ne '') { throw 'Changing the prompt did not invalidate the manual voice ID.' }
    Set-Control 216 $manualVoiceId
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if ($settings.defaultVoiceId -ne $configuredDefault) { throw 'Apply did not persist the configured default voice ID.' }
    $characterId = $settings.selectedCharacter
    if (-not $characterId) { throw 'The chosen character was not activated.' }
    $character = $settings.characters.$characterId
    $serialized = $character | ConvertTo-Json -Depth 8 -Compress
    if ($character.name -ne 'Test herald' -or $serialized -match 'sampleText' -or $character.voice.voiceId -cne $manualVoiceId -or $serialized -notmatch [regex]::Escape('test-character.mp4')) { throw "Character did not persist its name, entered voice ID, and video without a sample text. $serialized" }
    if ($settings.voices.claude -ne 'Mark') { throw 'Applying a character lost the existing local voice setting.' }
    Send-Control 202 0xF5 | Out-Null
    Set-Control 203 'Second character'
    Set-Control 206 $video
    Send-Control 201 0x186 0 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if (@($settings.characters.PSObject.Properties).Count -ne 13 -or $settings.selectedCharacter -ne $characterId -or $settings.defaultVoiceId -ne $configuredDefault) { throw 'Apply did not save both drafts, preserve the chosen character, and retain the configured default voice ID.' }
    Select-SettingsPage 1
    Send-Control 107 0xF5 | Out-Null
    if ([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'Apply on the Audio page exposed character controls.' }
    Select-CharactersPage
    $saved = Get-Content $settingsPath -Raw
    $saved | Set-Content (Join-Path $Evidence 'settings-applied.json')
    if ($saved -match 'character-ui-test-key|audio_base_64|"previews"|sampleText|voice_slots_used|voice_limit|voice_add_edit_counter|max_voice_add_edits') { throw 'Settings contain plaintext credentials, transient previews, account usage, or character sample text.' }
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
    Set-Control 216 $manualVoiceId
    Send-Control 107 0xF5 | Out-Null
    Send-Control 201 0x186 1 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    if ((Read-Control 203) -ne 'Second character') { throw 'The list did not select the second character.' }
    Delete-Character $false
    if ((Send-Control 201 0x18B) -ne 13 -or (Read-Control 203) -ne 'Second character') { throw 'Cancelling Delete changed the character list.' }
    Delete-Character
    if ((Send-Control 201 0x18B) -ne 12 -or [CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'Delete did not remove the selected character without activating another one.' }
    Send-Control 201 0x186 0 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    if ((Read-Control 203) -ne 'Test herald') { throw 'The remaining custom character could not be selected after deletion.' }
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if (@($settings.characters.PSObject.Properties).Count -ne 12 -or $settings.selectedCharacter -ne $characterId) { throw 'Character deletion did not persist.' }
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
    $examples = @($requests | Where-Object { $_.url -like '/v1/text-to-speech/saved-generated-0*' -and $_.body.text -eq $sample })
    if ($defaultRequests.Count -ne 1 -or $defaultRequests[0].url -notmatch 'output_format=pcm_16000' -or $defaultRequests[0].body.text -ne 'This is an announcement') { throw 'Audio preview did not use the configured default voice request path.' }
    if ($design.Count -ne 3 -or $design[0].url -notmatch 'output_format=pcm_16000' -or $design[0].body.model_id -ne 'eleven_ttv_v3' -or $design[0].body.text -ne $sample -or $design[1].body.text -ne $sample -or $design[2].body.text -ne $sample -or $design[1].body.voice_description -eq $design[2].body.voice_description -or $create.Count -ne 3 -or $examples.Count -ne 4) { throw 'Bundled and custom voice creation, shared example reuse, and prompt changes did not match user actions.' }
    if (-not $SettingsOnly) {
        $speech = @($requests | Where-Object { $_.url -ceq "/v1/text-to-speech/${manualVoiceId}?output_format=pcm_16000" -and $_.body.text -eq $notification.text })
        if ($speech.Count -ne 1 -or $speech[0].url -notmatch 'output_format=pcm_16000' -or $speech[0].body.model_id -ne 'eleven_flash_v2_5' -or $speech[0].body.text -ne $notification.text) { throw 'Runtime speech did not use the custom character voice.' }
    }
    if ((Test-Path (Join-Path $data 'errors.log')) -and (Get-Item (Join-Path $data 'errors.log')).Length -gt 0) { throw 'The custom character logged an error or used local speech.' }
    Copy-Item $requestsFile (Join-Path $Evidence 'requests.jsonl')
    Open-Settings
    Select-CharactersPage
    Delete-Character
    if ((Send-Control 201 0x18B) -ne 11 -or [CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'Deleting the final custom character did not leave the preinstalled catalog without activating it.' }
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if (@($settings.characters.PSObject.Properties).Count -ne 11 -or $settings.selectedCharacter) { throw 'Deleting the final custom character did not clear the saved selection.' }
    Send-Control 201 0x186 0 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    Delete-Character
    if ((Send-Control 201 0x18B) -ne 10 -or [CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'Deleting a bundled character did not leave its tombstone without activating another one.' }
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if (@($settings.characters.PSObject.Properties).Count -ne 10 -or @($settings.installedBundledCharacters).Count -ne 11 -or $settings.characters.'hatted-herald-01') { throw 'Deleting a bundled character did not persist its tombstone.' }
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000)) { throw 'Character settings did not close.' }
    Open-Settings
    Select-CharactersPage
    if ((Send-Control 201 0x18B) -ne 10 -or [CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'The bundled character tombstone did not survive reopening.' }
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000)) { throw 'Reopened character settings did not close.' }
    if (Test-Path (Join-Path $data 'history.jsonl')) { Copy-Item (Join-Path $data 'history.jsonl') (Join-Path $Evidence 'history.jsonl') }
    @{ passed = $true; settingsOnly = [bool]$SettingsOnly; runtimePlaybackVerified = -not $SettingsOnly } | ConvertTo-Json | Set-Content (Join-Path $Evidence 'result.json')
    Add-Content (Join-Path $Evidence 'actions.txt') 'Verified voice IDs and limits, preinstalled and custom profiles, Apply, Close, reopen, tombstones, and voice example requests.'
    Write-Output 'Voice IDs and limits, preinstalled profiles, voice examples, voice creation, deletion, tombstones, Apply, Close, and reopen passed.'
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
