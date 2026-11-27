# The checks CI runs on every push (GitHub Actions and Bitbucket Pipelines both call this file),
# and that anyone can run on their own PC before pushing:
#
#   powershell -ExecutionPolicy Bypass -File tools\ci.ps1
#
# Windows (Windows PowerShell 5.1 or PowerShell 7) and macOS (PowerShell 7, `pwsh tools/ci.ps1`, D53).
# It never touches the real data folder: the tests make their own temporary folders.
# Keep this file ASCII: Windows PowerShell 5.1 reads files without a BOM as the ANSI code page.

$ErrorActionPreference = 'Stop'
Set-Location (Split-Path -Parent $PSScriptRoot)

# Runs a program and stops CI when it fails (PowerShell itself does not stop on a failing exe).
function Step([string]$Title, [scriptblock]$Command) {
    Write-Host ""
    Write-Host "==> $Title" -ForegroundColor Cyan
    $started = Get-Date
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "FAILED: $Title (exit code $LASTEXITCODE)"
    }
    Write-Host ("    ok ({0:n0} s)" -f ((Get-Date) - $started).TotalSeconds) -ForegroundColor Green
}

Step 'Rust formatting' { cargo fmt --all --check }

# Preserve (docs/preserve.md): will an installed copy, with real books, still work after this change?
# Migrations, data formats, app identity and versions. Fast, so a blocked change fails first.
$py = if (Get-Command python -ErrorAction SilentlyContinue) { 'python' } else { 'python3' }
Step 'Preserve: tests of the pre-flight' { & $py -I -X utf8 tools/test_desktop_preflight.py }
Step 'Preserve: migrations, data formats, app identity, versions' { & $py -I -X utf8 tools/desktop_preflight.py --offline }

# Known vulnerabilities in Rust and npm dependencies (tools/audit.ps1; Bitbucket also runs it weekly).
Write-Host ""
Write-Host "==> Security audit" -ForegroundColor Cyan
& "$PSScriptRoot/audit.ps1"

# No dependency needs its install script (as in the release workflow), so none are run.
Push-Location frontend
try {
    Step 'Frontend packages' { npm ci --ignore-scripts --no-audit --no-fund }
    Step 'Frontend type check and build' { npm run build }
} finally {
    Pop-Location
}

# '--' is quoted so PowerShell hands it to cargo instead of keeping it for itself.
Step 'Clippy' { cargo clippy --workspace --all-targets --all-features --locked '--' -D warnings }
Step 'Tests' { cargo test --workspace --locked }

Write-Host ""
Write-Host "All checks passed." -ForegroundColor Green
