$ErrorActionPreference = "Stop"
Set-Location -LiteralPath $PSScriptRoot
if (-not (Test-Path "target\release\gdd-harness.exe")) {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "Rust build failed" }
}
$env:GDD_NO_BROWSER = ""
& ".\target\release\gdd-harness.exe"
