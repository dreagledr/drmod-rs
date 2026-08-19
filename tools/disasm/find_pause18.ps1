# Поиск push 0x12 (keybind 18 = KEYBIND_PAUSE) перед call isKeybindDown (0x61D280)
$exePath = 'C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising Revengeance\METAL GEAR RISING REVENGEANCE.exe'
$bytes = [System.IO.File]::ReadAllBytes($exePath)

$hits = New-Object System.Collections.ArrayList
for($i = 0; $i -lt $bytes.Length - 6; $i++) {
    if($bytes[$i] -eq 0x6A -and $bytes[$i+1] -eq 0x12 -and $bytes[$i+2] -eq 0xE8) {
        # Вычисляем target call
        $rel = $bytes[$i+3] -bor ($bytes[$i+4] -shl 8) -bor ($bytes[$i+5] -shl 16) -bor ($bytes[$i+6] -shl 24)
        if($rel -gt 0x7FFFFFFF) { $rel = $rel - 0x100000000 }
        $call_addr = 0x400000 + $i + 2
        $target = $call_addr + 5 + $rel
        if($target -eq 0x61D280) {
            $rva = 0x400000 + $i
            [void]$hits.Add($rva)
            Write-Host ('push 0x12; call isKeybindDown @ RVA=0x{0:X}' -f $rva)
        }
    }
}
Write-Host ("Total: {0} hits" -f $hits.Count)
