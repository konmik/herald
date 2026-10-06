$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    & bun install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) { throw 'Could not install locked JavaScript dependencies' }
    if (-not (Test-Path '.venv/Scripts/python.exe')) {
        & python -m venv .venv
        if ($LASTEXITCODE -ne 0) { throw 'Could not create the project Python environment' }
    }
    & ./.venv/Scripts/python.exe -m pip install -r development_tools/requirements-checks.txt
    if ($LASTEXITCODE -ne 0) { throw 'Could not install Python check dependencies' }
} finally {
    Pop-Location
}
