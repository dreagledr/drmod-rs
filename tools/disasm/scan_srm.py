# -*- coding: utf-8 -*-
"""Поиск обращений к глобалам cSlowRateManager (ms_Instance) в exe игры.

Читаем PE-заголовок (ImageBase и таблицу секций) — в exe на диске файловое
смещение НЕ равно RVA (MSVC: .text VA 0x1000, raw 0x400), поэтому маппинг
считаем по секциям. Код игры читает поля абсолютной адресацией
(fld/fst/fadd [ImageBase + RVA поля]), поэтому ищем 4 байта VA поля.

Использование:
    py -3 tools/disasm/scan_srm.py [--exe PATH]
    py -3 tools/disasm/scan_srm.py --dump 0x9F8230 --len 0x100
"""
import argparse
import struct
import sys

EXE = (r"C:\Program Files (x86)\Steam\steamapps\common"
       r"\METAL GEAR RISING REVENGEANCE\METAL GEAR RISING REVENGEANCE.exe")
SRM_RVA = 0x17E93B0

FIELDS = {
    "ms_Instance  +0x00": SRM_RVA,
    "m_fTickRate  +0x7C": SRM_RVA + 0x7C,
    "m_fTicks     +0x80": SRM_RVA + 0x80,
    "field_84     +0x84": SRM_RVA + 0x84,
    "m_fTickDelay +0x88": SRM_RVA + 0x88,
    "m_fTickDiff  +0x8C": SRM_RVA + 0x8C,
    "field_90     +0x90": SRM_RVA + 0x90,
}

OPCODES = {
    (0xD9, 0x05): "fld",   (0xD9, 0x15): "fst",   (0xD9, 0x1D): "fstp",
    (0xD8, 0x05): "fadd",  (0xD8, 0x0D): "fmul",  (0xD8, 0x25): "fsub",
    (0xD8, 0x2D): "fsubr", (0xD8, 0x35): "fdiv",  (0xD8, 0x3D): "fdivr",
    (0xDC, 0x05): "fadd",  (0xDC, 0x0D): "fmul",  (0xDC, 0x25): "fsub",
    (0xDC, 0x35): "fdiv",  (0xDD, 0x05): "fld",   (0xDD, 0x1D): "fstp",
    (0xDA, 0x05): "fiadd", (0xDA, 0x25): "fisub", (0xDE, 0x05): "fiadd",
    (0xF3, 0x0F): "movss-ish",
}


class PE:
    def __init__(self, data):
        self.data = data
        e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
        assert data[e_lfanew:e_lfanew + 4] == b"PE\0\0"
        coff = e_lfanew + 4
        self.n_sections = struct.unpack_from("<H", data, coff + 2)[0]
        opt_size = struct.unpack_from("<H", data, coff + 16)[0]
        opt = coff + 20
        magic = struct.unpack_from("<H", data, opt)[0]
        self.is64 = magic == 0x20B
        self.image_base = struct.unpack_from("<I", data, opt + 28)[0] if not self.is64 \
            else struct.unpack_from("<Q", data, opt + 24)[0]
        sec = opt + opt_size
        self.sections = []
        for i in range(self.n_sections):
            off = sec + i * 40
            name = data[off:off + 8].rstrip(b"\0").decode("ascii", "replace")
            vsize, vaddr, rawsize, rawptr = struct.unpack_from("<IIII", data, off + 8)
            self.sections.append((name, vaddr, vsize, rawptr, rawsize))

    def raw(self, rva):
        for name, vaddr, vsize, rawptr, rawsize in self.sections:
            if vaddr <= rva < vaddr + max(vsize, rawsize):
                return rawptr + (rva - vaddr)
        return None

    def rva_of_raw(self, off):
        for name, vaddr, vsize, rawptr, rawsize in self.sections:
            if rawptr <= off < rawptr + rawsize:
                return vaddr + (off - rawptr)
        return None


def scan(pe):
    data = pe.data
    for name, rva in FIELDS.items():
        va = pe.image_base + rva
        pat = struct.pack("<I", va)
        hits = []
        start = 0
        while True:
            i = data.find(pat, start)
            if i < 0:
                break
            start = i + 1
            op = None
            for back in (1, 2, 3, 4, 5):
                if i - back - 1 < 0:
                    break
                key = (data[i - back], data[i - back + 1])
                if key in OPCODES:
                    op = OPCODES[key]
                    break
            hits.append((pe.rva_of_raw(i - (back)), op))
        print(f"\n=== {name} VA=0x{va:08X} ({len(hits)}) ===")
        for rva, op in hits:
            print(f"  RVA=0x{rva:X}  {op}")


def dump(pe, rva, length):
    off = pe.raw(rva)
    if off is None:
        print(f"RVA 0x{rva:X} вне секций")
        return
    data = pe.data
    end = min(off + length, len(data))
    for i in range(off, end, 16):
        chunk = data[i:i + 16]
        hexs = " ".join(f"{b:02X}" for b in chunk)
        print(f"0x{pe.rva_of_raw(i):08X}: {hexs:<48}")


def scan_disp(pe, offset):
    """Ищет x87/SSE обращения к [reg+offset] (disp8/disp32) и печатает RVA+опкод."""
    data = pe.data
    names_d8 = {0x00: "fadd", 0x08: "fmul", 0x10: "fcom", 0x18: "fcomp",
                0x20: "fsub", 0x28: "fsubr", 0x30: "fdiv", 0x38: "fdivr"}
    regs = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"]
    hits = []
    n = len(data)
    i = 0
    while i < n - 8:
        b = data[i]
        if b in (0xD8, 0xD9, 0xDC, 0xDD, 0xDE, 0xDF):
            modrm = data[i + 1]
            mod = modrm >> 6
            if mod in (1, 2):
                rm = modrm & 7
                if rm in (4, 5) and mod == 1:
                    i += 1
                    continue
                if mod == 1:
                    disp = struct.unpack_from("<b", data, i + 2)[0]
                    step = 3
                else:
                    disp = struct.unpack_from("<i", data, i + 2)[0]
                    step = 6
                if disp == offset:
                    if b == 0xD9:
                        op = {0x00: "fld", 0x10: "fst", 0x18: "fstp"}.get(
                            modrm & 0x38, "?")
                    elif b == 0xD8:
                        op = names_d8.get(modrm & 0x38, "?")
                    elif b == 0xDD:
                        op = {0x00: "fld", 0x18: "fstp"}.get(modrm & 0x38, "?")
                    else:
                        op = "?"
                    hits.append((pe.rva_of_raw(i), f"{op} [{regs[rm]}+0x{offset:X}]"))
                    i += step
                    continue
        i += 1
    print(f"\n=== [reg+0x{offset:X}] ({len(hits)}) ===")
    for rva, text in hits:
        print(f"  RVA=0x{rva:X}  {text}")


def scan_calls(pe, target_rva):
    """Ищет `call rel32` (E8) на target_rva и печатает RVA инструкции."""
    data = pe.data
    hits = []
    for i in range(len(data) - 5):
        if data[i] != 0xE8:
            continue
        rel = struct.unpack_from("<i", data, i + 1)[0]
        # файловое смещение → RVA: цель = RVA(i) + 5 + rel; используем RVA начала.
        start_rva = pe.rva_of_raw(i)
        if start_rva is None:
            continue
        if start_rva + 5 + rel == target_rva:
            hits.append(start_rva)
    print(f"\n=== call 0x{target_rva:X} ({len(hits)}) ===")
    for rva in hits:
        print(f"  RVA=0x{rva:X}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", default=EXE)
    ap.add_argument("--dump", type=lambda s: int(s, 0), default=None)
    ap.add_argument("--len", type=lambda s: int(s, 0), default=0x200)
    ap.add_argument("--find-disp", type=lambda s: int(s, 0), default=None)
    ap.add_argument("--calls", type=lambda s: int(s, 0), default=None)
    a = ap.parse_args()
    with open(a.exe, "rb") as f:
        data = f.read()
    pe = PE(data)
    print(f"ImageBase=0x{pe.image_base:X}, секций={pe.n_sections}")
    if a.dump is not None:
        dump(pe, a.dump, a.len)
        return 0
    if a.find_disp is not None:
        scan_disp(pe, a.find_disp)
        return 0
    if a.calls is not None:
        scan_calls(pe, a.calls)
        return 0
    scan(pe)
    return 0


if __name__ == "__main__":
    sys.exit(main())
