<#
.SYNOPSIS
    Pack the TAS Editor (Rust) into a shippable zip.

.DESCRIPTION
    The Rust editor is one file: its build script embeds the mod payload (`drmod_rs_lib.asi` and
    `d3d9.dll`) into the exe, so there is no runtime folder to assemble and nothing to trim. What
    this script adds is the two things a zip needs to be a distribution rather than a bare binary:

      - the `examples/` scripts, so a first launch opens on a workspace instead of an empty pane
        (`EditorSettings::first_folder` looks for them beside the exe first);
      - the release exe itself, taken from `target\<target>\release`.

    ⚠️ The exe is **not** slimmed. It carries the mod and the ASI loader as embedded bytes — 22 MB
    of which ~9 MB is the payload — and that is the point: a user who unzips this and clicks Install
    gets the mod the same build produced. Stripping it would break that, so nothing here does.

    ⚠️ Release only. A debug build links a different runtime and expects a debugger's world; a zip
    made from one is a zip that fails on a machine that has no Rust toolchain.

.EXAMPLE
    pwsh -File pack.ps1

    Build Release (if -Build), assemble `out\drmod-tas-editor`, and drop `drmod-tas-editor.zip` next to
    this script.

.EXAMPLE
    pwsh -File pack.ps1 -Build -OutDir .\out\drmod-tas-editor -ZipPath .\out\drmod-tas-editor.zip

    What CI does: assemble an already built tree into the root's `out\` and zip it there.

.EXAMPLE
    pwsh -File pack.ps1 -SkipCargo

    Repack a tree that was just built, without rebuilding it.
#>
#Requires -Version 7.0
[CmdletBinding()]
param(
    # Where to assemble the distribution. Recreated from scratch on every run.
    [string]$OutDir,

    # Where the .zip goes when -Zip is given. Defaults to drmod-tas-editor.zip next to this script.
    [string]$ZipPath,

    # Build Release before packing. Off by default: a zip should describe an explicit build, not one
    # the script quietly made for you.
    [switch]$Build,

    # With -Build, reuse the mod DLL the last `cargo build --release` at the repository root left in
    # `target/` instead of building it again. The release job runs the root `build.ps1` first, which
    # already built it, so this is what keeps one release from compiling the same mod twice.
    #
    # ⚠️ It sets TAS_EDITOR_SKIP_MOD_BUILD for the editor's build script. That skips the *mod build*,
    # not the check for it: a DLL still has to exist, and the build fails with a message naming it if
    # none does.
    [switch]$SkipCargo,

    # Also produce the .zip.
    [switch]$Zip
)

$ErrorActionPreference = 'Stop'

# $PSScriptRoot is empty while parameter defaults are evaluated, so paths are built here.
$scriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }
if (-not $OutDir) { $OutDir = Join-Path $scriptRoot 'out\drmod-tas-editor' }

# The crate builds for x64 (its own .cargo/config.toml), which is why the path names the triple
# rather than the host default.
$target = 'x86_64-pc-windows-msvc'
$exe = Join-Path $scriptRoot "target\$target\release\drmod-tas-editor.exe"
$examples = Join-Path $scriptRoot 'examples'

if ($Build) {
    Write-Host "cargo build --release" -ForegroundColor Cyan
    Push-Location $scriptRoot
    try {
        # The editor's build script builds the mod itself. With -SkipCargo that step is handed off to
        # whatever the root build already left, which is the only reason this variable exists.
        if ($SkipCargo) {
            Write-Host "  -SkipCargo: reusing the mod DLL already in the root's target/" -ForegroundColor DarkGray
            $env:TAS_EDITOR_SKIP_MOD_BUILD = '1'
        }

        & cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build exited with code $LASTEXITCODE" }
    }
    finally {
        if ($SkipCargo) { Remove-Item Env:\TAS_EDITOR_SKIP_MOD_BUILD -ErrorAction SilentlyContinue }
        Pop-Location
    }
}

if (-not (Test-Path -LiteralPath $exe)) {
    throw ("Not found: $exe. Build first: cargo build --release in $scriptRoot " +
        "(or run this script with -Build).")
}

# The payload is embedded, so its presence inside the exe is what a release rests on. A build that
# somehow produced an editor without it would install nothing, and the failure would surface in
# front of a user — so it is checked here, where the answer is still cheap.
$exeMb = [math]::Round((Get-Item -LiteralPath $exe).Length / 1MB, 1)
if ($exeMb -lt 10) {
    throw ("$exe is only $exeMb MB, which is too small to carry the mod payload. The test that " +
        "catches this is in the build script's output: it prints `mod payload embedded from ...`. " +
        "A build made with TAS_EDITOR_SKIP_MOD_BUILD and no mod DLL anywhere else would fail it.")
}

if (-not (Test-Path -LiteralPath $examples)) {
    throw "Not found: $examples. A distribution without it opens on an empty workspace."
}

if (Test-Path -LiteralPath $OutDir) { Remove-Item -LiteralPath $OutDir -Recurse -Force }
New-Item -ItemType Directory -Path $OutDir | Out-Null

Copy-Item -LiteralPath $exe -Destination $OutDir -Force

# The scripts are copies of `tools/demo/`, staged here by hand rather than linked: a link into a
# Rust tool directory would break the moment this tree is built on its own.
Copy-Item -LiteralPath $examples -Destination (Join-Path $OutDir 'examples') -Recurse -Force

$files = Get-ChildItem -Recurse -File -LiteralPath $OutDir
$sizeMb = [math]::Round((($files | Measure-Object Length -Sum).Sum / 1MB), 1)

Write-Host ''
Write-Host "Done: $OutDir" -ForegroundColor Green
Write-Host ("  files: {0}, size: {1} MB" -f $files.Count, $sizeMb)
Write-Host ("  exe: {0} MB (payload embedded)" -f $exeMb)

if ($Zip) {
    if (-not $ZipPath) { $ZipPath = Join-Path $scriptRoot 'drmod-tas-editor.zip' }
    $zipDir = Split-Path -Parent $ZipPath
    if ($zipDir -and -not (Test-Path -LiteralPath $zipDir)) {
        New-Item -ItemType Directory -Path $zipDir -Force | Out-Null
    }

    $tempZip = Join-Path $env:TEMP ("drmod-tas-editor-{0}.zip" -f [guid]::NewGuid().ToString('N'))
    Compress-Archive -Path (Join-Path $OutDir '*') -DestinationPath $tempZip -Force
    Move-Item -LiteralPath $tempZip -Destination $ZipPath -Force

    $zipMb = [math]::Round((Get-Item -LiteralPath $ZipPath).Length / 1MB, 1)
    Write-Host ("  archive: {0} ({1} MB)" -f $ZipPath, $zipMb)
}

Write-Host ''
Write-Host "Run it: $OutDir\drmod-tas-editor.exe" -ForegroundColor Cyan
