# Поиск всех записей mov [base+0x17E9F9C], value
# В exe: 9C 9F 7E 01 (little-endian для 0x017E9F9C)
$bytes = [System.IO.File]::ReadAllBytes('C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising Revengeance\METAL GEAR RISING REVENGEANCE.exe')
$count = 0
for($i=0; $i -lt $bytes.Length-9 -and $count -lt 100; $i++) {
    # C7 05 xx xx xx xx vv vv vv vv = mov [addr], imm32
    if($bytes[$i] -eq 0xC7 -and $bytes[$i+1] -eq 0x05) {
        $addr = $bytes[$i+2] + ($bytes[$i+3] * 256) + ($bytes[$i+4] * 65536) + ($bytes[$i+5] * 16777216)
        if($addr -eq 0x017E9F9C) {
            $val = $bytes[$i+6] + ($bytes[$i+7] * 256) + ($bytes[$i+8] * 65536) + ($bytes[$i+9] * 16777216)
            $rva = 0x400000 + $i
            Write-Host ('mov [GameMenuStatus], {0} (0x{1:X}) @ RVA=0x{2:X}' -f $val, $val, $rva)
            $count++
        }
    }
}
