<#
.SYNOPSIS
    Упаковка self-contained поставки TAS Editor.

.DESCRIPTION
    Копирует из каталога профиля Cargo (target/<triple>/<profile>) только то, что нужно
    конечному пользователю: сам exe, приватный Windows App Runtime (кладёт его build-скрипт
    через windows-reactor-setup) и каталог ресурсов XAML. Артефакты сборки (deps, build,
    .fingerprint, incremental, examples, *.d, *.pdb) в поставку не попадают; языковые
    каталоги прореживаются до -KeepCultures.

    Скрипт падает с ненулевым кодом, если exe нет или staging runtime неполон — это тот же
    сбой, что даёт обрезанная загрузка NuGet-пакета в кэше helper'а (см. README).

.EXAMPLE
    pwsh -File pack.ps1 -Build -Zip

    Собрать release, упаковать в dist/ и положить рядом dist/tas-editor.zip.
#>
#Requires -Version 5.1
[CmdletBinding()]
param(
    # Профиль Cargo, из которого берётся сборка.
    [ValidateSet('release', 'debug')]
    [string]$Profile = 'release',

    # Куда собирать поставку. Каталог полностью пересоздаётся.
    [string]$OutDir,

    # Языковые каталоги runtime, которые остаются в поставке. 'all' — оставить все.
    [string[]]$KeepCultures = @('en-us', 'ru-RU'),

    # Нужен только приложениям с XAML-контролом WebView2.
    [switch]$IncludeWebView2,

    # Сначала выполнить cargo build с этим профилем.
    [switch]$Build,

    # Дополнительно собрать dist/tas-editor.zip.
    [switch]$Zip,

    [string]$Triple = 'x86_64-pc-windows-msvc'
)

$ErrorActionPreference = 'Stop'

# $PSScriptRoot пуст, пока вычисляются значения по умолчанию параметров, поэтому пути собираем здесь.
$scriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }
if (-not $OutDir) { $OutDir = Join-Path $scriptRoot 'dist' }

if ($Build) {
    Write-Host "cargo build --profile $Profile" -ForegroundColor Cyan
    & cargo build --profile $Profile
    if ($LASTEXITCODE -ne 0) { throw "cargo build завершился с кодом $LASTEXITCODE" }
}

$src = Join-Path $scriptRoot "target\$Triple\$Profile"
$exe = Join-Path $src 'tas-editor.exe'
if (-not (Test-Path -LiteralPath $exe)) {
    throw "Не найдено: $exe. Сначала соберите проект: cargo build --$Profile (или запустите скрипт с -Build)."
}

# Артефакты сборки cargo и служебные файлы — в поставку не идут.
$skipDirs = @('deps', 'build', '.fingerprint', 'incremental', 'examples')
$skipFiles = @('tas-editor.d', 'tas_editor.pdb')

# Каталог ресурсов XAML: не локаль, но и не артефакт сборки.
$resourceDir = 'Microsoft.UI.Xaml'

# Без этих файлов self-contained exe не стартует — их отсутствие означает незавершённый staging.
$requiredFiles = @(
    'Microsoft.WindowsAppRuntime.dll',
    'Microsoft.UI.Xaml.dll',
    'Microsoft.UI.Xaml.Controls.dll',
    'CoreMessagingXP.dll',
    'resources.pri'
)

function Test-CultureName([string]$name) {
    return $name -match '^[A-Za-z]{2,3}(-[A-Za-z]{2,8})+$'
}

if (Test-Path -LiteralPath $OutDir) { Remove-Item -LiteralPath $OutDir -Recurse -Force }
New-Item -ItemType Directory -Path $OutDir | Out-Null

$copiedCultures = New-Object System.Collections.Generic.List[string]

foreach ($item in Get-ChildItem -Force -LiteralPath $src) {
    if ($item.PSIsContainer) {
        if ($skipDirs -contains $item.Name) { continue }

        if ((Test-CultureName $item.Name) -and ($KeepCultures -notcontains 'all') -and
            ($KeepCultures -notcontains $item.Name)) {
            continue
        }

        Copy-Item -LiteralPath $item.FullName -Destination (Join-Path $OutDir $item.Name) -Recurse -Force
        if (Test-CultureName $item.Name) { $copiedCultures.Add($item.Name) }
        continue
    }

    $name = $item.Name
    if ($skipFiles -contains $name) { continue }
    if ($name -like '.cargo-*') { continue }
    if ($item.Extension -in @('.d', '.pdb')) { continue }
    if (($name -eq 'Microsoft.Web.WebView2.Core.dll') -and (-not $IncludeWebView2)) {
        Write-Host "skip  $name (нет -IncludeWebView2)" -ForegroundColor DarkGray
        continue
    }

    Copy-Item -LiteralPath $item.FullName -Destination $OutDir -Force
}

foreach ($required in $requiredFiles) {
    if (-not (Test-Path -LiteralPath (Join-Path $OutDir $required))) {
        throw ("Поставка неполна: нет $required. Это признак незавершённого self-contained staging — " +
            "проверьте кэш %LOCALAPPDATA%\windows-reactor-setup\temp (подробности — README, раздел «Self-contained»).")
    }
}

$files = Get-ChildItem -Recurse -File -LiteralPath $OutDir
$sizeMb = [math]::Round((($files | Measure-Object Length -Sum).Sum / 1MB), 1)

Write-Host ''
Write-Host "Готово: $OutDir" -ForegroundColor Green
Write-Host ("  файлов: {0}, размер: {1} МБ" -f $files.Count, $sizeMb)
Write-Host ("  локали: {0}" -f $(if ($copiedCultures.Count) { $copiedCultures -join ', ' } else { 'нет' }))

if ($Zip) {
    $zipPath = Join-Path $OutDir 'tas-editor.zip'
    $tempZip = Join-Path $env:TEMP ("tas-editor-{0}.zip" -f [guid]::NewGuid().ToString('N'))
    Compress-Archive -Path (Join-Path $OutDir '*') -DestinationPath $tempZip -Force
    Move-Item -LiteralPath $tempZip -Destination $zipPath -Force
    $zipMb = [math]::Round((Get-Item -LiteralPath $zipPath).Length / 1MB, 1)
    Write-Host ("  архив: {0} ({1} МБ)" -f $zipPath, $zipMb)
}

Write-Host ''
Write-Host "Запуск поставки: $OutDir\tas-editor.exe" -ForegroundColor Cyan
