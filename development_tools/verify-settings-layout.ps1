param(
    [string]$Binary = (Join-Path (Split-Path $PSScriptRoot -Parent) 'native-announcer/target/debug/civilized-announcer.exe'),
    [string]$Evidence = ('temp/verification/settings-layout-' + [guid]::NewGuid())
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$binaryPath = [IO.Path]::GetFullPath($Binary)
$evidencePath = if ([IO.Path]::IsPathRooted($Evidence)) { [IO.Path]::GetFullPath($Evidence) } else { [IO.Path]::GetFullPath((Join-Path $root $Evidence)) }
$scratch = Join-Path $env:LOCALAPPDATA ('Temp/opencode/civilized-settings-layout-' + [guid]::NewGuid())
$settingsPath = Join-Path $scratch 'settings.json'
$process = $null
$windowHandle = [IntPtr]::Zero
$binaryHash = $null
$scratchCreated = $false
$transcribing = $false
$launchCount = 0

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class CivilizedSettingsLayoutNative {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X; public int Y; }
    [StructLayout(LayoutKind.Sequential)] public struct MinMaxInfo { public Point Reserved; public Point MaxSize; public Point MaxPosition; public Point MinTrackSize; public Point MaxTrackSize; }
    public sealed class ChildInfo { public int Id; public bool Visible; public bool Enabled; public int Left; public int Top; public int Right; public int Bottom; }
    private delegate bool EnumChildProc(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, StringBuilder lparam);
    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)] public static extern IntPtr SetText(IntPtr window, uint message, IntPtr wparam, string text);
    [DllImport("user32.dll", EntryPoint = "PostMessageW")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr window);
    [DllImport("user32.dll", EntryPoint = "FindWindowW", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string className, string title);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll", EntryPoint = "GetWindowTextLengthW")] public static extern int GetWindowTextLength(IntPtr window);
    [DllImport("user32.dll", EntryPoint = "GetWindowTextW", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder text, int length);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] private static extern bool EnumChildWindows(IntPtr window, EnumChildProc callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref Point point);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window, IntPtr insertAfter, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr SetFocus(IntPtr window);
    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")] public static extern IntPtr GetWindowLongPtr(IntPtr window, int index);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool RedrawWindow(IntPtr window, IntPtr rect, IntPtr region, uint flags);
    public static int[] ClientSize(IntPtr window) { Rect rect; if (!GetClientRect(window, out rect)) throw new InvalidOperationException("Could not read the settings client rectangle."); return new[] { rect.Right - rect.Left, rect.Bottom - rect.Top }; }
    public static int[] WindowSize(IntPtr window) { Rect rect; if (!GetWindowRect(window, out rect)) throw new InvalidOperationException("Could not read the settings window rectangle."); return new[] { rect.Right - rect.Left, rect.Bottom - rect.Top }; }
    public static int[] MinimumSize(IntPtr window) {
        var memory = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(MinMaxInfo)));
        try { Marshal.StructureToPtr(new MinMaxInfo(), memory, false); SendMessage(window, 0x24, IntPtr.Zero, memory); var info = Marshal.PtrToStructure<MinMaxInfo>(memory); return new[] { info.MinTrackSize.X, info.MinTrackSize.Y }; }
        finally { Marshal.FreeHGlobal(memory); }
    }
    public static string WindowText(IntPtr window) { var text = new StringBuilder(GetWindowTextLength(window) + 1); GetWindowText(window, text, text.Capacity); return text.ToString(); }
    public static string ChildText(IntPtr parent) { var result = new StringBuilder(); EnumChildWindows(parent, (window, parameter) => { var text = WindowText(window); if (text.Length > 0) result.AppendLine(text); return true; }, IntPtr.Zero); return result.ToString(); }
    public static string[] ListBoxItems(IntPtr list) {
        var count = SendMessage(list, 0x18B, IntPtr.Zero, IntPtr.Zero).ToInt64();
        if (count <= 0) return new string[0];
        var items = new string[(int)count];
        for (var index = 0; index < count; index++) { var length = SendMessage(list, 0x18A, new IntPtr(index), IntPtr.Zero).ToInt64(); var text = new StringBuilder((int)Math.Max(length + 1, 1)); SendMessage(list, 0x189, new IntPtr(index), text); items[index] = text.ToString(); }
        return items;
    }
    public static ChildInfo[] Children(IntPtr parent) {
        var origin = new Point();
        if (!ClientToScreen(parent, ref origin)) throw new InvalidOperationException("Could not find the settings client origin.");
        var children = new List<ChildInfo>();
        EnumChildWindows(parent, (window, parameter) => { if (GetParent(window) != parent) return true; Rect rect; if (!GetWindowRect(window, out rect)) return true; children.Add(new ChildInfo { Id = GetDlgCtrlID(window), Visible = IsWindowVisible(window), Enabled = IsWindowEnabled(window), Left = rect.Left - origin.X, Top = rect.Top - origin.Y, Right = rect.Right - origin.X, Bottom = rect.Bottom - origin.Y }); return true; }, IntPtr.Zero);
        return children.ToArray();
    }
}
'@

$pageNames = @('Characters', 'Audio', 'Quiet hours', 'Speech service', 'Offline voice', 'Announcements')
$pageControls = @{ Characters = @(201, 202); Audio = @(105, 106, 111, 112); 'Quiet hours' = @(101, 102, 103, 104); 'Speech service' = @(113, 114, 121, 122, 123); 'Offline voice' = @(124, 125); Announcements = @(130, 131, 132, 133, 134, 135) }
$editorControls = @(203, 206, 216, 217, 210, 214)
$snapshotIds = @(101, 102, 103, 104, 105, 106, 107, 108, 109, 111, 112, 113, 114, 121, 122, 123, 124, 125, 130, 131, 132, 133, 134, 135, 200, 201, 202, 203, 206, 216, 217, 210, 214, 400)
$wmKeyDown = 0x100
$wmKeyUp = 0x101
$wmCommand = 0x111
$vkHome = 0x24
$vkDown = 0x28
$lbGetCurSel = 0x188
$lbGetCount = 0x18B
$lbSetCurSel = 0x186
$lbnSelChange = 1
$bmClick = 0xF5
$tbmGetPos = 0x400
$tbmSetPos = 0x405
$enChange = 0x300
$configuredDefault = 'configured-default'
$existingDefault = 'JBFqnCBsd6RMkjVDRZzb'

function Control([int]$Id) {
    $handle = [CivilizedSettingsLayoutNative]::GetDlgItem($script:windowHandle, $Id)
    if ($handle -eq [IntPtr]::Zero) { throw "Missing settings control $Id" }
    $handle
}
function Send-Control([int]$Id, [uint32]$Message, [long]$Wparam = 0, [long]$Lparam = 0) {
    [CivilizedSettingsLayoutNative]::SendMessage((Control $Id), $Message, [IntPtr]$Wparam, [IntPtr]$Lparam).ToInt64()
}
function Send-Window([uint32]$Message, [long]$Wparam = 0, [long]$Lparam = 0) {
    [CivilizedSettingsLayoutNative]::SendMessage($script:windowHandle, $Message, [IntPtr]$Wparam, [IntPtr]$Lparam).ToInt64()
}
function Read-Combo-Control([int]$Id) {
    $index = Send-Control $Id 0x147
    if ($index -lt 0) { return '' }
    $length = Send-Control $Id 0x149 $index
    if ($length -lt 0) { throw "Could not read combo control $Id" }
    $text = [Text.StringBuilder]::new([int]$length + 1)
    [CivilizedSettingsLayoutNative]::SendMessage((Control $Id), 0x148, [IntPtr]$index, $text) | Out-Null
    if ($Id -eq 134) { $text.ToString().Replace("`r`n", "`n") } else { $text.ToString() }
}
function Read-Control([int]$Id) {
    if ($Id -in @(106, 113, 130, 131, 132, 133)) { return Read-Combo-Control $Id }
    $text = [Text.StringBuilder]::new(4096)
    [CivilizedSettingsLayoutNative]::SendMessage((Control $Id), 0xD, [IntPtr]$text.Capacity, $text) | Out-Null
    $text.ToString()
}
function Set-Control([int]$Id, [string]$Value) {
    $control = Control $Id
    $nativeValue = if ($Id -eq 134) { $Value.Replace("`r`n", "`n").Replace("`n", "`r`n") } else { $Value }
    if ([CivilizedSettingsLayoutNative]::SetText($control, 0xC, [IntPtr]::Zero, $nativeValue) -eq [IntPtr]::Zero) { throw "Could not set control $Id" }
    if ((Read-Control $Id) -ne $Value) { throw "Control $Id did not accept its draft text" }
    Send-Window $wmCommand (([long]$Id) -bor (([long]$enChange) -shl 16)) $control.ToInt64() | Out-Null
}
function Wait-Until([scriptblock]$Condition, [string]$Failure, [int]$Seconds = 10) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while (-not (& $Condition)) { if ([DateTime]::UtcNow -ge $deadline) { throw $Failure }; Start-Sleep -Milliseconds 50 }
}
function Dismiss-ApplyError {
    Wait-Until {
        $candidate = [CivilizedSettingsLayoutNative]::FindWindow('#32770', 'Civilized Agent settings')
        if ($candidate -eq [IntPtr]::Zero) { return $false }
        $owner = [uint32]0
        [CivilizedSettingsLayoutNative]::GetWindowThreadProcessId($candidate, [ref]$owner) | Out-Null
        if ($owner -ne $script:process.Id) { return $false }
        $script:errorDialog = $candidate
        return $true
    } 'Apply did not report the invalid default voice ID.'
    $dialog = $script:errorDialog
    if (([CivilizedSettingsLayoutNative]::ChildText($dialog)) -notmatch 'ElevenLabs voice ID is invalid') { throw 'Apply did not show the existing invalid ElevenLabs voice ID error.' }
    $controls = @([CivilizedSettingsLayoutNative]::Children($dialog) | ForEach-Object {
        [pscustomobject]@{ id = $_.Id; visible = $_.Visible; enabled = $_.Enabled; text = [CivilizedSettingsLayoutNative]::WindowText([CivilizedSettingsLayoutNative]::GetDlgItem($dialog, $_.Id)) }
    })
    $controls | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath 'apply-error-controls.json') -Encoding utf8NoBOM
    $buttons = @($controls | Where-Object { $_.visible -and $_.enabled -and $_.text.Replace('&', '') -eq 'OK' })
    if ($buttons.Count -ne 1) { throw 'The invalid default voice ID error has no unique OK button.' }
    $button = [CivilizedSettingsLayoutNative]::GetDlgItem($dialog, $buttons[0].id)
    [CivilizedSettingsLayoutNative]::SendMessage($button, $bmClick, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Wait-Until { -not [CivilizedSettingsLayoutNative]::IsWindowVisible($dialog) } 'The invalid default voice ID error did not close.'
}
function Assert-NoVisibleOverlaps([string]$Name) {
    $children = @([CivilizedSettingsLayoutNative]::Children($script:windowHandle) | Where-Object Visible)
    for ($first = 0; $first -lt $children.Count; $first++) {
        for ($second = $first + 1; $second -lt $children.Count; $second++) {
            $left = $children[$first]
            $right = $children[$second]
            if ($left.Right -gt $right.Left -and $right.Right -gt $left.Left -and $left.Bottom -gt $right.Top -and $right.Bottom -gt $left.Top) { throw "$Name controls $($left.Id) and $($right.Id) overlap" }
        }
    }
}
function Assert-PromptRegion([string]$Name) {
    $children = @([CivilizedSettingsLayoutNative]::Children($script:windowHandle) | Where-Object Visible)
    $prompt = @($children | Where-Object { $_.Id -eq 134 })
    if ($prompt.Count -ne 1) { throw "$Name does not expose one visible prompt region" }
    foreach ($child in @($children | Where-Object { $_.Id -ne 134 })) {
        if ($prompt[0].Right -gt $child.Left -and $child.Right -gt $prompt[0].Left -and $prompt[0].Bottom -gt $child.Top -and $child.Bottom -gt $prompt[0].Top) { throw "$Name prompt region overlaps control $($child.Id)" }
    }
}
function Page-Index { [int](Send-Control 200 $lbGetCurSel) }
function Page-Title { Read-Control 400 }
function Save-ControlSnapshot([string]$Name) {
    $size = [CivilizedSettingsLayoutNative]::ClientSize($script:windowHandle)
    $controls = [ordered]@{}
    foreach ($id in $snapshotIds) { $handle = [CivilizedSettingsLayoutNative]::GetDlgItem($script:windowHandle, $id); if ($handle -ne [IntPtr]::Zero) { $controls["$id"] = [ordered]@{ text = ''; visible = [bool][CivilizedSettingsLayoutNative]::IsWindowVisible($handle); enabled = [bool][CivilizedSettingsLayoutNative]::IsWindowEnabled($handle) } } }
    foreach ($id in @($controls.Keys)) { $controls[$id].text = Read-Control ([int]$id) }
    $children = @([CivilizedSettingsLayoutNative]::Children($script:windowHandle) | ForEach-Object { [ordered]@{ id = $_.Id; visible = [bool]$_.Visible; enabled = [bool]$_.Enabled; rect = [ordered]@{ left = $_.Left; top = $_.Top; right = $_.Right; bottom = $_.Bottom } } })
    $snapshot = [ordered]@{ name = $Name; captured = [DateTime]::UtcNow.ToString('o'); pageIndex = Page-Index; pageTitle = Page-Title; navigation = [ordered]@{ items = @([CivilizedSettingsLayoutNative]::ListBoxItems((Control 200))); selected = Page-Index; style = [CivilizedSettingsLayoutNative]::GetWindowLongPtr((Control 200), -16).ToInt64() }; client = [ordered]@{ width = $size[0]; height = $size[1] }; controls = $controls; children = $children }
    $snapshot | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $evidencePath ($Name + '.json')) -Encoding utf8NoBOM
    $snapshot
}
function Save-WindowPng([string]$Name) {
    if (-not [CivilizedSettingsLayoutNative]::RedrawWindow($script:windowHandle, [IntPtr]::Zero, [IntPtr]::Zero, 0x585)) { throw 'Could not repaint the settings window before capture' }
    $size = [CivilizedSettingsLayoutNative]::WindowSize($script:windowHandle)
    $bitmap = $null; $graphics = $null; $dc = [IntPtr]::Zero
    try {
        $bitmap = [Drawing.Bitmap]::new([int]$size[0], [int]$size[1], [Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $rendered = $false
        foreach ($flags in @(2, 0)) { $dc = $graphics.GetHdc(); try { $rendered = [CivilizedSettingsLayoutNative]::PrintWindow($script:windowHandle, $dc, [uint32]$flags) } finally { $graphics.ReleaseHdc($dc); $dc = [IntPtr]::Zero }; if ($rendered) { break } }
        if (-not $rendered) { throw "PrintWindow could not capture $Name" }
        $bitmap.Save((Join-Path $evidencePath ($Name + '.png')), [Drawing.Imaging.ImageFormat]::Png)
    } finally {
        if ($dc -ne [IntPtr]::Zero) { $graphics.ReleaseHdc($dc) }
        if ($graphics) { $graphics.Dispose() }
        if ($bitmap) { $bitmap.Dispose() }
    }
}
function Assert-VisibleChildren([string]$Name) {
    $client = [CivilizedSettingsLayoutNative]::ClientSize($script:windowHandle)
    $children = @([CivilizedSettingsLayoutNative]::Children($script:windowHandle) | Where-Object Visible)
    if ($children.Count -eq 0) { throw "$Name has no visible child controls" }
    foreach ($child in $children) {
        if ($child.Right -le $child.Left -or $child.Bottom -le $child.Top) { throw "$Name has a zero-sized visible child $($child.Id)" }
        if ($child.Left -lt 0 -or $child.Top -lt 0 -or $child.Right -gt $client[0] -or $child.Bottom -gt $client[1]) { throw "$Name child $($child.Id) is outside the client rectangle $($client[0])x$($client[1])" }
    }
}
function Assert-Page([int]$Index, [bool]$Editor = $false) {
    $name = $pageNames[$Index]
    if ((Page-Index) -ne $Index -or (Page-Title) -ne $name) { throw "Expected page $Index '$name'" }
    foreach ($id in @($pageControls[$name])) { if (-not [CivilizedSettingsLayoutNative]::IsWindowVisible((Control $id))) { throw "Control $id is hidden on $name" } }
    foreach ($other in $pageNames) { if ($other -ne $name) { foreach ($id in @($pageControls[$other])) { if ([CivilizedSettingsLayoutNative]::IsWindowVisible((Control $id))) { throw "Control $id from $other is visible on $name" } } } }
    foreach ($id in $editorControls) { $visible = [CivilizedSettingsLayoutNative]::IsWindowVisible((Control $id)); if (($Index -eq 0 -and $Editor -and -not $visible) -or (($Index -ne 0 -or -not $Editor) -and $visible)) { throw "Character editor control $id has unexpected visibility on $name" } }
    foreach ($id in @(200, 400, 107, 108, 109)) { if (-not [CivilizedSettingsLayoutNative]::IsWindowVisible((Control $id))) { throw "Global control $id is hidden on $name" } }
    Assert-VisibleChildren $name
}
function Select-Page([int]$Index) {
    $nav = Control 200
    [CivilizedSettingsLayoutNative]::SetFocus($nav) | Out-Null
    [CivilizedSettingsLayoutNative]::SendMessage($nav, $wmKeyDown, [IntPtr]$vkHome, [IntPtr]1) | Out-Null
    [CivilizedSettingsLayoutNative]::SendMessage($nav, $wmKeyUp, [IntPtr]$vkHome, [IntPtr]0) | Out-Null
    for ($step = 0; $step -lt $Index; $step++) { [CivilizedSettingsLayoutNative]::SendMessage($nav, $wmKeyDown, [IntPtr]$vkDown, [IntPtr]1) | Out-Null; [CivilizedSettingsLayoutNative]::SendMessage($nav, $wmKeyUp, [IntPtr]$vkDown, [IntPtr]0) | Out-Null }
    Wait-Until { (Page-Index) -eq $Index -and (Page-Title) -eq $pageNames[$Index] } "Could not select page $($pageNames[$Index]) through the sidebar keyboard path."
}
function Capture-Page([int]$Index, [string]$Name, [bool]$Editor = $false) { Assert-Page $Index $Editor; if ($Index -eq 5) { Assert-NoVisibleOverlaps $Name; Assert-PromptRegion $Name }; Save-ControlSnapshot $Name | Out-Null; Save-WindowPng $Name }
function Doctor {
    if (-not $script:process) { throw 'Settings process is missing' }
    $script:process.Refresh()
    if ($script:process.HasExited) { throw "Settings exited with $($script:process.ExitCode)" }
    if ([IO.Path]::GetFullPath($script:process.Path) -ne $binaryPath) { throw "Settings process path is '$($script:process.Path)'" }
    if ($script:process.MainWindowHandle -eq [IntPtr]::Zero -or $script:process.MainWindowTitle -ne 'Civilized Agent settings') { throw 'Settings window is missing or has the wrong title' }
    if ((Get-FileHash $binaryPath -Algorithm SHA256).Hash -ne $binaryHash) { throw 'The settings binary changed during verification' }
    $errors = Join-Path $scratch 'errors.log'
    if (Test-Path -LiteralPath $errors) { throw (Get-Content -LiteralPath $errors -Raw) }
    if ((Read-Control 107) -ne 'Apply' -or (Read-Control 108) -ne 'Close') { throw 'Apply and Close are not ready' }
    foreach ($obsolete in @(204, 311)) { if ([CivilizedSettingsLayoutNative]::GetDlgItem($script:windowHandle, $obsolete) -ne [IntPtr]::Zero) { throw "Removed description control $obsolete remains" } }
}
function Launch-Settings {
    if ($script:process -and -not $script:process.HasExited) { throw 'The previous settings process is still running' }
    $script:process = [Diagnostics.Process]::Start($script:settingsInfo)
    $script:launchCount++
    $started = $script:process.StartTime.ToUniversalTime()
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do { Start-Sleep -Milliseconds 50; $script:process.Refresh(); if ($script:process.HasExited) { throw "Settings exited early with $($script:process.ExitCode)" } } while ($script:process.MainWindowHandle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
    if ($script:process.MainWindowHandle -eq [IntPtr]::Zero) { throw 'Settings window did not open' }
    $script:windowHandle = $script:process.MainWindowHandle
    $identity = [ordered]@{ binary = $binaryPath; sha256 = $binaryHash; pid = $script:process.Id; started = $started.ToString('o'); hwnd = $script:windowHandle.ToInt64(); scratch = $scratch }
    $identity | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath ("instance-$launchCount.json")) -Encoding utf8NoBOM
    $identity | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath 'instance.json') -Encoding utf8NoBOM
}
function Close-Settings {
    Send-Control 108 $bmClick | Out-Null
    if (-not $script:process.WaitForExit(5000) -or $script:process.ExitCode -ne 0) { throw 'Close did not exit cleanly' }
    $script:windowHandle = [IntPtr]::Zero
}

try {
    if (-not (Test-Path -LiteralPath $binaryPath)) { throw "Build or provide the settings binary: $binaryPath" }
    if (Test-Path -LiteralPath $evidencePath) { throw "Evidence directory already exists: $evidencePath" }
    if (Test-Path -LiteralPath $scratch) { throw "Scratch directory already exists: $scratch" }
    New-Item -ItemType Directory -Path $evidencePath | Out-Null
    New-Item -ItemType Directory -Path $scratch | Out-Null
    $scratchCreated = $true
    Start-Transcript -Path (Join-Path $evidencePath 'actions.txt') | Out-Null
    $transcribing = $true
    $binaryHash = (Get-FileHash $binaryPath -Algorithm SHA256).Hash
    $layoutPrompt = [string]::Join("`n", @('Summary {{status}}.', 'Report data {{report}}'))
    $fixture = [ordered]@{ quietMode = $false; scheduleEnabled = $true; quietStart = 1320; quietEnd = 480; volume = 35; outputDevice = $null; speechModel = 'eleven_flash_v2_5'; voices = @{ claude = 'Mark' }; characters = @{ 'layout-verification-character' = @{ name = 'Stored layout character'; animationPath = $null } }; selectedCharacter = $null; announcementBodyFont = [ordered]@{ family = 'Segoe UI'; size = 16 }; announcementTitleFont = [ordered]@{ family = 'Segoe UI'; size = 26 }; summaryPrompt = $layoutPrompt }
    $fixture | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $settingsPath -Encoding utf8NoBOM
    Copy-Item -LiteralPath $settingsPath -Destination (Join-Path $evidencePath 'settings-before.json')
    $emptyEnvironment = Join-Path $scratch 'empty.env'
    Set-Content -LiteralPath $emptyEnvironment -Value '' -Encoding utf8NoBOM
    $script:settingsInfo = [Diagnostics.ProcessStartInfo]::new($binaryPath)
    $script:settingsInfo.UseShellExecute = $false
    $script:settingsInfo.Environment['CIVILIZED_AGENT_DATA'] = $scratch
    $script:settingsInfo.Environment['CIVILIZED_AGENT_TTS'] = Join-Path $root 'native-announcer/resources/tts/kitten-nano-en-v0_8-int8'
    $script:settingsInfo.Environment['CIVILIZED_AGENT_ENV'] = $emptyEnvironment
    $script:settingsInfo.Environment['ELEVENLABS_API_KEY'] = ''
    foreach ($argument in @('--settings', '--assets', (Join-Path $root 'native-announcer/resources'))) { $script:settingsInfo.ArgumentList.Add($argument) }
    Launch-Settings
    Doctor
    $navItems = @([CivilizedSettingsLayoutNative]::ListBoxItems((Control 200)))
    if ($navItems.Count -ne 6 -or ($navItems -join '|') -ne ($pageNames -join '|')) { throw 'The settings sidebar items do not match the native navigation contract' }
    if (([CivilizedSettingsLayoutNative]::GetWindowLongPtr((Control 200), -16).ToInt64() -band 1) -eq 0) { throw 'The settings sidebar listbox does not use LBS_NOTIFY' }
    if ((Page-Index) -ne 1 -or (Page-Title) -ne 'Audio') { throw 'Audio is not the default settings page' }
    Capture-Page 1 'page-audio'
    Select-Page 2; Capture-Page 2 'page-quiet-hours'
    Select-Page 3; Capture-Page 3 'page-speech-service'
    Select-Page 4; Capture-Page 4 'page-offline-voice'
    Select-Page 5; Capture-Page 5 'page-announcements'
    if ((Read-Control 121) -ne $existingDefault) { throw "An old settings file did not show the existing default voice ID '$existingDefault'." }
    if ((Read-Control 122) -ne 'Default voice ID' -or (Read-Control 123) -notmatch 'custom ElevenLabs voice') { throw 'The default voice controls do not explain their purpose.' }
    Set-Control 121 $configuredDefault
    Select-Page 0; Assert-Page 0 $false
    if ((Send-Control 201 $lbGetCount) -le 0) { throw 'The Characters page has no native list entries' }
    $characterList = Control 201
    Send-Control 201 $lbSetCurSel 0 | Out-Null
    Send-Window $wmCommand (([long]201) -bor (([long]$lbnSelChange) -shl 16)) $characterList.ToInt64() | Out-Null
    Wait-Until { [CivilizedSettingsLayoutNative]::IsWindowVisible((Control 203)) } 'Native character selection did not show the editor.'
    Assert-Page 0 $true
    if ([String]::IsNullOrWhiteSpace((Read-Control 203))) { throw 'Native character selection did not populate the name editor' }
    Send-Control 202 $bmClick | Out-Null
    Wait-Until { [CivilizedSettingsLayoutNative]::IsWindowVisible((Control 203)) } 'New did not show the character editor.'
    $draftName = 'Layout draft ' + [guid]::NewGuid().ToString('N').Substring(0, 8)
    Set-Control 203 $draftName
    Select-Page 1; Assert-Page 1 $false
    Select-Page 0; Assert-Page 0 $true
    if ((Read-Control 203) -ne $draftName) { throw 'Character name draft was lost while switching sidebar pages' }
    Capture-Page 0 'page-characters' $true
    Select-Page 1; Assert-Page 1 $false
    $beforePreview = Get-Content -LiteralPath $settingsPath -Raw
    Send-Control 105 $tbmSetPos 1 0 | Out-Null
    Send-Window 0x114 5 (Control 105).ToInt64() | Out-Null
    Send-Control 112 $bmClick | Out-Null
    Wait-Until { (Read-Control 109) -eq 'Preview is silent at 0% volume.' } 'Zero-volume preview did not report a silent preview.'
    if ((Get-Content -LiteralPath $settingsPath -Raw) -ne $beforePreview) { throw 'Preview saved unapplied settings' }
    Send-Control 105 $tbmSetPos 1 37 | Out-Null
    Send-Window 0x114 5 (Control 105).ToInt64() | Out-Null
    Save-ControlSnapshot 'audio-after-preview' | Out-Null
    Send-Control 107 $bmClick | Out-Null
    Wait-Until { (Get-Content -LiteralPath $settingsPath -Raw) -ne $beforePreview } 'Apply did not write the isolated settings file.'
    $appliedSettings = Get-Content -LiteralPath $settingsPath -Raw | ConvertFrom-Json
    if ($appliedSettings.defaultVoiceId -ne $configuredDefault) { throw 'Apply did not persist the configured default voice ID.' }
    if (@($appliedSettings.characters.PSObject.Properties | Where-Object { $_.Value.name -eq $draftName }).Count -ne 1) { throw 'Apply did not persist the new character draft' }
    if ([int]$appliedSettings.volume -ne 37 -or [int](Send-Control 105 $tbmGetPos) -ne 37) { throw 'Apply did not persist the audio draft' }
    $appliedSettings | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $evidencePath 'settings-applied.json') -Encoding utf8NoBOM
    $appliedBytes = Get-Content -LiteralPath $settingsPath -Raw
    Select-Page 3; Assert-Page 3
    Set-Control 121 'bad voice'
    [CivilizedSettingsLayoutNative]::PostMessage((Control 107), $bmClick, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
    Dismiss-ApplyError
    if ((Get-Content -LiteralPath $settingsPath -Raw) -ne $appliedBytes) { throw 'An invalid default voice ID replaced the saved settings.' }
    Select-Page 0; Assert-Page 0 $true; Set-Control 203 ($draftName + ' unapplied')
    Select-Page 3; Assert-Page 3; Set-Control 121 ($configuredDefault + '-unapplied')
    Select-Page 1; Assert-Page 1 $false; Send-Control 105 $tbmSetPos 1 11 | Out-Null
    Close-Settings
    if ((Get-Content -LiteralPath $settingsPath -Raw) -ne $appliedBytes) { throw 'Close saved an unapplied settings edit' }
    Launch-Settings
    Doctor
    if ((Page-Index) -ne 1 -or (Page-Title) -ne 'Audio' -or [int](Send-Control 105 $tbmGetPos) -ne 37) { throw 'Reopening did not restore the applied Audio settings' }
    Select-Page 3; Assert-Page 3
    if ((Read-Control 121) -ne $configuredDefault) { throw 'Reopening did not restore the applied default voice ID.' }
    Select-Page 0; Assert-Page 0 $true
    if ((Read-Control 203) -ne $draftName) { throw 'Applied character draft did not survive reopening' }
    Save-ControlSnapshot 'page-characters-reopened' | Out-Null; Save-WindowPng 'page-characters-reopened'
    $minimum = [CivilizedSettingsLayoutNative]::MinimumSize($script:windowHandle)
    if ($minimum[0] -le 0 -or $minimum[1] -le 0) { throw 'The settings window reported no minimum tracking size' }
    if (-not [CivilizedSettingsLayoutNative]::SetWindowPos($script:windowHandle, [IntPtr]::Zero, 0, 0, $minimum[0], $minimum[1], 0x16)) { throw 'Could not request the enforced minimum-size resize' }
    Start-Sleep -Milliseconds 200
    Assert-VisibleChildren 'characters-minimum'; Assert-NoVisibleOverlaps 'characters-minimum'; Save-ControlSnapshot 'characters-minimum' | Out-Null; Save-WindowPng 'characters-minimum'
    foreach ($index in @(1, 2, 3, 4, 5)) {
        Select-Page $index
        Assert-Page $index
        if ($index -eq 3) { Assert-NoVisibleOverlaps 'speech-service-minimum' }
        if ($index -eq 5) { Assert-NoVisibleOverlaps 'announcements-minimum'; Assert-PromptRegion 'announcements-minimum' }
        Save-ControlSnapshot ('minimum-page-' + $index) | Out-Null
        Save-WindowPng ('minimum-page-' + $index)
    }
    Select-Page 0
    Assert-Page 0 $true
    if (-not [CivilizedSettingsLayoutNative]::SetWindowPos($script:windowHandle, [IntPtr]::Zero, 0, 0, 1200, 900, 0x16)) { throw 'Could not request large resize' }
    Start-Sleep -Milliseconds 200
    Assert-VisibleChildren 'characters-large'; Assert-NoVisibleOverlaps 'characters-large'; Save-ControlSnapshot 'characters-large' | Out-Null; Save-WindowPng 'characters-large'
    Select-Page 5
    Assert-Page 5
    Assert-NoVisibleOverlaps 'announcements-large'; Assert-PromptRegion 'announcements-large'; Save-ControlSnapshot 'announcements-large' | Out-Null; Save-WindowPng 'announcements-large'
    Copy-Item -LiteralPath $settingsPath -Destination (Join-Path $evidencePath 'settings-after.json')
    Close-Settings
    @{ passed = $true; binary = $binaryPath; sha256 = $binaryHash; screenshots = @('page-characters.png', 'page-audio.png', 'page-quiet-hours.png', 'page-speech-service.png', 'page-offline-voice.png', 'page-announcements.png', 'minimum-page-4.png', 'minimum-page-5.png', 'characters-minimum.png', 'characters-large.png', 'announcements-large.png'); launches = $launchCount } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidencePath 'result.json') -Encoding utf8NoBOM
    Write-Output "PASS: settings sidebar layout, native navigation, draft retention, resizing, preview, Apply, Close and reopen; evidence: $evidencePath"
} catch {
    if (Test-Path -LiteralPath $evidencePath) {
        $_ | Out-String | Set-Content -LiteralPath (Join-Path $evidencePath 'failure.txt') -Encoding utf8NoBOM
        $errors = Join-Path $scratch 'errors.log'
        if (Test-Path -LiteralPath $errors) { Copy-Item -LiteralPath $errors -Destination (Join-Path $evidencePath 'errors.log') -Force }
        if ($script:windowHandle -ne [IntPtr]::Zero) {
            try { Save-ControlSnapshot 'failure-controls' | Out-Null } catch { $_ | Out-String | Set-Content -LiteralPath (Join-Path $evidencePath 'failure-controls-error.txt') -Encoding utf8NoBOM }
            try { Save-WindowPng 'failure' } catch { $_ | Out-String | Set-Content -LiteralPath (Join-Path $evidencePath 'failure-screenshot-error.txt') -Encoding utf8NoBOM }
        }
    }
    throw
} finally {
    if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit(5000) | Out-Null }
    if ($scratchCreated -and (Test-Path -LiteralPath $scratch)) { Remove-Item -LiteralPath $scratch -Recurse -Force }
    if (Test-Path -LiteralPath $evidencePath) { @{ scratchRemoved = -not (Test-Path -LiteralPath $scratch); processExited = (-not $process -or $process.HasExited) } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath 'cleanup.json') -Encoding utf8NoBOM }
    if ($transcribing) { Stop-Transcript | Out-Null }
}
