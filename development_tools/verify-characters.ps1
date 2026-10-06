param(
    [string]$Binary = (Join-Path (Split-Path $PSScriptRoot -Parent) 'native-announcer/target/debug/civilized-announcer.exe'),
    [string]$Evidence = (Join-Path (Split-Path $PSScriptRoot -Parent) ('temp/verification/characters-' + [guid]::NewGuid()))
)
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CivilizedCharacterTest {
    public delegate bool ChildCallback(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr window, ChildCallback callback, IntPtr parameter);
    [DllImport("user32.dll", EntryPoint = "GetClassNameW", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr window, StringBuilder name, int length);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
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
    [CivilizedCharacterTest]::SetText((Get-Control $Id), 0xC, [IntPtr]::Zero, $Value) | Out-Null
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
function Select-CharactersTab {
    Send-Control 200 0x100 0x27 | Out-Null
    Send-Control 200 0x101 0x27 | Out-Null
    Add-Content (Join-Path $Evidence 'actions.txt') "Tab count $(Send-Control 200 0x1304), selected $(Send-Control 200 0x130B), editor visible $([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203)))"
    Wait-Until { [CivilizedCharacterTest]::IsWindowVisible((Get-Control 203)) } 'Characters tab did not show its editor.'
    Add-Content (Join-Path $Evidence 'actions.txt') 'Selected the Characters tab through its native tab control.'
}
function Pick-Animation {
    [CivilizedCharacterTest]::PostMessage((Get-Control 207), 0xF5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
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
    Set-Control 114 'character-ui-test-key'
    if ([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'The General tab must not show character fields.' }
    Select-CharactersTab
    Send-Control 202 0xF5 | Out-Null
    Set-Control 203 'Test herald'
    Set-Control 204 'A warm theatrical herald with a rich British baritone, clear speech, and cheerful urgency.'
    $sample = 'I bring news for your attention. Listen as I deliver this announcement. Your work is ready, and all of the checks have passed.'
    Set-Control 205 $sample
    Pick-Animation
    Send-Control 212 0xF1 1 | Out-Null
    $before = Get-Content $settingsPath -Raw
    $before | Set-Content (Join-Path $Evidence 'settings-before.json')
    Send-Control 208 0xF5 | Out-Null
    Wait-Until { (Send-Control 209 0x146) -eq 3 } "Voice previews did not arrive. $(Read-Control 109)"
    if ((Get-Content $settingsPath -Raw) -ne $before) { throw 'Designing a voice saved unapplied settings.' }
    Send-Control 209 0x14E 1 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](209 + 65536), (Get-Control 209)) | Out-Null
    Send-Control 210 0xF5 | Out-Null
    Wait-Until { (Read-Control 109) -match 'finished|complete' } "Voice preview did not finish. $(Read-Control 109)"
    Send-Control 211 0xF5 | Out-Null
    Wait-Until { [CivilizedCharacterTest]::IsWindowEnabled((Get-Control 208)) -and (Read-Control 109) -match 'saved|created|chosen' } "Chosen voice was not created. $(Read-Control 109)"
    Send-Control 211 0xF5 | Out-Null
    if ((Read-Control 109) -notmatch 'already saved') { throw 'Saving the same preview twice must reuse the saved voice.' }
    if ((Get-Content $settingsPath -Raw) -ne $before) { throw 'Choosing a voice saved unapplied settings.' }
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    $characterId = $settings.selectedCharacter
    if (-not $characterId) { throw 'The chosen character was not activated.' }
    $character = $settings.characters.$characterId
    $serialized = $character | ConvertTo-Json -Depth 8 -Compress
    if ($character.name -ne 'Test herald' -or $serialized -notmatch 'saved-generated-1' -or $serialized -notmatch [regex]::Escape('test-character.mp4')) { throw "Character did not persist its name, selected voice, and animation. $serialized" }
    if ($settings.voices.claude -ne 'Mark') { throw 'Applying a character lost the existing local voice setting.' }
    Send-Control 202 0xF5 | Out-Null
    Set-Control 203 'Second character'
    Set-Control 206 $video
    Send-Control 201 0x14E 0 | Out-Null
    [CivilizedCharacterTest]::SendMessage($process.MainWindowHandle, 0x111, [IntPtr](201 + 65536), (Get-Control 201)) | Out-Null
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if (@($settings.characters.PSObject.Properties).Count -ne 2 -or $settings.selectedCharacter -ne $characterId) { throw 'Apply did not save both drafts and preserve the chosen character.' }
    Send-Control 200 0x100 0x25 | Out-Null
    Send-Control 200 0x101 0x25 | Out-Null
    Send-Control 107 0xF5 | Out-Null
    if ([CivilizedCharacterTest]::IsWindowVisible((Get-Control 203))) { throw 'Apply on the General tab exposed character controls.' }
    Select-CharactersTab
    $saved = Get-Content $settingsPath -Raw
    $saved | Set-Content (Join-Path $Evidence 'settings-applied.json')
    if ($saved -match 'character-ui-test-key|generated-0|audio_base_64') { throw 'Settings contain plaintext credentials or transient previews.' }
    Set-Control 203 'Unapplied name'
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000)) { throw 'Settings did not close.' }
    if ((Get-Content $settingsPath -Raw) -ne $saved) { throw 'Close saved an unapplied character edit.' }
    Open-Settings
    Select-CharactersTab
    if ((Read-Control 203) -ne 'Test herald' -or (Read-Control 206) -ne $video) { throw 'The character did not survive reopening.' }
    @{ name = (Read-Control 203); animation = (Read-Control 206); active = (Send-Control 212 0xF0) } | ConvertTo-Json | Set-Content (Join-Path $Evidence 'reopened-controls.json')
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000)) { throw 'Reopened settings did not close.' }
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
    $requests = @(Get-Content $requestsFile | ForEach-Object { $_ | ConvertFrom-Json })
    $design = @($requests | Where-Object { $_.url -like '/v1/text-to-voice/design*' })
    $create = @($requests | Where-Object { $_.url -eq '/v1/text-to-voice' })
    $speech = @($requests | Where-Object { $_.url -like '/v1/text-to-speech/saved-generated-1*' })
    if ($design.Count -ne 1 -or $design[0].url -notmatch 'output_format=pcm_16000' -or $design[0].body.model_id -ne 'eleven_ttv_v3' -or $design[0].body.text -ne $sample -or $create.Count -ne 1 -or $create[0].body.generated_voice_id -ne 'generated-1' -or $speech.Count -ne 1 -or $speech[0].url -notmatch 'output_format=pcm_16000' -or $speech[0].body.model_id -ne 'eleven_flash_v2_5' -or $speech[0].body.text -ne $notification.text) { throw 'The design, chosen voice, and runtime speech requests did not match user actions.' }
    if ((Test-Path (Join-Path $data 'errors.log')) -and (Get-Item (Join-Path $data 'errors.log')).Length -gt 0) { throw 'The custom character logged an error or used a local speech fallback.' }
    Copy-Item $requestsFile (Join-Path $Evidence 'requests.jsonl')
    if (Test-Path (Join-Path $data 'history.jsonl')) { Copy-Item (Join-Path $data 'history.jsonl') (Join-Path $Evidence 'history.jsonl') }
    Add-Content (Join-Path $Evidence 'actions.txt') 'Verified Apply, Close, reopened controls, and actual runtime animation and speech requests.'
    Write-Output 'Characters tab, voice design, selected preview, Apply, Close, reopen, custom video decoding, and ElevenLabs runtime speech passed.'
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
