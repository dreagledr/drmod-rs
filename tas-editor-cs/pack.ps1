<#
.SYNOPSIS
    Pack the TAS Editor (C#) into a shippable zip.

.DESCRIPTION
    Takes the `dotnet publish` output for the app and copies only what a user needs to run it:
    the native exe, the app's own resource index, the private Windows App SDK runtime and the
    locale the UI is written in.

    The publish folder is a build artifact directory, not a distribution. It carries NativeAOT's
    debug symbols, the Windows App SDK's full payload — including subsystems this editor never
    touches (onnxruntime/DirectML, the Windows.AI.* stack, Windows Search and Semantic Index,
    the Widgets and Workloads runtimes, WebView2) — and `.mui` resource strings for ~84 locales.
    None of that is used by an editor whose UI is English-only, so it does not ship.

    The script fails with a non-zero exit code when the publish folder has no exe or when the
    required runtime core did not survive the copy — a publish that produced a truncated payload
    should not turn into a silently broken zip.

    Requires PowerShell 7 (`pwsh`). Windows PowerShell 5.1 reads scripts as ANSI and mangles the
    punctuation below into a syntax error; `pwsh` reads them as UTF-8, which is what the files in
    this repository are.

.EXAMPLE
    pwsh -File pack.ps1 -Build -Zip

    Publish Release (if -Build), pack into dist/, and drop the archive next to this script.

.EXAMPLE
    pwsh -File pack.ps1 -BuildMod -Build -Zip

    Build the mod payload the editor embeds, publish Release, pack into dist/, and drop the
    archive next to this script. This is the whole chain, and what a release is made of.

.EXAMPLE
    pwsh -File pack.ps1 -OutDir .\out\tas-editor -ZipPath .\out\tas-editor-cs.zip

    What CI does: pack an already published tree into its own artifact directory.

.EXAMPLE
    cd <repository root>
    pwsh -File tas-editor-cs\pack.ps1 -BuildMod -Build `
        -OutDir .\out\tas-editor -Zip -ZipPath .\out\tas-editor-cs.zip

    One command for a release-ready editor carrying the **current** mod: build the mod payload
    from source (`cargo build --release` at the repository root), publish the editor Release, trim
    into `out/tas-editor`, and drop the archive at `out/tas-editor-cs.zip`.

    ⚠️ No `-SkipCargo` on purpose - it reuses whatever the last `cargo build --release` left in
    `target/`, which is only right when the root `build.ps1` has *just* built the mod in the same
    run (the CI job does exactly that). For "give me a release with the current mod", let this
    build it.

    ⚠️ `-OutDir` and `-ZipPath` are taken as written and resolved against your **current
    directory**, so run this from the repository root for `.\out` to mean the root's `out/`.
    (Only the defaults - `publish`, `dist`, `tas-editor-cs.zip` - are built from the script's own
    folder.)
#>
#Requires -Version 7.0
[CmdletBinding()]
param(
    # Where `dotnet publish` wrote the app. Recreated only when -Build is given.
    [string]$PublishDir,

    # Where to put the trimmed distribution. Recreated from scratch on every run.
    [string]$OutDir,

    # Locales kept in the distribution. 'all' keeps every locale folder.
    [string[]]$KeepCultures = @('en-us'),

    # The Reactor devtools switch is Debug-only in the csproj; its package output is dead weight
    # in a retail zip. Kept out unless asked for.
    [switch]$IncludeDevtools,

    # Building is off by default: a packed zip should describe an explicit publish, not one the
    # script quietly made for you.
    [switch]$Build,

    # Build the mod payload the editor embeds (cargo + the vendored loader) before publishing.
    # Without it the csproj fails on the missing payload, so this is only for a repack of an
    # already-built tree.
    [switch]$BuildMod,

    # With -BuildMod, reuse what the last `cargo build --release` left in target/ instead of
    # building the mod again. The release job runs the root `build.ps1` first, which already built
    # it, so this is what keeps the same tree from being compiled twice in one run.
    [switch]$SkipCargo,

    # Also produce the .zip.
    [switch]$Zip,

    # Where the .zip goes when -Zip is given. Defaults to tas-editor-cs.zip next to this script;
    # CI points it at its own artifact directory.
    [string]$ZipPath,

    [string]$Configuration = 'Release'
)

$ErrorActionPreference = 'Stop'

# $PSScriptRoot is empty while parameter defaults are evaluated, so paths are built here.
$scriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }
$project = Join-Path $scriptRoot 'TasEditorCs\TasEditorCs.csproj'
if (-not $PublishDir) { $PublishDir = Join-Path $scriptRoot 'publish' }
if (-not $OutDir) { $OutDir = Join-Path $scriptRoot 'dist' }

if ($BuildMod) {
    Write-Host "build-mod.ps1 - stage the embedded mod payload" -ForegroundColor Cyan
    # -SkipCargo is forwarded only when asked for: the payload is the *same* DLL the ASI archive
    # carries, and the release job builds it once in the root script before this one runs.
    $modArgs = @{}
    if ($SkipCargo) { $modArgs['SkipCargo'] = $true }

    # ⚠️ Success is judged by the payload being there, not by `$LASTEXITCODE`. A dot-sourced script
    # leaves that variable alone when it ends on a cmdlet (a `Copy-Item`), so the check read an empty
    # value and failed a run that had just printed its own success — measured with `-SkipCargo`.
    # `build-mod.ps1` throws on every way it can fail, and the csproj refuses to build without the
    # files, so the files are the answer.
    & (Join-Path $scriptRoot 'build-mod.ps1') @modArgs

    foreach ($needed in @('Mod\drmod_rs_lib.asi', 'Mod\d3d9.dll')) {
        if (-not (Test-Path -LiteralPath (Join-Path $scriptRoot "TasEditorCs\$needed"))) {
            throw "build-mod.ps1 left no $needed - see its output above."
        }
    }
}

if ($Build) {
    Write-Host "dotnet publish -c $Configuration -o $PublishDir" -ForegroundColor Cyan
    & dotnet publish $project -c $Configuration -o $PublishDir
    if ($LASTEXITCODE -ne 0) { throw "dotnet publish exited with code $LASTEXITCODE" }
}

$exe = Join-Path $PublishDir 'TasEditorCs.exe'
if (-not (Test-Path -LiteralPath $exe)) {
    throw "Not found: $exe. Publish first: dotnet publish $project -c $Configuration -o $PublishDir (or run this script with -Build)."
}

# Subsystems this editor does not use, verified against the live module list of a running build
# (34 of the publish folder's 54 binaries are never loaded by the editor):
#   - TasEditorCs.pdb — NativeAOT debug symbols, ~88 MB the runtime never reads;
#   - onnxruntime / DirectML — ML inference accelerators behind the Windows.AI stack;
#   - Microsoft.Windows.AI.* — the Windows AI APIs (Text, Imaging, Video, ContentSafety, ...);
#   - Microsoft.Windows.Search / Microsoft.Asg.SemanticIndex.* / PerceptiveStreaming — Search
#     and semantic-index runtimes;
#   - Microsoft.Windows.Widgets / Microsoft.Windows.Workloads.* — Widgets and ML Workloads;
#   - Microsoft.Web.WebView2.Core / WebView2Loader — needed only by apps hosting a WebView2;
#   - Microsoft.UI.Designer — the XAML designer host, a Visual Studio companion;
#   - NPUDetect — NPU capability probing for the AI stack.
$excludePatterns = @(
    '*.pdb',
    'onnxruntime.dll',
    'DirectML.dll',
    'Microsoft.Windows.AI.*.dll',
    'Microsoft.Windows.AI.winmd',
    'Microsoft.Windows.Search.dll',
    'Microsoft.Windows.Search.winmd',
    'Microsoft.Asg.SemanticIndex.AiFabric.Compatibility.dll',
    'PerceptiveStreaming.dll',
    'Microsoft.Windows.Widgets.dll',
    'Microsoft.Windows.Widgets.winmd',
    'Microsoft.Windows.Workloads*.dll',
    'Microsoft.Windows.Workloads*.pri',
    'Microsoft.Windows.Workloads.winmd',
    'Microsoft.Web.WebView2.Core.dll',
    'WebView2Loader.dll',
    'Microsoft.UI.Designer.dll',
    'NPUDetect.dll'
)

# The devtools payload only exists for Debug publishes; in a retail zip it is dead weight.
if (-not $IncludeDevtools) {
    $excludePatterns += @(
        'Microsoft.UI.Reactor.Devtools.dll',
        'Microsoft.UI.Reactor.Devtools.pri',
        'Microsoft.UI.Reactor.Devtools.pri.xml'
    )
}

# Without these the self-contained app cannot start: the exe carries a self-contained manifest and
# looks for the private Windows App SDK next to itself, and the app's merged resource index holds
# the WinUI control resources. Their absence means an incomplete publish — the failure mode behind
# the 0xC000027B crash documented in README.md.
$requiredFiles = @(
    'TasEditorCs.exe',
    'TasEditorCs.pri',
    'Microsoft.WindowsAppRuntime.dll',
    'Microsoft.UI.Xaml.dll',
    'Microsoft.UI.Xaml.Controls.dll',
    'CoreMessagingXP.dll',
    'Reactor.pri'
)

function Test-CultureName([string]$name) {
    return $name -match '^[A-Za-z]{2,3}(-[A-Za-z]{2,8})+$'
}

if (Test-Path -LiteralPath $OutDir) { Remove-Item -LiteralPath $OutDir -Recurse -Force }
New-Item -ItemType Directory -Path $OutDir | Out-Null

$copiedCultures = New-Object System.Collections.Generic.List[string]

foreach ($item in Get-ChildItem -Force -LiteralPath $PublishDir) {
    if ($item.PSIsContainer) {
        if ((Test-CultureName $item.Name) -and ($KeepCultures -notcontains 'all') -and
            ($KeepCultures -notcontains $item.Name)) {
            continue
        }

        Copy-Item -LiteralPath $item.FullName -Destination (Join-Path $OutDir $item.Name) -Recurse -Force
        if (Test-CultureName $item.Name) { $copiedCultures.Add($item.Name) }
        continue
    }

    $matched = $false
    foreach ($pattern in $excludePatterns) {
        if ($item.Name -like $pattern) { $matched = $true; break }
    }
    if ($matched) { continue }

    Copy-Item -LiteralPath $item.FullName -Destination $OutDir -Force
}

foreach ($required in $requiredFiles) {
    if (-not (Test-Path -LiteralPath (Join-Path $OutDir $required))) {
        throw ("Distribution is incomplete: $required is missing. The publish folder looks truncated — " +
            "re-run: dotnet publish $project -c $Configuration -o $PublishDir")
    }
}

$files = Get-ChildItem -Recurse -File -LiteralPath $OutDir
$sizeMb = [math]::Round((($files | Measure-Object Length -Sum).Sum / 1MB), 1)

Write-Host ''
Write-Host "Done: $OutDir" -ForegroundColor Green
Write-Host ("  files: {0}, size: {1} MB" -f $files.Count, $sizeMb)
Write-Host ("  locales: {0}" -f $(if ($copiedCultures.Count) { $copiedCultures -join ', ' } else { 'none' }))

if ($Zip) {
    if (-not $ZipPath) { $ZipPath = Join-Path $scriptRoot 'tas-editor-cs.zip' }
    $zipDir = Split-Path -Parent $ZipPath
    if ($zipDir -and -not (Test-Path -LiteralPath $zipDir)) {
        New-Item -ItemType Directory -Path $zipDir -Force | Out-Null
    }
    $tempZip = Join-Path $env:TEMP ("tas-editor-cs-{0}.zip" -f [guid]::NewGuid().ToString('N'))
    Compress-Archive -Path (Join-Path $OutDir '*') -DestinationPath $tempZip -Force
    Move-Item -LiteralPath $tempZip -Destination $ZipPath -Force
    $zipMb = [math]::Round((Get-Item -LiteralPath $ZipPath).Length / 1MB, 1)
    Write-Host ("  archive: {0} ({1} MB)" -f $ZipPath, $zipMb)
}

Write-Host ''
Write-Host "Run it: $OutDir\TasEditorCs.exe" -ForegroundColor Cyan
