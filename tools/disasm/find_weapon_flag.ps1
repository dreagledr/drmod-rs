# Поиск обращений к флагу GAME_WEAPON_SELECT (base+0x17EA090, бит 0x40000)
# и к GameMenuStatus (base+0x17E9F9C) через любые формы адресации.
# Адрес в exe (ImageBase 0x400000): flag = 0x400000+0x17EA090 = 0x1BEA090
# little-endian: 90 A0 BE 01
$exePath = 'C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising Revengeance\METAL GEAR RISING REVENGEANCE.exe'
$bytes = [System.IO.File]::ReadAllBytes($exePath)

# Ищем 5-байтовые последовательности с адресом 0x01BEA090 в любом opcode
$target = @(0x90, 0xA0, 0xBE, 0x01)
$hits = New-Object System.Collections.ArrayList
for($i = 0; $i -lt $bytes.Length - 5; $i++) {
    $m = $true
    for($j = 0; $j -lt 4; $j++) {
        if($bytes[$i+$j] -ne $target[$j]) { $m = $false; break }
    }
    if($m) {
        # opcode за 4 байта до адреса
        $opStart = $i - 2
        if($opStart -ge 0) {
            $op1 = $bytes[$opStart]
            $op2 = $bytes[$opStart+1]
            $rva = 0x400000 + $opStart
            $hit = [pscustomobject]@{ Rva = ('0x{0:X}' -f $rva); Op = ('{0:X2} {1:X2}' -f $op1, $op2) }
            [void]$hits.Add($hit)
            Write-Host ('op {0:X2} {1:X2} -> [0x17EA090] @ RVA=0x{2:X}' -f $op1, $op2, $rva)
        }
    }
}
Write-Host ("Total: {0} hits" -f $hits.Count)
