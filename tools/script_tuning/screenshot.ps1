# Скриншот окна игры (PrintWindow — работает и когда окно перекрыто).
# Печатает путь к PNG; читать его можно инструментом read_file.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\script_tuning\screenshot.ps1
param(
    [string]$ProcessName = 'METAL GEAR RISING REVENGEANCE',
    [string]$Out = 'out\game.png'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public class DrmodShot {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)]
    public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    public struct RECT { public int Left, Top, Right, Bottom; }
    public struct POINT { public int X, Y; }
}
'@

# Ищем окно через Process.MainWindowHandle — надёжнее, чем FindWindow по
# заголовку (заголовок локализуется/меняется, а хендл процесса всегда есть).
$proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $proc) { Write-Error "процесс/окно не найдены: $ProcessName"; exit 1 }
$hwnd = $proc.MainWindowHandle
$rect = New-Object DrmodShot+RECT
if (-not [DrmodShot]::GetWindowRect($hwnd, [ref]$rect)) { Write-Error 'GetWindowRect FAIL'; exit 1 }
$w = $rect.Right - $rect.Left
$h = $rect.Bottom - $rect.Top
if ($w -le 0 -or $h -le 0) { Write-Error "нулевой размер окна ($w x $h)"; exit 1 }

# Захват через CopyFromScreen (BitBlt рабочего стола): PrintWindow для D3D9-окна
# отдаёт пустой кадр. Требуется, чтобы окно было видно на экране.
$bmp = New-Object System.Drawing.Bitmap $w, $h
$gfx = [System.Drawing.Graphics]::FromImage($bmp)
$gfx.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size $w, $h))
$gfx.Dispose()

$full = Join-Path (Get-Location) $Out
$dir = Split-Path $full -Parent
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
$bmp.Save($full, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output ("{0}  ({1}x{2}, PrintWindow={3})" -f $full, $w, $h, $ok)
