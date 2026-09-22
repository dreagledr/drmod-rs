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
$dll = "target/i686-pc-windows-msvc/release/drmod_rs_lib.dll"

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

# The same mod in its ASI form: the DLL the launcher embeds, renamed and nested
# under plugins/ next to a loader, so the game loads it itself instead of being
# injected into. Nothing here is UPX-packed - the .asi is the payload the
# launcher extracts byte for byte, and d3d9.dll is a third party's binary.
if (-not (Test-Path $dll)) { throw "Missing $dll - the library did not build" }
$loader = "vendor/asi-loader/d3d9.dll"
if (-not (Test-Path $loader)) {
    throw "Missing $loader (Ultimate-ASI-Loader, Win32) - see vendor/asi-loader/README.md"
}

$asiStage = "$outDir/asi"
if (Test-Path $asiStage) { Remove-Item $asiStage -Recurse -Force }
$null = New-Item -ItemType Directory -Force "$asiStage/plugins"
Copy-Item $dll "$asiStage/plugins/drmod_rs_lib.asi" -Force
Copy-Item $loader $asiStage -Force
Copy-Item "docs/asi-readme.txt" "$asiStage/readme.txt" -Force

$asiZipPath = "$outDir/drmod-asi.zip"
if (Test-Path $asiZipPath) { Remove-Item $asiZipPath -Force }
Compress-Archive -Path "$asiStage/*" -DestinationPath $asiZipPath

$asiSize = (Get-Item $asiZipPath).Length / 1KB
$dllSize = [math]::Round((Get-Item $dll).Length / 1KB, 0)
Write-Host "==> ASI zip: $asiZipPath ($([math]::Round($asiSize, 0)) KB; payload $dllSize KB)" -ForegroundColor Green
