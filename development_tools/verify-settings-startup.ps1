param(
    [string]$Binary = 'native-announcer/target/debug/herald.exe',
    [string]$Evidence = ('temp/verification/settings-startup-' + [guid]::NewGuid())
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;
public static class StartupCapture {
    public struct Rect { public int Left, Top, Right, Bottom; }
    public struct Point { public int X, Y; }
    delegate bool Callback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr window, ref Point point);
    [DllImport("user32.dll")] static extern IntPtr GetDC(IntPtr window);
    [DllImport("user32.dll")] static extern int ReleaseDC(IntPtr window, IntPtr dc);
    [DllImport("gdi32.dll")] static extern uint GetPixel(IntPtr dc, int x, int y);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point point);
    [DllImport("dwmapi.dll")] static extern int DwmFlush();
    public static string[] Capture(int pid) {
        SetThreadDpiAwarenessContext(new IntPtr(-4));
        var rows = new List<string>();
        var timer = Stopwatch.StartNew();
        long first = -1;
        while (timer.ElapsedMilliseconds < 15000) {
            IntPtr target = IntPtr.Zero;
            EnumWindows((window, data) => {
                uint owner;
                GetWindowThreadProcessId(window, out owner);
                if (owner == pid && IsWindowVisible(window)) target = window;
                return true;
            }, IntPtr.Zero);
            if (target != IntPtr.Zero) {
                SetWindowPos(target, new IntPtr(-1), 0, 0, 0, 0, 0x4013);
                Rect rect;
                GetClientRect(target, out rect);
                if (rect.Right > 100 && rect.Bottom > 100) {
                    var origin = new Point();
                    ClientToScreen(target, ref origin);
                    if (WindowFromPoint(new Point { X = origin.X + 8, Y = origin.Y + 8 }) != target) { Thread.Sleep(2); continue; }
                    DwmFlush();
                    var dc = GetDC(IntPtr.Zero);
                    var pixels = new List<string>();
                    foreach (var point in new Point[] {
                        new Point { X = 8, Y = 8 }, new Point { X = rect.Right - 8, Y = 8 },
                        new Point { X = 8, Y = rect.Bottom - 8 }, new Point { X = rect.Right - 8, Y = rect.Bottom - 8 }
                    }) pixels.Add(GetPixel(dc, origin.X + point.X, origin.Y + point.Y).ToString("X6"));
                    ReleaseDC(IntPtr.Zero, dc);
                    rows.Add(timer.ElapsedMilliseconds + "," + string.Join(",", pixels));
                    if (first < 0) {
                        first = timer.ElapsedMilliseconds;
                    }
                    if (timer.ElapsedMilliseconds - first > 1500) return rows.ToArray();
                }
            }
            Thread.Sleep(2);
        }
        throw new Exception("No visible settings window");
    }
}
'@
$evidencePath = [IO.Path]::GetFullPath($Evidence)
if (Test-Path -LiteralPath $evidencePath) { throw "Evidence directory already exists: $evidencePath" }
New-Item -ItemType Directory -Force $evidencePath | Out-Null
$scratch = Join-Path $env:LOCALAPPDATA ('Temp/opencode/startup-' + [guid]::NewGuid())
$process = $null
$background = $null
try {
    New-Item -ItemType Directory $scratch | Out-Null
    $info = [Diagnostics.ProcessStartInfo]::new([IO.Path]::GetFullPath($Binary))
    foreach ($argument in @('--settings', '--assets', [IO.Path]::GetFullPath('native-announcer/resources'))) { $info.ArgumentList.Add($argument) }
    $info.Environment['HERALD_DATA'] = $scratch
    $info.Environment['HERALD_ENV_FILE'] = Join-Path $scratch 'empty.env'
    [IO.File]::WriteAllText($info.Environment['HERALD_ENV_FILE'], '')
    $background = [Windows.Forms.Form]::new()
    $background.BackColor = [Drawing.Color]::Magenta
    $background.TopMost = $true
    $background.WindowState = [Windows.Forms.FormWindowState]::Maximized
    $background.Show()
    [Windows.Forms.Application]::DoEvents()
    $binaryHash = (Get-FileHash -LiteralPath $info.FileName -Algorithm SHA256).Hash
    $process = [Diagnostics.Process]::Start($info)
    $samples = [StartupCapture]::Capture($process.Id)
    @{
        binary = $info.FileName
        sha256 = $binaryHash
        pid = $process.Id
        started = $process.StartTime.ToUniversalTime().ToString('o')
        scratch = $scratch
    } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'instance.json')
    $samples | Set-Content (Join-Path $evidencePath 'samples.csv')
    $first = ($samples[0] -split ',', 2)[1]
    $last = ($samples[-1] -split ',', 2)[1]
    Write-Output "First visible pixels $first"
    Write-Output "Settled pixels $last"
    if ($first -ne $last -or $last -match 'FF00FF|FFFFFFFF') { throw 'FAIL: first visible settings frame is not painted.' }
    Write-Output 'PASS: first visible settings frame is painted.'
} finally {
    if ($background) { $background.Close(); $background.Dispose() }
    if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    Start-Sleep -Milliseconds 300
    if (Test-Path -LiteralPath $scratch) { Remove-Item -LiteralPath $scratch -Recurse -Force }
    @{ processExited = (-not $process -or $process.HasExited); scratchRemoved = (-not (Test-Path $scratch)) } | ConvertTo-Json | Set-Content (Join-Path $evidencePath 'cleanup.json')
}
