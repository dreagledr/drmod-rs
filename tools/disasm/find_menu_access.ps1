# Поиск всех обращений к GameMenuStatus (base+0x17E9F9C)
# Адрес в exe (ImageBase 0x400000): 0x400000 + 0x17E9F9C = 0x1BE9F9C
# little-endian: 9C 9F BE 01
# Паттерны:
#   C7 05 9C 9F BE 01 vv vv vv vv  = mov dword ptr [addr], imm32
#   A3 9C 9F BE 01                  = mov eax, [addr] / mov [addr], eax
#   8B 0D 9C 9F BE 01               = mov ecx, [addr]
#   8B 15 9C 9F BE 01               = mov edx, [addr]
#   89 0D 9C 9F BE 01               = mov [addr], ecx
#   89 15 9C 9F BE 01               = mov [addr], edx
#   89 1D 9C 9F BE 01               = mov [addr], ebx
#   89 35 9C 9F BE 01               = mov [addr], esi
#   89 3D 9C 9F BE 01               = mov [addr], edi
#   FF 05 9C 9F BE 01               = inc dword ptr [addr]
#   FF 0D 9C 9F BE 01               = dec dword ptr [addr]
$exePath = 'C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising Revengeance\METAL GEAR RISING REVENGEANCE.exe'
$bytes = [System.IO.File]::ReadAllBytes($exePath)

$patterns = @(
    @{ Name='mov [mem],imm32 (C7 05)'; Op=@(0xC7,0x05) },
    @{ Name='mov eax,[mem]/mov [mem],eax (A3)'; Op=@(0xA3) },
    @{ Name='mov ecx,[mem] (8B 0D)'; Op=@(0x8B,0x0D) },
    @{ Name='mov edx,[mem] (8B 15)'; Op=@(0x8B,0x15) },
    @{ Name='mov [mem],ecx (89 0D)'; Op=@(0x89,0x0D) },
    @{ Name='mov [mem],edx (89 15)'; Op=@(0x89,0x15) },
    @{ Name='mov [mem],ebx (89 1D)'; Op=@(0x89,0x1D) },
    @{ Name='mov [mem],esi (89 35)'; Op=@(0x89,0x35) },
    @{ Name='mov [mem],edi (89 3D)'; Op=@(0x89,0x3D) },
    @{ Name='inc dword ptr [mem] (FF 05)'; Op=@(0xFF,0x05) },
    @{ Name='dec dword ptr [mem] (FF 0D)'; Op=@(0xFF,0x0D) }
)

$hits = New-Object System.Collections.ArrayList
foreach($p in $patterns) {
    $op = $p.Op
    $opLen = $op.Length
    for($i = 0; $i -lt $bytes.Length - ($opLen + 4); $i++) {
        $match = $true
        for($j = 0; $j -lt $opLen; $j++) {
            if($bytes[$i+$j] -ne $op[$j]) { $match = $false; break }
        }
        if(-not $match) { continue }
        # Проверяем адрес
        $addr = $bytes[$i+$opLen] -bor ($bytes[$i+$opLen+1] -shl 8) -bor ($bytes[$i+$opLen+2] -shl 16) -bor ($bytes[$i+$opLen+3] -shl 24)
        if($addr -eq 0x01BE9F9C) {
            $rva = 0x400000 + $i
            $hit = [pscustomobject]@{ Rva = ('0x{0:X}' -f $rva); Name = $p.Name }
            [void]$hits.Add($hit)
            Write-Host ('{0} @ RVA=0x{1:X}' -f $p.Name, $rva)
        }
    }
}
Write-Host ("Total: {0} hits" -f $hits.Count)
