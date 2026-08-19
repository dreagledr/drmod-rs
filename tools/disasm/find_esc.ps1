# Поиск push 0x8E (KEY_ESC) рядом с call isKeyDown (0x9D93A0)
$exePath = 'C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising Revengeance\METAL GEAR RISING REVENGEANCE.exe'
$bytes = [System.IO.File]::ReadAllBytes($exePath)

# Паттерн: 68 8E 00 00 00 (push 0x8E) — ищем все
$hits = New-Object System.Collections.ArrayList
for($i = 0; $i -lt $bytes.Length - 5; $i++) {
    if($bytes[$i] -eq 0x68 -and $bytes[$i+1] -eq 0x8E -and $bytes[$i+2] -eq 0x00 -and $bytes[$i+3] -eq 0x00 -and $bytes[$i+4] -eq 0x00) {
        $rva = 0x400000 + $i
        [void]$hits.Add($rva)
        Write-Host ('push 0x8E @ RVA=0x{0:X}' -f $rva)
    }
}
Write-Host ("Total: {0} hits" -f $hits.Count)
