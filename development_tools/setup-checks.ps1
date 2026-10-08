$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    & pnpm install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw 'Could not install locked JavaScript dependencies' }
    '' | & pnpm exec claude --plugin-dir ./claude-plugin --print --input-format stream-json --output-format stream-json --verbose --no-session-persistence
    if ($LASTEXITCODE -ne 0) { throw 'Could not generate Claude plugin types' }
    $python = if ($IsWindows) { './.venv/Scripts/python.exe' } else { './.venv/bin/python' }
    if (-not (Test-Path $python)) {
        & python -m venv .venv
        if ($LASTEXITCODE -ne 0) { throw 'Could not create the project Python environment' }
    }
    & $python -m pip install -r development_tools/requirements-checks.txt
    if ($LASTEXITCODE -ne 0) { throw 'Could not install Python check dependencies' }
} finally {
    Pop-Location
}
