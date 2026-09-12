# -*- coding: utf-8 -*-
"""Обёртка над llvm-objdump для дизассемблирования exe игры.

Дизассемблирует диапазон по RVA (не VA!): `--rva 0xA03970 --len 0x140`.
Адрес для objdump = ImageBase + RVA. Путь к llvm-objdump берётся из rustup.

Примеры:
    py -3 tools/disasm/disasm.py --rva 0xA03970 --len 0x140
    py -3 tools/disasm/disasm.py --rva 0xA52510 --len 0x200
"""
import argparse
import glob
import os
import subprocess
import sys

EXE = (r"C:\Program Files (x86)\Steam\steamapps\common"
       r"\METAL GEAR RISING REVENGEANCE\METAL GEAR RISING REVENGEANCE.exe")
IMAGE_BASE = 0x400000


def find_objdump():
    pat = os.path.join(os.environ["USERPROFILE"], ".rustup", "toolchains", "*",
                       "lib", "rustlib", "*", "bin", "llvm-objdump.exe")
    hits = glob.glob(pat)
    if not hits:
        raise SystemExit("llvm-objdump.exe не найден в rustup toolchains")
    return hits[0]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--rva", type=lambda s: int(s, 0), required=True)
    ap.add_argument("--len", type=lambda s: int(s, 0), default=0x200)
    ap.add_argument("--exe", default=EXE)
    a = ap.parse_args()
    objdump = find_objdump()
    va = IMAGE_BASE + a.rva
    cmd = [objdump, "-d", "--x86-asm-syntax=intel",
           f"--start-address=0x{va:X}", f"--stop-address=0x{va + a.len:X}", a.exe]
    out = subprocess.run(cmd, capture_output=True, text=True, errors="replace")
    sys.stdout.write(out.stdout)
    if out.returncode != 0:
        sys.stderr.write(out.stderr)
    return out.returncode


if __name__ == "__main__":
    sys.exit(main())
