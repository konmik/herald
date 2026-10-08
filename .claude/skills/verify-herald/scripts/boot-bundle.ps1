param(
    [Parameter(Mandatory)][string]$AppDirectory,
    [string]$PluginDirectory,
    [string]$Evidence = ('temp/verification/bundle-boot-' + [guid]::NewGuid())
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../../..'))
$app = [IO.Path]::GetFullPath($AppDirectory)
if (-not $PluginDirectory) { $PluginDirectory = $app }
$PluginDirectory = [IO.Path]::GetFullPath($PluginDirectory)
$evidencePath = [IO.Path]::GetFullPath($Evidence, $root)
$scratch = Join-Path $env:LOCALAPPDATA ('Temp/opencode/herald-boot-' + [guid]::NewGuid())
$driver = $null
$existing = @(Get-CimInstance Win32_Process -Filter "Name LIKE 'herald%.exe'" | ForEach-Object { $_.ProcessId })
$started = [DateTime]::UtcNow
if (Test-Path -LiteralPath $evidencePath) { throw 'Use a new evidence directory' }
New-Item -ItemType Directory -Path $evidencePath, "$scratch/config/opencode", "$scratch/state", "$scratch/data", "$scratch/app-data" -Force | Out-Null
try {
    . (Join-Path $app 'install.ps1')
    @{ plugins = @($PluginDirectory.Replace('\', '/')); snapshots = $false } | ConvertTo-Json | Set-Content -LiteralPath "$scratch/config/opencode/opencode.json" -Encoding utf8NoBOM
    @{ quietMode = $true; scheduleEnabled = $false; volume = 0 } | ConvertTo-Json | Set-Content -LiteralPath "$scratch/data/settings.json" -Encoding utf8NoBOM
    $start = [Diagnostics.ProcessStartInfo]::new((Get-Command bun).Source)
    $start.UseShellExecute = $false
    $start.WorkingDirectory = $root
    $start.Environment['HERALD_DATA'] = "$scratch/data"
    foreach ($name in @('HERALD_EXTERNAL_COMPANION', 'HERALD_BINARY', 'HERALD_TTS')) { $start.Environment.Remove($name) | Out-Null }
    $start.Environment['XDG_CONFIG_HOME'] = "$scratch/config"
    $start.Environment['XDG_STATE_HOME'] = "$scratch/state"
    $start.Environment['XDG_DATA_HOME'] = "$scratch/app-data"
    $start.Environment['OPENCODE_CONFIG'] = "$scratch/config/opencode/opencode.json"
    foreach ($argument in @((Join-Path $root 'development_tools/host-verification/boot.ts'), $scratch, $PluginDirectory, $evidencePath)) { $start.ArgumentList.Add($argument) }
    $driver = [Diagnostics.Process]::Start($start)
    if (-not $driver.WaitForExit(60000)) { throw 'Private package bootstrap exceeded its deadline' }
    if ($driver.ExitCode -ne 0) { throw "Private package bootstrap failed with exit code $($driver.ExitCode)" }
    $errors = Get-Content -LiteralPath (Join-Path $evidencePath 'server-errors.txt') -Raw
    if ($errors -match 'Reinstall the application bundle|Could not initialize Kitten|missing at') { throw 'Installed package bootstrap reported a runtime failure' }
    Write-Output "PASS: OpenCode package at $PluginDirectory booted without executable or external-companion overrides"
} catch {
    $_ | Out-String | Set-Content -LiteralPath (Join-Path $evidencePath 'failure.txt')
    throw
} finally {
    if ($driver -and -not $driver.HasExited) { & taskkill /PID $driver.Id /T /F | Out-Null; $driver.WaitForExit(10000) | Out-Null }
    foreach ($owned in Get-CimInstance Win32_Process -Filter "Name LIKE 'herald%.exe'") {
        if ($owned.ExecutablePath -and $owned.ExecutablePath.StartsWith((Join-Path $app 'native-announcer'), [StringComparison]::OrdinalIgnoreCase) -and $owned.ProcessId -notin $existing -and $owned.CreationDate.ToUniversalTime() -ge $started) {
            $handle = Get-Process -Id $owned.ProcessId -ErrorAction SilentlyContinue
            if ($handle -and -not $handle.HasExited) { $handle.Kill(); $handle.WaitForExit(10000) | Out-Null }
        }
    }
    $serverExited = -not (Test-Path -LiteralPath "$scratch/state/opencode/service.json")
    if ($serverExited -and (Test-Path -LiteralPath $scratch)) { Remove-Item -LiteralPath $scratch -Recurse -Force }
    @{ scratchRemoved = -not (Test-Path -LiteralPath $scratch); driverExited = (-not $driver -or $driver.HasExited); serverExited = $serverExited } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath 'cleanup.json')
}
