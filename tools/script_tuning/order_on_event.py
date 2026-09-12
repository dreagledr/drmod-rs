# -*- coding: utf-8 -*-
"""Ждёт, пока пойдёт сцена-событие (STA_EVENT в геймплее), и заказывает подфазу.

⚠️ Заказ отрабатывает (движок реально выгружает и загружает подфазу) только
когда игра **не на паузе**: при выставленном `STA_PAUSE` (код 19) машина загрузки
стоит, и подфаза меняет лишь подпись (проверено 2026-09-12: с открытым катсценным
меню сцена не менялась, а после снятия `STA_PAUSE` заказ отработал — сцена
сменилась, объект состояния показал `P370_EVENT`). Поэтому ждём сцену И отсутствие
паузы; `--allow-pause` снимает это условие (для диагностики).

    py -3 -u tools\\script_tuning\\order_on_event.py P370_EVENT [--timeout 600]
"""
import argparse
import ctypes
import ctypes.wintypes as wt
import struct
import sys
import time

import drmod_api as api

WIN_TITLE = "METAL GEAR RISING: REVENGEANCE"
STA = 0x17EA060
MENU = 0x17E9F9C
STA_EVENT_MASK = 0x4000_0000    # код 1? (наблюдённое значение при сцене)
STA_PAUSE_MASK = 0x0000_1000    # код 19: игра на паузе

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


def u32v(h, addr):
    buf = ctypes.create_string_buffer(4)
    n = ctypes.c_size_t()
    if not k32.ReadProcessMemory(h, ctypes.c_void_p(addr), buf, 4, ctypes.byref(n)):
        return None
    return struct.unpack("<I", buf.raw)[0]


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("name")
    p.add_argument("--arg", type=lambda s: int(s, 0), default=1)
    p.add_argument("--timeout", type=float, default=600.0)
    p.add_argument("--allow-pause", action="store_true",
                   help="заказывать даже при STA_PAUSE (загрузка не сработает)")
    a = p.parse_args(argv)
    api.setup_stdout()
    h, mod = open_game()
    print(f"module base = 0x{mod:08X}; жду сцену (STA_EVENT) без паузы, "
          f"затем закажу {a.name}", flush=True)
    t0 = time.time()
    fired = False
    while time.time() - t0 < a.timeout:
        flags = u32v(h, mod + STA)
        menu = u32v(h, mod + MENU)
        if flags is None or menu is None:
            time.sleep(0.5)
            continue
        paused = bool(flags & STA_PAUSE_MASK)
        if (flags & STA_EVENT_MASK and menu != 3
                and (a.allow_pause or not paused) and not fired):
            print(f"[{time.time() - t0:5.1f}s] сцена идёт (STA=0x{flags:08X}, "
                  f"menu={menu}, пауза={paused}) → заказ {a.name}", flush=True)
            try:
                resp = api.http(api.DEFAULT_URL, "/order", "POST",
                                {"name": a.name, "arg": a.arg,
                                 "clear_event": True})
                print("  ответ /order:", resp, flush=True)
            except Exception as e:  # noqa: BLE001
                print("  /order ошибка:", e, flush=True)
            fired = True
            time.sleep(6)
            for i in range(10):
                try:
                    st = api.state()
                    pl = st.get("player") or {}
                    print(f"  [{6 + 3 * i:3d}s] menu={st.get('menu_status')} "
                          f"phase={st.get('mission_name')} pos={pl.get('pos')} "
                          f"found={pl.get('found')}", flush=True)
                except Exception as e:  # noqa: BLE001
                    print(f"  [{6 + 3 * i:3d}s] /state недоступен: {e}", flush=True)
                time.sleep(3)
            break
        time.sleep(0.4)
    if not fired:
        print("сцена не началась за отведённое время — заказ не отправлен",
              flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
