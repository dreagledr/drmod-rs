# -*- coding: utf-8 -*-
"""Поиск 4-байтных значений (u32) в памяти игры.

    py -3 out\\mem_find_u32.py 0x3c9a2f06 0x6915135d 0x70754d29
    py -3 out\\mem_find_u32.py 0x3c9a2f06 --context 0x40
"""
import argparse
import ctypes
import ctypes.wintypes as wt
import struct
import sys

WIN_TITLE = "METAL GEAR RISING: REVENGEANCE"
MEM_COMMIT = 0x1000
PAGE_GUARD = 0x100
READABLE = (0x02, 0x04, 0x20, 0x40)

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
u32 = ctypes.WinDLL("user32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)


class MBI(ctypes.Structure):
    _fields_ = [("BaseAddress", ctypes.c_void_p),
                ("AllocationBase", ctypes.c_void_p),
                ("AllocationProtect", wt.DWORD),
                ("RegionSize", ctypes.c_size_t),
                ("State", wt.DWORD),
                ("Protect", wt.DWORD),
                ("Type", wt.DWORD)]


def open_game():
    hwnd = u32.FindWindowW(None, WIN_TITLE)
    if not hwnd:
        raise SystemExit("окно игры не найдено")
    pid = wt.DWORD()
    u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    h = k32.OpenProcess(0x0010 | 0x0400, False, pid.value)
    mods = (wt.HMODULE * 64)()
    need = wt.DWORD()
    psapi.EnumProcessModulesEx(h, mods, ctypes.sizeof(mods), ctypes.byref(need), 0x01)
    return h, int(mods[0])


def regions(h):
    addr, mbi = 0, MBI()
    while addr < 0x7FFF0000:
        if not k32.VirtualQueryEx(h, ctypes.c_void_p(addr), ctypes.byref(mbi),
                                  ctypes.sizeof(mbi)):
            break
        base = int(mbi.BaseAddress or 0)
        if (mbi.State == MEM_COMMIT and mbi.RegionSize > 0
                and int(mbi.Protect) & 0xFF in READABLE
                and not int(mbi.Protect) & PAGE_GUARD):
            yield base, int(mbi.RegionSize)
        addr = base + max(int(mbi.RegionSize), 0x1000)


def read(h, base, size):
    buf = ctypes.create_string_buffer(size)
    n = ctypes.c_size_t()
    if not k32.ReadProcessMemory(h, ctypes.c_void_p(base), buf, size,
                                 ctypes.byref(n)):
        return None
    return buf.raw[:n.value]


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("values", nargs="+", type=lambda s: int(s, 0))
    p.add_argument("--max-mb", type=int, default=3072)
    p.add_argument("--context", type=lambda s: int(s, 0), default=0)
    a = p.parse_args(argv)
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    h, mod = open_game()
    print(f"module base = 0x{mod:08X}; ищу "
          + ", ".join(f"0x{v:08X}" for v in a.values))
    needles = {struct.pack("<I", v): v for v in a.values}
    budget, scanned, hits = a.max_mb * 1024 * 1024, 0, 0
    for base, size in regions(h):
        if scanned >= budget:
            break
        size = min(size, budget - scanned)
        data = read(h, base, size)
        if data is None:
            continue
        scanned += len(data)
        for needle, value in needles.items():
            start = 0
            while True:
                i = data.find(needle, start)
                if i < 0:
                    break
                start = i + 1
                va = base + i
                loc = (f"base+0x{va - mod:X}" if mod <= va < mod + 0x20000000
                       else f"0x{va:08X}")
                print(f"  0x{value:08X} → {loc}")
                hits += 1
                if a.context:
                    ctx = data[max(0, i - a.context):i + a.context]
                    for j in range(0, len(ctx), 16):
                        chunk = ctx[j:j + 16]
                        print("      " + " ".join(f"{x:02X}" for x in chunk))
    print(f"просмотрено {scanned / 1048576:.0f} МиБ, вхождений: {hits}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
