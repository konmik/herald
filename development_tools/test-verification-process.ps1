$ErrorActionPreference = 'Stop'
foreach ($directory in @($PSScriptRoot, "$PSScriptRoot/../.claude/skills/verify-civilized-agent/scripts")) {
    foreach ($file in Get-ChildItem -LiteralPath $directory -Filter '*.ps1' -Recurse) {
        $tokens = $null
        $errors = $null
        [Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$errors) | Out-Null
        if ($errors.Count) { throw "$($file.FullName): $($errors.Message -join '; ')" }
    }
}
. "$PSScriptRoot/../.claude/skills/verify-civilized-agent/scripts/process.ps1"
$missingLog = Join-Path $env:LOCALAPPDATA ('Temp/opencode/missing-' + [guid]::NewGuid())
function New-TestProcess {
    param([bool]$Exited, [string]$Path, [switch]$ExitDuringPath, [switch]$ThrowDuringPath)
    $value = [pscustomobject]@{ HasExited = $Exited; ExpectedPath = $Path; Reads = 0; ExitDuringPath = [bool]$ExitDuringPath; ThrowDuringPath = [bool]$ThrowDuringPath }
    $value | Add-Member ScriptMethod Refresh { }
    $value | Add-Member ScriptProperty Path {
        $this.Reads++
        if ($this.ExitDuringPath) { $this.HasExited = $true; return $null }
        if ($this.ThrowDuringPath) { $this.HasExited = $true; throw 'Process already exited' }
        return $this.ExpectedPath
    }
    return $value
}
$live = New-TestProcess $false 'owned.exe'
if (-not (Test-OwnedPlaybackProcess $live 'owned.exe' $missingLog)) { throw 'Owned live process was rejected' }
$exited = New-TestProcess $true 'owned.exe'
if (Test-OwnedPlaybackProcess $exited 'owned.exe' $missingLog) { throw 'Exited process was treated as live' }
if ($exited.Reads -ne 0) { throw 'Exited process path was queried' }
foreach ($option in @('ExitDuringPath', 'ThrowDuringPath')) {
    $parameters = @{ Exited = $false; Path = 'owned.exe'; $option = $true }
    if (Test-OwnedPlaybackProcess (New-TestProcess @parameters) 'owned.exe' $missingLog) { throw 'Exit during path lookup was treated as live' }
}
$rejected = $false
try { Test-OwnedPlaybackProcess (New-TestProcess $false 'other.exe') 'owned.exe' $missingLog | Out-Null } catch { $rejected = $_.Exception.Message -eq 'Doctor failed: wrong process or runtime errors' }
if (-not $rejected) { throw 'Wrong executable was accepted' }
$rejected = $false
try { Test-OwnedPlaybackProcess $live 'owned.exe' $PSCommandPath | Out-Null } catch { $rejected = $_.Exception.Message -eq 'Doctor failed: wrong process or runtime errors' }
if (-not $rejected) { throw 'Runtime error log was accepted' }
Write-Output '6 playback process checks passed.'
