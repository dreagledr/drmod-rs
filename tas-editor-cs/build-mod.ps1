<#
.SYNOPSIS
    Build the mod payload the editor embeds, and place it where the csproj picks it up.

.DESCRIPTION
    The mod is installed by the editor, not built by it: `TasEditorCs.exe` carries two binaries as
    embedded resources — the mod itself as an `.asi` and the ASI loader that the game needs in order
    to load it. Neither is committed; both are produced here, from the Rust tree that owns them.

    Running this is what turns a plain `dotnet build` of the editor into one that can install
    anything. Without it the csproj fails loudly on the missing payload rather than publishing an
    editor whose Install button has nothing to install.

    The sources, in one line each:
      - `drmod_rs_lib.dll` — `cargo build --release` at the repository root;
      - `d3d9.dll` — the vendored Ultimate-ASI-Loader, copied from `vendor/asi-loader/`.

.EXAMPLE
    pwsh -File build-mod.ps1

.EXAMPLE
    pwsh -File build-mod.ps1 -SkipCargo

    Stage the loader only, for a csproj change that needs no new mod build (a Rust build is ~a
    minute; the copy is instant).
#>
#Requires -Version 7.0
[CmdletBinding()]
param(
    # The repository root, where the Rust mod lives. Defaults to this script's parent.
    [string]$RepoRoot,

    # Where the csproj expects the payload.
    [string]$PayloadDir,

    # Reuse whatever the last `cargo build --release` left in target/, instead of building again.
    [switch]$SkipCargo
)

$ErrorActionPreference = 'Stop'

$scriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }
if (-not $RepoRoot) { $RepoRoot = Split-Path -Parent $scriptRoot }
if (-not $PayloadDir) { $PayloadDir = Join-Path $scriptRoot 'TasEditorCs\Mod' }

$dll = Join-Path $RepoRoot 'target\i686-pc-windows-msvc\release\drmod_rs_lib.dll'
$loader = Join-Path $RepoRoot 'vendor\asi-loader\d3d9.dll'

if (-not $SkipCargo) {
    Write-Host '==> cargo build --release (drmod_rs_lib)' -ForegroundColor Cyan
    Push-Location $RepoRoot
    try {
        & cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build exited with code $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }
}

# The DLL the launcher embeds, byte for byte: the ASI form of the same mod is that DLL renamed, and
# nothing here repacks it (see the root build.ps1).
if (-not (Test-Path -LiteralPath $dll)) {
    throw "Not found: $dll. Run without -SkipCargo: cargo build --release at the repository root."
}

# ⚠️ Win32, not Win64: the game is a 32-bit process and never loads a 64-bit d3d9.dll. A wrong-arch
# file installs silently and then does nothing, which is the worst possible failure to debug.
if (-not (Test-Path -LiteralPath $loader)) {
    throw "Not found: $loader. See vendor/asi-loader/README.md - it must be the Win32 build."
}

$null = New-Item -ItemType Directory -Force -Path $PayloadDir

Copy-Item -LiteralPath $dll -Destination (Join-Path $PayloadDir 'drmod_rs_lib.asi') -Force
Copy-Item -LiteralPath $loader -Destination (Join-Path $PayloadDir 'd3d9.dll') -Force

$asiSize = [math]::Round((Get-Item -LiteralPath (Join-Path $PayloadDir 'drmod_rs_lib.asi')).Length / 1KB, 0)
$loaderSize = [math]::Round((Get-Item -LiteralPath (Join-Path $PayloadDir 'd3d9.dll')).Length / 1KB, 0)

Write-Host ''
Write-Host "Done: $PayloadDir" -ForegroundColor Green
Write-Host "  drmod_rs_lib.asi: $asiSize KB"
Write-Host "  d3d9.dll (loader): $loaderSize KB"
