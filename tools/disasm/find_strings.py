# -*- coding: utf-8 -*-
"""Поиск ASCII-строк в exe игры по подстроке (регистронезависимо).

    py -3 out\\find_strings.py skip event cutscene pause resume
"""
import re
import struct
import sys

EXE = (r"C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising"
       r" Revengeance\METAL GEAR RISING REVENGEANCE.exe")


def main(argv):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    data = open(EXE, "rb").read()
    e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
    coff = e_lfanew + 4
    num_sec, = struct.unpack_from("<H", data, coff + 2)
    size_opt, = struct.unpack_from("<H", data, coff + 16)
    opt = coff + 20
    image_base, = struct.unpack_from("<I", data, opt + 28)
    secs, so = [], opt + size_opt
    for _ in range(num_sec):
        name = data[so:so + 8].rstrip(b"\0").decode(errors="replace")
        vsize, vaddr, rsize, rptr = struct.unpack_from("<IIII", data, so + 8)
        secs.append((name, vaddr, vsize, rptr, rsize))
        so += 40

    def va_of(off):
        for name, vaddr, vsize, rptr, rsize in secs:
            if rptr <= off < rptr + rsize:
                return image_base + vaddr + (off - rptr), name
        return None, None

    for word in argv:
        pat = re.compile(re.escape(word.encode()), re.I)
        hits = list(pat.finditer(data))
        print(f"\n=== {word!r}: {len(hits)} ===")
        shown = set()
        for m in hits[:200]:
            s = m.start()
            start = s
            while start > 0 and 32 <= data[start - 1] < 127:
                start -= 1
            end = s
            while end < len(data) and 32 <= data[end] < 127:
                end += 1
            text = data[start:end].decode("ascii", "replace")
            if len(text) > 80:
                text = text[:80]
            va, sec = va_of(start)
            key = (sec, text)
            if key in shown:
                continue
            shown.add(key)
            print(f"  VA=0x{va:08X} ({sec}) {text!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
