# -*- coding: utf-8 -*-
"""Быстрый снимок «текущей фазы/подфазы» из глобалов игры (только чтение).

Глобалы (RVA): 0x14B9140 — блок чекпойнта (phase id, хэш подфазы, имя),
0x14B9170 — блок текущей подфазы. Печатает их одной строкой.

    py -3 tools\\script_tuning\\subphase_now.py
"""
import ctypes
import ctypes.wintypes as wt
import struct
import sys

WIN_TITLE = "METAL GEAR RISING: REVENGEANCE"
CHK = 0x14B9140
CUR = 0x14B9170

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


def rd(h, addr, size):
    buf = ctypes.create_string_buffer(size)
    n = ctypes.c_size_t()
    if not k32.ReadProcessMemory(h, ctypes.c_void_p(addr), buf, size,
                                 ctypes.byref(n)):
        return None
    return buf.raw[:n.value]


def block(h, mod, rva):
    b = rd(h, mod + rva, 0x2C)
    if not b or len(b) < 0x2C:
        return "нет"
    a, c, pid, hsh = struct.unpack_from("<IIII", b, 0)
    # имя лежит на +0x0C (блок «текущей») или на +0x10 (блок чекпойнта) —
    # берём тот вариант, который похож на имя подфазы (A-Z0-9_)
    name = ""
    for off in (0x0C, 0x10):
        cand = b[off:off + 0x10].split(b"\0")[0].decode("cp1251", "replace")
        if cand[:1].isalnum():
            name = cand
            break
    return f"[{a},{c}] phase=0x{pid:X} sub=0x{hsh:08X} name={name!r}"


def main():
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    h, mod = open_game()
    print("чекпойнт :", block(h, mod, CHK))
    print("текущая  :", block(h, mod, CUR))
    return 0


if __name__ == "__main__":
    sys.exit(main())
