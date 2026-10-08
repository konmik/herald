[CmdletBinding()]
param(
    [ValidateRange(1, 32)][int[]]$Threads = @(1, 2, 4, 8),
    [ValidateRange(1, 20)][int]$Runs = 3
)

$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$names = @('CARGO_TARGET_DIR', 'HERALD_TTS', 'HERALD_TTS_THREADS', 'HERALD_TTS_BENCHMARK', 'PATH')
$previous = @{}
foreach ($name in $names) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    $env:CARGO_TARGET_DIR = Join-Path $env:LOCALAPPDATA 'Temp/opencode/herald-build'
    $env:HERALD_TTS = Join-Path $repository 'native-announcer/resources/tts/kitten-nano-en-v0_8-int8'
    & node (Join-Path $PSScriptRoot 'prepare-tts.mjs')
    if ($LASTEXITCODE -ne 0) { throw 'Could not prepare Kitten assets' }
    $messages = & cargo test --locked --no-run -j 6 --manifest-path (Join-Path $repository 'native-announcer/Cargo.toml') --message-format=json
    if ($LASTEXITCODE -ne 0) { throw 'Could not build Kitten benchmark test' }
    $artifacts = @($messages | ForEach-Object { $_ | ConvertFrom-Json } | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.profile.test -and $_.executable })
    if ($artifacts.Count -ne 1) { throw 'Could not identify native test executable' }
    foreach ($library in @('sherpa-onnx-c-api.dll', 'onnxruntime.dll')) {
        Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR "debug/$library") -Destination (Join-Path (Split-Path $artifacts[0].executable -Parent) $library) -Force
    }
    $env:PATH = (Join-Path $env:CARGO_TARGET_DIR 'debug') + [IO.Path]::PathSeparator + $env:PATH
    $results = @()
    foreach ($count in $Threads) {
        for ($run = 1; $run -le $Runs; $run++) {
            $env:HERALD_TTS_THREADS = "$count"
            $env:HERALD_TTS_BENCHMARK = Join-Path $env:LOCALAPPDATA "Temp/opencode/kitten-native-$count-$run.json"
            $process = Start-Process -FilePath $artifacts[0].executable -ArgumentList @('--exact', 'tts::tests::benchmark_native_kitten', '--ignored', '--nocapture') -Wait -PassThru -NoNewWindow
            if ($process.ExitCode -ne 0) { throw "Kitten benchmark failed: $($process.ExitCode)" }
            $result = Get-Content -LiteralPath $env:HERALD_TTS_BENCHMARK -Raw | ConvertFrom-Json
            $results += [pscustomobject]@{ Threads = $count; Run = $run; LoadingMs = [math]::Round($result.loadingMs); FirstGenerationMs = [math]::Round($result.firstGenerationMs); WarmGenerationMs = [math]::Round($result.warmGenerationMs) }
        }
    }
    $results | Format-Table
    Write-Output 'Each run starts a new native process. Loading excludes synthesis. Windows disk cache is not cleared.'
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
}
