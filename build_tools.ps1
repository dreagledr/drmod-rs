# build_tools.ps1 — Build the tools (dbdump, dump-replay-input), UPX-compress, and copy into /out
param(
    [switch]$Release = $true
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $projectRoot

Write-Host "==> Building dbdump (x64)..." -ForegroundColor Cyan
Push-Location "$projectRoot/tools/dbdump"
cargo build --release
$code = $LASTEXITCODE
Pop-Location
if ($code -ne 0) { throw "dbdump build failed" }

Write-Host "==> Building dump-replay-input (disasm)..." -ForegroundColor Cyan
Push-Location "$projectRoot/tools/disasm"
cargo build --release
$code = $LASTEXITCODE
Pop-Location
if ($code -ne 0) { throw "dump-replay-input build failed" }

$outDir = "$projectRoot/out"
$null = New-Item -ItemType Directory -Force $outDir

$exes = @(
    "$projectRoot/target/x86_64-pc-windows-msvc/release/dbdump.exe",
    "$projectRoot/tools/disasm/target/i686-pc-windows-msvc/release/dump-replay-input.exe"
)

foreach ($exe in $exes) {
    Write-Host "==> UPX compression: $exe ..." -ForegroundColor Cyan
    upx --best --lzma $exe
    if ($LASTEXITCODE -ne 0) { throw "UPX failed: $exe" }
    Copy-Item $exe $outDir -Force
    $size = (Get-Item $exe).Length / 1KB
    Write-Host "==> Done: $exe ($([math]::Round($size, 0)) KB)" -ForegroundColor Green
}

Write-Host "==> Tools in: $outDir" -ForegroundColor Green