# -*- coding: utf-8 -*-
"""Вызывающие функции: все `call rel32` на заданный RVA + что лежит на стеке.

Дополняет `scan_srm.py --calls` (тот просто ищет `call rel32`): здесь ещё
печатается «недавний `push`» перед вызовом, поэтому сразу видно аргумент —
например, `staIsSet` (`RVA 0x16910`) с `push 0x25` = код 37
(`STA_SOFT_EVENT_SKIP_OK`), что и вывело на обработчик паузы (`RVA 0x84204D`).

    py -3 tools\\disasm\\callers_of.py 0x16910 --back 20
    py -3 tools\\disasm\\callers_of.py 0x16910 --push4 --show 0x40
"""
import argparse
import struct
import sys

EXE = (r"C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising"
       r" Revengeance\METAL GEAR RISING REVENGEANCE.exe")


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("rva", type=lambda s: int(s, 0))
    p.add_argument("--back", type=int, default=24,
                   help="сколько байт перед вызовом просматривать")
    p.add_argument("--show", type=lambda s: int(s, 0), default=0)
    a = p.parse_args(argv)
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    data = open(EXE, "rb").read()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    coff = pe + 4
    num_sec, = struct.unpack_from("<H", data, coff + 2)
    size_opt, = struct.unpack_from("<H", data, coff + 16)
    opt = coff + 20
    secs, so = [], opt + size_opt
    for _ in range(num_sec):
        name = data[so:so + 8].rstrip(b"\0").decode(errors="replace")
        vsize, vaddr, rsize, rptr = struct.unpack_from("<IIII", data, so + 8)
        secs.append((name, vaddr, vsize, rptr, rsize))
        so += 40
    text = next(s for s in secs if s[0] == ".text")
    start, size = text[3], min(text[2], text[4])
    blob = data[start:start + size]
    target = a.rva
    hits = []
    for pos in range(len(blob) - 5):
        if blob[pos] != 0xE8:
            continue
        rel = struct.unpack_from("<i", blob, pos + 1)[0]
        if text[1] + pos + 5 + rel == target:
            hits.append(pos)
    print(f"call-сайтов 0x{target:08X}: {len(hits)}")
    for pos in hits:
        call_rva = text[1] + pos
        back = blob[max(0, pos - a.back):pos]
        pushes, j = [], len(back) - 1
        while j >= 1:
            if back[j] == 0x6A:
                pushes.insert(0, back[j + 1])
                j -= 2
                continue
            if back[j - 1] == 0x68 and j >= 4:
                pushes.insert(0, struct.unpack_from("<I", back, j - 3)[0] & 0xFF)
                j -= 5
                continue
            j -= 1
        print(f"  RVA=0x{call_rva:08X}  push(недавние)={pushes}")
        if a.show:
            for k in range(0, a.show, 16):
                chunk = blob[pos + k:pos + k + 16]
                print(f"     {call_rva + k:08X}: "
                      + " ".join(f"{x:02X}" for x in chunk))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
