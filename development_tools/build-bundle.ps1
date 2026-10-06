[CmdletBinding()]
param(
    [string]$OutputDirectory,
    [string]$PayloadDirectory
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot/bundle/install.ps1"
if (-not $IsWindows -or $PSVersionTable.PSVersion.Major -lt 7) { throw 'PowerShell 7 on Windows is required' }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'temp/bundles' }
Assert-NoLinks $OutputDirectory
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$stage = Join-Path $OutputDirectory ('.civilized-stage-' + [guid]::NewGuid())
$target = Join-Path $root 'native-announcer/target/bundle'
$archive = Join-Path $OutputDirectory ('.civilized-archive-' + [guid]::NewGuid() + '.zip')
try {
    New-Item -ItemType Directory -Path $stage | Out-Null
    if ($PayloadDirectory) {
        foreach ($directory in @('native-announcer', 'claude-plugin')) {
            Get-PayloadFiles (Join-Path $PayloadDirectory $directory) | Out-Null
            Copy-Item -LiteralPath (Join-Path $PayloadDirectory $directory) -Destination (Join-Path $stage $directory) -Recurse
        }
    } else {
        $arch = & node -p 'process.arch'
        if ($LASTEXITCODE -ne 0 -or $arch -notin @('x64', 'arm64')) { throw 'Unsupported Node architecture' }
        $osArch = if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { 'arm64' } else { 'x64' }
        if ($arch -ne $osArch) { throw 'Build with Node matching the Windows architecture' }
        $rustTarget = @{ x64 = 'x86_64-pc-windows-msvc'; arm64 = 'aarch64-pc-windows-msvc' }[$arch]
        Invoke-Checked 'node' @((Join-Path $PSScriptRoot 'prepare-tts.mjs'))
        $oldTarget = $env:CARGO_TARGET_DIR
        try {
            $env:CARGO_TARGET_DIR = $target
            Invoke-Checked 'cargo' @('build', '--release', '--locked', '--target', $rustTarget, '-j', '6', '--manifest-path', (Join-Path $root 'native-announcer/Cargo.toml'))
        } finally { $env:CARGO_TARGET_DIR = $oldTarget }
        foreach ($name in @('.claude-plugin/plugin.json', '.claude-plugin/marketplace.json', 'hooks/hooks.json', 'hooks/register.ts', 'scripts/bridge.mjs', 'scripts/runtime.mjs', 'scripts/session-title.mjs')) {
            $destination = Join-Path $stage "claude-plugin/$name"
            New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
            Copy-Item -LiteralPath (Join-Path $root "claude-plugin/$name") -Destination $destination -Recurse -Force
        }
        $runtime = Join-Path $stage 'native-announcer'
        New-Item -ItemType Directory -Path "$runtime/bin", "$runtime/resources" -Force | Out-Null
        Copy-Item -LiteralPath "$target/$rustTarget/release/civilized-announcer.exe" -Destination "$runtime/bin/civilized-announcer-win32-$arch.exe"
        foreach ($name in @('onnxruntime.dll', 'sherpa-onnx-c-api.dll')) { Copy-Item -LiteralPath "$target/$rustTarget/release/$name" -Destination "$runtime/bin/$name" }
        $licenses = Join-Path $runtime 'licenses'
        New-Item -ItemType Directory -Path $licenses | Out-Null
        $onnxVersion = (Get-Item "$runtime/bin/onnxruntime.dll").VersionInfo.FileVersion
        if ($onnxVersion -notmatch '^\d+\.\d+\.\d+$') { throw 'Could not identify the ONNX Runtime version for its license' }
        $sherpaVersion = Select-String -LiteralPath (Join-Path $root 'native-announcer/Cargo.lock') -Pattern '^name = "sherpa-onnx-sys"$' -Context 0, 1
        if ($sherpaVersion.Context.PostContext[0] -notmatch '^version = "([0-9.]+)"$') { throw 'Could not identify the sherpa-onnx version for its license' }
        $sherpaVersion = $Matches[1]
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v$sherpaVersion/LICENSE" -OutFile (Join-Path $licenses 'sherpa-onnx-LICENSE') -TimeoutSec 60
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/microsoft/onnxruntime/v$onnxVersion/LICENSE" -OutFile (Join-Path $licenses 'onnxruntime-LICENSE') -TimeoutSec 60
        foreach ($name in @('characters.json', 'videos', 'tts/kitten-nano-en-v0_8-int8')) {
            $destination = Join-Path $runtime "resources/$name"
            New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
            Copy-Item -LiteralPath (Join-Path $root "native-announcer/resources/$name") -Destination $destination -Recurse -Force
        }
        Copy-Item -LiteralPath $runtime -Destination (Join-Path $stage 'claude-plugin/native-announcer') -Recurse -Force
        $node = & node -p 'process.execPath'
        if ($LASTEXITCODE -ne 0) { throw 'Could not locate Node runtime' }
        Copy-Item -LiteralPath $node -Destination (Join-Path $stage 'claude-plugin/native-announcer/bin/node.exe')
        $nodeVersion = & node -p 'process.version'
        if ($LASTEXITCODE -ne 0 -or $nodeVersion -notmatch '^v\d+\.\d+\.\d+$') { throw 'Could not identify the bundled Node version for its license' }
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/nodejs/node/$nodeVersion/LICENSE" -OutFile (Join-Path $stage 'claude-plugin/native-announcer/licenses/node-LICENSE') -TimeoutSec 60
    }
    $package = Get-Content (Join-Path $root 'package.json') -Raw | ConvertFrom-Json
    $exports = @{}
    foreach ($entry in $package.exports.PSObject.Properties) { $exports[$entry.Name] = $entry.Value -replace '\.ts$', '.js' }
    @{ name = $package.name; version = $package.version; type = 'module'; private = $true; exports = $exports } | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $stage 'package.json') -Encoding utf8NoBOM
    $output = Join-Path $stage 'opencode-plugin'
    Invoke-Checked 'bun' @('build', (Join-Path $root 'opencode-plugin/index.ts'), '--target', 'bun', '--format', 'esm', '--minify', '--outdir', $output)
    Invoke-Checked 'bun' @('build', (Join-Path $root 'opencode-plugin/tui.ts'), '--target', 'bun', '--format', 'esm', '--minify', '--external', '@opencode/plugin/tui', '--external', 'solid-js', '--outdir', $output)
    foreach ($entry in @(@{ name = 'index'; export = '.' }, @{ name = 'tui'; export = './tui' })) {
        ('export { default } from "' + $exports[$entry.export] + '"') | Set-Content (Join-Path $stage "$($entry.name).ts") -Encoding utf8NoBOM
    }
    $register = Join-Path $stage 'claude-plugin/hooks/register.ts'
    $text = Get-Content -LiteralPath $register -Raw
    if (-not $text.Contains("['node',") -and -not $text.Contains("[$.plugin.root + '/native-announcer/bin/node.exe',")) { throw 'Claude hook does not contain a supported bridge launcher' }
    [IO.File]::WriteAllText($register, $text.Replace("['node',", "[$.plugin.root + '/native-announcer/bin/node.exe',"))
    $runtimeScript = Join-Path $stage 'claude-plugin/scripts/runtime.mjs'
    $runtimeText = Get-Content -LiteralPath $runtimeScript -Raw
    [IO.File]::WriteAllText($runtimeScript, $runtimeText.Replace('Civilized Agent native announcer is missing. Run npm run build:announcer on this platform.', 'Civilized Agent runtime is missing. Reinstall the application bundle.'))
    '{"schemaVersion":1}' | Set-Content (Join-Path $stage 'claude-plugin/.claude-plugin/packaged.json') -Encoding utf8NoBOM
    foreach ($name in @('install.ps1', 'shortcut.ps1', 'register-opencode.mjs')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot "bundle/$name") -Destination (Join-Path $stage $name) -Force }
    $package = Get-Content (Join-Path $stage 'package.json') -Raw | ConvertFrom-Json
    $arch = switch ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) { 'X64' { 'x64' } 'Arm64' { 'arm64' } default { throw 'Unsupported architecture' } }
    $files = @(Get-PayloadFiles $stage | Sort-Object FullName | ForEach-Object { @{ path = [IO.Path]::GetRelativePath($stage, $_.FullName).Replace('\', '/'); sha256 = Get-PayloadHash $_.FullName; size = $_.Length } })
    @{ schemaVersion = 1; name = 'civilized-agent'; version = $package.version; platform = 'win32'; arch = $arch; files = $files } | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $stage 'bundle-manifest.json') -Encoding utf8NoBOM
    Test-Bundle $stage | Out-Null
    $hash = (Get-FileHash (Join-Path $stage 'bundle-manifest.json')).Hash.ToLowerInvariant()
    $zip = Join-Path $OutputDirectory "civilized-agent-$($package.version)-win32-$arch-$hash.zip"
    if (Test-Path -LiteralPath $zip) { throw "Bundle output already exists: $zip" }
    [IO.Compression.ZipFile]::CreateFromDirectory($stage, $archive)
    [IO.File]::Move($archive, $zip)
    Write-Output $zip
} finally {
    Remove-DeploymentDirectory $stage
    if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive -Force }
}
