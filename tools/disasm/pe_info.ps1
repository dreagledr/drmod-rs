# Чтение PE-заголовка: ImageBase, SectionAlignment, FileAlignment, секции
$exePath = 'C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising Revengeance\METAL GEAR RISING REVENGEANCE.exe'
$fs = [System.IO.File]::OpenRead($exePath)
$br = New-Object System.IO.BinaryReader($fs)

$fs.Seek(0x3C, [System.IO.SeekOrigin]::Begin) | Out-Null
$peOffset = $br.ReadUInt32()

$fs.Seek($peOffset + 4, [System.IO.SeekOrigin]::Begin) | Out-Null
$machine = $br.ReadUInt16()
$numSections = $br.ReadUInt16()
$timeDate = $br.ReadUInt32()
$ptrSym = $br.ReadUInt32()
$numSym = $br.ReadUInt32()
$sizeOpt = $br.ReadUInt16()
$chars = $br.ReadUInt16()

# Optional header
$fs.Seek($peOffset + 24, [System.IO.SeekOrigin]::Begin) | Out-Null
$magic = $br.ReadUInt16()
Write-Host ("Machine: 0x{0:X}  Sections: {1}  OptMagic: 0x{2:X}" -f $machine, $numSections, $magic)
if($magic -eq 0x10B) { $is64 = $false } else { $is64 = $true }
Write-Host ("Is64: {0}" -f $is64)

# ImageBase
$fs.Seek($peOffset + 24 + 28, [System.IO.SeekOrigin]::Begin) | Out-Null
if($is64) { $imageBase = $br.ReadUInt64() } else { $imageBase = $br.ReadUInt32() }
Write-Host ("ImageBase: 0x{0:X}" -f $imageBase)

# SizeOfImage
$fs.Seek($peOffset + 24 + 56, [System.IO.SeekOrigin]::Begin) | Out-Null
if($is64) { $fs.Seek(8, [System.IO.SeekOrigin]::Current) | Out-Null }
$sizeOfImage = $br.ReadUInt32()
Write-Host ("SizeOfImage: 0x{0:X}" -f $sizeOfImage)

# Section headers
$sectionOffset = $peOffset + 24 + $sizeOpt
$fs.Seek($sectionOffset, [System.IO.SeekOrigin]::Begin) | Out-Null
for($i = 0; $i -lt $numSections; $i++) {
    $nameBytes = $br.ReadBytes(8)
    $name = [System.Text.Encoding]::ASCII.GetString($nameBytes).TrimEnd([char]0)
    $vsize = $br.ReadUInt32()
    $vaddr = $br.ReadUInt32()
    $rawSize = $br.ReadUInt32()
    $rawPtr = $br.ReadUInt32()
    Write-Host ("  {0,-8} VA=0x{1:X8} VSize=0x{2:X8} RawPtr=0x{3:X8} RawSize=0x{4:X8}" -f $name, $vaddr, $vsize, $rawPtr, $rawSize)
    $fs.Seek(16, [System.IO.SeekOrigin]::Current) | Out-Null
}
$br.Close()
$fs.Close()
