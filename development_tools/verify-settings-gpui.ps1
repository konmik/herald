param(
    [string]$Binary = (Join-Path (Split-Path $PSScriptRoot -Parent) 'native-announcer/target/release/herald.exe'),
    [string]$Evidence = ('temp/verification/gpui-settings-' + [guid]::NewGuid()),
    [ValidateSet('Settings', 'Quiet', 'Output', 'Preview', 'Characters', 'Announcements', 'All')]
    [string]$Feature = 'All'
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$binaryPath = if ([IO.Path]::IsPathRooted($Binary)) { [IO.Path]::GetFullPath($Binary) } else { [IO.Path]::GetFullPath((Join-Path $root $Binary)) }
$evidencePath = if ([IO.Path]::IsPathRooted($Evidence)) { [IO.Path]::GetFullPath($Evidence) } else { [IO.Path]::GetFullPath((Join-Path $root $Evidence)) }
$scratch = Join-Path (Join-Path $env:LOCALAPPDATA 'Temp/opencode') ('herald-settings-gpui-' + [guid]::NewGuid())
$settingsPath = Join-Path $scratch 'settings.json'
$emptyEnvironment = Join-Path $scratch 'empty.env'
$binaryHash = $null
$scratchCreated = $false
$transcribing = $false
$launchCount = 0
$activeRecord = $null
$activeRoot = $null
$ownedRecords = @()
$featureGaps = @()
$actionRecords = @()
$snapshotPaths = @()
$pageResults = @()
$missingOutput = 'missing-gpui-output-device'
$characterDraftName = $null
$announcementPrompt = "GPUI exact prompt line one.`nLine two must remain separate."
$appliedBytes = $null
$firstLaunchBytes = $null

$pageNames = @('Characters', 'Audio', 'Quiet hours', 'Speech service', 'Offline voice', 'Announcements')
$pageSpecs = [ordered]@{
    'Characters' = [ordered]@{ ids = @('characters'); names = @('Characters') }
    'Audio' = [ordered]@{ ids = @('audio'); names = @('Audio') }
    'Quiet hours' = [ordered]@{ ids = @('quiet-hours'); names = @('Quiet hours') }
    'Speech service' = [ordered]@{ ids = @('speech-service'); names = @('Speech service') }
    'Offline voice' = [ordered]@{ ids = @('offline-voice'); names = @('Offline voice') }
    'Announcements' = [ordered]@{ ids = @('announcements'); names = @('Announcements') }
}
$controlSpecs = [ordered]@{
    apply = [ordered]@{ ids = @('apply'); names = @('Apply') }
    close = [ordered]@{ ids = @('close'); names = @('Close') }
    quiet = [ordered]@{ ids = @('quiet-mode'); names = @('Quiet mode') }
    schedule = [ordered]@{ ids = @('schedule'); names = @('Daily schedule') }
    quietStart = [ordered]@{ ids = @('quiet-start'); names = @('From') }
    quietEnd = [ordered]@{ ids = @('quiet-end'); names = @('To') }
    volume = [ordered]@{ ids = @('volume'); names = @('Announcer volume') }
    output = [ordered]@{ ids = @('output-device'); names = @('Output device') }
    preview = [ordered]@{ ids = @('preview'); names = @('Play example') }
    status = [ordered]@{ ids = @('status'); names = @('Status') }
    apiKey = [ordered]@{ ids = @('api-key'); names = @('ElevenLabs key') }
    characterNew = [ordered]@{ ids = @('new-character'); names = @('New') }
    characterName = [ordered]@{ ids = @('character-name'); names = @('Name') }
    characterPrompt = [ordered]@{ ids = @('character-prompt'); names = @('Summary prompt (blank uses default)') }
    summaryPrompt = [ordered]@{ ids = @('summary-prompt'); names = @('Default summary prompt') }
    pageTitle = [ordered]@{ ids = @('page-title'); names = @() }
}
$script:pageNames = $pageNames
$script:pageSpecs = $pageSpecs
$script:controlSpecs = $controlSpecs
$script:missingOutput = $missingOutput
$script:root = $root
$script:binaryPath = $binaryPath
$script:evidencePath = $evidencePath
$script:scratch = $scratch
$script:settingsPath = $settingsPath
$script:emptyEnvironment = $emptyEnvironment
$script:binaryHash = $null
$script:transcribing = $false

$frameworkCandidates = @()
if ([IntPtr]::Size -eq 8) {
    $frameworkCandidates += Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/WPF'
}
$frameworkCandidates += Join-Path $env:WINDIR 'Microsoft.NET/Framework/v4.0.30319/WPF'
$frameworkDirectory = @($frameworkCandidates | Where-Object { Test-Path -LiteralPath $_ }) | Select-Object -First 1
if (-not $frameworkDirectory) { throw 'The Windows .NET Framework UI Automation assemblies were not found.' }
foreach ($assemblyName in @('UIAutomationTypes.dll', 'UIAutomationClient.dll')) {
    $assemblyPath = Join-Path $frameworkDirectory $assemblyName
    if (-not (Test-Path -LiteralPath $assemblyPath)) { throw "The UI Automation assembly is missing: $assemblyPath" }
    try { Add-Type -Path $assemblyPath -ErrorAction Stop } catch { [Reflection.Assembly]::LoadFrom($assemblyPath) | Out-Null }
}
if (-not [type]::GetType('System.Windows.Automation.AutomationElement, UIAutomationClient')) { throw 'System.Windows.Automation could not be loaded from the Windows .NET Framework assemblies.' }
try { Add-Type -AssemblyName System.Drawing -ErrorAction Stop } catch { Add-Type -AssemblyName System.Drawing.Common -ErrorAction Stop }
if (-not ('HeraldGpuiSettingsCaptureNative' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class HeraldGpuiSettingsCaptureNative
{
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
}
'@
}

function Write-JsonFile {
    param([string]$Path, $Value)
    $Value | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $Path -Encoding utf8NoBOM
}

function Write-Action {
    param([string]$Action, [hashtable]$Detail = @{})
    $record = [ordered]@{ action = $Action; at = [DateTime]::UtcNow.ToString('o'); detail = [ordered]@{} }
    foreach ($key in $Detail.Keys) { $record.detail[$key] = $Detail[$key] }
    $script:actionRecords += $record
    if ($script:transcribing) { Write-Host ('ACTION ' + $Action + ' ' + (($record.detail | ConvertTo-Json -Compress -Depth 8))) }
}

function Add-FeatureGap {
    param([string]$Message)
    if ($script:featureGaps -notcontains $Message) { $script:featureGaps += $Message }
    Write-Action 'feature-gap' @{ message = $Message }
}

function Wait-Until {
    param([scriptblock]$Condition, [string]$Failure, [int]$Seconds = 10)
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        try { if (& $Condition) { return } } catch {}
        Start-Sleep -Milliseconds 75
    }
    throw $Failure
}

function Read-Bytes {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { throw "Settings file is missing: $Path" }
    [IO.File]::ReadAllBytes($Path)
}

function Bytes-Equal {
    param([byte[]]$Left, [byte[]]$Right)
    if ($null -eq $Left -or $null -eq $Right -or $Left.Length -ne $Right.Length) { return $false }
    for ($index = 0; $index -lt $Left.Length; $index++) { if ($Left[$index] -ne $Right[$index]) { return $false } }
    $true
}

function Redact-JsonValue {
    param($Value, [string]$PropertyName = '')
    if ($PropertyName -match '(?i)(api.?key|secret|password|token|credential)') { return '[REDACTED]' }
    if ($null -eq $Value) { return $null }
    if ($Value -is [System.Management.Automation.PSCustomObject]) {
        $result = [ordered]@{}
        foreach ($property in $Value.PSObject.Properties) { $result[$property.Name] = Redact-JsonValue $property.Value $property.Name }
        return $result
    }
    if ($Value -is [System.Collections.IDictionary]) {
        $result = [ordered]@{}
        foreach ($key in $Value.Keys) { $result[$key] = Redact-JsonValue $Value[$key] ([string]$key) }
        return $result
    }
    if ($Value -is [System.Collections.IEnumerable] -and $Value -isnot [string]) {
        $result = @()
        foreach ($item in $Value) { $result += ,(Redact-JsonValue $item $PropertyName) }
        return $result
    }
    $Value
}

function Save-SettingsEvidence {
    param([string]$Name)
    $path = Join-Path $script:evidencePath ($Name + '.json')
    $value = Get-Content -LiteralPath $script:settingsPath -Raw | ConvertFrom-Json
    Write-JsonFile $path (Redact-JsonValue $value)
    $path
}

function Assert-NoRuntimeErrors {
    param([string]$Stage)
    $path = Join-Path $script:scratch 'errors.log'
    if (Test-Path -LiteralPath $path) {
        $text = Get-Content -LiteralPath $path -Raw
        if (-not [string]::IsNullOrWhiteSpace($text)) { throw "The owned Herald instance has runtime errors during $Stage." }
    }
}

function Get-ProcessPath {
    param($Process)
    $Process.Refresh()
    [IO.Path]::GetFullPath($Process.Path)
}

function Test-OwnedProcess {
    param($Record, [switch]$AllowExited)
    if ($null -eq $Record -or $null -eq $Record.Process) { return $false }
    try {
        $process = $Record.Process
        $process.Refresh()
        if ($process.Id -ne $Record.Pid) { return $false }
        if ($process.HasExited) { return [bool]$AllowExited }
        if (-not [string]::Equals((Get-ProcessPath $process), $script:binaryPath, [StringComparison]::OrdinalIgnoreCase)) { return $false }
        $started = $process.StartTime.ToUniversalTime()
        [math]::Abs(($started - $Record.StartedUtc).TotalMilliseconds) -lt 2000
    } catch { $false }
}

function Assert-ActiveProcess {
    if (-not (Test-OwnedProcess $script:activeRecord)) { throw 'The owned Herald settings process is not alive or its identity changed.' }
    if ($script:binaryHash -and (Get-FileHash -LiteralPath $script:binaryPath -Algorithm SHA256).Hash -ne $script:binaryHash) { throw 'The settings executable changed during verification.' }
}

function Get-UiWindowForProcess {
    param([int]$ProcessId)
    $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $ProcessId)
    $windows = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
    foreach ($window in $windows) {
        try {
            if ($window.Current.Name -ceq 'Herald settings' -and $window.Current.NativeWindowHandle -ne 0 -and -not $window.Current.IsOffscreen -and [HeraldGpuiSettingsCaptureNative]::IsWindowVisible([IntPtr]$window.Current.NativeWindowHandle)) { return $window }
        } catch {}
    }
    $null
}

function Get-UiRootForRecord {
    param($Record)
    $handle = [IntPtr]$Record.Process.MainWindowHandle
    if ($handle -eq [IntPtr]::Zero) {
        $window = Get-UiWindowForProcess $Record.Pid
        if ($window) { $handle = [IntPtr]$window.Current.NativeWindowHandle }
    }
    if ($handle -eq [IntPtr]::Zero) { return $null }
    try { [System.Windows.Automation.AutomationElement]::FromHandle($handle) } catch { $null }
}

function Assert-UiRoot {
    param($Record, $Root)
    if ($null -eq $Root) { throw 'The owned Herald settings UI Automation root is missing.' }
    $current = $Root.Current
    if ($current.Name -cne 'Herald settings') { throw "The owned settings window title is '$($current.Name)', not 'Herald settings'." }
    if ($current.ProcessId -ne $Record.Pid) { throw 'The settings UI Automation root belongs to another process.' }
    if ($current.NativeWindowHandle -eq 0) { throw 'The settings UI Automation root has no native window.' }
    if (-not [HeraldGpuiSettingsCaptureNative]::IsWindowVisible([IntPtr]$current.NativeWindowHandle)) { throw 'The owned settings window is not visible.' }
}

function New-StartInfo {
    $info = [Diagnostics.ProcessStartInfo]::new($script:binaryPath)
    $info.UseShellExecute = $false
    $info.Environment['HERALD_DATA'] = $script:scratch
    $info.Environment['HERALD_ENV'] = $script:emptyEnvironment
    $info.Environment['HERALD_TTS'] = Join-Path $script:root 'native-announcer/resources/tts/kitten-nano-en-v0_8-int8'
    $info.Environment['ELEVENLABS_API_KEY'] = 'ignored-environment-test-key'
    $info.Environment['HERALD_ELEVENLABS_API_KEY'] = 'ignored-legacy-environment-test-key'
    $info.Environment['ELEVENLABS_API_BASE_URL'] = 'http://127.0.0.1:1'
    foreach ($argument in @('--settings', '--assets', (Join-Path $script:root 'native-announcer/resources'))) { [void]$info.ArgumentList.Add($argument) }
    $info
}

function Write-LaunchIdentity {
    param($Record, [string]$Name, [hashtable]$Extra = @{})
    $identity = [ordered]@{
        role = $Record.Role
        binary = $script:binaryPath
        sha256 = $script:binaryHash
        pid = $Record.Pid
        started = $Record.StartedUtc.ToString('o')
        hwnd = $Record.Hwnd.ToInt64()
        scratch = $script:scratch
    }
    foreach ($key in $Extra.Keys) { $identity[$key] = $Extra[$key] }
    $path = Join-Path $script:evidencePath $Name
    Write-JsonFile $path $identity
    $identity
}

function Launch-Settings {
    param([string]$Role)
    $info = New-StartInfo
    $process = [Diagnostics.Process]::Start($info)
    $script:launchCount++
    $record = [pscustomobject]@{
        Role = $Role
        Process = $process
        Pid = $process.Id
        StartedUtc = $process.StartTime.ToUniversalTime()
        Hwnd = [IntPtr]::Zero
    }
    $script:ownedRecords += $record
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) { throw "The $Role settings process exited early with code $($process.ExitCode)." }
        $root = Get-UiRootForRecord $record
        if ($root) {
            try {
                Assert-UiRoot $record $root
                if (-not [string]::Equals((Get-ProcessPath $process), $script:binaryPath, [StringComparison]::OrdinalIgnoreCase)) { throw "The $Role settings process executable is not the requested binary." }
                $record.Hwnd = [IntPtr]$root.Current.NativeWindowHandle
                $script:activeRecord = $record
                $script:activeRoot = $root
                Assert-ActiveProcess
                Write-LaunchIdentity $record ("instance-$($script:launchCount).json") | Out-Null
                Assert-NoRuntimeErrors $Role
                return
            } catch {}
        }
        Start-Sleep -Milliseconds 75
    }
    Assert-NoRuntimeErrors $Role
    throw "The owned $Role settings window did not become an exact visible Herald settings UI Automation window."
}

function Launch-ExistingInstanceProbe {
    Assert-ActiveProcess
    $first = $script:activeRecord
    $info = New-StartInfo
    $process = [Diagnostics.Process]::Start($info)
    $script:launchCount++
    $record = [pscustomobject]@{
        Role = 'second-launch'
        Process = $process
        Pid = $process.Id
        StartedUtc = $process.StartTime.ToUniversalTime()
        Hwnd = [IntPtr]::Zero
    }
    $script:ownedRecords += $record
    if (-not $process.WaitForExit(10000)) { throw 'The second settings launch did not exit within the bounded singleton wait.' }
    $process.Refresh()
    if (-not (Test-OwnedProcess $record -AllowExited)) { throw 'The second settings launch identity did not match the requested executable.' }
    if ($process.ExitCode -ne 0) { throw "The second settings launch exited with code $($process.ExitCode)." }
    if (-not (Test-OwnedProcess $first)) { throw 'The second settings launch did not preserve the first owned settings process.' }
    Assert-NoRuntimeErrors 'second-launch'
    $identity = Write-LaunchIdentity $record "instance-$($script:launchCount).json" @{ existingWindowHwnd = $first.Hwnd.ToInt64(); firstPidStillAlive = $true; exitCode = $process.ExitCode; exited = $true }
    Write-JsonFile (Join-Path $script:evidencePath 'instance-second-launch.json') $identity
    Write-Action 'second-launch' @{ pid = $record.Pid; firstPid = $first.Pid; exitCode = $process.ExitCode }
    $identity
}

function Get-UiaPattern {
    param($Element, [string]$Kind)
    if ($null -eq $Element) { return $null }
    $pattern = switch ($Kind) {
        'Invoke' { [System.Windows.Automation.InvokePattern]::Pattern }
        'Toggle' { [System.Windows.Automation.TogglePattern]::Pattern }
        'Value' { [System.Windows.Automation.ValuePattern]::Pattern }
        'SelectionItem' { [System.Windows.Automation.SelectionItemPattern]::Pattern }
        'ExpandCollapse' { [System.Windows.Automation.ExpandCollapsePattern]::Pattern }
        'RangeValue' { [System.Windows.Automation.RangeValuePattern]::Pattern }
        default { throw "Unknown UI Automation pattern '$Kind'." }
    }
    try { $Element.GetCurrentPattern($pattern) } catch { $null }
}

function Get-UiaElements {
    param($Scope = $script:activeRoot)
    Assert-UiRoot $script:activeRecord $script:activeRoot
    $result = @($Scope)
    $descendants = $Scope.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($element in $descendants) { $result += $element }
    $result
}

function Test-UiaElementUsable {
    param($Element, [switch]$AllowDisabled)
    try {
        $current = $Element.Current
        if ($current.ProcessId -ne $script:activeRecord.Pid) { return $false }
        if ($current.IsOffscreen) { return $false }
        $rect = $current.BoundingRectangle
        if ($rect.Width -le 0 -or $rect.Height -le 0) { return $false }
        if (-not $AllowDisabled -and -not $current.IsEnabled) { return $false }
        $true
    } catch { $false }
}

function Find-Semantic {
    param(
        [string]$Label,
        [string[]]$AutomationIds = @(),
        [string[]]$Names = @(),
        [ValidateSet('', 'Invoke', 'Toggle', 'Value', 'SelectionItem', 'ExpandCollapse', 'RangeValue')]
        [string]$PatternKind = '',
        [switch]$Optional,
        [switch]$AllowDisabled
    )
    $all = @(Get-UiaElements)
    $ordered = @()
    foreach ($automationId in $AutomationIds) { $ordered += @($all | Where-Object { try { $_.Current.AutomationId -ceq $automationId } catch { $false } }) }
    foreach ($name in $Names) { $ordered += @($all | Where-Object { try { $_.Current.Name -ceq $name } catch { $false } }) }
    foreach ($element in $ordered) {
        if (-not (Test-UiaElementUsable $element -AllowDisabled:$AllowDisabled)) { continue }
        if ($PatternKind -and -not (Get-UiaPattern $element $PatternKind)) {
            if ($PatternKind -eq 'SelectionItem' -and ($Ids -contains $element.Current.AutomationId -or $Names -contains $element.Current.Name)) {
                $parent = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($element)
                if ($parent -and (Get-UiaPattern $parent $PatternKind)) { return $parent }
            }
            if ($PatternKind -eq 'RangeValue' -and ($Ids -contains $element.Current.AutomationId -or $Names -contains $element.Current.Name)) {
                foreach ($child in @(Get-UiaElements $element)) {
                    if (Get-UiaPattern $child $PatternKind) { return $child }
                }
            }
            continue
        }
        return $element
    }
    if ($Optional) { return $null }
    throw "UI Automation control '$Label' was not accessible by its contract AutomationId or exact Name."
}

function Get-UiaValue {
    param($Element)
    $valuePattern = Get-UiaPattern $Element 'Value'
    if ($valuePattern) { return [string]$valuePattern.Current.Value }
    $rangePattern = Get-UiaPattern $Element 'RangeValue'
    if ($rangePattern) { return ([string]$rangePattern.Current.Value) }
    $togglePattern = Get-UiaPattern $Element 'Toggle'
    if ($togglePattern) { return ([string]$togglePattern.Current.ToggleState) }
    [string]$Element.Current.Name
}

function Get-UiaToggleState {
    param($Element)
    $pattern = Get-UiaPattern $Element 'Toggle'
    if (-not $pattern) { throw 'The requested checkbox does not expose TogglePattern.' }
    $pattern.Current.ToggleState.ToString()
}

function Get-UiaRangeValue {
    param($Element)
    $pattern = Get-UiaPattern $Element 'RangeValue'
    if (-not $pattern) { throw 'The requested slider does not expose RangeValuePattern.' }
    [double]$pattern.Current.Value
}

function Set-UiaValue {
    param($Element, [string]$Value, [string]$Label)
    $pattern = Get-UiaPattern $Element 'Value'
    if (-not $pattern) { throw "UI Automation control '$Label' does not expose ValuePattern." }
    $pattern.SetValue($Value)
    Write-Action 'set-value' @{ target = $Label; length = $Value.Length }
    Wait-Until { (Get-UiaValue $Element) -ceq $Value } "UI Automation did not reread the '$Label' value after setting it."
}

function Set-UiaToggle {
    param($Element, [bool]$Desired, [string]$Label)
    $desiredState = if ($Desired) { 'On' } else { 'Off' }
    $before = Get-UiaToggleState $Element
    if ($before -ne $desiredState) {
        $pattern = Get-UiaPattern $Element 'Toggle'
        $pattern.Toggle()
        Write-Action 'toggle' @{ target = $Label; desired = $desiredState }
    }
    Wait-Until { (Get-UiaToggleState $Element) -eq $desiredState } "UI Automation did not reread the '$Label' toggle state."
}

function Set-UiaRange {
    param($Element, [double]$Value, [string]$Label)
    $pattern = Get-UiaPattern $Element 'RangeValue'
    if (-not $pattern) { throw "UI Automation control '$Label' does not expose RangeValuePattern." }
    $minimum = [double]$pattern.Current.Minimum
    $maximum = [double]$pattern.Current.Maximum
    if ($Value -lt $minimum -or $Value -gt $maximum) { throw "The '$Label' range does not contain the requested value." }
    if (-not [HeraldGpuiSettingsCaptureNative]::SetForegroundWindow($script:activeRecord.Hwnd)) { throw 'Could not focus the owned settings window.' }
    $bounds = $Element.Current.BoundingRectangle
    $fraction = ($Value - $minimum) / ($maximum - $minimum)
    [HeraldGpuiSettingsCaptureNative]::SetCursorPos([int]($bounds.X + $bounds.Width * $fraction), [int]($bounds.Y + $bounds.Height / 2)) | Out-Null
    [HeraldGpuiSettingsCaptureNative]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    [HeraldGpuiSettingsCaptureNative]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
    Write-Action 'set-range' @{ target = $Label; value = $Value }
    Wait-Until { [math]::Abs((Get-UiaRangeValue $Element) - $Value) -lt 0.01 } "UI Automation did not reread the '$Label' range value."
}

function Invoke-Uia {
    param($Element, [string]$Label)
    $pattern = Get-UiaPattern $Element 'Invoke'
    if (-not $pattern) { throw "UI Automation control '$Label' does not expose InvokePattern." }
    $pattern.Invoke()
    Write-Action 'invoke' @{ target = $Label }
}

function Click-Uia {
    param($Element)
    Assert-ActiveProcess
    if (-not [HeraldGpuiSettingsCaptureNative]::SetForegroundWindow($script:activeRecord.Hwnd)) { throw 'Could not focus the owned settings window.' }
    $bounds = $Element.Current.BoundingRectangle
    [HeraldGpuiSettingsCaptureNative]::SetCursorPos([int]($bounds.X + $bounds.Width / 2), [int]($bounds.Y + $bounds.Height / 2)) | Out-Null
    [HeraldGpuiSettingsCaptureNative]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
    [HeraldGpuiSettingsCaptureNative]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
}

function Select-UiaItem {
    param($Element, [string]$Label)
    $selection = Get-UiaPattern $Element 'SelectionItem'
    if ($selection) {
        Click-Uia $Element
        Write-Action 'select-item' @{ target = $Label; pattern = 'owned-pointer-click' }
        return
    }
    $invoke = Get-UiaPattern $Element 'Invoke'
    if ($invoke) {
        $invoke.Invoke()
        Write-Action 'select-item' @{ target = $Label; pattern = 'InvokePattern' }
        return
    }
    throw "UI Automation navigation item '$Label' exposes neither SelectionItemPattern nor InvokePattern."
}

function Get-PageElement {
    param([string]$Page)
    $spec = $script:pageSpecs[$Page]
    $button = Find-Semantic $Page $spec.ids $spec.names 'Invoke' -Optional
    if ($button) { return $button }
    Find-Semantic $Page $spec.ids $spec.names 'SelectionItem'
}

function Select-Page {
    param([string]$Page)
    $element = Get-PageElement $Page
    Select-UiaItem $element $Page
    Wait-Until {
        $title = Find-Semantic 'page title' @('page-title') @() '' -Optional -AllowDisabled
        $title -and $title.Current.Name -ceq $Page
    } "The '$Page' page heading did not settle."
    Write-Action 'select-page' @{ page = $Page }
}

function Get-SensitiveElement {
    param($Element)
    try {
        $current = $Element.Current
        return (($current.AutomationId + ' ' + $current.Name) -match '(?i)(api.?key|secret|password|token|credential)')
    } catch { $false }
}

function Get-ElementSnapshot {
    param($Element)
    try {
        $current = $Element.Current
        $sensitive = Get-SensitiveElement $Element
        $patterns = @()
        foreach ($kind in @('Invoke', 'Toggle', 'Value', 'SelectionItem', 'ExpandCollapse', 'RangeValue')) { if (Get-UiaPattern $Element $kind) { $patterns += $kind + 'Pattern' } }
        $value = if ($sensitive) { '[REDACTED]' } else { Get-UiaValue $Element }
        $rect = $current.BoundingRectangle
        [ordered]@{
            automationId = $current.AutomationId
            name = $current.Name
            controlType = $current.ControlType.ProgrammaticName
            className = $current.ClassName
            frameworkId = $current.FrameworkId
            processId = $current.ProcessId
            nativeWindowHandle = $current.NativeWindowHandle
            enabled = $current.IsEnabled
            offscreen = $current.IsOffscreen
            value = $value
            valueRedacted = [bool]$sensitive
            patterns = $patterns
            bounds = [ordered]@{ left = $rect.Left; top = $rect.Top; right = $rect.Right; bottom = $rect.Bottom; width = $rect.Width; height = $rect.Height }
        }
    } catch { $null }
}

function Save-UiaSnapshot {
    param([string]$Name)
    Assert-ActiveProcess
    Assert-UiRoot $script:activeRecord $script:activeRoot
    $elements = @()
    foreach ($element in @(Get-UiaElements)) {
        $snapshot = Get-ElementSnapshot $element
        if ($snapshot) { $elements += ,$snapshot }
    }
    $rootCurrent = $script:activeRoot.Current
    $snapshot = [ordered]@{
        name = $Name
        captured = [DateTime]::UtcNow.ToString('o')
        processId = $script:activeRecord.Pid
        hwnd = $script:activeRecord.Hwnd.ToInt64()
        root = [ordered]@{ name = $rootCurrent.Name; automationId = $rootCurrent.AutomationId; processId = $rootCurrent.ProcessId; nativeWindowHandle = $rootCurrent.NativeWindowHandle }
        elements = $elements
    }
    $path = Join-Path $script:evidencePath ($Name + '.json')
    Write-JsonFile $path $snapshot
    $script:snapshotPaths += $path
    $path
}

function Capture-Window {
    param([string]$Name)
    Assert-ActiveProcess
    $hwnd = $script:activeRecord.Hwnd
    $owner = [uint32]0
    [HeraldGpuiSettingsCaptureNative]::GetWindowThreadProcessId($hwnd, [ref]$owner) | Out-Null
    if ($owner -ne [uint32]$script:activeRecord.Pid -or -not [HeraldGpuiSettingsCaptureNative]::IsWindowVisible($hwnd)) { throw "The capture target '$Name' is not the owned visible settings window." }
    [HeraldGpuiSettingsCaptureNative]::ShowWindow($hwnd, 9) | Out-Null
    if (-not [HeraldGpuiSettingsCaptureNative]::SetForegroundWindow($hwnd)) { throw "The owned settings window could not be foregrounded for '$Name'." }
    Wait-Until { [HeraldGpuiSettingsCaptureNative]::GetForegroundWindow() -eq $hwnd } "The owned settings window did not become foreground for '$Name'." 5
    $rect = [HeraldGpuiSettingsCaptureNative+Rect]::new()
    [HeraldGpuiSettingsCaptureNative]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
    if (-not [HeraldGpuiSettingsCaptureNative]::GetWindowRect($hwnd, [ref]$rect)) { throw "The owned settings window rectangle could not be read for '$Name'." }
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -le 0 -or $height -le 0) { throw "The owned settings window has no capture region for '$Name'." }
    $bitmap = $null
    $graphics = $null
    try {
        $bitmap = [Drawing.Bitmap]::new($width, $height, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, [Drawing.Size]::new($width, $height))
        $path = Join-Path $script:evidencePath ($Name + '.png')
        $bitmap.Save($path, [Drawing.Imaging.ImageFormat]::Png)
        Write-Action 'capture-window' @{ name = $Name; method = 'owned-desktop-region'; width = $width; height = $height }
        [ordered]@{ path = $path; method = 'owned-desktop-region'; width = $width; height = $height }
    } finally {
        if ($graphics) { $graphics.Dispose() }
        if ($bitmap) { $bitmap.Dispose() }
    }
}

function Get-TextMatch {
    param([string[]]$Expected)
    foreach ($element in @(Get-UiaElements)) {
        if (-not (Test-UiaElementUsable $element -AllowDisabled)) { continue }
        try {
            $current = $element.Current
            $valuePattern = Get-UiaPattern $element 'Value'
            $value = if ($valuePattern) { [string]$valuePattern.Current.Value } else { [string]$current.Name }
            if ($Expected -contains $value) { return [ordered]@{ text = $value; automationId = $current.AutomationId; name = $current.Name; valueRedacted = [bool](Get-SensitiveElement $element) } }
        } catch {}
    }
    $null
}

function Wait-TextMatch {
    param([string[]]$Expected, [string]$Failure, [int]$Seconds = 10)
    $script:lastTextMatch = $null
    Wait-Until { $match = Get-TextMatch $Expected; if ($match) { $script:lastTextMatch = $match; return $true }; $false } $Failure $Seconds
    $script:lastTextMatch
}

function Get-Control {
    param([string]$Name, [string]$PatternKind = '', [switch]$Optional, [switch]$AllowDisabled)
    $spec = $script:controlSpecs[$Name]
    Find-Semantic $Name $spec.ids $spec.names $PatternKind -Optional:$Optional -AllowDisabled:$AllowDisabled
}

function Get-ComboSelection {
    param($Combo)
    $valuePattern = Get-UiaPattern $Combo 'Value'
    if ($valuePattern) {
        $value = [string]$valuePattern.Current.Value
        if (-not [string]::IsNullOrWhiteSpace($value) -and $value -notin @('Audio output', 'Output device')) { return $value }
    }
    foreach ($element in @(Get-UiaElements $Combo)) {
        $selection = Get-UiaPattern $element 'SelectionItem'
        if ($selection) {
            try { if ($selection.Current.IsSelected -and -not [string]::IsNullOrWhiteSpace($element.Current.Name)) { return [string]$element.Current.Name } } catch {}
        }
    }
    $null
}

function Inspect-OutputDropdown {
    $combo = Get-Control 'output' ''
    $expand = Get-UiaPattern $combo 'ExpandCollapse'
    if (-not $expand) { Add-FeatureGap 'The output dropdown did not expose ExpandCollapsePattern, so its item-selection path is unverified.'; return [ordered]@{ available = $false } }
    $before = Get-ComboSelection $combo
    if ([string]::IsNullOrWhiteSpace($before)) { Add-FeatureGap 'Output selection text was not exposed through UI Automation.'; return [ordered]@{ available = $false } }
    Click-Uia $combo
    Write-Action 'expand-output' @{ target = 'output' }
    Start-Sleep -Milliseconds 300
    Save-UiaSnapshot 'uia-output-expanded' | Out-Null
    Capture-Window 'output-expanded'
    try {
        Wait-Until { (Find-Semantic 'system default output item' @() @('System default') '' -Optional) -ne $null } 'The output dropdown did not expose its options.' 5
        $item = Find-Semantic 'current output item' @() @($before) 'SelectionItem' -Optional -AllowDisabled
        if (-not $item) { Add-FeatureGap 'Output dropdown items were not exposed as owned SelectionItemPattern elements.'; return [ordered]@{ available = $true; before = $before; itemSelected = $false } }
        Select-UiaItem $item $before
        Wait-Until { $selected = Get-ComboSelection (Get-Control 'output' ''); $selected -ceq $before } 'The output dropdown did not reread its selected item.' 5
        [ordered]@{ available = $true; before = $before; itemSelected = $true; after = Get-ComboSelection (Get-Control 'output' '') }
    } finally {
        Wait-Until { (Find-Semantic 'system default output item' @() @('System default') 'SelectionItem' -Optional) -eq $null } 'The selected output menu did not close.' 5
        Write-Action 'collapse-output' @{ target = 'output' }
    }
}

function Assert-EnvironmentKeyIgnored {
    $key = Get-Control 'apiKey' '' -AllowDisabled
    if (-not $key.Current.IsPassword) { throw 'The ElevenLabs key field must be masked.' }
    Wait-TextMatch @('Enter an ElevenLabs key to load voice usage.') 'The settings UI did not show its saved-only, unconfigured key status.' | Out-Null
    Write-Action 'legacy-environment-key' @{ ignored = $true }
}

function Verify-SystemDefaultOutput {
    Select-Page 'Audio'
    Click-Uia (Get-Control 'output' '')
    Wait-Until { (Find-Semantic 'system default output item' @() @('System default') 'SelectionItem' -Optional) -ne $null } 'The output dropdown did not expose System default.'
    Select-UiaItem (Find-Semantic 'system default output item' @() @('System default') 'SelectionItem') 'System default'
    Wait-Until { (Get-ComboSelection (Get-Control 'output' '')) -ceq 'System default' } 'The output dropdown did not select System default.'
    Invoke-Uia (Get-Control 'apply' 'Invoke') 'Apply default output'
    Start-Sleep -Milliseconds 300
    Save-UiaSnapshot 'uia-after-default-apply' | Out-Null
    Copy-Item -LiteralPath $script:settingsPath -Destination (Join-Path $script:evidencePath 'settings-default-output.json')
    Wait-Until { $settings = Get-Content -LiteralPath $script:settingsPath -Raw | ConvertFrom-Json; $null -eq $settings.outputDevice } 'System default did not persist a null output device.'
    Save-UiaSnapshot 'uia-system-default' | Out-Null
    Write-Action 'system-default-persisted' @{ outputDevice = $null }
}

function Set-QuietDraft {
    Select-Page 'Quiet hours'
    $quiet = Get-Control 'quiet' 'Toggle'
    $schedule = Get-Control 'schedule' 'Toggle'
    Set-UiaToggle $quiet $true 'quiet mode'
    Set-UiaToggle $schedule $true 'quiet schedule'
    $start = Get-Control 'quietStart' 'Value' -AllowDisabled
    $end = Get-Control 'quietEnd' 'Value' -AllowDisabled
    Set-UiaValue $start '22:30' 'quiet start'
    Set-UiaValue $end '08:15' 'quiet end'
    Save-UiaSnapshot 'uia-quiet-draft' | Out-Null
}

function Run-SilentPreview {
    Select-Page 'Audio'
    $before = Read-Bytes $script:settingsPath
    $volume = Get-Control 'volume' 'RangeValue'
    Set-UiaRange $volume 35 'volume draft'
    Set-UiaRange $volume 0 'volume draft'
    $preview = Get-Control 'preview' 'Invoke'
    Invoke-Uia $preview 'Play example'
    $match = Wait-TextMatch @('Preview is silent at 0% volume.', 'Preview is silent at 0% volume') 'The zero-volume preview did not report a silent preview.' 10
    $after = Read-Bytes $script:settingsPath
    if (-not (Bytes-Equal $before $after)) { throw 'The zero-volume preview saved unsaved settings bytes.' }
    Write-JsonFile (Join-Path $script:evidencePath 'preview-silent.json') ([ordered]@{ status = $match; savedBytesUnchanged = $true; volume = 0; audiblePreview = $false })
    Save-UiaSnapshot 'uia-after-silent-preview' | Out-Null
}

function Try-CharacterDraft {
    Select-Page 'Characters'
    $new = Get-Control 'characterNew' 'Invoke' -Optional
    if (-not $new) { Add-FeatureGap 'Character create/edit controls were not exposed through the UI Automation contract.'; return $false }
    Invoke-Uia $new 'New character'
    Wait-Until { $candidate = Get-Control 'characterName' 'Value' -Optional -AllowDisabled; $null -ne $candidate } 'The new character editor did not expose its name field.' 8
    $name = 'GPUI verification character ' + [guid]::NewGuid().ToString('N').Substring(0, 8)
    $nameControl = Get-Control 'characterName' 'Value' -AllowDisabled
    Set-UiaValue $nameControl $name 'character name'
    $prompt = Get-Control 'characterPrompt' 'Value' -Optional -AllowDisabled
    if ($prompt) { Set-UiaValue $prompt 'Character draft prompt.' 'character prompt' }
    $script:characterDraftName = $name
    Save-UiaSnapshot 'uia-character-draft' | Out-Null
    $true
}

function Set-AnnouncementDraft {
    Select-Page 'Announcements'
    $prompt = Get-Control 'summaryPrompt' 'Value'
    Set-UiaValue $prompt $script:announcementPrompt 'announcement prompt'
    Save-UiaSnapshot 'uia-announcement-draft' | Out-Null
}

function Invoke-Apply {
    $before = Read-Bytes $script:settingsPath
    $apply = Get-Control 'apply' 'Invoke'
    Invoke-Uia $apply 'Apply'
    Wait-Until { $current = Read-Bytes $script:settingsPath; -not (Bytes-Equal $before $current) } 'Apply did not persist a changed settings file.' 10
    Assert-NoRuntimeErrors 'Apply'
    $script:appliedBytes = Read-Bytes $script:settingsPath
    Save-SettingsEvidence 'settings-after-apply' | Out-Null
    Save-UiaSnapshot 'uia-after-apply' | Out-Null
    Write-Action 'apply-reread' @{ savedBytesChanged = $true }
}

function Get-JsonProperty {
    param($Object, [string]$Name)
    if ($null -eq $Object) { return $null }
    $property = $Object.PSObject.Properties[$Name]
    if ($property) { $property.Value } else { $null }
}

function Assert-SavedValues {
    param([bool]$CheckQuiet, [bool]$CheckOutput, [bool]$CheckPrompt, [bool]$CheckCharacter)
    $settings = Get-Content -LiteralPath $script:settingsPath -Raw | ConvertFrom-Json
    if ($CheckQuiet) {
        if ([bool](Get-JsonProperty $settings 'quietMode') -ne $true -or [bool](Get-JsonProperty $settings 'scheduleEnabled') -ne $true -or [int](Get-JsonProperty $settings 'quietStart') -ne 1350 -or [int](Get-JsonProperty $settings 'quietEnd') -ne 495) { throw 'Saved quiet schedule values do not match the UI inputs.' }
    }
    if ($CheckOutput -and [string](Get-JsonProperty $settings 'outputDevice') -cne $script:missingOutput) { throw 'Apply did not preserve the unavailable output device.' }
    $voices = Get-JsonProperty $settings 'voices'
    if ($null -eq $voices -or [string](Get-JsonProperty $voices 'claude') -cne 'Mark') { throw 'Apply did not preserve the seeded Claude voice preference.' }
    if ($CheckPrompt -and [string](Get-JsonProperty $settings 'summaryPrompt') -cne $script:announcementPrompt) { throw 'Apply did not save the announcement prompt exactly.' }
    if ($CheckCharacter) {
        $characters = Get-JsonProperty $settings 'characters'
        $matches = @($characters.PSObject.Properties | Where-Object { [string](Get-JsonProperty $_.Value 'name') -ceq $script:characterDraftName })
        if ($matches.Count -ne 1) { throw 'Apply did not save the character draft.' }
    }
    Write-Action 'saved-values' @{ quiet = $CheckQuiet; output = $CheckOutput; prompt = $CheckPrompt; character = $CheckCharacter; voice = 'Mark' }
}

function Change-PromptForDiscard {
    Select-Page 'Announcements'
    $prompt = Get-Control 'summaryPrompt' 'Value'
    $discardText = "This text must be discarded.`nSecond line is also unapplied."
    Set-UiaValue $prompt $discardText 'unapplied announcement prompt'
    Save-UiaSnapshot 'uia-before-discard-close' | Out-Null
    if (Bytes-Equal $script:appliedBytes (Read-Bytes $script:settingsPath)) { Write-Action 'discard-draft-ready' @{ savedBytesUnchanged = $true } }
}

function Close-ActiveSettings {
    Assert-ActiveProcess
    $close = Get-Control 'close' 'Invoke'
    Invoke-Uia $close 'Close'
    $process = $script:activeRecord.Process
    if (-not $process.WaitForExit(10000)) { throw 'Close did not exit the owned settings process within the bounded wait.' }
    $process.Refresh()
    if ($process.ExitCode -ne 0) { throw "Close exited the owned settings process with code $($process.ExitCode)." }
    Assert-NoRuntimeErrors 'Close'
    Write-Action 'close-reread' @{ exitCode = $process.ExitCode }
    $script:activeRoot = $null
    $script:activeRecord = $null
}

function Read-ReopenedOutput {
    $combo = Get-Control 'output' ''
    $value = Get-ComboSelection $combo
    if ([string]::IsNullOrWhiteSpace($value)) { Add-FeatureGap 'The reopened output selection was not readable through UI Automation.'; return }
    if ($value -eq 'System default') { throw 'The reopened settings UI lost the unavailable output selection.' }
    Write-Action 'reopen-output' @{ selected = 'redacted-unavailable-selection' }
}

function Verify-ReopenedValues {
    param([bool]$CheckQuiet, [bool]$CheckOutput, [bool]$CheckPrompt, [bool]$CheckCharacter)
    if ($CheckQuiet) {
        Select-Page 'Quiet hours'
        if ((Get-UiaToggleState (Get-Control 'quiet' 'Toggle')) -ne 'On' -or (Get-UiaToggleState (Get-Control 'schedule' 'Toggle')) -ne 'On') { throw 'Reopening did not restore quiet schedule checkboxes.' }
        if ((Get-UiaValue (Get-Control 'quietStart' 'Value' -AllowDisabled)) -cne '22:30' -or (Get-UiaValue (Get-Control 'quietEnd' 'Value' -AllowDisabled)) -cne '08:15') { throw 'Reopening did not restore quiet schedule input values.' }
    }
    if ($CheckOutput) { Select-Page 'Audio'; if ([math]::Abs((Get-UiaRangeValue (Get-Control 'volume' 'RangeValue')) - 0) -gt 0.01) { throw 'Reopening did not restore zero volume.' }; Read-ReopenedOutput }
    if ($CheckPrompt) { Select-Page 'Announcements'; if ((Get-UiaValue (Get-Control 'summaryPrompt' 'Value')) -cne $script:announcementPrompt) { throw 'Reopening did not restore the exact announcement prompt.' } }
    if ($CheckCharacter) {
        Select-Page 'Characters'
        $nameControl = Get-Control 'characterName' 'Value' -Optional -AllowDisabled
        if ($nameControl) {
            if ((Get-UiaValue $nameControl) -cne $script:characterDraftName) { throw 'Reopening did not restore the character name.' }
        } else {
            $item = Find-Semantic 'saved character' @() @($script:characterDraftName) 'SelectionItem' -Optional -AllowDisabled
            if (-not $item) { Add-FeatureGap 'The saved character was not readable through the reopened UI Automation tree.' }
        }
    }
    Save-UiaSnapshot 'uia-after-reopen' | Out-Null
}

function Stop-OwnedProcesses {
    $allExited = $true
    $ownershipConfirmed = $true
    foreach ($record in @($script:ownedRecords)) {
        try {
            $record.Process.Refresh()
            if ($record.Process.HasExited) { continue }
            if (-not (Test-OwnedProcess $record)) { $ownershipConfirmed = $false; $allExited = $false; continue }
            $record.Process.Kill()
            if (-not $record.Process.WaitForExit(5000)) { $allExited = $false }
        } catch { $allExited = $false; $ownershipConfirmed = $false }
    }
    foreach ($record in @($script:ownedRecords)) {
        try { $record.Process.Refresh(); if (-not $record.Process.HasExited) { $allExited = $false } } catch { $allExited = $false }
    }
    [ordered]@{ processExited = $allExited; ownershipConfirmed = $ownershipConfirmed }
}

try {
    if (-not (Test-Path -LiteralPath $script:binaryPath)) { throw "Build or provide the release settings binary: $script:binaryPath" }
    if (Test-Path -LiteralPath $script:evidencePath) { throw "Evidence directory already exists: $script:evidencePath" }
    if (Test-Path -LiteralPath $script:scratch) { throw "Scratch directory already exists: $script:scratch" }
    New-Item -ItemType Directory -Path $script:evidencePath -Force | Out-Null
    New-Item -ItemType Directory -Path $script:scratch -Force | Out-Null
    $scratchCreated = $true
    Set-Content -LiteralPath $script:emptyEnvironment -Value '' -Encoding utf8NoBOM
    Start-Transcript -Path (Join-Path $script:evidencePath 'actions.txt') | Out-Null
    $transcribing = $true
    $script:transcribing = $true
    $script:root = $root
    $script:binaryPath = $binaryPath
    $script:evidencePath = $evidencePath
    $script:scratch = $scratch
    $script:settingsPath = $settingsPath
    $script:emptyEnvironment = $emptyEnvironment
    $binaryHash = (Get-FileHash -LiteralPath $binaryPath -Algorithm SHA256).Hash
    $script:binaryHash = $binaryHash
    $fixture = [ordered]@{
        quietMode = $false
        scheduleEnabled = $false
        quietStart = 1320
        quietEnd = 480
        volume = 0
        silentSoundSeconds = 0
        outputDevice = $missingOutput
        voices = [ordered]@{ claude = 'Mark' }
        summaryPrompt = 'Fixture prompt.'
    }
    $fixture | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $settingsPath -Encoding utf8NoBOM
    Save-SettingsEvidence 'settings-before' | Out-Null
    $firstLaunchBytes = Read-Bytes $settingsPath
    Write-Action 'fixture' @{ volume = 0; scheduleEnabled = $false; missingOutput = $true; seededVoice = 'Mark'; legacyEnvironmentKey = 'ignored' }

    Launch-Settings 'first-launch'
    Assert-NoRuntimeErrors 'ready'
    Save-UiaSnapshot 'uia-before' | Out-Null
    $secondIdentity = Launch-ExistingInstanceProbe
    $pageResults = @()
    foreach ($page in $script:pageNames) {
        Select-Page $page
        $slug = $page.ToLowerInvariant().Replace(' ', '-')
        $snapshot = Save-UiaSnapshot ('uia-page-' + $slug)
        $capture = Capture-Window ('page-' + $slug)
        $pageResults += [ordered]@{ page = $page; snapshot = $snapshot; screenshot = $capture.path; capture = $capture }
    }
    Select-Page 'Speech service'
    Assert-EnvironmentKeyIgnored
    Select-Page 'Audio'
    $outputDropdown = if ($Feature -in @('All', 'Settings', 'Output')) { Inspect-OutputDropdown } else { $null }
    Save-UiaSnapshot 'uia-before-actions' | Out-Null

    $runQuiet = $Feature -in @('All', 'Settings', 'Quiet')
    $runOutput = $Feature -in @('All', 'Settings', 'Output')
    $runPreview = $Feature -in @('All', 'Settings', 'Preview')
    $runCharacters = $Feature -in @('All', 'Settings', 'Characters')
    $runAnnouncements = $Feature -in @('All', 'Settings', 'Announcements')
    if ($runQuiet) { Set-QuietDraft }
    if ($runAnnouncements) { Set-AnnouncementDraft }
    if ($runCharacters) { $characterCreated = Try-CharacterDraft } else { $characterCreated = $false }
    if ($runPreview) { Run-SilentPreview }

    $needsApply = $runQuiet -or $runOutput -or ($runCharacters -and $characterCreated) -or $runAnnouncements
    if ($needsApply) {
        Select-Page 'Audio'
        Invoke-Apply
        Assert-SavedValues $runQuiet $runOutput $runAnnouncements ($runCharacters -and $characterCreated)
    } else {
        $script:appliedBytes = Read-Bytes $settingsPath
        Save-SettingsEvidence 'settings-after-apply' | Out-Null
        Save-UiaSnapshot 'uia-after-preview' | Out-Null
    }
    if ($Feature -in @('All', 'Settings')) {
        if (-not $runAnnouncements) { Add-FeatureGap 'The discard-text check requires the Announcements page and was not selected.' } else {
            Change-PromptForDiscard
            if (-not (Bytes-Equal $script:appliedBytes (Read-Bytes $settingsPath))) { throw 'The unapplied discard text changed settings before Close.' }
        }
    }
    Close-ActiveSettings
    if (-not (Bytes-Equal $script:appliedBytes (Read-Bytes $settingsPath))) { throw 'Close did not discard the later unapplied settings text.' }
    Save-SettingsEvidence 'settings-after-close' | Out-Null

    Launch-Settings 'reopen'
    Assert-NoRuntimeErrors 'reopen-ready'
    Save-UiaSnapshot 'uia-reopened' | Out-Null
    Verify-ReopenedValues $runQuiet $runOutput $runAnnouncements ($runCharacters -and $characterCreated)
    if ($runPreview -and -not $runOutput) { Select-Page 'Audio'; if ([math]::Abs((Get-UiaRangeValue (Get-Control 'volume' 'RangeValue')) - 0) -gt 0.01) { throw 'Reopening did not restore zero volume after preview.' } }
    $reopenedBytes = Read-Bytes $settingsPath
    if (-not (Bytes-Equal $script:appliedBytes $reopenedBytes)) { throw 'Reopening changed saved settings bytes.' }
    Save-SettingsEvidence 'settings-after-reopen' | Out-Null
    if ($runOutput) { Verify-SystemDefaultOutput }
    Close-ActiveSettings
    Assert-NoRuntimeErrors 'complete'
    $verdict = if ($script:featureGaps.Count -eq 0) { 'VERIFIED' } else { 'INCONCLUSIVE' }
    $result = [ordered]@{
        verdict = $verdict
        feature = $Feature
        binary = $binaryPath
        sha256 = $binaryHash
        uiAutomation = [ordered]@{ root = 'owned-window'; controlDriver = 'UIAutomationPatternsAndOwnedPointer'; internalSetters = $false; nativeControlHandles = $false }
        launches = @($script:ownedRecords | ForEach-Object { [ordered]@{ role = $_.Role; pid = $_.Pid; started = $_.StartedUtc.ToString('o'); hwnd = $_.Hwnd.ToInt64() } })
        secondLaunch = $secondIdentity
        pages = $pageResults
        outputDropdown = $outputDropdown
        systemDefaultPersisted = $runOutput
        snapshots = $script:snapshotPaths
        screenshots = @($pageResults | ForEach-Object { $_.screenshot })
        savedValues = [ordered]@{ quiet = $runQuiet; output = $runOutput; previewSilent = $runPreview; characters = ($runCharacters -and $characterCreated); announcements = $runAnnouncements; discardedOnClose = ($Feature -in @('All', 'Settings') -and $runAnnouncements); reopened = $true; voicesPreserved = $true }
        audiblePreview = $false
        featureGaps = $script:featureGaps
        actions = $script:actionRecords.Count
        actionTranscript = (Join-Path $evidencePath 'actions.txt')
        scope = 'Real owned GPUI settings window selected through UI Automation, with owned-pointer actions for sliders and dropdowns; preview was exercised only at zero volume.'
    }
    Write-JsonFile (Join-Path $evidencePath 'result.json') $result
    Write-Output ("${verdict}: GPUI settings UI Automation, persistence, singleton and screenshots; evidence: $evidencePath")
}
catch {
    if (Test-Path -LiteralPath $evidencePath) {
        $_ | Out-String | Set-Content -LiteralPath (Join-Path $evidencePath 'failure.txt') -Encoding utf8NoBOM
        try {
            $failureResult = [ordered]@{ verdict = 'FAILED'; feature = $Feature; binary = $binaryPath; sha256 = $binaryHash; error = $_.Exception.Message; snapshots = $script:snapshotPaths; actions = $script:actionRecords.Count; actionTranscript = (Join-Path $evidencePath 'actions.txt') }
            Write-JsonFile (Join-Path $evidencePath 'result.json') $failureResult
        } catch {}
    }
    throw
}
finally {
    if ($transcribing) {
        try { Stop-Transcript | Out-Null } catch {}
        $transcribing = $false
        $script:transcribing = $false
    }
    $processCleanup = Stop-OwnedProcesses
    $scratchRemoved = $true
    if ($scratchCreated -and (Test-Path -LiteralPath $scratch)) {
        if ($processCleanup.processExited) { try { Remove-Item -LiteralPath $scratch -Recurse -Force } catch { $scratchRemoved = $false } } else { $scratchRemoved = $false }
    }
    if (Test-Path -LiteralPath $evidencePath) {
        try {
            Write-JsonFile (Join-Path $evidencePath 'cleanup.json') ([ordered]@{ scratchRemoved = $scratchRemoved -and -not (Test-Path -LiteralPath $scratch); processExited = [bool]$processCleanup.processExited; ownershipConfirmed = [bool]$processCleanup.ownershipConfirmed; ownedProcessCount = @($ownedRecords).Count })
        } catch {}
    }
}
if ($script:featureGaps.Count -gt 0) { throw "Verification is inconclusive. Read the feature gaps in $evidencePath/result.json." }
