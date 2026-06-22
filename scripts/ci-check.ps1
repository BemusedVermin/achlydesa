#!/usr/bin/env pwsh
# Quality gate — the single source of truth shared by local runs and CI.
#
# `.github/workflows/ci.yml` runs THIS script (via `pwsh`) so what CI gates is exactly
# what you can check locally before pushing:
#
#     pwsh scripts/ci-check.ps1            # cross-platform (PowerShell 7+)
#     powershell -File scripts/ci-check.ps1   # Windows PowerShell 5.1
#
# Gates, in order: rustfmt --check, clippy (-D warnings), tests, doctests.
#
# Scope mirrors CI: the Bevy-free sim crates only. `app` (the full Bevy front-end) and
# `voice` (the heavy candle SLM) are excluded — they need extra system deps / build time
# and are not gated here. `cargo fmt --check` still covers the whole workspace because it
# parses rather than compiles.

$ErrorActionPreference = 'Continue'

# app = the Bevy engine front-end; voice = the candle-backed on-device SLM.
$excludes = @('--exclude', 'app', '--exclude', 'voice')

$failures = @()

function Invoke-Gate {
    param(
        [string]$Name,
        [string[]]$CargoArgs
    )
    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan
    Write-Host "    cargo $($CargoArgs -join ' ')" -ForegroundColor DarkGray
    & cargo @CargoArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $Name (exit $LASTEXITCODE)" -ForegroundColor Red
        $script:failures += $Name
    }
    else {
        Write-Host "ok: $Name" -ForegroundColor Green
    }
}

Invoke-Gate 'rustfmt (--check)' @('fmt', '--all', '--', '--check')
Invoke-Gate 'clippy (-D warnings)' (@('clippy', '--workspace') + $excludes + @('--all-targets', '--', '-D', 'warnings'))
Invoke-Gate 'tests' (@('test', '--workspace') + $excludes + @('--all-targets', '--locked'))
Invoke-Gate 'doctests' (@('test', '--workspace') + $excludes + @('--doc', '--locked'))
# Structural coupling ratchet (data/logic-intertwining lint). The `tests` gate already runs its
# #[test]; this prints the readable report and fails on any coupling added beyond the baseline.
Invoke-Gate 'coupling ratchet' @('run', '--quiet', '-p', 'coupling-lint')

Write-Host ""
if ($failures.Count -gt 0) {
    Write-Host "Quality gate FAILED: $($failures -join ', ')" -ForegroundColor Red
    exit 1
}
Write-Host "Quality gate PASSED - all checks green." -ForegroundColor Green
exit 0
