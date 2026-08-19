# 0x17E9F9C in little-endian: 9C 9F 7E 01
$bytes = [System.IO.File]::ReadAllBytes('C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising Revengeance\METAL GEAR RISING REVENGEANCE.exe')
$count = 0
for($i=0; $i -lt $bytes.Length-9 -and $count -lt 50; $i++) {
    if($bytes[$i] -eq 0xC7 -and $bytes[$i+1] -eq 0x05 -and $bytes[$i+2] -eq 0x9C -and $bytes[$i+3] -eq 0x9F -and $bytes[$i+4] -eq 0x7E) {
        $val = $bytes[$i+5] + ($bytes[$i+6] * 256) + ($bytes[$i+7] * 65536) + ($bytes[$i+8] * 16777216)
        $rva = 0x400000 + $i
        Write-Host ('mov [GameMenuStatus], {0} @ RVA=0x{1:X}' -f $val, $rva)
        $count++
    }
}
