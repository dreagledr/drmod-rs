# build.ps1 — Build drmod-rs, UPX-compress the exe, and package into /out zip
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

Write-Host "==> Packaging to out/ ..." -ForegroundColor Cyan
$outDir = "$projectRoot/out"
$null = New-Item -ItemType Directory -Force $outDir
Copy-Item $exe $outDir -Force

$zipPath = "$outDir/drmod-rs.zip"
if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
Compress-Archive -Path "$outDir/drmod.exe" -DestinationPath $zipPath
$zipSize = (Get-Item $zipPath).Length / 1KB

$exeSize = (Get-Item $exe).Length / 1KB
Write-Host "==> Done: $exe ($([math]::Round($exeSize, 0)) KB)" -ForegroundColor Green
Write-Host "==> Zip:   $zipPath ($([math]::Round($zipSize, 0)) KB)" -ForegroundColor Green
