param(
    [string]$Binary = (Join-Path (Split-Path $PSScriptRoot -Parent) 'native-announcer/target/debug/herald.exe'),
    [string]$Evidence = ('temp/verification/settings-status-' + [guid]::NewGuid())
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$binaryPath = [IO.Path]::GetFullPath($Binary)
$evidencePath = if ([IO.Path]::IsPathRooted($Evidence)) { [IO.Path]::GetFullPath($Evidence) } else { [IO.Path]::GetFullPath((Join-Path $root $Evidence)) }
$scratch = Join-Path $env:LOCALAPPDATA ('Temp/opencode/settings-status-' + [guid]::NewGuid())
$settingsPath = Join-Path $scratch 'settings.json'
$process = $null
$windowHandle = [IntPtr]::Zero
$ownedPid = 0
$ownedStart = $null
$launchCount = 0
$scratchCreated = $false
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class SettingsStatusNative {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct MinMax { public Point Reserved, MaxSize, MaxPosition, MinTrackSize, MaxTrackSize; }
    [StructLayout(LayoutKind.Sequential)] public struct TextMetric { public int Height, Ascent, Descent, InternalLeading, ExternalLeading, AveCharWidth, MaxCharWidth, Weight, Overhang, DigitizedAspectX, DigitizedAspectY; public ushort FirstChar, LastChar, DefaultChar, BreakChar; public byte Italic, Underlined, StruckOut, PitchAndFamily, CharSet; }
    public sealed class Child { public int Id, Left, Top, Right, Bottom; public bool Visible; }
    private delegate bool EnumChildProc(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr window, int id);
    [DllImport("user32.dll", EntryPoint="SendMessageW")] public static extern IntPtr Send(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll", EntryPoint="SendMessageW", CharSet=CharSet.Unicode)] private static extern IntPtr SendText(IntPtr window, uint message, IntPtr wparam, StringBuilder text);
    [DllImport("user32.dll", EntryPoint="SendMessageW")] private static extern IntPtr SendRect(IntPtr window, uint message, IntPtr wparam, ref Rect rect);
    [DllImport("user32.dll", EntryPoint="SendMessageW")] private static extern IntPtr SendSelection(IntPtr window, uint message, ref int start, ref int end);
    [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW")] public static extern IntPtr Style(IntPtr window, int index);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref Point point);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr window);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] private static extern bool EnumChildWindows(IntPtr window, EnumChildProc callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] public static extern bool RedrawWindow(IntPtr window, IntPtr rect, IntPtr region, uint flags);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    [DllImport("user32.dll")] private static extern IntPtr GetDC(IntPtr window);
    [DllImport("user32.dll")] private static extern int ReleaseDC(IntPtr window, IntPtr dc);
    [DllImport("gdi32.dll")] private static extern IntPtr SelectObject(IntPtr dc, IntPtr value);
    [DllImport("gdi32.dll")] private static extern bool GetTextMetricsW(IntPtr dc, out TextMetric metric);
    [DllImport("user32.dll", EntryPoint="DrawTextW", CharSet=CharSet.Unicode)] private static extern int DrawText(IntPtr dc, string text, int count, ref Rect rect, uint flags);
    public static string Text(IntPtr window) { var text = new StringBuilder(32768); SendText(window, 0xD, new IntPtr(text.Capacity), text); return text.ToString(); }
    public static Rect EditRect(IntPtr window) { var rect = new Rect(); SendRect(window, 0xB2, IntPtr.Zero, ref rect); return rect; }
    public static int[] Selection(IntPtr window) { var start = 0; var end = 0; SendSelection(window, 0xB0, ref start, ref end); return new[] { start, end }; }
    public static Point Minimum(IntPtr window) { var memory = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(MinMax))); try { Send(window, 0x24, IntPtr.Zero, memory); return Marshal.PtrToStructure<MinMax>(memory).MinTrackSize; } finally { Marshal.FreeHGlobal(memory); } }
    public static Point CharacterPosition(IntPtr window, int character) { var value = Send(window, 0xD6, new IntPtr(character), IntPtr.Zero).ToInt64(); return new Point { X = (short)(value & 0xffff), Y = (short)((value >> 16) & 0xffff) }; }
    public static int[] MeasureWrappedText(IntPtr window, string text, int width) {
        var dc = GetDC(window); if (dc == IntPtr.Zero) throw new InvalidOperationException("Could not get the status device context.");
        var previous = SelectObject(dc, Send(window, 0x31, IntPtr.Zero, IntPtr.Zero));
        try { TextMetric metric; if (!GetTextMetricsW(dc, out metric)) throw new InvalidOperationException("Could not read the status font metrics."); var rect = new Rect { Right = width }; DrawText(dc, text, text.Length, ref rect, 0x400 | 0x10 | 0x800 | 0x2000); return new[] { metric.Height, rect.Right - rect.Left, rect.Bottom - rect.Top }; }
        finally { SelectObject(dc, previous); ReleaseDC(window, dc); }
    }
    public static Child[] Children(IntPtr parent) {
        var origin = new Point(); if (!ClientToScreen(parent, ref origin)) throw new InvalidOperationException("Could not find the settings client origin.");
        var children = new List<Child>();
        EnumChildWindows(parent, (window, parameter) => { if (GetParent(window) != parent) return true; Rect rect; if (!GetWindowRect(window, out rect)) return true; children.Add(new Child { Id = GetDlgCtrlID(window), Visible = IsWindowVisible(window), Left = rect.Left - origin.X, Top = rect.Top - origin.Y, Right = rect.Right - origin.X, Bottom = rect.Bottom - origin.Y }); return true; }, IntPtr.Zero);
        return children.ToArray();
    }
}
'@
$bmClick = 0xF5
$expectedPrefix = 'Preview failed: Missing Kitten TTS asset:'
function Control([int]$Id) { $handle = [SettingsStatusNative]::GetDlgItem($script:windowHandle, $Id); if ($handle -eq [IntPtr]::Zero) { throw "Missing settings control $Id" }; $handle }
function Read-Control([int]$Id) { [SettingsStatusNative]::Text((Control $Id)) }
function Send-Control([int]$Id, [uint32]$Message, [long]$Wparam = 0, [long]$Lparam = 0) { [SettingsStatusNative]::Send((Control $Id), $Message, [IntPtr]$Wparam, [IntPtr]$Lparam).ToInt64() }
function Wait-Until([scriptblock]$Condition, [string]$Failure, [int]$Seconds = 10) { $deadline = [DateTime]::UtcNow.AddSeconds($Seconds); while (-not (& $Condition)) { if ([DateTime]::UtcNow -ge $deadline) { throw $Failure }; Start-Sleep -Milliseconds 50 } }
function Client-Rect { $rect = [SettingsStatusNative+Rect]::new(); if (-not [SettingsStatusNative]::GetClientRect($script:windowHandle, [ref]$rect)) { throw 'Could not read the settings client rectangle.' }; $rect }
function Children { @([SettingsStatusNative]::Children($script:windowHandle) | Where-Object Visible | ForEach-Object { [ordered]@{ id = $_.Id; left = $_.Left; top = $_.Top; right = $_.Right; bottom = $_.Bottom } }) }
function Point-Object($Point) { [ordered]@{ x = [int]$Point.X; y = [int]$Point.Y } }
function Wait-Layout { Start-Sleep -Milliseconds 200; Wait-Until { $client = Client-Rect; $status = @(Children | Where-Object { $_.id -eq 109 })[0]; $status -and (($status.right - $status.left) -eq ($client.Right - $client.Left - 268)) } 'The status footer width did not settle to client width minus 268 pixels.' }
function Assert-Geometry([string]$Name) {
    $client = Client-Rect; $children = @(Children); if ($children.Count -eq 0) { throw "$Name has no visible direct child controls." }
    foreach ($child in $children) { if ($child.right -le $child.left -or $child.bottom -le $child.top -or $child.left -lt 0 -or $child.top -lt 0 -or $child.right -gt $client.Right -or $child.bottom -gt $client.Bottom) { throw "$Name child $($child.id) is outside the client rectangle or has no size." } }
    $status = @($children | Where-Object { $_.id -eq 109 })[0]; $apply = @($children | Where-Object { $_.id -eq 107 })[0]; $close = @($children | Where-Object { $_.id -eq 108 })[0]
    if (-not $status -or -not $apply -or -not $close) { throw "$Name is missing the visible status, Apply, or Close control." }
    if ($status.left -ne 24 -or $status.top -ne ($client.Bottom - 96) -or ($status.right - $status.left) -ne ($client.Right - 268) -or ($status.bottom - $status.top) -ne 80) { throw "$Name STATUS109 does not use the expected 80-pixel footer bounds." }
    foreach ($button in @($apply, $close)) { if ($status.right -gt $button.left -and $status.left -lt $button.right -and $status.bottom -gt $button.top -and $status.top -lt $button.bottom) { throw "$Name status overlaps a footer button." } }
}
function Save-Png([string]$Name) {
    $rect = [SettingsStatusNative+Rect]::new(); if (-not [SettingsStatusNative]::GetWindowRect($script:windowHandle, [ref]$rect)) { throw "Could not read the window rectangle for $Name." }
    $bitmap = [Drawing.Bitmap]::new($rect.Right - $rect.Left, $rect.Bottom - $rect.Top); $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try { $dc = $graphics.GetHdc(); try { if (-not [SettingsStatusNative]::PrintWindow($script:windowHandle, $dc, 2)) { throw "PrintWindow could not capture $Name." } } finally { $graphics.ReleaseHdc($dc) }; $bitmap.Save((Join-Path $evidencePath ($Name + '.png')), [Drawing.Imaging.ImageFormat]::Png) } finally { $graphics.Dispose(); $bitmap.Dispose() }
}
function Capture([string]$Name) {
    [SettingsStatusNative]::RedrawWindow($script:windowHandle, [IntPtr]::Zero, [IntPtr]::Zero, [uint32]0x185) | Out-Null
    $status = Control 109; $text = Read-Control 109; $client = Client-Rect; $statusClient = [SettingsStatusNative+Rect]::new(); if (-not [SettingsStatusNative]::GetClientRect($status, [ref]$statusClient)) { throw 'Could not read the STATUS109 client rectangle.' }; $format = [SettingsStatusNative]::EditRect($status); $draw = [SettingsStatusNative]::MeasureWrappedText($status, $text, $statusClient.Right)
    $first = [SettingsStatusNative]::CharacterPosition($status, 0); $last = [SettingsStatusNative]::CharacterPosition($status, [Math]::Max(0, $text.Length - 1)); $window = [SettingsStatusNative+Rect]::new(); [SettingsStatusNative]::GetWindowRect($script:windowHandle, [ref]$window) | Out-Null; $selection = [SettingsStatusNative]::Selection($status); $children = @(Children)
    $static = [ordered]@{ height = 32; requiredHeight = $draw[2]; requiredWidth = $draw[1]; heightClipped = ($draw[2] -gt 32); widthClipped = ($draw[1] -gt $statusClient.Right); clipped = (($draw[2] -gt 32) -or ($draw[1] -gt $statusClient.Right)) }
    $result = [ordered]@{ name = $Name; captured = [DateTime]::UtcNow.ToString('o'); text = $text; client = [ordered]@{ width = $statusClient.Right; height = $statusClient.Bottom }; windowClient = [ordered]@{ width = $client.Right; height = $client.Bottom }; formatRect = [ordered]@{ left = $format.Left; top = $format.Top; right = $format.Right; bottom = $format.Bottom }; width = $statusClient.Right; height = $statusClient.Bottom; lineCount = [int](Send-Control 109 0xBA); firstVisibleLine = [int](Send-Control 109 0xCE); fontHeight = $draw[0]; positions = [ordered]@{ first = (Point-Object $first); last = (Point-Object $last) }; selection = [ordered]@{ start = $selection[0]; end = $selection[1] }; style = [SettingsStatusNative]::Style($status, -16).ToInt64(); window = [ordered]@{ left = $window.Left; top = $window.Top; right = $window.Right; bottom = $window.Bottom }; children = $children; staticBaseline = $static }
    $result | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $evidencePath ($Name + '.json')) -Encoding utf8NoBOM; Save-Png $Name; $result
}
function Assert-PointFits($Point, $Format, [int]$FontHeight, [string]$Name) { if ($Point.x -lt $Format.left -or $Point.x -ge $Format.right -or $Point.y -lt $Format.top -or ($Point.y + $FontHeight) -gt $Format.bottom) { throw "$Name is outside the EDIT formatting rectangle." } }
function Scroll-Top { $lines = [int](Send-Control 109 0xBA); Send-Control 109 0xB6 0 ([long](-$lines - 1)) | Out-Null }
function Scroll-Bottom { $lines = [int](Send-Control 109 0xBA); Send-Control 109 0xB6 0 ([long]($lines + 1)) | Out-Null }
function Assert-Editor {
    $status = Control 109; $style = [SettingsStatusNative]::Style($status, -16).ToInt64(); if (($style -band 0x4) -eq 0 -or ($style -band 0x800) -eq 0 -or ($style -band 0x200000) -eq 0 -or ($style -band 0x80) -ne 0) { throw 'STATUS109 is not a multiline read-only vertically scrolling EDIT without horizontal auto-scroll.' }
    $before = Read-Control 109; Send-Control 109 0x102 ([int][char]'X') 0 | Out-Null; if ((Read-Control 109) -ne $before) { throw 'STATUS109 accepted WM_CHAR despite being read-only.' }
    Send-Control 109 0xB1 0 $before.Length | Out-Null; $selection = [SettingsStatusNative]::Selection($status); if ($selection[0] -ne 0 -or $selection[1] -ne $before.Length) { throw 'STATUS109 did not expose selectable text.' }; Send-Control 109 0xB1 0 0 | Out-Null
}
function Preview-Error([string]$TtsPath) {
    if ([IO.Directory]::Exists($TtsPath) -or [IO.File]::Exists($TtsPath)) { throw "The TTS fixture unexpectedly exists: $TtsPath" }
    Send-Control 112 $bmClick | Out-Null; Wait-Until { (Read-Control 109).StartsWith($expectedPrefix) } "Preview did not produce the expected missing-model error for $TtsPath."; [ordered]@{ tts = $TtsPath; status = (Read-Control 109) }
}
function Launch([string]$TtsPath) {
    $info = [Diagnostics.ProcessStartInfo]::new($binaryPath); $info.UseShellExecute = $false; $info.Environment['HERALD_DATA'] = $scratch; $info.Environment['HERALD_TTS'] = $TtsPath; $info.Environment['HERALD_ENV'] = (Join-Path $scratch 'empty.env'); $info.Environment['ELEVENLABS_API_KEY'] = ''; foreach ($argument in @('--settings', '--assets', (Join-Path $root 'native-announcer/resources'))) { $info.ArgumentList.Add($argument) }
    $script:process = [Diagnostics.Process]::Start($info); $script:launchCount++; $script:ownedPid = $script:process.Id; $script:ownedStart = $script:process.StartTime; $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do { Start-Sleep -Milliseconds 50; $script:process.Refresh(); if ($script:process.HasExited) { throw "Settings exited early with $($script:process.ExitCode)." } } while ($script:process.MainWindowHandle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
    if ($script:process.MainWindowHandle -eq [IntPtr]::Zero -or $script:process.MainWindowTitle -ne 'Herald settings' -or [IO.Path]::GetFullPath($script:process.Path) -ne $binaryPath) { throw 'The owned settings window did not open with the requested binary.' }
    $script:windowHandle = $script:process.MainWindowHandle; $identity = [ordered]@{ binary = $binaryPath; sha256 = $binaryHash; pid = $script:ownedPid; started = $script:ownedStart.ToUniversalTime().ToString('o'); hwnd = $script:windowHandle.ToInt64(); scratch = $scratch; tts = $TtsPath }; $identity | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidencePath ("instance-$launchCount.json")) -Encoding utf8NoBOM; $identity | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $evidencePath 'instance.json') -Encoding utf8NoBOM; Wait-Layout
}
function Close-Settings { Send-Control 108 $bmClick | Out-Null; if (-not $script:process.WaitForExit(5000) -or $script:process.ExitCode -ne 0) { throw 'Close did not exit the owned settings process cleanly.' }; $script:windowHandle = [IntPtr]::Zero }
try {
    if (-not (Test-Path -LiteralPath $binaryPath)) { throw "Build or provide the settings binary: $binaryPath" }
    if (Test-Path -LiteralPath $evidencePath) { throw "Evidence directory already exists: $evidencePath" }
    if (Test-Path -LiteralPath $scratch) { throw "Scratch directory already exists: $scratch" }
    New-Item -ItemType Directory -Path $evidencePath | Out-Null; New-Item -ItemType Directory -Path $scratch | Out-Null; $scratchCreated = $true; Set-Content -LiteralPath (Join-Path $scratch 'empty.env') -Value '' -Encoding utf8NoBOM
    @{ volume = 35; quietMode = $false; scheduleEnabled = $false } | ConvertTo-Json | Set-Content -LiteralPath $settingsPath -Encoding utf8NoBOM; Copy-Item -LiteralPath $settingsPath -Destination (Join-Path $evidencePath 'settings-before.json'); $binaryHash = (Get-FileHash $binaryPath -Algorithm SHA256).Hash
    $normalPath = Join-Path $scratch 'missing-model/long-missing-model-directory-for-the-settings-error-message'; $longPath = Join-Path $scratch ('missing-' + ('deep-folder-' * 120)); if ([IO.Directory]::Exists($longPath) -or [IO.File]::Exists($longPath)) { throw 'The long TTS fixture unexpectedly exists.' }
    Launch $normalPath; Assert-Geometry 'default'; Assert-Editor; $normalError = Preview-Error $normalPath; Scroll-Top; $normal = Capture 'normal-error'; if ($normal.height -ne 80 -or $normal.staticBaseline.requiredHeight -le 32 -or $normal.staticBaseline.requiredHeight -gt $normal.height -or -not $normal.staticBaseline.clipped -or $normal.lineCount -lt 2) { throw 'The regular missing-model error did not demonstrate the 80-pixel wrapped footer requirement.' }; Assert-PointFits $normal.positions.last $normal.formatRect $normal.fontHeight 'The normal error last character'; $minimum = [SettingsStatusNative]::Minimum($windowHandle); if ($minimum.X -le 0 -or $minimum.Y -le 0) { throw 'The settings window reported no minimum tracking size.' }; if (-not [SettingsStatusNative]::SetWindowPos($windowHandle, [IntPtr]::Zero, 0, 0, $minimum.X, $minimum.Y, 0x16)) { throw 'Could not request the enforced minimum-size resize.' }; Wait-Layout; Assert-Geometry 'minimum'; Scroll-Top; $minimumError = Capture 'minimum-error'; if ($minimumError.height -ne 80 -or $minimumError.staticBaseline.requiredHeight -le 32 -or $minimumError.staticBaseline.requiredHeight -gt $minimumError.height) { throw 'The minimum-size status footer does not contain the complete normal error.' }; Assert-PointFits $minimumError.positions.last $minimumError.formatRect $minimumError.fontHeight 'The minimum error last character'; Close-Settings
    Launch $longPath; Assert-Geometry 'long-default'; $longError = Preview-Error $longPath; Scroll-Top; $longStart = Capture 'long-scrolled-start'; if ($longStart.lineCount -le 1 -or (($longStart.style -band 0x80) -ne 0)) { throw 'The long missing-model error did not wrap without horizontal auto-scroll.' }; if ($longStart.firstVisibleLine -ne 0) { throw 'Native vertical scrolling did not reach the start of the long error.' }; Assert-PointFits $longStart.positions.first $longStart.formatRect $longStart.fontHeight 'The long error first character'; Scroll-Bottom; $longEnd = Capture 'long-scrolled-end'; if ($longEnd.firstVisibleLine -le $longStart.firstVisibleLine) { throw 'Native vertical scrolling did not move to the end of the long error.' }; Assert-PointFits $longEnd.positions.last $longEnd.formatRect $longEnd.fontHeight 'The long error last character'; $longBefore = Read-Control 109; Send-Control 109 0x102 ([int][char]'Z') 0 | Out-Null; if ((Read-Control 109) -ne $longBefore) { throw 'The long error EDIT accepted WM_CHAR.' }; Send-Control 109 0xB1 0 $longBefore.Length | Out-Null; $longSelection = [SettingsStatusNative]::Selection((Control 109)); if ($longSelection[1] -ne $longBefore.Length) { throw 'The long error was not selectable.' }; Send-Control 109 0xB1 0 0 | Out-Null; Close-Settings
    $result = [ordered]@{ verdict = 'VERIFIED'; binary = $binaryPath; sha256 = $binaryHash; scratch = $scratch; launches = $launchCount; normalPreview = $normalError; normal = $normal; minimum = $minimumError; longPreview = $longError; longStart = $longStart; longEnd = $longEnd; minimumTrack = [ordered]@{ width = $minimum.X; height = $minimum.Y }; screenshots = @('normal-error.png', 'minimum-error.png', 'long-scrolled-start.png', 'long-scrolled-end.png') }; $result | ConvertTo-Json -Depth 16 | Set-Content -LiteralPath (Join-Path $evidencePath 'result.json') -Encoding utf8NoBOM; Write-Output "VERIFIED. Wrapped status fits at default and minimum; long status reached both native scroll ends. Evidence: $evidencePath"
} catch {
    if (Test-Path -LiteralPath $evidencePath) { $_ | Out-String | Set-Content -LiteralPath (Join-Path $evidencePath 'failure.txt') -Encoding utf8NoBOM }
    throw
} finally {
    if ($process -and -not $process.HasExited -and $process.Id -eq $ownedPid -and $process.StartTime -eq $ownedStart) { $process.Kill(); $process.WaitForExit(5000) | Out-Null }
    if ($scratchCreated -and (Test-Path -LiteralPath $scratch)) { Remove-Item -LiteralPath $scratch -Recurse -Force }
    if (Test-Path -LiteralPath $evidencePath) { @{ scratchRemoved = -not (Test-Path -LiteralPath $scratch); processExited = (-not $process -or $process.HasExited); launches = $launchCount } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath 'cleanup.json') -Encoding utf8NoBOM }
}
