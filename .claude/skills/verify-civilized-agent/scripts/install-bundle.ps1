param(
    [Parameter(Mandatory)][string]$Archive,
    [string]$Evidence = ('temp/verification/bundle-install-' + [guid]::NewGuid())
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../../..'))
$evidencePath = [IO.Path]::GetFullPath($Evidence, $root)
$scratch = Join-Path $env:LOCALAPPDATA ('Temp/opencode/ca Ω ' + [guid]::NewGuid())
$profile = Join-Path $scratch 'claude-profile'
$config = Join-Path $scratch 'opencode-profile/opencode.jsonc'
$programs = Join-Path $scratch 'programs'
$oldClaude = $env:CLAUDE_CONFIG_DIR
$process = $null
$transcribing = $false
if (Test-Path -LiteralPath $evidencePath) { throw 'Use a new evidence directory' }
New-Item -ItemType Directory -Path $evidencePath, $scratch, (Split-Path $config -Parent) -Force | Out-Null
try {
    Start-Transcript -Path (Join-Path $evidencePath 'actions.txt') | Out-Null
    $transcribing = $true
    $extract = Join-Path $scratch 'extracted'
    Expand-Archive -LiteralPath $Archive -DestinationPath $extract
    $app = & (Join-Path $extract 'install.ps1') -InstallDirectory (Join-Path $scratch 'installed') -SkipHostRegistration -NoStart
    . (Join-Path $app 'install.ps1')
    Test-Bundle $app | Out-Null
    $env:CLAUDE_CONFIG_DIR = $profile
    Register-ClaudeBundle $app $profile
    $cache = @(Get-ClaudeInstallations $profile)[0].installPath
    Get-PayloadFiles $cache | Out-Null
    $manifest = Get-Content -LiteralPath (Join-Path $app 'bundle-manifest.json') -Raw | ConvertFrom-Json
    foreach ($file in $manifest.files | Where-Object { $_.path.StartsWith('claude-plugin/') }) {
        $path = Join-Path $cache $file.path.Substring('claude-plugin/'.Length)
        if (-not (Test-Path -LiteralPath $path) -or (Get-FileHash -LiteralPath $path).Hash -ne $file.sha256) { throw "Claude cache differs from the bundle at $($file.path)" }
    }
    $original = '{/* preserved */ "plugins": [{"package": ' + (ConvertTo-Json ([string]$root)) + ', "options": {"minimumSeconds": 90}}, "unrelated-plugin"], "model": "preserved"}'
    [IO.File]::WriteAllText($config, $original)
    Register-OpenCodeBundle $app @($config)
    $registered = Get-Content -LiteralPath $config -Raw
    if ($registered.Contains($root.Replace('\', '\\')) -or -not $registered.Contains('/* preserved */') -or -not $registered.Contains('"minimumSeconds": 90') -or -not $registered.Contains('"unrelated-plugin"')) { throw 'OpenCode migration did not preserve settings or remove the checkout reference' }
    $marketplace = Get-Content -LiteralPath (Join-Path $profile 'plugins/known_marketplaces.json') -Raw | ConvertFrom-Json
    if ($marketplace.'civilized-agent-local'.source.path -ne (Join-Path $app 'claude-plugin')) { throw 'Claude marketplace does not use the installed source' }
    $binary = Join-Path $app "native-announcer/bin/civilized-announcer-win32-$($manifest.arch).exe"
    Install-SettingsShortcut $binary $programs
    $shortcut = Read-NativeShortcut (Join-Path $programs 'Civilized Agent settings.lnk')
    if ($shortcut.TargetPath -ne $binary -or $shortcut.Arguments -ne '--settings') { throw 'Installed shortcut points outside the bundle' }
    Remove-DeploymentDirectory $extract
    if (Test-Path -LiteralPath (Join-Path $app 'node_modules')) { throw 'Installed package must not contain checkout dependencies' }
    & pwsh -NoProfile -File (Join-Path $PSScriptRoot 'boot-bundle.ps1') -AppDirectory $app -Evidence (Join-Path $evidencePath 'opencode-bootstrap')
    if ($LASTEXITCODE -ne 0) { throw 'Real installed OpenCode bootstrap failed' }
    $again = Install-Payload $app (Join-Path $scratch 'installed')
    if ($again -ne $app) { throw 'Repeated installation changed its location' }
    $start = [Diagnostics.ProcessStartInfo]::new($binary)
    $start.UseShellExecute = $false
    $data = Join-Path $scratch 'stop-proof-data'
    $start.Environment['CIVILIZED_AGENT_DATA'] = $data
    $start.Environment.Remove('CIVILIZED_AGENT_TTS') | Out-Null
    foreach ($argument in @('--isolated', '--assets', (Join-Path $app 'native-announcer/resources'), '--test-seconds', '60')) { $start.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($start)
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath (Join-Path $data 'inbox'))) {
        if ($process.HasExited -or [DateTime]::UtcNow -gt $deadline) { throw 'Installed announcer did not become ready for the ownership check' }
        Start-Sleep -Milliseconds 50
    }
    $stopped = @(Stop-OwnedAnnouncers @((Join-Path $app 'native-announcer')))
    if ($stopped.Count -ne 1 -or $stopped[0].binary -ne $binary -or -not $process.WaitForExit(5000) -or $stopped[0].assets.TrimEnd('\', '/') -ne (Join-Path $app 'native-announcer/resources').TrimEnd('\', '/')) { throw 'Owned installed announcer was not stopped correctly' }
    @{ archive = [IO.Path]::GetFullPath($Archive); app = $app; claudePlugin = $cache; claudeConfig = $profile; openCodeConfig = $config; programs = $programs; scratch = $scratch; extractionRemoved = -not (Test-Path -LiteralPath $extract); passed = $true } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath 'installation.json')
    Copy-Item -LiteralPath $config -Destination (Join-Path $evidencePath 'opencode.jsonc')
    Write-Output "PASS: real bundle installation, host registrations, physical Claude cache, shortcut and owned process handover. App saved at $app"
} catch {
    @{ scratch = $scratch; app = $app; claudePlugin = $cache; passed = $false } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath 'installation.json')
    $_ | Out-String | Set-Content -LiteralPath (Join-Path $evidencePath 'failure.txt')
    throw
} finally {
    if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    $env:CLAUDE_CONFIG_DIR = $oldClaude
    if ($transcribing) { Stop-Transcript | Out-Null }
}
