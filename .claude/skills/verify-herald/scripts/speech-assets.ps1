param(
    [Parameter(Mandatory)][string]$AppDirectory,
    [Parameter(Mandatory)][string]$TestExecutable,
    [string]$ClaudePluginDirectory,
    [ValidateSet('OpenCode', 'Claude')][string]$Runtime = 'OpenCode',
    [string]$Evidence = ('temp/verification/speech-assets-' + [guid]::NewGuid())
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../../..'))
$app = [IO.Path]::GetFullPath($AppDirectory)
$native = Join-Path $(if ($Runtime -eq 'Claude') { Join-Path $app 'claude-plugin' } else { $app }) 'native-announcer'
if ($Runtime -eq 'Claude' -and $ClaudePluginDirectory) { $native = Join-Path ([IO.Path]::GetFullPath($ClaudePluginDirectory)) 'native-announcer' }
$evidencePath = [IO.Path]::GetFullPath($Evidence, $root)
$scratchRoot = Join-Path $env:LOCALAPPDATA ('Temp/opencode/herald-speech-proof-Ω-' + [guid]::NewGuid())
$scratch = Join-Path $scratchRoot ('versions/' + (Split-Path $app -Leaf) + '/native-announcer')
$process = $null
if (-not (Test-Path -LiteralPath $TestExecutable -PathType Leaf)) { throw 'Specify the Rust test executable produced by cargo test' }
if (Test-Path -LiteralPath $evidencePath) { throw 'Use a new evidence directory' }
New-Item -ItemType Directory -Path $evidencePath | Out-Null
try {
    New-Item -ItemType Directory -Path "$scratch/bin", "$scratch/resources" -Force | Out-Null
    Copy-Item -LiteralPath $TestExecutable -Destination "$scratch/bin/speech-tests.exe"
    $libraries = @('sherpa-onnx-c-api.dll', 'onnxruntime.dll')
    foreach ($library in $libraries) {
        Copy-Item -LiteralPath (Join-Path $native "bin/$library") -Destination "$scratch/bin/$library"
        if ((Get-FileHash -LiteralPath (Join-Path $native "bin/$library")).Hash -ne (Get-FileHash -LiteralPath "$scratch/bin/$library").Hash) { throw "Installed library copy differs for $library" }
    }
    Copy-Item -LiteralPath (Join-Path $native 'resources/tts') -Destination "$scratch/resources/tts" -Recurse
    $info = [Diagnostics.ProcessStartInfo]::new("$scratch/bin/speech-tests.exe")
    $info.UseShellExecute = $false
    $info.WorkingDirectory = $scratch
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.Environment.Remove('HERALD_TTS') | Out-Null
    $info.Environment['PATH'] = "$scratch/bin;$env:SystemRoot/system32"
    foreach ($argument in @('--exact', 'tts::tests::native_synthesis_streams_each_sentence_once_and_can_stop_early', '--ignored', '--nocapture')) { $info.ArgumentList.Add($argument) }
    $process = [Diagnostics.Process]::Start($info)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(120000)) { throw 'Silent CPU synthesis verification exceeded its deadline' }
    $output = $stdout.GetAwaiter().GetResult()
    $errors = $stderr.GetAwaiter().GetResult()
    $output | Set-Content -LiteralPath (Join-Path $evidencePath 'stdout.txt')
    $errors | Set-Content -LiteralPath (Join-Path $evidencePath 'stderr.txt')
    if ($process.ExitCode -ne 0 -or $output -notmatch 'test result: ok\. 1 passed') { throw 'Installed speech model and libraries did not pass silent inference' }
    @{ app = $app; runtime = $Runtime; scope = 'Rust synthesis harness with copied installed DLLs and executable-relative installed model, not the installed app executable'; passed = $true; testExecutable = [IO.Path]::GetFullPath($TestExecutable) } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath 'proof.json')
    Write-Output "PASS: silent CPU inference with installed $Runtime speech assets and libraries"
} catch {
    $_ | Out-String | Set-Content -LiteralPath (Join-Path $evidencePath 'failure.txt')
    throw
} finally {
    if ($process -and -not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
    if (Test-Path -LiteralPath $scratchRoot) { Remove-Item -LiteralPath $scratchRoot -Recurse -Force }
    @{ scratchRemoved = -not (Test-Path -LiteralPath $scratchRoot); processExited = (-not $process -or $process.HasExited) } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath 'cleanup.json')
}
