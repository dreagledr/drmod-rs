<#
.SYNOPSIS
    Снимок окна TAS Editor для отладки вёрстки.

.DESCRIPTION
    Делает `PrintWindow` с флагом `PW_RENDERFULLCONTENT` (2): без него DirectComposition-содержимое
    WinUI выходит пустым кадром. Печатает размер окна и «цветосумму» — по ней видно, что кадр
    не чёрный.

.EXAMPLE
    pwsh -File screenshot.ps1 -Out shot.png

    Снять окно процесса tas-editor в shot.png рядом со скриптом.
#>
param(
    [string]$ProcName = 'tas-editor',
    [string]$Out = "$PSScriptRoot\shot.png"
)

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class WinShot {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdc, uint flags);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@

$p = Get-Process -Name $ProcName -ErrorAction Stop | Select-Object -First 1
$h = $p.MainWindowHandle
[WinShot]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Milliseconds 800

$r = New-Object WinShot+RECT
[WinShot]::GetWindowRect($h, [ref]$r) | Out-Null
$width = $r.Right - $r.Left
$height = $r.Bottom - $r.Top

$bmp = New-Object System.Drawing.Bitmap($width, $height)
$graphics = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $graphics.GetHdc()
$ok = [WinShot]::PrintWindow($h, $hdc, 2)
$graphics.ReleaseHdc($hdc)
$graphics.Dispose()

$bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)

$sum = 0
for ($y = 0; $y -lt $height; $y += 20) {
    for ($x = 0; $x -lt $width; $x += 20) {
        $c = $bmp.GetPixel($x, $y)
        $sum += $c.R + $c.G + $c.B
    }
}
$bmp.Dispose()

"SAVED $Out ${width}x${height} printwindow=$ok colorsum=$sum"
