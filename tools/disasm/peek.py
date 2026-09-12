# -*- coding: utf-8 -*-
"""Чтение dword/float по RVA модуля игры (диагностика).

    py -3 out\\peek.py 0x18B9174 0x17EA060 0x17EA064 --float 0x17E93B0
"""
import argparse
import ctypes
import ctypes.wintypes as wt
import struct
import sys

WIN_TITLE = "METAL GEAR RISING: REVENGEANCE"
k32 = ctypes.WinDLL("kernel32", use_last_error=True)
u32 = ctypes.WinDLL("user32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)


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


def rd(h, addr, size=4):
    buf = ctypes.create_string_buffer(size)
    n = ctypes.c_size_t()
    if not k32.ReadProcessMemory(h, ctypes.c_void_p(addr), buf, size,
                                 ctypes.byref(n)):
        return None
    return buf.raw[:n.value]


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("rvas", nargs="*", type=lambda s: int(s, 0))
    p.add_argument("--float", dest="floats", nargs="*", type=lambda s: int(s, 0),
                   default=[])
    p.add_argument("--str", dest="strs", nargs="*", type=lambda s: int(s, 0),
                   default=[])
    p.add_argument("--dump", type=lambda s: int(s, 0), default=None)
    p.add_argument("--abs", type=lambda s: int(s, 0), default=None,
                   help="абсолютный адрес (не RVA) для --dump")
    p.add_argument("--len", dest="length", type=lambda s: int(s, 0), default=0x80)
    a = p.parse_args(argv)
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    h, mod = open_game()
    print(f"module base = 0x{mod:08X}")
    if a.dump is not None or a.abs is not None:
        addr = a.abs if a.abs is not None else mod + a.dump
        b = rd(h, addr, a.length)
        if not b:
            print(f"  дамп 0x{addr:X} не прочитался")
        else:
            for i in range(0, len(b), 16):
                chunk = b[i:i + 16]
                hexs = " ".join(f"{x:02X}" for x in chunk)
                text = "".join(chr(x) if 32 <= x < 127 else "." for x in chunk)
                print(f"  0x{addr + i:08X}: {hexs:<48} {text}")
    for rva in a.rvas:
        b = rd(h, mod + rva)
        v = struct.unpack("<I", b)[0] if b else None
        s = struct.unpack("<i", b)[0] if b else None
        print(f"  0x{rva:08X}: raw=0x{v:08X} u={v} i={s}" if b else
              f"  0x{rva:08X}: чтение не удалось")
    for rva in a.floats:
        b = rd(h, mod + rva)
        print(f"  0x{rva:08X} (float): {struct.unpack('<f', b)[0] if b else None}")
    for rva in a.strs:
        b = rd(h, mod + rva, 32)
        print(f"  0x{rva:08X} (str): {b.split(b'\\0')[0].decode('cp1251', 'replace') if b else None!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
