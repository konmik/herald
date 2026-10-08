[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$Baseline,
    [string]$Treatment,
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$Evidence,
    [ValidateRange(1, 100)]
    [int]$Runs = 5,
    [ValidateRange(0, 3600)]
    [int]$WarmupSeconds = 5,
    [ValidateRange(1, 3600)]
    [int]$SampleSeconds = 10,
    [string]$TtsDirectory
)

$ErrorActionPreference = 'Stop'

if (-not $IsWindows) { throw 'This benchmark requires Windows PowerShell 7.' }

if ($null -eq ('HeraldSettingsMemoryNative' -as [type])) {
    $null = Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class HeraldSettingsMemoryNative
{
    [StructLayout(LayoutKind.Sequential)]
    private struct MemoryRegion
    {
        public IntPtr BaseAddress;
        public IntPtr AllocationBase;
        public uint AllocationProtect;
        public ushort PartitionId;
        public UIntPtr RegionSize;
        public uint State;
        public uint Protect;
        public uint Type;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern UIntPtr VirtualQueryEx(IntPtr process, IntPtr address, out MemoryRegion region, UIntPtr length);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr OpenThread(uint access, bool inherit, uint threadId);

    [DllImport("kernel32.dll")]
    private static extern bool CloseHandle(IntPtr handle);

    [DllImport("ntdll.dll")]
    private static extern int NtQueryInformationThread(IntPtr thread, int informationClass, out IntPtr address, uint length, IntPtr returnLength);

    public static long ReadThreadStartAddress(int threadId)
    {
        var thread = OpenThread(0x40, false, (uint)threadId);
        if (thread == IntPtr.Zero) return 0;
        try
        {
            IntPtr address;
            return NtQueryInformationThread(thread, 9, out address, (uint)IntPtr.Size, IntPtr.Zero) == 0 ? address.ToInt64() : 0;
        }
        finally { CloseHandle(thread); }
    }

    public static Dictionary<string, ulong> ReadCommittedAddressSpace(IntPtr process)
    {
        var totals = new Dictionary<string, ulong> { { "private", 0 }, { "mapped", 0 }, { "image", 0 } };
        ulong address = 0;
        MemoryRegion region;
        var length = new UIntPtr((uint)Marshal.SizeOf(typeof(MemoryRegion)));
        while (VirtualQueryEx(process, new IntPtr((long)address), out region, length) != UIntPtr.Zero)
        {
            ulong size = region.RegionSize.ToUInt64();
            if (region.State == 0x1000)
            {
                string kind = region.Type == 0x20000 ? "private" : region.Type == 0x40000 ? "mapped" : region.Type == 0x1000000 ? "image" : null;
                if (kind != null) totals[kind] += size;
            }
            ulong next = (ulong)region.BaseAddress.ToInt64() + size;
            if (next <= address || next > long.MaxValue) break;
            address = next;
        }
        return totals;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct MemoryStatus
    {
        public uint Length;
        public uint MemoryLoad;
        public ulong TotalPhys;
        public ulong AvailPhys;
        public ulong TotalPageFile;
        public ulong AvailPageFile;
        public ulong TotalVirtual;
        public ulong AvailVirtual;
        public ulong AvailExtendedVirtual;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeFileTime
    {
        public uint Low;
        public uint High;
    }

    private delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GlobalMemoryStatusEx(ref MemoryStatus status);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetSystemTimes(out NativeFileTime idle, out NativeFileTime kernel, out NativeFileTime user);

    [DllImport("user32.dll", EntryPoint = "EnumWindows")]
    private static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern bool IsWindow(IntPtr window);

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll", EntryPoint = "GetWindowTextW", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr window, StringBuilder text, int length);


    [DllImport("user32.dll", EntryPoint = "PostMessageW")]
    private static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    public static MemoryStatus ReadMemoryStatus()
    {
        var status = new MemoryStatus { Length = (uint)Marshal.SizeOf(typeof(MemoryStatus)) };
        if (!GlobalMemoryStatusEx(ref status)) throw new Win32Exception(Marshal.GetLastWin32Error());
        return status;
    }

    public static ulong[] ReadSystemTimes()
    {
        NativeFileTime idle;
        NativeFileTime kernel;
        NativeFileTime user;
        if (!GetSystemTimes(out idle, out kernel, out user)) throw new Win32Exception(Marshal.GetLastWin32Error());
        return new[] { ToUInt64(idle), ToUInt64(kernel), ToUInt64(user) };
    }

    public static IntPtr FindSettingsWindow(int processId)
    {
        var found = IntPtr.Zero;
        EnumWindows((window, parameter) =>
        {
            if (IsOwnedVisibleSettingsWindow(window, processId))
            {
                found = window;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }

    public static bool IsOwnedVisibleSettingsWindow(IntPtr window, int processId)
    {
        if (window == IntPtr.Zero || !IsWindow(window) || !IsWindowVisible(window)) return false;
        uint owner;
        if (GetWindowThreadProcessId(window, out owner) == 0 || owner != (uint)processId) return false;
        return GetWindowTitle(window) == "Herald settings";
    }

    public static string GetWindowTitle(IntPtr window)
    {
        var value = new StringBuilder(512);
        var length = GetWindowText(window, value, value.Capacity);
        return length <= 0 ? String.Empty : value.ToString();
    }

    public static bool RequestClose(IntPtr window)
    {
        return window != IntPtr.Zero && PostMessage(window, 0x0010, IntPtr.Zero, IntPtr.Zero);
    }

    private static ulong ToUInt64(NativeFileTime value)
    {
        return ((ulong)value.High << 32) | value.Low;
    }
}
'@
}

function Write-JsonFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Value
    )
    $json = $Value | ConvertTo-Json -Depth 12
    Set-Content -LiteralPath $Path -Value $json -Encoding utf8NoBOM
}

function Add-RunError {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][System.Collections.Generic.List[string]]$Errors,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not [string]::IsNullOrWhiteSpace($Message) -and -not $Errors.Contains($Message)) {
        [void]$Errors.Add($Message)
    }
}

function Test-SamePath {
    param(
        [Parameter(Mandatory = $true)][string]$Left,
        [Parameter(Mandatory = $true)][string]$Right
    )
    return [string]::Equals([IO.Path]::GetFullPath($Left), [IO.Path]::GetFullPath($Right), [StringComparison]::OrdinalIgnoreCase)
}

function Get-ExecutableInfo {
    param([Parameter(Mandatory = $true)][string]$Path)
    $file = Get-Item -LiteralPath $Path -ErrorAction Stop
    if (-not ($file -is [IO.FileInfo])) { throw "Executable path is not a file: $Path" }
    $hash = Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256
    return [pscustomobject][ordered]@{
        path = [IO.Path]::GetFullPath($file.FullName)
        hash = $hash.Hash.ToUpperInvariant()
    }
}

function Get-ProcessExecutablePath {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Process)
    $Process.Refresh()
    if ($Process.HasExited) { throw "Process $($Process.Id) exited before its executable path could be captured." }
    return [IO.Path]::GetFullPath($Process.MainModule.FileName)
}

function Get-ErrorLogMessages {
    param([Parameter(Mandatory = $true)][string]$DataDirectory)
    $path = Join-Path $DataDirectory 'errors.log'
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { return @() }
    $content = Get-Content -LiteralPath $path -Raw -ErrorAction Stop
    if ([string]::IsNullOrWhiteSpace($content)) { return @('errors.log exists but is empty.') }
    return @("errors.log: $($content.Trim())")
}

function Assert-NoErrorLog {
    param([Parameter(Mandatory = $true)][string]$DataDirectory)
    $messages = @(Get-ErrorLogMessages -DataDirectory $DataDirectory)
    if ($messages.Count -gt 0) { throw ($messages -join [Environment]::NewLine) }
}

function Get-MachineSnapshot {
    $memory = [HeraldSettingsMemoryNative]::ReadMemoryStatus()
    $first = [HeraldSettingsMemoryNative]::ReadSystemTimes()
    Start-Sleep -Milliseconds 250
    $second = [HeraldSettingsMemoryNative]::ReadSystemTimes()
    $kernelDelta = [uint64]($second[1] - $first[1])
    $userDelta = [uint64]($second[2] - $first[2])
    $idleDelta = [uint64]($second[0] - $first[0])
    $totalDelta = [uint64]($kernelDelta + $userDelta)
    $busyDelta = if ($totalDelta -gt $idleDelta) { $totalDelta - $idleDelta } else { [uint64]0 }
    $load = if ($totalDelta -eq 0) { $null } else { [math]::Round(100.0 * [double]$busyDelta / [double]$totalDelta, 3) }
    $totalRam = [uint64]$memory.TotalPhys
    $availableRam = [uint64]$memory.AvailPhys
    $usedRam = if ($availableRam -le $totalRam) { $totalRam - $availableRam } else { [uint64]0 }
    return [pscustomobject][ordered]@{
        capturedAt = [DateTimeOffset]::UtcNow.ToString('o')
        ram = [pscustomobject][ordered]@{
            totalBytes = $totalRam
            availableBytes = $availableRam
            usedBytes = $usedRam
            memoryLoadPercent = [int]$memory.MemoryLoad
        }
        cores = [pscustomobject][ordered]@{
            logical = [Environment]::ProcessorCount
        }
        load = [pscustomobject][ordered]@{
            percent = $load
            intervalMilliseconds = 250
            source = 'GetSystemTimes'
        }
        gpuVramIncluded = $false
    }
}

function Get-ChildProcessCount {
    param([Parameter(Mandatory = $true)][int]$ParentId)
    $filter = 'ParentProcessId = {0}' -f $ParentId
    $children = @(Get-CimInstance -ClassName Win32_Process -Filter $filter -ErrorAction Stop)
    return [int]$children.Count
}

function Wait-ForSettingsWindow {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Process)
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds(30)
    while ([DateTimeOffset]::UtcNow -lt $deadline) {
        if ($Process.HasExited) { throw "Settings process exited before its owned window was ready with exit code $($Process.ExitCode)." }
        $Process.Refresh()
        $window = [HeraldSettingsMemoryNative]::FindSettingsWindow($Process.Id)
        if ($window -ne [IntPtr]::Zero -and [HeraldSettingsMemoryNative]::IsOwnedVisibleSettingsWindow($window, $Process.Id)) {
            return $window
        }
        Start-Sleep -Milliseconds 100
    }
    throw "Settings window for process $($Process.Id) was not visible and owned by that process within 30 seconds."
}

function Test-OwnedProcess {
    param(
        [Parameter(Mandatory = $true)][Diagnostics.Process]$Process,
        [Parameter(Mandatory = $true)][string]$ExpectedPath
    )
    try {
        if ($Process.HasExited) { return $false }
        $actualPath = Get-ProcessExecutablePath -Process $Process
        return Test-SamePath -Left $actualPath -Right $ExpectedPath
    } catch {
        return $false
    }
}

function Invoke-SettingsMemoryRun {
    param(
        [Parameter(Mandatory = $true)][string]$Variant,
        [Parameter(Mandatory = $true)][int]$Repetition,
        [Parameter(Mandatory = $true)]$Executable,
        [Parameter(Mandatory = $true)][string]$Assets,
        [Parameter(Mandatory = $true)][string]$Repository,
        [Parameter(Mandatory = $true)][string]$DataDirectory,
        [Parameter(Mandatory = $true)][int]$WarmupSeconds,
        [Parameter(Mandatory = $true)][int]$SampleSeconds
    )
    $errors = [System.Collections.Generic.List[string]]::new()
    $workingSetSamples = [System.Collections.Generic.List[object]]::new()
    $privateBytesSamples = [System.Collections.Generic.List[object]]::new()
    $process = $null
    $window = [IntPtr]::Zero
    $forcedKill = $false
    $processCleanupSuccess = $true
    $record = [ordered]@{
        variant = $Variant
        repetition = $Repetition
        executablePath = $Executable.path
        executableHash = $Executable.hash
        processExecutablePath = $null
        processExecutableHash = $null
        pid = $null
        startedAt = $null
        workingSet = @()
        privateBytes = @()
        peakWorkingSet = $null
        peakPrivateBytes = $null
        workingSetMedian = $null
        privateBytesMedian = $null
        cpu = $null
        windowTitle = $null
        childProcessCount = $null
        childProcessCountAtEnd = $null
        loadedModules = @()
        sampleCount = 0
        sampleIntervalMilliseconds = 500
        sampledWallSeconds = $null
        heraldData = $DataDirectory
        forcedKill = $false
        cleanupSuccess = $true
        errors = @()
    }
    try {
        $existingFixtureContents = @(Get-ChildItem -LiteralPath $DataDirectory -Force -ErrorAction Stop)
        if ($existingFixtureContents.Count -ne 0) { throw 'The benchmark HERALD_DATA fixture was not empty before launch.' }
        $launchStartedAt = [DateTimeOffset]::UtcNow
        $record['startedAt'] = $launchStartedAt.ToString('o')
        $startInfo = [Diagnostics.ProcessStartInfo]::new()
        $startInfo.FileName = $Executable.path
        $startInfo.WorkingDirectory = $Repository
        $startInfo.UseShellExecute = $false
        $startInfo.CreateNoWindow = $true
        $startInfo.Environment['HERALD_DATA'] = $DataDirectory
        $startInfo.Environment['HERALD_TTS'] = if ($TtsDirectory) { $TtsDirectory } else { Join-Path $Assets 'tts/kitten-nano-en-v0_8-int8' }
        $startInfo.ArgumentList.Add('--settings')
        $startInfo.ArgumentList.Add('--assets')
        $startInfo.ArgumentList.Add($Assets)
        $process = [Diagnostics.Process]::Start($startInfo)
        if ($null -eq $process) { throw 'The settings process did not start.' }
        $record['pid'] = [int]$process.Id
        $window = Wait-ForSettingsWindow -Process $process
        $runningPath = Get-ProcessExecutablePath -Process $process
        $record['processExecutablePath'] = $runningPath
        if (-not (Test-SamePath -Left $runningPath -Right $Executable.path)) { throw "Running executable path did not match the requested path: $runningPath" }
        $runningHash = (Get-FileHash -LiteralPath $runningPath -Algorithm SHA256).Hash.ToUpperInvariant()
        $record['processExecutableHash'] = $runningHash
        if ($runningHash -ne $Executable.hash) { throw 'Running executable hash changed after launch.' }
        $record['windowTitle'] = [HeraldSettingsMemoryNative]::GetWindowTitle($window)
        if ($record['windowTitle'] -ne 'Herald settings') { throw "Unexpected settings window title: $($record['windowTitle'])" }
        Assert-NoErrorLog -DataDirectory $DataDirectory
        $record['childProcessCount'] = Get-ChildProcessCount -ParentId $process.Id
        $warmupDeadline = [DateTimeOffset]::UtcNow.AddSeconds($WarmupSeconds)
        while ([DateTimeOffset]::UtcNow -lt $warmupDeadline) {
            if ($process.HasExited) { throw "Settings process exited during warmup with exit code $($process.ExitCode)." }
            $process.Refresh()
            if (-not [HeraldSettingsMemoryNative]::IsOwnedVisibleSettingsWindow($window, $process.Id)) { throw 'The owned Herald settings window was no longer visible during warmup.' }
            Assert-NoErrorLog -DataDirectory $DataDirectory
            Start-Sleep -Milliseconds 100
        }
        if ($process.HasExited) { throw "Settings process exited before sampling with exit code $($process.ExitCode)." }
        $process.Refresh()
        if (-not [HeraldSettingsMemoryNative]::IsOwnedVisibleSettingsWindow($window, $process.Id)) { throw 'The owned Herald settings window was not visible at the start of sampling.' }
        $record['loadedModules'] = @($process.Modules | ForEach-Object { $_.ModuleName })
        $process.Refresh()
        $cpuStart = $process.TotalProcessorTime
        $sampleStart = [Diagnostics.Stopwatch]::GetTimestamp()
        $sampleDeadline = [DateTimeOffset]::UtcNow.AddSeconds($SampleSeconds)
        while ([DateTimeOffset]::UtcNow -lt $sampleDeadline) {
            if ($process.HasExited) { throw "Settings process exited during sampling with exit code $($process.ExitCode)." }
            $process.Refresh()
            if (-not [HeraldSettingsMemoryNative]::IsOwnedVisibleSettingsWindow($window, $process.Id)) { throw 'The owned Herald settings window was no longer visible during sampling.' }
            $capturedAt = [DateTimeOffset]::UtcNow.ToString('o')
            [void]$workingSetSamples.Add([pscustomobject][ordered]@{
                capturedAt = $capturedAt
                bytes = [int64]$process.WorkingSet64
            })
            [void]$privateBytesSamples.Add([pscustomobject][ordered]@{
                capturedAt = $capturedAt
                bytes = [int64]$process.PrivateMemorySize64
            })
            Assert-NoErrorLog -DataDirectory $DataDirectory
            if ([DateTimeOffset]::UtcNow -ge $sampleDeadline) { break }
            Start-Sleep -Milliseconds 500
        }
        if ($workingSetSamples.Count -eq 0) { throw 'No process memory samples were captured.' }
        if ($process.HasExited) { throw "Settings process exited at the end of sampling with exit code $($process.ExitCode)." }
        $process.Refresh()
        $cpuEnd = $process.TotalProcessorTime
        $sampledWallSeconds = ([Diagnostics.Stopwatch]::GetTimestamp() - $sampleStart) / [double][Diagnostics.Stopwatch]::Frequency
        if (-not [HeraldSettingsMemoryNative]::IsOwnedVisibleSettingsWindow($window, $process.Id)) { throw 'The owned Herald settings window was not visible at the end of sampling.' }
        Assert-NoErrorLog -DataDirectory $DataDirectory
        $record['childProcessCountAtEnd'] = Get-ChildProcessCount -ParentId $process.Id
        $record['committedAddressSpaceAtEnd'] = [HeraldSettingsMemoryNative]::ReadCommittedAddressSpace($process.Handle)
        $record['threadCountAtEnd'] = $process.Threads.Count
        $modules = @($process.Modules)
        $record['threadStartModulesAtEnd'] = @($process.Threads | ForEach-Object {
            $address = [HeraldSettingsMemoryNative]::ReadThreadStartAddress($_.Id)
            $module = $modules | Where-Object { $address -ge $_.BaseAddress.ToInt64() -and $address -lt $_.BaseAddress.ToInt64() + $_.ModuleMemorySize } | Select-Object -First 1
            if ($module) { $module.ModuleName } else { 'unknown' }
        } | Group-Object | ForEach-Object { [pscustomobject]@{ module = $_.Name; threads = $_.Count } })
        $record['sampledWallSeconds'] = [math]::Round($sampledWallSeconds, 6)
        $sampledProcessorSeconds = ($cpuEnd - $cpuStart).TotalSeconds
        $record['cpu'] = [pscustomobject][ordered]@{
            totalProcessorSeconds = [math]::Round($cpuEnd.TotalSeconds, 6)
            sampledProcessorSeconds = [math]::Round($sampledProcessorSeconds, 6)
            sampledPercentOfOneCore = [math]::Round(100.0 * $sampledProcessorSeconds / [math]::Max($sampledWallSeconds, 0.001), 3)
            sampledPercentOfMachine = [math]::Round(100.0 * $sampledProcessorSeconds / ([math]::Max($sampledWallSeconds, 0.001) * [Environment]::ProcessorCount), 3)
        }
        $record['workingSet'] = @($workingSetSamples.ToArray())
        $record['privateBytes'] = @($privateBytesSamples.ToArray())
        $record['sampleCount'] = $workingSetSamples.Count
        $record['peakWorkingSet'] = [int64](($workingSetSamples | Measure-Object -Property bytes -Maximum).Maximum)
        $record['peakPrivateBytes'] = [int64](($privateBytesSamples | Measure-Object -Property bytes -Maximum).Maximum)
        $record['workingSetMedian'] = (Get-Statistics -Values @($workingSetSamples | ForEach-Object { $_.bytes })).median
        $record['privateBytesMedian'] = (Get-Statistics -Values @($privateBytesSamples | ForEach-Object { $_.bytes })).median
    } catch {
        Add-RunError -Errors $errors -Message $_.Exception.Message
        try {
            foreach ($message in @(Get-ErrorLogMessages -DataDirectory $DataDirectory)) { Add-RunError -Errors $errors -Message $message }
        } catch {
            Add-RunError -Errors $errors -Message "Could not inspect errors.log: $($_.Exception.Message)"
        }
    } finally {
        if ($null -ne $process) {
            $running = $false
            try {
                $running = -not $process.HasExited
            } catch {
                $processCleanupSuccess = $false
                Add-RunError -Errors $errors -Message "Could not determine whether process $($record['pid']) was still running: $($_.Exception.Message)"
            }
            if ($running) {
                try {
                    $closeTarget = if ($window -ne [IntPtr]::Zero -and [HeraldSettingsMemoryNative]::IsOwnedVisibleSettingsWindow($window, $process.Id)) { $window } else { [HeraldSettingsMemoryNative]::FindSettingsWindow($process.Id) }
                    if ($closeTarget -ne [IntPtr]::Zero) {
                        if (-not [HeraldSettingsMemoryNative]::RequestClose($closeTarget)) {
                            $processCleanupSuccess = $false
                            Add-RunError -Errors $errors -Message 'Could not post WM_CLOSE to the owned Herald settings window.'
                        }
                    }
                } catch {
                    $processCleanupSuccess = $false
                    Add-RunError -Errors $errors -Message "Could not request settings window close: $($_.Exception.Message)"
                }
                try {
                    $closed = $process.WaitForExit(10000)
                    if (-not $closed) {
                        if (-not (Test-OwnedProcess -Process $process -ExpectedPath $Executable.path)) {
                            $processCleanupSuccess = $false
                            Add-RunError -Errors $errors -Message 'Refused to kill a process whose ownership could not be confirmed.'
                        } else {
                            $process.Kill()
                            $forcedKill = $true
                            if (-not $process.WaitForExit(5000)) {
                                $processCleanupSuccess = $false
                                Add-RunError -Errors $errors -Message 'Owned settings process did not exit after the required kill.'
                            }
                        }
                    }
                } catch {
                    $processCleanupSuccess = $false
                    Add-RunError -Errors $errors -Message "Could not clean up settings process $($record['pid']): $($_.Exception.Message)"
                }
            }
            try {
                foreach ($message in @(Get-ErrorLogMessages -DataDirectory $DataDirectory)) { Add-RunError -Errors $errors -Message $message }
            } catch {
                Add-RunError -Errors $errors -Message "Could not inspect errors.log during cleanup: $($_.Exception.Message)"
            }
            try { $process.Dispose() } catch { $processCleanupSuccess = $false; Add-RunError -Errors $errors -Message "Could not release process handle: $($_.Exception.Message)" }
        }
        $record['forcedKill'] = $forcedKill
        $record['cleanupSuccess'] = $processCleanupSuccess
        $record['errors'] = @($errors.ToArray())
        $record['finishedAt'] = [DateTimeOffset]::UtcNow.ToString('o')
    }
    return [pscustomobject]$record
}

function Get-Statistics {
    param([object[]]$Values)
    $numbers = @($Values | ForEach-Object {
        if ($null -ne $_) {
            $number = [double]$_
            if ([double]::IsFinite($number)) { $number }
        }
    } | Sort-Object)
    if ($numbers.Count -eq 0) { return $null }
    $middle = [int][math]::Floor($numbers.Count / 2)
    $median = if (($numbers.Count % 2) -eq 1) { $numbers[$middle] } else { ($numbers[$middle - 1] + $numbers[$middle]) / 2.0 }
    return [pscustomobject][ordered]@{
        count = $numbers.Count
        median = $median
        min = $numbers[0]
        max = $numbers[$numbers.Count - 1]
        range = @($numbers[0], $numbers[$numbers.Count - 1])
    }
}

function Get-VariantSummary {
    param(
        [Parameter(Mandatory = $true)][string]$Variant,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Records
    )
    $all = @($Records | Where-Object { $_.variant -eq $Variant })
    $valid = @($all | Where-Object { @($_.errors).Count -eq 0 -and $_.sampleCount -gt 0 })
    return [pscustomobject][ordered]@{
        variant = $Variant
        runs = $all.Count
        successfulRuns = $valid.Count
        failedRuns = $all.Count - $valid.Count
        peakWorkingSet = Get-Statistics -Values @($valid | ForEach-Object { $_.peakWorkingSet })
        peakPrivateBytes = Get-Statistics -Values @($valid | ForEach-Object { $_.peakPrivateBytes })
        workingSetMedian = Get-Statistics -Values @($valid | ForEach-Object { $_.workingSetMedian })
        privateBytesMedian = Get-Statistics -Values @($valid | ForEach-Object { $_.privateBytesMedian })
        cpu = [pscustomobject][ordered]@{
            totalProcessorSeconds = Get-Statistics -Values @($valid | ForEach-Object { $_.cpu.totalProcessorSeconds })
            sampledProcessorSeconds = Get-Statistics -Values @($valid | ForEach-Object { $_.cpu.sampledProcessorSeconds })
            sampledPercentOfOneCore = Get-Statistics -Values @($valid | ForEach-Object { $_.cpu.sampledPercentOfOneCore })
            sampledPercentOfMachine = Get-Statistics -Values @($valid | ForEach-Object { $_.cpu.sampledPercentOfMachine })
        }
    }
}

function New-Summary {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Records,
        [Parameter(Mandatory = $true)][bool]$HasTreatment,
        [Parameter(Mandatory = $true)]$MachineBefore,
        [Parameter(Mandatory = $true)]$MachineAfter,
        [Parameter(Mandatory = $true)][bool]$CleanupSuccess,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$CleanupErrors,
        [Parameter(Mandatory = $true)][string]$RawPath,
        [Parameter(Mandatory = $true)][string]$SummaryPath,
        [string]$FatalError
    )
    $variantNames = if ($HasTreatment) { @('baseline', 'treatment') } else { @('baseline') }
    $variantSummaries = @($variantNames | ForEach-Object { Get-VariantSummary -Variant $_ -Records $Records })
    $failedRuns = @($Records | Where-Object { @($_.errors).Count -gt 0 }).Count
    $comparison = $null
    if ($HasTreatment) {
        $baseline = $variantSummaries | Where-Object { $_.variant -eq 'baseline' }
        $treatment = $variantSummaries | Where-Object { $_.variant -eq 'treatment' }
        $metrics = [ordered]@{}
        foreach ($metric in @('workingSetMedian', 'privateBytesMedian', 'peakWorkingSet', 'peakPrivateBytes')) {
            $before = $baseline.$metric
            $after = $treatment.$metric
            if ($null -ne $before -and $null -ne $after) {
                $delta = $after.median - $before.median
                $percent = if ($before.median -eq 0) { $null } else { [math]::Round(100.0 * $delta / $before.median, 3) }
                $metrics[$metric] = [pscustomobject][ordered]@{
                    baselineMedian = $before.median
                    treatmentMedian = $after.median
                    delta = $delta
                    percentChange = $percent
                    observedRangesOverlap = $before.min -le $after.max -and $after.min -le $before.max
                    interpretation = if ($before.count -lt 2 -or $after.count -lt 2) { 'Single-run descriptive values only' } elseif ($before.min -le $after.max -and $after.min -le $before.max) { 'Observed ranges overlap; no clear difference established' } else { 'Observed ranges are separated; no formal significance test performed' }
                }
            }
        }
        $comparison = [pscustomobject][ordered]@{
            metrics = [pscustomobject]$metrics
        }
    }
    $status = if (-not [string]::IsNullOrEmpty($FatalError)) { 'failed' } elseif ($failedRuns -gt 0 -or -not $CleanupSuccess) { 'inconclusive' } elseif ($HasTreatment) { 'complete' } else { 'baseline-only' }
    return [pscustomobject][ordered]@{
        schemaVersion = 1
        benchmark = 'settings-memory'
        status = $status
        rawEvidence = $RawPath
        summaryEvidence = $SummaryPath
        failedRuns = $failedRuns
        variants = $variantSummaries
        comparison = $comparison
        machine = [pscustomobject][ordered]@{
            before = $MachineBefore
            after = $MachineAfter
            gpuVramIncluded = $false
        }
        measurement = [pscustomobject][ordered]@{
            memoryCounters = @('Process.WorkingSet64', 'Process.PrivateMemorySize64')
            sampleIntervalMilliseconds = 500
            units = 'bytes'
            phase = 'Idle settings window after warmup'
            peakDefinition = 'Maximum sampled counter after warmup, not lifetime peak'
            gpuVramIncluded = $false
        }
        cleanup = [pscustomobject][ordered]@{
            success = $CleanupSuccess
            errors = @($CleanupErrors)
        }
        fatalError = $FatalError
    }
}

$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$assets = [IO.Path]::GetFullPath((Join-Path $repository 'native-announcer/resources'))
if (-not (Test-Path -LiteralPath $assets -PathType Container)) { throw "Repository resources directory was not found: $assets" }
$baselineInfo = Get-ExecutableInfo -Path $Baseline
$hasTreatment = -not [string]::IsNullOrWhiteSpace($Treatment)
$treatmentInfo = if ($hasTreatment) { Get-ExecutableInfo -Path $Treatment } else { $null }
$executableByVariant = @{ baseline = $baselineInfo }
if ($hasTreatment) { $executableByVariant.treatment = $treatmentInfo }

$evidenceRoot = [IO.Path]::GetFullPath($Evidence)
New-Item -ItemType Directory -Path $evidenceRoot -Force | Out-Null
$runName = 'settings-memory-{0}-{1}' -f [DateTimeOffset]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'), ([guid]::NewGuid().ToString('N'))
$evidenceDirectory = Join-Path $evidenceRoot $runName
New-Item -ItemType Directory -Path $evidenceDirectory -Force | Out-Null
$rawPath = Join-Path $evidenceDirectory 'raw.json'
$summaryPath = Join-Path $evidenceDirectory 'summary.json'
$scratchDirectories = [System.Collections.Generic.List[string]]::new()
$records = [System.Collections.Generic.List[object]]::new()
$cleanupErrors = [System.Collections.Generic.List[string]]::new()
$machineBefore = $null
$machineAfter = $null
$machineAfterError = $null
$fatalException = $null
$benchmarkStartedAt = [DateTimeOffset]::UtcNow
$rawPayload = $null
$summaryPayload = $null
$artifactError = $null

try {
    $machineBefore = Get-MachineSnapshot
    for ($repetition = 1; $repetition -le $Runs; $repetition++) {
        if ($hasTreatment) {
            $variantOrder = @('baseline', 'treatment')
        } else {
            $variantOrder = @('baseline')
        }
        foreach ($variant in $variantOrder) {
            $fixture = Join-Path $env:LOCALAPPDATA ('Temp/opencode/herald-settings-memory-' + [guid]::NewGuid().ToString('N'))
            New-Item -ItemType Directory -Path $fixture -Force | Out-Null
            [void]$scratchDirectories.Add($fixture)
            $record = Invoke-SettingsMemoryRun -Variant $variant -Repetition $repetition -Executable $executableByVariant[$variant] -Assets $assets -Repository $repository -DataDirectory $fixture -WarmupSeconds $WarmupSeconds -SampleSeconds $SampleSeconds
            [void]$records.Add($record)
        }
    }
} catch {
    $fatalException = $_.Exception
} finally {
    foreach ($scratchDirectory in $scratchDirectories.ToArray()) {
        if (Test-Path -LiteralPath $scratchDirectory) {
            try {
                Remove-Item -LiteralPath $scratchDirectory -Recurse -Force -ErrorAction Stop
            } catch {
                Add-RunError -Errors $cleanupErrors -Message "Could not remove benchmark scratch directory ${scratchDirectory}: $($_.Exception.Message)"
            }
        }
    }
    try {
        $machineAfter = Get-MachineSnapshot
    } catch {
        $machineAfterError = $_.Exception.Message
        if ($null -eq $fatalException) { $fatalException = $_.Exception }
    }
    $runCleanupFailures = @($records | Where-Object { -not $_.cleanupSuccess }).Count
    if ($runCleanupFailures -gt 0) { Add-RunError -Errors $cleanupErrors -Message "$runCleanupFailures settings process cleanup operation(s) did not complete cleanly." }
    $cleanupSuccess = $cleanupErrors.Count -eq 0
    $fatalMessage = if ($null -ne $fatalException) { $fatalException.Message } else { $null }
    $rawPayload = [pscustomobject][ordered]@{
        schemaVersion = 1
        benchmark = 'settings-memory'
        startedAt = $benchmarkStartedAt.ToString('o')
        finishedAt = [DateTimeOffset]::UtcNow.ToString('o')
        parameters = [pscustomobject][ordered]@{
            ttsDirectory = if ($TtsDirectory) { $TtsDirectory } else { Join-Path $assets 'tts/kitten-nano-en-v0_8-int8' }
            baseline = $baselineInfo.path
            treatment = if ($hasTreatment) { $treatmentInfo.path } else { $null }
            evidence = $evidenceDirectory
            runs = $Runs
            warmupSeconds = $WarmupSeconds
            sampleSeconds = $SampleSeconds
            assets = $assets
        }
        executables = [pscustomobject][ordered]@{
            baseline = $baselineInfo
            treatment = if ($hasTreatment) { $treatmentInfo } else { $null }
        }
        machine = [pscustomobject][ordered]@{
            before = $machineBefore
            after = $machineAfter
            afterError = $machineAfterError
            gpuVramIncluded = $false
        }
        measurement = [pscustomobject][ordered]@{
            memoryCounters = @('Process.WorkingSet64', 'Process.PrivateMemorySize64')
            sampleIntervalMilliseconds = 500
            units = 'bytes'
            phase = 'Idle settings window after warmup'
            peakDefinition = 'Maximum sampled counter after warmup, not lifetime peak'
            wholeProcess = $true
            gpuVramIncluded = $false
        }
        runs = @($records.ToArray())
        cleanup = [pscustomobject][ordered]@{
            success = $cleanupSuccess
            errors = @($cleanupErrors.ToArray())
        }
        fatalError = $fatalMessage
    }
    $summaryPayload = New-Summary -Records @($records.ToArray()) -HasTreatment $hasTreatment -MachineBefore $machineBefore -MachineAfter $machineAfter -CleanupSuccess $cleanupSuccess -CleanupErrors @($cleanupErrors.ToArray()) -RawPath $rawPath -SummaryPath $summaryPath -FatalError $fatalMessage
    try {
        Write-JsonFile -Path $rawPath -Value $rawPayload
    } catch {
        $artifactError = $_.Exception
    }
    try {
        Write-JsonFile -Path $summaryPath -Value $summaryPayload
    } catch {
        if ($null -eq $artifactError) { $artifactError = $_.Exception }
    }
}

if ($null -ne $artifactError) { throw $artifactError }
if ($null -ne $fatalException) { throw $fatalException }
if (-not $cleanupSuccess) { throw "Benchmark cleanup was not successful. Evidence: $evidenceDirectory" }
$failedRuns = @($records | Where-Object { @($_.errors).Count -gt 0 }).Count
if ($failedRuns -gt 0) { throw "Benchmark completed with $failedRuns failed run(s). Evidence: $evidenceDirectory" }
Write-Output "Raw evidence: $rawPath"
Write-Output "Summary: $summaryPath"
