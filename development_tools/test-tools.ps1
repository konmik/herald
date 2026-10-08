$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$python = Join-Path $root $(if ($IsWindows) { '.venv/Scripts/python.exe' } else { '.venv/bin/python' })
if (-not (Test-Path -LiteralPath $python)) { throw 'Run pnpm run setup:checks first' }
Push-Location $root
try {
    & $python -m unittest discover -s development_tools -p 'test_*.py'
    if ($LASTEXITCODE -ne 0) { throw 'Python checks failed' }
} finally {
    Pop-Location
}
