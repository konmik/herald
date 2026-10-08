$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    foreach ($arguments in @(
        @('run', 'lint'),
        @('run', 'test'),
        @('run', 'test:claude'),
        @('run', 'typecheck'),
        @('run', 'test:assets'),
        @('run', 'test:bundle'),
        @('run', 'test:tools'),
        @('run', 'test:verification'),
        @('run', 'test:companion')
    )) {
        if (-not $IsWindows -and $arguments[1] -eq 'test:bundle') {
            Write-Output 'Windows bundle tests require Windows. Running portable registration tests on this platform.'
            & pnpm exec bun test development_tools/tests/registration.test.mjs
            if ($LASTEXITCODE -ne 0) { throw "Registration tests failed with exit code $LASTEXITCODE" }
            continue
        }
        & pnpm @arguments
        if ($LASTEXITCODE -ne 0) { throw "pnpm $($arguments -join ' ') failed with exit code $LASTEXITCODE" }
    }
} finally {
    Pop-Location
}
