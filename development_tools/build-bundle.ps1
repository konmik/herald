[CmdletBinding()]
param(
    [string]$OutputDirectory,
    [string]$PayloadDirectory
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
. "$PSScriptRoot/bundle/install.ps1"
. "$PSScriptRoot/bundle-licenses.ps1"
if (-not $IsWindows -or $PSVersionTable.PSVersion.Major -lt 7) { throw 'PowerShell 7 on Windows is required' }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'temp/bundles' }
Assert-NoLinks $OutputDirectory
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$stage = Join-Path $OutputDirectory ('.herald-stage-' + [guid]::NewGuid())
$target = Join-Path $root 'native-announcer/target/bundle'
$archive = Join-Path $OutputDirectory ('.herald-archive-' + [guid]::NewGuid() + '.zip')
$metafile = Join-Path $OutputDirectory ('.herald-modules-' + [guid]::NewGuid() + '.json')
try {
    New-Item -ItemType Directory -Path $stage | Out-Null
    if ($PayloadDirectory) {
        foreach ($directory in @('native-announcer', 'claude-plugin', 'codex-plugin')) {
            Get-PayloadFiles (Join-Path $PayloadDirectory $directory) | Out-Null
            Copy-Item -LiteralPath (Join-Path $PayloadDirectory $directory) -Destination (Join-Path $stage $directory) -Recurse
        }
    } else {
        $osArch = if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { 'arm64' } else { 'x64' }
        $arch = $osArch
        $rustTarget = @{ x64 = 'x86_64-pc-windows-msvc'; arm64 = 'aarch64-pc-windows-msvc' }[$arch]
        $oldTarget = $env:CARGO_TARGET_DIR
        try {
            $env:CARGO_TARGET_DIR = $target
            Invoke-Checked 'cargo' @('build', '--release', '--locked', '--target', $rustTarget, '-j', '6', '--manifest-path', (Join-Path $root 'native-announcer/Cargo.toml'))
        } finally { $env:CARGO_TARGET_DIR = $oldTarget }
        foreach ($name in @('.claude-plugin/plugin.json', '.claude-plugin/marketplace.json', 'hooks/hooks.json', 'hooks/register.ts')) {
            $destination = Join-Path $stage "claude-plugin/$name"
            New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
            Copy-Item -LiteralPath (Join-Path $root "claude-plugin/$name") -Destination $destination -Recurse -Force
        }
        foreach ($name in @('.codex-plugin/plugin.json', '.agents/plugins/marketplace.json', 'hooks/hooks.json', 'hooks/herald.mjs')) {
            $destination = Join-Path $stage "codex-plugin/$name"
            New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
            Copy-Item -LiteralPath (Join-Path $root "codex-plugin/$name") -Destination $destination -Force
        }
        $runtime = Join-Path $stage 'native-announcer'
        New-Item -ItemType Directory -Path "$runtime/bin", "$runtime/resources" -Force | Out-Null
        Copy-Item -LiteralPath "$target/$rustTarget/release/herald.exe" -Destination "$runtime/bin/herald-win32-$arch.exe"
        foreach ($name in @('characters.json', 'videos')) {
            $destination = Join-Path $runtime "resources/$name"
            New-Item -ItemType Directory -Path (Split-Path $destination -Parent) -Force | Out-Null
            Copy-Item -LiteralPath (Join-Path $root "native-announcer/resources/$name") -Destination $destination -Recurse -Force
        }
        foreach ($plugin in @('claude-plugin', 'codex-plugin')) { Copy-Item -LiteralPath $runtime -Destination (Join-Path $stage "$plugin/native-announcer") -Recurse -Force }
    }
    $package = Get-Content (Join-Path $root 'package.json') -Raw | ConvertFrom-Json
    $exports = @{}
    foreach ($entry in $package.exports.PSObject.Properties) { $exports[$entry.Name] = $entry.Value -replace '\.ts$', '.js' }
    @{ name = $package.name; version = $package.version; type = 'module'; private = $true; exports = $exports } | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $stage 'package.json') -Encoding utf8NoBOM
    $output = Join-Path $stage 'opencode-plugin'
    $licenseDirectory = Join-Path $stage 'licenses/javascript'
    Invoke-Checked 'bun' @('build', (Join-Path $root 'opencode-plugin/index.ts'), '--target', 'bun', '--format', 'esm', '--minify', '--outdir', $output, "--metafile=$metafile") | Out-Host
    Copy-BundledLicenses $metafile $root $licenseDirectory
    Invoke-Checked 'bun' @('build', (Join-Path $root 'opencode-plugin/tui.ts'), '--target', 'bun', '--format', 'esm', '--minify', '--outdir', $output, "--metafile=$metafile") | Out-Host
    Copy-BundledLicenses $metafile $root $licenseDirectory
    foreach ($entry in @(@{ name = 'index'; export = '.' }, @{ name = 'tui'; export = './tui' })) {
        ('export { default } from "' + $exports[$entry.export] + '"') | Set-Content (Join-Path $stage "$($entry.name).ts") -Encoding utf8NoBOM
    }
    '{"schemaVersion":1}' | Set-Content (Join-Path $stage 'claude-plugin/.claude-plugin/packaged.json') -Encoding utf8NoBOM
    foreach ($name in @('install.ps1', 'shortcut.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot "bundle/$name") -Destination (Join-Path $stage $name) -Force }
    Invoke-Checked 'bun' @('build', (Join-Path $PSScriptRoot 'bundle/register-opencode.mjs'), '--target', 'node', '--format', 'esm', '--minify', "--outfile=$(Join-Path $stage 'register-opencode.mjs')", "--metafile=$metafile") | Out-Host
    Copy-BundledLicenses $metafile $root $licenseDirectory
    $package = Get-Content (Join-Path $stage 'package.json') -Raw | ConvertFrom-Json
    $arch = switch ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) { 'X64' { 'x64' } 'Arm64' { 'arm64' } default { throw 'Unsupported architecture' } }
    $files = @(Get-PayloadFiles $stage | Sort-Object FullName | ForEach-Object { @{ path = [IO.Path]::GetRelativePath($stage, $_.FullName).Replace('\', '/'); sha256 = Get-PayloadHash $_.FullName; size = $_.Length } })
    @{ schemaVersion = 1; name = 'herald'; version = $package.version; platform = 'win32'; arch = $arch; files = $files } | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $stage 'bundle-manifest.json') -Encoding utf8NoBOM
    Test-Bundle $stage | Out-Null
    $hash = (Get-FileHash (Join-Path $stage 'bundle-manifest.json')).Hash.ToLowerInvariant()
    $zip = Join-Path $OutputDirectory "herald-$($package.version)-win32-$arch-$hash.zip"
    if (Test-Path -LiteralPath $zip) { throw "Bundle output already exists: $zip" }
    [IO.Compression.ZipFile]::CreateFromDirectory($stage, $archive)
    [IO.File]::Move($archive, $zip)
    Write-Output $zip
} finally {
    Remove-DeploymentDirectory $stage
    if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive -Force }
    if (Test-Path -LiteralPath $metafile) { Remove-Item -LiteralPath $metafile -Force }
}
