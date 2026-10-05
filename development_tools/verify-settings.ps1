param([string]$Binary = (Join-Path (Split-Path $PSScriptRoot -Parent) 'native-announcer/bin/civilized-announcer-win32-x64.exe'))
$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CivilizedSettingsTest {
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)] public static extern IntPtr SetText(IntPtr window, uint message, IntPtr wparam, string text);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)] public static extern IntPtr ReadText(IntPtr window, uint message, IntPtr length, StringBuilder text);
}
'@
$temporary = Join-Path $env:LOCALAPPDATA ('Temp/opencode/settings-test-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temporary | Out-Null
$settingsPath = Join-Path $temporary 'settings.json'
@{ nightStart = 22; nightEnd = 8; outputDevice = 'unavailable-test-device'; voices = @{ claude = 'Mark' } } | ConvertTo-Json | Set-Content $settingsPath -Encoding utf8NoBOM
$process = $null
function Send-Control {
    param([int]$Id, [uint32]$Message, [long]$Wparam = 0, [long]$Lparam = 0)
    $control = [CivilizedSettingsTest]::GetDlgItem($process.MainWindowHandle, $Id)
    if ($control -eq [IntPtr]::Zero) { throw "Missing settings control $Id" }
    return [CivilizedSettingsTest]::SendMessage($control, $Message, [IntPtr]$Wparam, [IntPtr]$Lparam).ToInt64()
}
function Read-Control {
    param([int]$Id)
    $buffer = [Text.StringBuilder]::new(512)
    [CivilizedSettingsTest]::ReadText([CivilizedSettingsTest]::GetDlgItem($process.MainWindowHandle, $Id), 0xD, [IntPtr]512, $buffer) | Out-Null
    return $buffer.ToString()
}
try {
    $info = [Diagnostics.ProcessStartInfo]::new([IO.Path]::GetFullPath($Binary))
    $info.UseShellExecute = $false
    $info.Environment['CIVILIZED_AGENT_DATA'] = $temporary
    $info.ArgumentList.Add('--settings')
    $process = [Diagnostics.Process]::Start($info)
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 50
        $process.Refresh()
        if ($process.HasExited) { throw "Settings app exited early: $($process.ExitCode)" }
    } while ($process.MainWindowHandle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
    if ($process.MainWindowHandle -eq [IntPtr]::Zero) { throw 'Settings window did not open' }
    $selected = Send-Control 106 0x147
    $count = Send-Control 106 0x146
    if ($selected -ne $count - 1 -or $selected -le 0) { throw 'Unavailable output must remain selected with system fallback' }
    if ((Read-Control 107) -ne 'Apply' -or (Read-Control 108) -ne 'Close') { throw 'Settings must have Apply and Close buttons' }
    if ((Send-Control 113 0xF0) -ne 0) { throw 'CPU must be the default' }
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if ($settings.outputDevice -ne 'unavailable-test-device') { throw 'Saving lost the unavailable selected device' }
    $saved = Get-Content $settingsPath -Raw
    Send-Control 105 0x405 1 35 | Out-Null
    Send-Control 112 0xF5 | Out-Null
    if ((Read-Control 112) -ne 'Stop example') { throw 'Audio preview did not start' }
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ((Read-Control 112) -eq 'Stop example' -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    if ((Read-Control 109) -ne 'Preview finished.') { throw "Audio preview did not finish: $(Read-Control 109)" }
    if ((Get-Content $settingsPath -Raw) -ne $saved) { throw 'Preview saved unsaved settings' }
    Send-Control 113 0xF1 1 | Out-Null
    Send-Control 112 0xF5 | Out-Null
    $deadline = [DateTime]::UtcNow.AddSeconds(25)
    while ((Read-Control 112) -eq 'Stop example' -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    if ((Read-Control 109) -ne 'Preview finished.') { throw "GPU preview did not finish: $(Read-Control 109)" }
    if (Test-Path (Join-Path $temporary 'errors.log')) { throw (Get-Content (Join-Path $temporary 'errors.log') -Raw) }
    if ((Get-Content $settingsPath -Raw) -ne $saved) { throw 'GPU preview saved unapplied changes' }
    Send-Control 112 0xF5 | Out-Null
    Send-Control 112 0xF5 | Out-Null
    if ((Read-Control 109) -ne 'Preview stopped.') { throw 'Preview could not be cancelled' }
    Send-Control 105 0x405 1 0 | Out-Null
    Send-Control 112 0xF5 | Out-Null
    if ((Read-Control 109) -ne 'Preview is silent at 0% volume.') { throw 'Preview did not honor zero volume' }
    Send-Control 101 0xF1 1 | Out-Null
    Send-Control 102 0xF1 1 | Out-Null
    [CivilizedSettingsTest]::SetText([CivilizedSettingsTest]::GetDlgItem($process.MainWindowHandle, 103), 0xC, [IntPtr]::Zero, '22:30') | Out-Null
    [CivilizedSettingsTest]::SetText([CivilizedSettingsTest]::GetDlgItem($process.MainWindowHandle, 104), 0xC, [IntPtr]::Zero, '08:15') | Out-Null
    Send-Control 105 0x405 1 35 | Out-Null
    Send-Control 106 0x14E 0 | Out-Null
    Send-Control 107 0xF5 | Out-Null
    $settings = Get-Content $settingsPath -Raw | ConvertFrom-Json
    if (-not $settings.useGpu -or -not $settings.quietMode -or -not $settings.scheduleEnabled -or $settings.quietStart -ne 1350 -or $settings.quietEnd -ne 495 -or $settings.volume -ne 35 -or $null -ne $settings.outputDevice -or $settings.voices.claude -ne 'Mark') { throw "Settings controls did not persist their values or preserve the voice selection: $($settings | ConvertTo-Json -Compress)" }
    $applied = Get-Content $settingsPath -Raw
    Send-Control 105 0x405 1 15 | Out-Null
    Send-Control 113 0xF1 0 | Out-Null
    Send-Control 108 0xF5 | Out-Null
    if (-not $process.WaitForExit(5000) -or $process.ExitCode -ne 0) { throw 'Settings app did not close cleanly' }
    if ((Get-Content $settingsPath -Raw) -ne $applied) { throw 'Close must not save unapplied changes' }
    $process = [Diagnostics.Process]::Start($info)
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do { Start-Sleep -Milliseconds 50; $process.Refresh() } while ($process.MainWindowHandle -eq [IntPtr]::Zero -and -not $process.HasExited -and [DateTime]::UtcNow -lt $deadline)
    if ((Send-Control 113 0xF0) -ne 1 -or (Send-Control 101 0xF0) -ne 1 -or (Send-Control 105 0x400) -ne 35 -or (Send-Control 106 0x147) -ne 0) { throw 'Saved settings did not survive reopening' }
    $reopen = [Diagnostics.Process]::Start($info)
    if (-not $reopen.WaitForExit(5000) -or $reopen.ExitCode -ne 0 -or $process.HasExited) { throw 'Reopening should show the existing settings window' }
    [CivilizedSettingsTest]::SendMessage($process.MainWindowHandle, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    if (-not $process.WaitForExit(5000)) { throw 'Settings app did not exit' }
    Write-Output 'Settings UI, persistence, preview, cancellation, output fallback and reopen checks passed.'
} finally {
    if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    Remove-Item -LiteralPath $temporary -Recurse -Force
}
