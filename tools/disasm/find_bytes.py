# -*- coding: utf-8 -*-
"""Поиск байтовых паттернов в .text exe игры (`?` — wildcard).

Нужен, когда прямых `call rel32` нет (функция вызывается косвенно), а найти
код можно только по константе в инструкции: `test [0x1bea060], 0x8000000`
(SOFT_EVENT), `mov [0x1be9f9c], 6` (CutscenePause) и т.п.

⚠️ В `mov`/`test [abs32]` операнд — это **file VA** = `ImageBase (0x400000)` +
RVA, т.е. для `base + 0x17EA060` искать `60 A0 BE 01`, а не `60 A0 17 01`
(вывод `py -3 out\\scan_addr.py` подтверждает: ссылки на `0x1BEA060`).

    py -3 tools\\disasm\\find_bytes.py "F7 05 60 A0 BE 01 00 00 00 08"
    py -3 tools\\disasm\\find_bytes.py "C7 05 9C 9F BE 01 06 00 00 00" --len 0x40
"""
import argparse
import struct
import sys

EXE = (r"C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising"
       r" Revengeance\METAL GEAR RISING REVENGEANCE.exe")


def parse(pat):
    return [None if tok in ("?", "??") else int(tok, 16) for tok in pat.split()]


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("patterns", nargs="+")
    p.add_argument("--len", dest="length", type=lambda s: int(s, 0), default=0,
                   help="печатать столько байт после совпадения")
    p.add_argument("--max", type=int, default=40)
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
    for pat_str in a.patterns:
        pat = parse(pat_str)
        first = next((i for i, b in enumerate(pat) if b is not None), 0)
        hits, i = [], 0
        while True:
            i = blob.find(bytes([pat[first]]), i)
            if i < 0:
                break
            pos = i - first
            if pos >= 0 and all(b is None or blob[pos + j] == b
                                for j, b in enumerate(pat)):
                hits.append(pos)
            i += 1
        print(f"\n=== {pat_str!r}: {len(hits)} ===")
        for pos in hits[:a.max]:
            rva = text[1] + pos
            print(f"  RVA=0x{rva:08X}")
            for j in range(0, a.length, 16):
                chunk = blob[pos + j:pos + j + 16]
                print(f"    {rva + j:08X}: "
                      + " ".join(f"{x:02X}" for x in chunk))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
