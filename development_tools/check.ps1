$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    foreach ($arguments in @(
        @('run', 'lint'),
        @('run', 'test'),
        @('run', 'test:claude'),
        @('run', 'typecheck'),
        @('run', 'test:assets'),
        @('run', 'test:tools'),
        @('run', 'test:verification'),
        @('run', 'test:companion')
    )) {
        & bun @arguments
        if ($LASTEXITCODE -ne 0) { throw "bun $($arguments -join ' ') failed with exit code $LASTEXITCODE" }
    }
} finally {
    Pop-Location
}
