# Найти call site функции по RVA: сканирует exe на диске (по RVA) и,
# опционально (-Mem), память запущенной игры (по base + RVA).
# Пример:
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools/disasm/scan_calls.ps1 -Rva 0x785190
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools/disasm/scan_calls.ps1 -Rva 0x785190 -Mem
param(
    [Parameter(Mandatory = $true)][int64]$Rva,
    [switch]$Mem,
    [int]$GamePid = 0
)
$ErrorActionPreference = 'Stop'
[System.Threading.Thread]::CurrentThread.CurrentCulture = [System.Globalization.CultureInfo]::InvariantCulture

$targetRva = $Rva
$exe = 'C:\Program Files (x86)\Steam\steamapps\common\METAL GEAR RISING REVENGEANCE\METAL GEAR RISING REVENGEANCE.exe'

Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class Disasm {
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, byte[] buf, int size, out int read);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool CloseHandle(IntPtr h);
    public static int[] FindCalls(byte[] buf, long baseAddr, long target) {
        var res = new List<int>();
        for (int i = 0; i <= buf.Length - 5; i++) {
            if (buf[i] != 0xE8) continue;
            long t = baseAddr + i + 5 + (int)BitConverter.ToUInt32(buf, i + 1);
            if (t == target) res.Add(i);
        }
        return res.ToArray();
    }
}
"@

# --- режим 1: exe на диске (target = RVA) ---
function ScanExe {
    $bytes = [System.IO.File]::ReadAllBytes($exe)
    function ReadU32([int]$off) { return [BitConverter]::ToUInt32($bytes, $off) }
    function ReadU16([int]$off) { return [BitConverter]::ToUInt16($bytes, $off) }

    $e_lfanew = ReadU32 0x3C
    $numSections = ReadU16 ($e_lfanew + 6)
    $sizeOpt = ReadU16 ($e_lfanew + 20)
    $optOff = $e_lfanew + 24
    $secOff = $optOff + $sizeOpt
    $imageBase = ReadU32 ($optOff + 28)
    Write-Output ("exe: ImageBase=0x{0:X8} sections={1}" -f $imageBase, $numSections)

    for ($s = 0; $s -lt $numSections; $s++) {
        $o = $secOff + $s * 40
        $name = [System.Text.Encoding]::ASCII.GetString($bytes, $o, 8).TrimEnd([char]0)
        $virtualAddr = ReadU32 ($o + 12)
        $sizeRaw = ReadU32 ($o + 16)
        $ptrRaw = ReadU32 ($o + 20)
        $chars = ReadU32 ($o + 36)
        if (($chars -band 0x20000000) -eq 0) { continue }  # только executable
        $size = [Math]::Min($sizeRaw, $bytes.Length - $ptrRaw)
        $buf = New-Object byte[] $size
        [Array]::Copy($bytes, $ptrRaw, $buf, 0, $size)
        $hits = [Disasm]::FindCalls($buf, $virtualAddr, $targetRva)
        foreach ($h in $hits) {
            $rva = $virtualAddr + $h
            Write-Output ("CALL target=0x{0:X8} @ RVA=0x{1:X8} (abs 0x{2:X8}) sec={3}" -f $targetRva, $rva, ($imageBase + $rva), $name)
        }
    }
}

# --- режим 2: память процесса (target = base + RVA) ---
function ScanMem {
    if ($GamePid -eq 0) {
        $proc = Get-Process -Name 'METAL GEAR RISING REVENGEANCE' -ErrorAction SilentlyContinue
        if (-not $proc) { Write-Output 'game process not found'; return }
    } else {
        $proc = Get-Process -Id $GamePid -ErrorAction SilentlyContinue
        if (-not $proc) { Write-Output "PID $GamePid not found"; return }
    }
    $base = $proc.MainModule.BaseAddress.ToInt64()
    Write-Output ("mem: base=0x{0:X8}" -f $base)

    $h = [Disasm]::OpenProcess(0x0410, $false, $proc.Id)
    if ($h -eq [IntPtr]::Zero) { Write-Output 'OpenProcess FAIL'; return }
    $read = 0
    $tmp = New-Object byte[] 0x1000
    [void][Disasm]::ReadProcessMemory($h, [IntPtr]$base, $tmp, 0x1000, [ref]$read)
    $e_lfanew = [BitConverter]::ToInt32($tmp, 0x3C)
    $sizeOfImage = [BitConverter]::ToInt32($tmp, ($e_lfanew + 24) + 56)

    $target = $base + $targetRva
    $chunk = 0x100000
    $buf = New-Object byte[] $chunk
    for ($off = 0; $off -lt $sizeOfImage; $off += $chunk) {
        $sz = [Math]::Min($chunk, $sizeOfImage - $off)
        $curBase = $base + $off
        if (-not [Disasm]::ReadProcessMemory($h, [IntPtr]$curBase, $buf, $sz, [ref]$read)) { continue }
        $hits = [Disasm]::FindCalls($buf, $curBase, $target)
        foreach ($c in $hits) {
            $rva = $off + $c
            Write-Output ("CALL target=0x{0:X8} @ RVA=0x{1:X8} (abs 0x{2:X8})" -f $targetRva, $rva, ($base + $rva))
        }
    }
    [void][Disasm]::CloseHandle($h)
}

ScanExe
if ($Mem) { Write-Output '--- memory ---'; ScanMem }
