# Security audit of the dependencies (called by tools/ci.ps1; Bitbucket also runs it weekly by itself,
# pipeline "audit"). New advisories appear without any push, so the weekly run catches them.
#
#   powershell -ExecutionPolicy Bypass -File tools\audit.ps1
#
# - Rust: cargo audit (RustSec) on Cargo.lock. A known vulnerability fails the run. Advisories that do
#   not apply are listed with a reason in .cargo/audit.toml. Warnings (unmaintained, yanked) are printed only.
# - npm, packages the app ships (frontend/ and the root): high or critical advisories fail the run.
# - npm, all packages including build tools: printed as a warning only (build tools never reach a user).
# It reads package-lock.json and Cargo.lock, so nothing needs installing first (cargo-audit installs itself once).
# Keep this file ASCII: Windows PowerShell 5.1 reads files without a BOM as the ANSI code page.

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

function Check([string]$Title, [scriptblock]$Command) {
    Write-Host ""
    Write-Host "==> $Title" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "FAILED: $Title (exit code $LASTEXITCODE)"
    }
    Write-Host "    ok" -ForegroundColor Green
}

# cargo-audit is installed once per machine (a few minutes), then reused.
$installed = (cargo install --list) -match '^cargo-audit v'
if (-not $installed) {
    Check 'Install cargo-audit (once)' { cargo install cargo-audit --locked }
}

Check 'Rust advisories (cargo audit)' { cargo audit }

foreach ($dir in @('frontend', '.')) {
    Push-Location $dir
    try {
        Check "npm advisories in the shipped packages ($dir)" { npm audit --omit=dev --audit-level=high }
        Write-Host ""
        Write-Host "==> npm advisories in all packages incl. build tools ($dir): warning only" -ForegroundColor Cyan
        npm audit --audit-level=high
        if ($LASTEXITCODE -ne 0) {
            Write-Host "    WARNING: build-tool advisories above (they never reach a user); fix them when convenient." -ForegroundColor Yellow
        }
        $global:LASTEXITCODE = 0
    } finally {
        Pop-Location
    }
}

Write-Host ""
Write-Host "Audit passed." -ForegroundColor Green
