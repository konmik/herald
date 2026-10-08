param(
    [string]$Binary = (Join-Path (Split-Path $PSScriptRoot -Parent) 'native-announcer/target/debug/civilized-announcer.exe'),
    [string]$Evidence = ('temp/verification/settings-theme-' + [guid]::NewGuid())
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$binaryPath = [IO.Path]::GetFullPath($Binary)
$evidencePath = if ([IO.Path]::IsPathRooted($Evidence)) { [IO.Path]::GetFullPath($Evidence) } else { [IO.Path]::GetFullPath((Join-Path $root $Evidence)) }
$scratch = Join-Path $env:LOCALAPPDATA ('Temp/opencode/civilized-settings-theme-' + [guid]::NewGuid())
$settingsPath = Join-Path $scratch 'settings.json'
$ttsPath = Join-Path $root 'native-announcer/resources/tts/kitten-nano-en-v0_8-int8'
$process = $null
$windowHandle = [IntPtr]::Zero
$ownedHwnd = [IntPtr]::Zero
$ownedPid = 0
$ownedStart = $null
$binaryHash = $null
$scratchCreated = $false
$transcribing = $false
$pageResults = @()
$notificationResults = @()
$dropdownResult = $null

Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CivilizedSettingsThemeCheckNative {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct HighContrastInfo { public uint Size, Flags; public IntPtr Scheme; }
    [StructLayout(LayoutKind.Sequential)] public struct ComboBoxInfo { public int Size; public Rect Item, Button; public int State; public IntPtr Combo, ItemWindow, List; }
    private delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll", EntryPoint = "SendMessageW")] public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)] public static extern IntPtr SendText(IntPtr window, uint message, IntPtr wparam, StringBuilder text);
    [DllImport("user32.dll", EntryPoint = "SendMessageW", CharSet = CharSet.Unicode)] public static extern IntPtr SetText(IntPtr window, uint message, IntPtr wparam, string text);
    [DllImport("user32.dll", EntryPoint = "GetClassNameW", CharSet = CharSet.Unicode)] private static extern int GetClassName(IntPtr window, StringBuilder text, int length);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] public static extern IntPtr SetFocus(IntPtr window);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] private static extern bool GetComboBoxInfo(IntPtr combo, ref ComboBoxInfo info);
    [DllImport("user32.dll")] public static extern bool RedrawWindow(IntPtr window, IntPtr rect, IntPtr region, uint flags);
    [DllImport("user32.dll", EntryPoint = "SystemParametersInfoW")] private static extern bool SystemParametersInfo(uint action, uint parameter, ref HighContrastInfo info, uint flags);
    [DllImport("user32.dll")] private static extern uint GetSysColor(int index);
    public static string WindowText(IntPtr window) { var length = SendMessage(window, 0xe, IntPtr.Zero, IntPtr.Zero).ToInt64(); var text = new StringBuilder((int)Math.Max(length + 1, 1)); SendText(window, 0xd, new IntPtr(text.Capacity), text); return text.ToString(); }
    public static string ClassName(IntPtr window) { var text = new StringBuilder(256); GetClassName(window, text, text.Capacity); return text.ToString(); }
    public static int[] Bounds(IntPtr window) { Rect rect; if (!GetWindowRect(window, out rect)) throw new InvalidOperationException("Could not read a window rectangle."); return new[] { rect.Left, rect.Top, rect.Right, rect.Bottom }; }
    public static IntPtr FindWindowForProcess(uint pid, string title) { IntPtr found = IntPtr.Zero; EnumWindows((window, parameter) => { uint owner; GetWindowThreadProcessId(window, out owner); if (owner == pid && IsWindowVisible(window) && WindowText(window) == title) { found = window; return false; } return true; }, IntPtr.Zero); return found; }
    public static IntPtr ComboList(IntPtr combo) { var info = new ComboBoxInfo { Size = Marshal.SizeOf(typeof(ComboBoxInfo)) }; return GetComboBoxInfo(combo, ref info) ? info.List : IntPtr.Zero; }
    public static uint HighContrastFlags() { var info = new HighContrastInfo { Size = (uint)Marshal.SizeOf(typeof(HighContrastInfo)) }; if (!SystemParametersInfo(0x42, info.Size, ref info, 0)) throw new InvalidOperationException("Could not read high contrast state."); return info.Flags; }
    public static uint SystemColor(int index) { return GetSysColor(index); }
    public static string[] ListBoxItems(IntPtr list) { var count = SendMessage(list, 0x18b, IntPtr.Zero, IntPtr.Zero).ToInt64(); if (count <= 0) return new string[0]; var items = new string[(int)count]; for (var i = 0; i < count; i++) { var length = SendMessage(list, 0x18a, new IntPtr(i), IntPtr.Zero).ToInt64(); var text = new StringBuilder((int)Math.Max(length + 1, 1)); SendText(list, 0x189, new IntPtr(i), text); items[i] = text.ToString(); } return items; }
}
'@

if ([CivilizedSettingsThemeCheckNative]::SetThreadDpiAwarenessContext([IntPtr](-4)) -eq [IntPtr]::Zero) { throw 'Could not use physical coordinates for the owned window capture.' }

$pageNames = @('Characters', 'Audio', 'Quiet hours', 'Speech service', 'Offline voice', 'Announcements')
$sampleControls = [ordered]@{
    Characters = [ordered]@{ id = 203; kind = 'input' }
    Audio = [ordered]@{ id = 106; kind = 'input' }
    'Quiet hours' = [ordered]@{ id = 103; kind = 'input' }
    'Speech service' = [ordered]@{ id = 121; kind = 'input' }
    'Offline voice' = [ordered]@{ id = 125; kind = 'button' }
    Announcements = [ordered]@{ id = 134; kind = 'input' }
}
$wmCommand = 0x111
$wmSettingChange = 0x1a
$wmSysColorChange = 0x15
$wmThemeChanged = 0x31a
$enChange = 0x300
$lbSetCurSel = 0x186
$lbGetCurSel = 0x188
$lbGetCount = 0x18b
$cbGetCount = 0x146
$cbShowDropdown = 0x14f
$cbGetDroppedState = 0x157
$bmClick = 0xf5

function Control([int]$Id) {
    $handle = [CivilizedSettingsThemeCheckNative]::GetDlgItem($script:windowHandle, $Id)
    if ($handle -eq [IntPtr]::Zero) { throw "Missing settings control $Id" }
    $handle
}
function Send-Control([int]$Id, [uint32]$Message, [long]$Wparam = 0, [long]$Lparam = 0) { [CivilizedSettingsThemeCheckNative]::SendMessage((Control $Id), $Message, [IntPtr]$Wparam, [IntPtr]$Lparam).ToInt64() }
function Send-Window([uint32]$Message, [long]$Wparam = 0, [long]$Lparam = 0) { [CivilizedSettingsThemeCheckNative]::SendMessage($script:windowHandle, $Message, [IntPtr]$Wparam, [IntPtr]$Lparam).ToInt64() }
function Read-Control([int]$Id) { [CivilizedSettingsThemeCheckNative]::WindowText((Control $Id)) }
function Wait-Until([scriptblock]$Condition, [string]$Failure, [int]$Seconds = 10) { $deadline = [DateTime]::UtcNow.AddSeconds($Seconds); while (-not (& $Condition)) { if ([DateTime]::UtcNow -ge $deadline) { throw $Failure }; Start-Sleep -Milliseconds 50 } }
function Normalize-Text([string]$Value) { $Value.Replace("`r`n", "`n").Replace("`r", "`n") }
function Set-Control-Text([int]$Id, [string]$Value) { $native = if ($Id -eq 134) { $Value.Replace("`n", "`r`n") } else { $Value }; if ([CivilizedSettingsThemeCheckNative]::SetText((Control $Id), 0xc, [IntPtr]::Zero, $native) -eq [IntPtr]::Zero) { throw "Could not set control $Id" }; $actual = if ($Id -eq 134) { Normalize-Text (Read-Control $Id) } else { Read-Control $Id }; if ($actual -cne $Value) { throw "Control $Id did not retain its draft text" } }
function Notify-Control([int]$Id) { $handle = Control $Id; Send-Window $wmCommand (([long]$Id) -bor (([long]$enChange) -shl 16)) $handle.ToInt64() | Out-Null }
function Page-Index { [int](Send-Control 200 $lbGetCurSel) }
function Select-Page([int]$Index) { $sidebar = Control 200; [CivilizedSettingsThemeCheckNative]::SetFocus($sidebar) | Out-Null; Send-Control 200 $lbSetCurSel $Index | Out-Null; Send-Window $wmCommand (([long]200) -bor (1 -shl 16)) $sidebar.ToInt64() | Out-Null; Wait-Until { (Page-Index) -eq $Index -and (Read-Control 400) -ceq $pageNames[$Index] } "Could not select settings page '$($pageNames[$Index])'." }
function Assert-NoErrors { $path = Join-Path $scratch 'errors.log'; if (Test-Path -LiteralPath $path) { $text = Get-Content -LiteralPath $path -Raw; if (-not [string]::IsNullOrWhiteSpace($text)) { throw "Settings errors.log: $text" } } }
function Rgb([int]$Red, [int]$Green, [int]$Blue) { [uint32]($Red -bor ($Green -shl 8) -bor ($Blue -shl 16)) }
function Color-Data([uint32]$Value) { $red = [int]($Value -band 0xff); $green = [int](($Value -shr 8) -band 0xff); $blue = [int](($Value -shr 16) -band 0xff); [ordered]@{ r = $red; g = $green; b = $blue; luma = [math]::Round(0.2126 * $red + 0.7152 * $green + 0.0722 * $blue, 2) } }
function Pixel-Data($Pixel) { Color-Data ([uint32]([int]$Pixel.R -bor ([int]$Pixel.G -shl 8) -bor ([int]$Pixel.B -shl 16))) }
function Color-Distance($Left, $Right) { [math]::Max([math]::Abs([int]$Left.r - [int]$Right.r), [math]::Max([math]::Abs([int]$Left.g - [int]$Right.g), [math]::Abs([int]$Left.b - [int]$Right.b))) }
function Box([int]$Left, [int]$Top, [int]$Right, [int]$Bottom) { [pscustomobject]@{ left = $Left; top = $Top; right = $Right; bottom = $Bottom } }
function Sample-Stats($Bitmap, $Region, [int]$Step = 2) {
    $left = [math]::Max(0, [int]$Region.left); $top = [math]::Max(0, [int]$Region.top); $right = [math]::Min($Bitmap.Width, [int]$Region.right); $bottom = [math]::Min($Bitmap.Height, [int]$Region.bottom)
    if ($right -le $left -or $bottom -le $top) { throw 'The screenshot sample region is empty.' }
    $counts = @{}; $count = 0; $total = 0.0; $min = 999.0; $max = -1.0
    for ($y = $top; $y -lt $bottom; $y += [math]::Max(1, $Step)) { for ($x = $left; $x -lt $right; $x += [math]::Max(1, $Step)) { $color = Pixel-Data ($Bitmap.GetPixel($x, $y)); $key = "$($color.r),$($color.g),$($color.b)"; if ($counts.ContainsKey($key)) { $counts[$key]++ } else { $counts[$key] = 1 }; $count++; $total += $color.luma; $min = [math]::Min($min, $color.luma); $max = [math]::Max($max, $color.luma) } }
    $modeEntry = @($counts.GetEnumerator() | Sort-Object Value -Descending)[0]; $parts = $modeEntry.Key.Split(','); [ordered]@{ count = $count; unique = $counts.Count; mode = [ordered]@{ r = [int]$parts[0]; g = [int]$parts[1]; b = [int]$parts[2]; luma = [math]::Round(0.2126 * [int]$parts[0] + 0.7152 * [int]$parts[1] + 0.0722 * [int]$parts[2], 2) }; modeCount = [int]$modeEntry.Value; meanLuma = [math]::Round($total / $count, 2); minLuma = [math]::Round($min, 2); maxLuma = [math]::Round($max, 2) }
}
function Assert-Background($Bitmap, $Region, $Expected, [string]$Name) { $stats = Sample-Stats $Bitmap $Region; $distance = Color-Distance $stats.mode $Expected; if ($distance -gt 20) { throw "$Name background was $($stats.mode.r),$($stats.mode.g),$($stats.mode.b), expected $($Expected.r),$($Expected.g),$($Expected.b)." }; [ordered]@{ expected = $Expected; distance = $distance; stats = $stats } }
function Assert-Text($Bitmap, $Region, $Background, $Foregrounds, [string]$Name) {
    $left = [math]::Max(0, [int]$Region.left + 5); $top = [math]::Max(0, [int]$Region.top + 4); $right = [math]::Min($Bitmap.Width, [int]$Region.right - 5); $bottom = [math]::Min($Bitmap.Height, [int]$Region.bottom - 4); $foregroundPixels = 0; $contrastPixels = 0
    for ($y = $top; $y -lt $bottom; $y += 2) { for ($x = $left; $x -lt $right; $x += 2) { $pixel = Pixel-Data ($Bitmap.GetPixel($x, $y)); $backgroundDistance = Color-Distance $pixel $Background; $best = 999; foreach ($foreground in @($Foregrounds)) { $best = [math]::Min($best, (Color-Distance $pixel $foreground)) }; if ($backgroundDistance -ge 18 -and $best -le 150) { $foregroundPixels++ }; if ($backgroundDistance -ge 35) { $contrastPixels++ } } }
    if ($foregroundPixels -lt 5 -or $contrastPixels -lt 5) { throw "$Name text was not visible with screenshot contrast (foreground=$foregroundPixels, contrast=$contrastPixels)." }
    [ordered]@{ foregroundPixels = $foregroundPixels; contrastPixels = $contrastPixels }
}
function Capture-Window([IntPtr]$Handle, [string]$Name) {
    $owner = [uint32]0
    [CivilizedSettingsThemeCheckNative]::GetWindowThreadProcessId($Handle, [ref]$owner) | Out-Null
    if ($owner -ne $ownedPid -or -not [CivilizedSettingsThemeCheckNative]::IsWindowVisible($Handle)) { throw "The capture target '$Name' is not an owned visible window." }
    if (-not [CivilizedSettingsThemeCheckNative]::SetWindowPos($Handle, [IntPtr](-1), 0, 0, 0, 0, 0x13)) { throw "Could not bring the owned capture target '$Name' above other windows." }
    [CivilizedSettingsThemeCheckNative]::RedrawWindow($Handle, [IntPtr]::Zero, [IntPtr]::Zero, 0x585) | Out-Null
    Start-Sleep -Milliseconds 100
    $bounds = [CivilizedSettingsThemeCheckNative]::Bounds($Handle); $width = $bounds[2] - $bounds[0]; $height = $bounds[3] - $bounds[1]; if ($width -le 0 -or $height -le 0) { throw "The capture target '$Name' has no size." }
    $bitmap = $null; $graphics = $null
    try {
        $bitmap = [Drawing.Bitmap]::new($width, $height, [Drawing.Imaging.PixelFormat]::Format32bppArgb); $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $graphics.CopyFromScreen($bounds[0], $bounds[1], 0, 0, [Drawing.Size]::new($width, $height))
        $graphics.Dispose(); $graphics = $null; $path = Join-Path $evidencePath ($Name + '.png'); $bitmap.Save($path, [Drawing.Imaging.ImageFormat]::Png) | Out-Null; if (-not (Test-Path -LiteralPath $path)) { throw "The native capture '$Name' was not written." }
        [pscustomobject]@{ bitmap = $bitmap; path = $path; width = $width; height = $height; method = 'owned-desktop-window' }
    } catch { if ($graphics) { $graphics.Dispose() }; if ($bitmap) { $bitmap.Dispose() }; throw }
}
function Control-Box([int]$Id) { $window = [CivilizedSettingsThemeCheckNative]::Bounds($script:windowHandle); $control = [CivilizedSettingsThemeCheckNative]::Bounds((Control $Id)); Box ($control[0] - $window[0]) ($control[1] - $window[1]) ($control[2] - $window[0]) ($control[3] - $window[1]) }
function Sample-Box([int]$Id) { $box = Control-Box $Id; if ([CivilizedSettingsThemeCheckNative]::ClassName((Control $Id)) -match 'COMBOBOX') { $box.bottom = [math]::Min($box.bottom, $box.top + 36) }; $box }
function Read-System-Theme {
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'; $property = Get-ItemProperty -LiteralPath $path -ErrorAction SilentlyContinue; $value = $null; if ($property -and ($property.PSObject.Properties.Name -contains 'AppsUseLightTheme')) { try { $value = [int]$property.AppsUseLightTheme } catch { $value = $null } }
    $flags = [uint32]([CivilizedSettingsThemeCheckNative]::HighContrastFlags()); $highContrast = ([uint64]$flags -band 1) -ne 0; $expected = if ($highContrast) { 'high-contrast' } elseif ($value -eq 0) { 'dark' } else { 'light' }
    if ($highContrast) { $palette = [ordered]@{ main = Color-Data ([CivilizedSettingsThemeCheckNative]::SystemColor(5)); sidebar = Color-Data ([CivilizedSettingsThemeCheckNative]::SystemColor(5)); input = Color-Data ([CivilizedSettingsThemeCheckNative]::SystemColor(5)); button = Color-Data ([CivilizedSettingsThemeCheckNative]::SystemColor(5)); text = Color-Data ([CivilizedSettingsThemeCheckNative]::SystemColor(8)); selectedText = Color-Data ([CivilizedSettingsThemeCheckNative]::SystemColor(14)) } }
    elseif ($expected -eq 'dark') { $palette = [ordered]@{ main = Color-Data (Rgb 30 30 30); sidebar = Color-Data (Rgb 37 37 38); input = Color-Data (Rgb 43 43 43); button = Color-Data (Rgb 51 51 51); text = Color-Data (Rgb 241 241 241); selectedText = Color-Data (Rgb 255 255 255) } }
    else { $palette = [ordered]@{ main = Color-Data (Rgb 255 255 255); sidebar = Color-Data (Rgb 246 248 251); input = Color-Data (Rgb 255 255 255); button = Color-Data (Rgb 240 240 240); text = Color-Data (Rgb 32 37 43); selectedText = Color-Data (Rgb 25 80 160) } }
    $script:palette = $palette; $script:expectedTheme = $expected; [ordered]@{ registryPath = $path; appsUseLightTheme = $value; expectedTheme = $expected; highContrastFlags = $flags; highContrast = $highContrast; palette = $palette; readAt = [DateTime]::UtcNow.ToString('o') }
}
function Doctor {
    if (-not $script:process) { throw 'Settings process is missing.' }; $script:process.Refresh(); if ($script:process.HasExited) { throw "Settings exited with $($script:process.ExitCode)." }; if ($script:process.Id -ne $ownedPid) { throw 'The settings process identity changed.' }; if (-not [string]::Equals([IO.Path]::GetFullPath($script:process.Path), $binaryPath, [StringComparison]::OrdinalIgnoreCase)) { throw "Settings process path is '$($script:process.Path)'." }
    $owner = [uint32]0; [CivilizedSettingsThemeCheckNative]::GetWindowThreadProcessId($script:windowHandle, [ref]$owner) | Out-Null; $title = [CivilizedSettingsThemeCheckNative]::WindowText($script:windowHandle); if ($owner -ne [uint32]$ownedPid -or $title -cne 'Civilized Agent settings') { throw "Owned settings window is invalid: hwnd=$($script:windowHandle.ToInt64()), pid=$owner, title='$title'." }
    Assert-NoErrors; foreach ($id in @(107, 108)) { $handle = Control $id; if (-not [CivilizedSettingsThemeCheckNative]::IsWindowVisible($handle) -or -not [CivilizedSettingsThemeCheckNative]::IsWindowEnabled($handle)) { throw "Settings control $id is not ready." } }; if ((Read-Control 107) -cne 'Apply' -or (Read-Control 108) -cne 'Close') { throw 'Apply and Close are not ready.' }
}
function Launch-Settings {
    $info = [Diagnostics.ProcessStartInfo]::new($binaryPath); $info.UseShellExecute = $false; $info.Environment['CIVILIZED_AGENT_DATA'] = $scratch; $info.Environment['CIVILIZED_AGENT_TTS'] = $ttsPath; $info.Environment['CIVILIZED_AGENT_ENV'] = (Join-Path $scratch 'empty.env'); $info.Environment['ELEVENLABS_API_KEY'] = ''; $info.ArgumentList.Add('--settings'); $info.ArgumentList.Add('--assets'); $info.ArgumentList.Add((Join-Path $root 'native-announcer/resources'))
    $script:process = [Diagnostics.Process]::Start($info); $script:ownedPid = $script:process.Id; $script:ownedStart = $script:process.StartTime; $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline) { $script:process.Refresh(); if ($script:process.HasExited) { Assert-NoErrors; throw "Settings exited early with $($script:process.ExitCode)." }; $candidate = [CivilizedSettingsThemeCheckNative]::FindWindowForProcess([uint32]$ownedPid, 'Civilized Agent settings'); if ($candidate -ne [IntPtr]::Zero) { $script:windowHandle = $candidate; $script:ownedHwnd = $candidate; $apply = [CivilizedSettingsThemeCheckNative]::GetDlgItem($candidate, 107); $close = [CivilizedSettingsThemeCheckNative]::GetDlgItem($candidate, 108); if ($apply -ne [IntPtr]::Zero -and $close -ne [IntPtr]::Zero -and [CivilizedSettingsThemeCheckNative]::WindowText($apply) -eq 'Apply' -and [CivilizedSettingsThemeCheckNative]::WindowText($close) -eq 'Close') { Doctor; $identity = [ordered]@{ binary = $binaryPath; sha256 = $binaryHash; pid = $ownedPid; started = $ownedStart.ToUniversalTime().ToString('o'); hwnd = $windowHandle.ToInt64(); scratch = $scratch }; $identity | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidencePath 'instance.json') -Encoding utf8NoBOM; return } }; Start-Sleep -Milliseconds 75 }
    Assert-NoErrors; throw 'The owned settings window did not become ready with its title and Apply/Close controls.'
}
function Assert-Page([int]$Index) { $name = $pageNames[$Index]; if ((Page-Index) -ne $Index -or (Read-Control 400) -cne $name) { throw "Expected settings page '$name'." }; $sample = $sampleControls[$name]; $sampleHandle = Control $sample.id; if (-not [CivilizedSettingsThemeCheckNative]::IsWindowVisible($sampleHandle)) { throw "Theme sample control $($sample.id) is hidden on '$name'." }; foreach ($id in @(107, 108)) { if (-not [CivilizedSettingsThemeCheckNative]::IsWindowVisible((Control $id))) { throw "Footer control $id is hidden on '$name'." } } }
function Verify-Page([int]$Index) {
    Select-Page $Index; Assert-Page $Index; Start-Sleep -Milliseconds 75; [CivilizedSettingsThemeCheckNative]::RedrawWindow($windowHandle, [IntPtr]::Zero, [IntPtr]::Zero, 0x585) | Out-Null; $name = $pageNames[$Index]; $capture = Capture-Window $windowHandle ('page-' + $Index + '-' + $name.ToLowerInvariant().Replace(' ', '-'))
    try {
        $status = Control-Box 109; $main = Box ($status.right + 20) ($status.top - 38) ($status.right + 48) ($status.top - 8); $sidebar = Control-Box 200; $sample = $sampleControls[$name]; $sampleBox = Sample-Box $sample.id; $sampleBackground = if ($sample.kind -eq 'button') { $script:palette.button } else { $script:palette.input }; $backgrounds = [ordered]@{ main = Assert-Background $capture.bitmap $main $script:palette.main "$name main"; sidebar = Assert-Background $capture.bitmap $sidebar $script:palette.sidebar "$name sidebar"; sample = Assert-Background $capture.bitmap $sampleBox $sampleBackground "$name control $($sample.id)"; apply = Assert-Background $capture.bitmap (Control-Box 107) $script:palette.button "$name Apply"; close = Assert-Background $capture.bitmap (Control-Box 108) $script:palette.button "$name Close" }; $foregrounds = @($script:palette.text); $text = [ordered]@{ title = Assert-Text $capture.bitmap (Control-Box 400) $script:palette.main $foregrounds "$name page title"; sample = Assert-Text $capture.bitmap $sampleBox $sampleBackground $foregrounds "$name control $($sample.id)"; apply = Assert-Text $capture.bitmap (Control-Box 107) $script:palette.button $foregrounds "$name Apply"; close = Assert-Text $capture.bitmap (Control-Box 108) $script:palette.button $foregrounds "$name Close" }
        [ordered]@{ name = $name; pageIndex = $Index; capture = [ordered]@{ path = $capture.path; width = $capture.width; height = $capture.height; method = $capture.method }; backgrounds = $backgrounds; text = $text }
    } finally { $capture.bitmap.Dispose() }
}
function Open-Dropdown {
    Select-Page 3; $id = 113; $combo = Control $id; if ((Send-Control $id $cbGetCount) -le 0) { throw 'Speech model combo has no items.' }; [CivilizedSettingsThemeCheckNative]::SetFocus($combo) | Out-Null; Send-Control $id $cbShowDropdown 1 | Out-Null
    try {
        Wait-Until { (Send-Control $id $cbGetDroppedState) -ne 0 } 'The speech model dropdown did not open.' 3; Wait-Until { $list = [CivilizedSettingsThemeCheckNative]::ComboList($combo); $list -ne [IntPtr]::Zero -and [CivilizedSettingsThemeCheckNative]::IsWindowVisible($list) } 'The opened combo list window was not visible.' 3; $list = [CivilizedSettingsThemeCheckNative]::ComboList($combo); $capture = Capture-Window $list 'combo-dropdown'
        try { $stats = Sample-Stats $capture.bitmap (Box 0 0 $capture.width $capture.height); $allowed = @($script:palette.input, $script:palette.main, $script:palette.sidebar, $script:palette.button); $distance = @($allowed | ForEach-Object { Color-Distance $stats.mode $_ }); $minimum = ($distance | Measure-Object -Minimum).Minimum; if ($minimum -gt 24) { throw 'The opened combo dropdown has no expected themed background pixels.' }; $text = Assert-Text $capture.bitmap (Box 0 0 $capture.width $capture.height) $stats.mode @($script:palette.text, $script:palette.selectedText) 'Opened combo dropdown'; [ordered]@{ id = $id; class = [CivilizedSettingsThemeCheckNative]::ClassName($combo); dropped = $true; listHwnd = $list.ToInt64(); capture = [ordered]@{ path = $capture.path; width = $capture.width; height = $capture.height; method = $capture.method }; background = [ordered]@{ stats = $stats; nearestExpectedDistance = $minimum }; text = $text } } finally { $capture.bitmap.Dispose() }
    } finally { Send-Control $id $cbShowDropdown 0 | Out-Null; Wait-Until { (Send-Control $id $cbGetDroppedState) -eq 0 } 'The speech model dropdown did not close.' 3 }
}
function Drafts { [ordered]@{ character = Read-Control 203; start = Read-Control 103; voice = Read-Control 121; prompt = Normalize-Text (Read-Control 134) } }
function Assert-Drafts($Expected, [string]$Message) { $actual = Drafts; foreach ($key in $Expected.Keys) { if ($actual[$key] -cne $Expected[$key]) { throw "$Message lost the $key draft." } }; $actual }
function Close-Settings { Doctor; Send-Control 108 $bmClick | Out-Null; if (-not $script:process.WaitForExit(5000) -or $script:process.ExitCode -ne 0) { throw 'Close did not exit the owned settings process cleanly.' }; $script:windowHandle = [IntPtr]::Zero }

try {
    if (-not (Test-Path -LiteralPath $binaryPath)) { throw "Build or provide the settings binary: $binaryPath" }
    if (-not (Test-Path -LiteralPath $ttsPath)) { throw "Existing TTS assets are missing at $ttsPath" }
    if (Test-Path -LiteralPath $evidencePath) { throw "Evidence directory already exists: $evidencePath" }
    if (Test-Path -LiteralPath $scratch) { throw "Scratch directory already exists: $scratch" }
    New-Item -ItemType Directory -Path $evidencePath -Force | Out-Null; New-Item -ItemType Directory -Path $scratch -Force | Out-Null; $scratchCreated = $true; Set-Content -LiteralPath (Join-Path $scratch 'empty.env') -Value '' -Encoding utf8NoBOM; Start-Transcript -Path (Join-Path $evidencePath 'actions.txt') | Out-Null; $transcribing = $true
    $binaryHash = (Get-FileHash $binaryPath -Algorithm SHA256).Hash; $systemTheme = Read-System-Theme; $systemTheme | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $evidencePath 'system-theme.json') -Encoding utf8NoBOM
    $fixture = [ordered]@{ quietMode = $false; scheduleEnabled = $true; quietStart = 1320; quietEnd = 480; volume = 35; outputDevice = $null; speechModel = 'eleven_flash_v2_5'; defaultVoiceId = 'JBFqnCBsd6RMkjVDRZzb'; voices = @{ claude = 'Mark'; opencode = 'Luna' }; characters = @{ 'theme-verification-character' = @{ name = 'Theme verification character'; selected = $true; animationPath = $null; summaryPrompt = '' } }; selectedCharacter = 'theme-verification-character'; announcementBodyFont = @{ family = 'Segoe UI'; size = 16 }; announcementTitleFont = @{ family = 'Segoe UI'; size = 26 }; summaryPrompt = 'Theme verification summary prompt.' }
    $fixture | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $settingsPath -Encoding utf8NoBOM; Copy-Item -LiteralPath $settingsPath -Destination (Join-Path $evidencePath 'settings-before.json'); $beforeSettings = Get-Content -LiteralPath $settingsPath -Raw
    Launch-Settings; $items = @([CivilizedSettingsThemeCheckNative]::ListBoxItems((Control 200))); if ($items.Count -ne 6 -or ($items -join '|') -ne ($pageNames -join '|')) { throw 'The settings sidebar did not expose all six pages.' }; Select-Page 0; if (-not [CivilizedSettingsThemeCheckNative]::IsWindowVisible((Control 203))) { $list = Control 201; if ((Send-Control 201 $lbGetCount) -le 0) { throw 'The Characters page has no character entry.' }; Send-Control 201 $lbSetCurSel 0 | Out-Null; Send-Window $wmCommand (([long]201) -bor (1 -shl 16)) $list.ToInt64() | Out-Null; Wait-Until { [CivilizedSettingsThemeCheckNative]::IsWindowVisible((Control 203)) } 'The character editor did not become visible.' }
    foreach ($index in 0..5) { $pageResults += Verify-Page $index }; $dropdownResult = Open-Dropdown
    $characterDraft = 'Theme notification character'; $startDraft = '21:17'; $voiceDraft = 'theme-notification-voice'; $promptDraft = 'Theme notification draft ' + [guid]::NewGuid().ToString('N'); Select-Page 0; Set-Control-Text 203 $characterDraft; Notify-Control 203; Select-Page 2; Set-Control-Text 103 $startDraft; Notify-Control 103; Select-Page 3; Set-Control-Text 121 $voiceDraft; Notify-Control 121; Select-Page 5; Set-Control-Text 134 $promptDraft; Notify-Control 134; if ((Get-Content -LiteralPath $settingsPath -Raw) -cne $beforeSettings) { throw 'Draft setup changed settings before Apply.' }; $drafts = [ordered]@{ character = $characterDraft; start = $startDraft; voice = $voiceDraft; prompt = $promptDraft }
    foreach ($message in @([ordered]@{ name = 'WM_SETTINGCHANGE'; value = $wmSettingChange }, [ordered]@{ name = 'WM_THEMECHANGED'; value = $wmThemeChanged }, [ordered]@{ name = 'WM_SYSCOLORCHANGE'; value = $wmSysColorChange })) { $returnValue = Send-Window $message.value; Start-Sleep -Milliseconds 125; $actual = Assert-Drafts $drafts $message.name; if ((Get-Content -LiteralPath $settingsPath -Raw) -cne $beforeSettings) { throw "$($message.name) saved unapplied settings." }; $notificationResults += [ordered]@{ message = $message.name; returnValue = $returnValue; drafts = $actual; settingsUnchanged = $true } }
    [CivilizedSettingsThemeCheckNative]::RedrawWindow($windowHandle, [IntPtr]::Zero, [IntPtr]::Zero, 0x585) | Out-Null; $notificationCapture = Capture-Window $windowHandle 'after-theme-notifications'; $notificationCaptureInfo = [ordered]@{ path = $notificationCapture.path; width = $notificationCapture.width; height = $notificationCapture.height; method = $notificationCapture.method }; $notificationCapture.bitmap.Dispose(); $notificationResults | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $evidencePath 'theme-notifications.json') -Encoding utf8NoBOM
    Close-Settings; $afterSettings = Get-Content -LiteralPath $settingsPath -Raw; Copy-Item -LiteralPath $settingsPath -Destination (Join-Path $evidencePath 'settings-after.json'); if ($afterSettings -cne $beforeSettings) { throw 'Close saved unapplied settings.' }; Assert-NoErrors
    $result = [ordered]@{ verdict = 'VERIFIED'; binary = $binaryPath; sha256 = $binaryHash; systemTheme = $systemTheme; process = [ordered]@{ pid = $ownedPid; started = $ownedStart.ToUniversalTime().ToString('o'); hwnd = $ownedHwnd.ToInt64() }; pages = $pageResults; comboDropdown = $dropdownResult; themeNotifications = $notificationResults; notificationCapture = $notificationCaptureInfo; drafts = $drafts; screenshots = @($pageResults | ForEach-Object { $_.capture.path }) + @($dropdownResult.capture.path, $notificationCaptureInfo.path) }
    $result | ConvertTo-Json -Depth 24 | Set-Content -LiteralPath (Join-Path $evidencePath 'result.json') -Encoding utf8NoBOM; Write-Output "VERIFIED: $($systemTheme.expectedTheme) Settings theme, six page captures, dropdown pixels, native color/text contrast, and notification draft retention; evidence: $evidencePath"
}
catch {
    if (Test-Path -LiteralPath $evidencePath) { $_ | Out-String | Set-Content -LiteralPath (Join-Path $evidencePath 'failure.txt') -Encoding utf8NoBOM; $errors = Join-Path $scratch 'errors.log'; if (Test-Path -LiteralPath $errors) { Copy-Item -LiteralPath $errors -Destination (Join-Path $evidencePath 'errors.log') -Force }; if ($windowHandle -ne [IntPtr]::Zero) { try { $failedCapture = Capture-Window $windowHandle 'failure'; $failedCapture.bitmap.Dispose() } catch {} } }
    throw
}
finally {
    $processExited = $true; $ownershipConfirmed = $false; $killAttempted = $false
    if ($process) { try { $process.Refresh(); if ($process.HasExited) { $ownershipConfirmed = $true } else { $same = $process.Id -eq $ownedPid -and [string]::Equals([IO.Path]::GetFullPath($process.Path), $binaryPath, [StringComparison]::OrdinalIgnoreCase) -and ([math]::Abs(($process.StartTime.ToUniversalTime() - $ownedStart.ToUniversalTime()).TotalMilliseconds) -lt 1000); $ownershipConfirmed = $same; if ($same) { $killAttempted = $true; $process.Kill(); $process.WaitForExit(5000) | Out-Null }; $process.Refresh() }; $processExited = $process.HasExited } catch { $processExited = $false } }
    $scratchRemoved = $true; if ($scratchCreated -and (Test-Path -LiteralPath $scratch)) { try { Remove-Item -LiteralPath $scratch -Recurse -Force } catch { $scratchRemoved = $false } }; if (Test-Path -LiteralPath $evidencePath) { [ordered]@{ scratchRemoved = $scratchRemoved -and -not (Test-Path -LiteralPath $scratch); processExited = $processExited; ownershipConfirmed = $ownershipConfirmed; killAttempted = $killAttempted } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath 'cleanup.json') -Encoding utf8NoBOM }; if ($transcribing) { try { Stop-Transcript | Out-Null } catch {} }
}
