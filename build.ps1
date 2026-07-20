# build.ps1 — Build drmod-rs and UPX-compress the exe
param(
    [switch]$Release = $true
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $projectRoot

Write-Host "==> Building drmod-rs (release)..." -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Build failed" }

$exe = "target/i686-pc-windows-msvc/release/drmod.exe"

Write-Host "==> UPX compression..." -ForegroundColor Cyan
upx --best --lzma $exe
if ($LASTEXITCODE -ne 0) { throw "UPX failed" }

$size = (Get-Item $exe).Length / 1KB
Write-Host "==> Done: $exe ($([math]::Round($size, 0)) KB)" -ForegroundColor Green
