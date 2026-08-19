# Поиск записей mov dword ptr [GameMenuStatus], imm
# GameMenuStatus: base+0x17E9F9C. В exe (ImageBase 0x400000) адрес = 0x1BE9F9C
# (little-endian: 9C 9F BE 01). Паттерн: C7 05 9C 9F BE 01 vv vv vv vv
$exePath = 'C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising Revengeance\METAL GEAR RISING REVENGEANCE.exe'
$bytes = [System.IO.File]::ReadAllBytes($exePath)

# Ищем все C7 05 (mov dword ptr [mem], imm) и проверяем адрес + значение
$hits = New-Object System.Collections.ArrayList
for($i = 0; $i -lt $bytes.Length - 9; $i++) {
    if($bytes[$i] -eq 0xC7 -and $bytes[$i+1] -eq 0x05) {
        # адрес little-endian
        $addr = $bytes[$i+2] -bor ($bytes[$i+3] -shl 8) -bor ($bytes[$i+4] -shl 16) -bor ($bytes[$i+5] -shl 24)
        if($addr -eq 0x01BE9F9C) {
            $val = $bytes[$i+6] -bor ($bytes[$i+7] -shl 8) -bor ($bytes[$i+8] -shl 16) -bor ($bytes[$i+9] -shl 24)
            $rva = 0x400000 + $i
            $hit = [pscustomobject]@{ Rva = ('0x{0:X}' -f $rva); Value = ('0x{0:X}' -f $val) }
            [void]$hits.Add($hit)
            Write-Host ('mov [GameMenuStatus], {0} @ RVA=0x{1:X}' -f ('0x{0:X}' -f $val), $rva)
        }
    }
}
Write-Host ("Total: {0} hits" -f $hits.Count)
