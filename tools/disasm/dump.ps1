# Hex dump диапазона кода из памяти запущенной игры (для ручного
# дизассемблирования вокруг call site).
# Пример:
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools/disasm/dump.ps1 -Rva 0x810400 -Len 0x380
param(
    [Parameter(Mandatory = $true)][int64]$Rva,
    [int64]$Len = 0x200,
    [int]$GamePid = 0
)
$ErrorActionPreference = 'Stop'
[System.Threading.Thread]::CurrentThread.CurrentCulture = [System.Globalization.CultureInfo]::InvariantCulture

$rvaStart = $Rva
$length = $Len

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class Disasm {
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, byte[] buf, int size, out int read);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool CloseHandle(IntPtr h);
}
"@

if ($GamePid -eq 0) {
    $proc = Get-Process -Name 'METAL GEAR RISING REVENGEANCE' -ErrorAction SilentlyContinue
    if (-not $proc) { Write-Output 'game process not found'; exit 1 }
} else {
    $proc = Get-Process -Id $GamePid -ErrorAction SilentlyContinue
    if (-not $proc) { Write-Output "PID $GamePid not found"; exit 1 }
}
$base = $proc.MainModule.BaseAddress.ToInt64()
$h = [Disasm]::OpenProcess(0x0410, $false, $proc.Id)
if ($h -eq [IntPtr]::Zero) { Write-Output 'OpenProcess FAIL'; exit 1 }

$buf = New-Object byte[] $length
$read = 0
[void][Disasm]::ReadProcessMemory($h, [IntPtr]($base + $rvaStart), $buf, $length, [ref]$read)

for ($i = 0; $i -lt $length; $i += 16) {
    $rva = $rvaStart + $i
    $line = ''
    for ($j = 0; $j -lt 16; $j++) {
        if ($i + $j -lt $length) { $line += $buf[$i + $j].ToString('X2') + ' ' } else { $line += '   ' }
    }
    Write-Output ('0x{0:X8}: {1}' -f $rva, $line)
}
[void][Disasm]::CloseHandle($h)
