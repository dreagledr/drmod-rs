# build.ps1 - release build of the mod + UPX compression of the launcher.
#
# Only the launcher is packed: the DLL is the payload that the launcher embeds
# (include_bytes!) at compile time and extracts to
# %LOCALAPPDATA%\cutscene_skip\ at runtime. Packing the DLL in target/ before the
# exe is relinked would give double packing with no gain and an extra unpacker
# inside the game. UPX is always the last step: the next `cargo build` overwrites
# the exe uncompressed (cargo does not know about packing).
#
# Keep this file ASCII-only: Windows PowerShell 5.1 reads .ps1 as ANSI (cp1251)
# unless the file has a UTF-8 BOM, so non-ASCII text breaks the parser.
#
#   pwsh mods/cutscene_skip/build.ps1
#   pwsh mods/cutscene_skip/build.ps1 -NoUpx
param(
    [switch]$NoUpx
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $MyInvocation.MyCommand.Path)

$exe = "target/i686-pc-windows-msvc/release/cutscene_skip.exe"
$dll = "target/i686-pc-windows-msvc/release/cutscene_skip_lib.dll"

# Remove the exe before building: cargo relinks it and embeds the CURRENT DLL,
# and UPX will not fail with "already packed" on a second script run.
if (Test-Path $exe) { Remove-Item $exe -Force }

Write-Host "==> Building cutscene_skip (release)..." -ForegroundColor Cyan
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Build failed" }

if ($NoUpx) {
    Write-Host "==> UPX skipped (-NoUpx)" -ForegroundColor Yellow
} else {
    Write-Host "==> UPX compression (launcher only)..." -ForegroundColor Cyan
    upx --best --lzma $exe
    if ($LASTEXITCODE -ne 0) { throw "UPX failed" }
}

$exeSize = [math]::Round((Get-Item $exe).Length / 1KB, 0)
$dllSize = [math]::Round((Get-Item $dll).Length / 1KB, 0)
Write-Host "==> Done: $exe ($exeSize KB; embedded DLL payload $dllSize KB)" -ForegroundColor Green
