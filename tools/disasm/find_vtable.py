# -*- coding: utf-8 -*-
"""Поиск vtable и её методов по RTTI-имени класса (MSVC RTTI, x86).

Цепочка: строковое имя типа (`.?AVcActCodecEnd@Trigger@@`) лежит внутри
TypeDescriptor: descriptor = name_va - 8. Затем ищем указатель на descriptor —
это поле pTypeDescriptor в CompleteObjectLocator (COL+0x0C) → COL. Затем ищем
указатель на COL — это слот vtable[-1] → vtable = (место указателя) + 4.
Печатает VA vtable и первых N методов как RVA.

    py -3 out\\find_vtable.py 0x18B18C4 [--slots 8]
"""
import argparse
import struct
import sys

EXE = (r"C:\Program Files (x86)\Steam\steamapps\common\Metal Gear Rising"
       r" Revengeance\METAL GEAR RISING REVENGEANCE.exe")
IMAGE_BASE = 0x400000


class PE:
    def __init__(self, data):
        self.data = data
        e = struct.unpack_from("<I", data, 0x3C)[0]
        coff = e + 4
        n, = struct.unpack_from("<H", data, coff + 2)
        opt_size, = struct.unpack_from("<H", data, coff + 16)
        opt = coff + 20
        self.base, = struct.unpack_from("<I", data, opt + 28)
        self.sections, so = [], opt + opt_size
        for _ in range(n):
            name = data[so:so + 8].rstrip(b"\0").decode(errors="replace")
            vsize, vaddr, rsize, rptr = struct.unpack_from("<IIII", data, so + 8)
            self.sections.append((name, vaddr, vsize, rptr, rsize))
            so += 40

    def off(self, va):
        rva = va - self.base
        for name, vaddr, vsize, rptr, rsize in self.sections:
            if vaddr <= rva < vaddr + max(vsize, rsize):
                return rptr + (rva - vaddr)
        return None

    def sec_of(self, va):
        rva = va - self.base
        for name, vaddr, vsize, rptr, rsize in self.sections:
            if vaddr <= rva < vaddr + max(vsize, rsize):
                return name
        return None

    def u32(self, va):
        o = self.off(va)
        return struct.unpack_from("<I", self.data, o)[0] if o is not None else None

    def pointers_to(self, target, sections=(".rdata", ".data")):
        out = []
        pat = struct.pack("<I", target)
        for name, vaddr, vsize, rptr, rsize in self.sections:
            if name not in sections:
                continue
            blob = self.data[rptr:rptr + rsize]
            start = 0
            while True:
                i = blob.find(pat, start)
                if i < 0:
                    break
                start = i + 1
                out.append(self.base + vaddr + i)
        return out


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("name_va", type=lambda s: int(s, 0))
    p.add_argument("--slots", type=int, default=8)
    a = p.parse_args(argv)
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    pe = PE(open(EXE, "rb").read())
    # проверим, что в name_va действительно строка
    o = pe.off(a.name_va)
    nm = pe.data[o:o + 64].split(b"\0")[0].decode("ascii", "replace")
    print(f"имя по адресу: {nm!r} (секция {pe.sec_of(a.name_va)})")
    desc = a.name_va - 8
    print(f"TypeDescriptor = 0x{desc:X} (секция {pe.sec_of(desc)})")
    cols = pe.pointers_to(desc)
    print(f"ссылок на descriptor: {[hex(c) for c in cols]}")
    for col_ptr in cols:
        col = col_ptr - 0x0C
        if pe.u32(col + 0x14) != 0:  # signature обычно 0
            pass
        vt_ptrs = pe.pointers_to(col)
        vt = vt_ptrs[0] + 4 if vt_ptrs else None
        print(f"  COL=0x{col:X} (0x{col:X} → check sig=0x{pe.u32(col):X}, "
              f"0x{pe.u32(col + 0x04):X}, 0x{pe.u32(col + 0x08):X})  "
              f"указателей на COL: {[hex(v) for v in vt_ptrs]}")
        if vt:
            print(f"  vtable = 0x{vt:X}")
            for i in range(a.slots):
                fn = pe.u32(vt + 4 * i)
                if fn is None:
                    break
                print(f"    slot[{i}] = 0x{fn:08X}  RVA 0x{fn - IMAGE_BASE:X}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
