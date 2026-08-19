# Поиск push 0x10 перед call 0x61D280 (isKeybindDown)
# Паттерн: 6A 10 E8 xx xx xx xx (где call target = 0x61D280)
$bytes = [System.IO.File]::ReadAllBytes('C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising Revengeance\METAL GEAR RISING REVENGEANCE.exe')
for($i=0; $i -lt $bytes.Length-6; $i++) {
    if($bytes[$i] -eq 0x6A -and $bytes[$i+1] -eq 0x10 -and $bytes[$i+2] -eq 0xE8) {
        # Вычисляем target call
        $rel = $bytes[$i+3] + ($bytes[$i+4] * 256) + ($bytes[$i+5] * 65536) + ($bytes[$i+6] * 16777216)
        if($rel -gt 0x7FFFFFFF) { $rel = $rel - 0x100000000 }  # sign extend
        $call_addr = 0x400000 + $i + 2
        $target = $call_addr + 5 + $rel
        if($target -eq 0x61D280) {
            $rva = 0x400000 + $i
            Write-Host ('push 0x10; call isKeybindDown @ RVA=0x{0:X}' -f $rva)
        }
    }
}
