# -*- coding: utf-8 -*-
"""Консольное меню скипа катсцены + переход по скипу (штатным путём движка).

Пока идёт заданная подфаза (по умолчанию `P370_IN` — монолог Монсуна):

1. держит выставленными флаги `STA_SOFT_EVENT` (код 4) и
   `STA_SOFT_EVENT_SKIP_OK` (код 37) в `staFlags` (`base + 0x17EA060`) — от них
   Esc открывает **консольное катсценное меню с пунктом Skip** (проверено:
   `GameMenuStatus` = `Cutscene Pause`, 6);
2. как только в этом меню (`GameMenuStatus == 6`) нажат confirm (бит 0x10 в
   `InputUnit.buttons_pressed`, `base + 0x177B850` + 0x04) — заказывает
   следующую подфазу через `POST /order` (движковая `request_subphase`), причём
   с `clear_event` (снять `STA_EVENT`, иначе внутри события заказ игнорируется).

При выходе из сцены флаги снимаются (вернуть состояние движка как было).

    py -3 -u tools\\script_tuning\\cutscene_skip.py                # P370_IN → P370_EVENT
    py -3 -u tools\\script_tuning\\cutscene_skip.py --watch P370_IN --next P370_EVENT
"""
import argparse
import ctypes
import ctypes.wintypes as wt
import struct
import sys
import time
import zlib

sys.path.insert(0, r"D:\pet\drmod-rs\tools\script_tuning")
import drmod_api as api  # noqa: E402

WIN_TITLE = "METAL GEAR RISING: REVENGEANCE"
MENU = 0x17E9F9C
STA = 0x17EA060
SUB_HASH = 0x14B9178        # текущая подфаза: хэш (объект состояния +0x38)
INPUT_UNIT = 0x177B850      # cInput::g_InputUnit0
BUTTONS_DOWN = 0x00         # m_nButtonsDown
BUTTONS_PRESSED = 0x04      # m_nButtonsPressed
CONFIRM_BIT = 0x10          # confirm == JUMP (бит BUTTON_A)
MENU_CUTSCENE = 6           # GameMenuStatus::CutscenePause
SOFT_EVENT = 0x08000000     # код 4
SKIP_OK = 0x04000000        # код 37, но во ВТОРОМ dword staFlags

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
u32 = ctypes.WinDLL("user32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)


def open_game():
    hwnd = u32.FindWindowW(None, WIN_TITLE)
    if not hwnd:
        raise SystemExit("окно игры не найдено")
    pid = wt.DWORD()
    u32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    h = k32.OpenProcess(0x0008 | 0x0010 | 0x0020 | 0x0400, False, pid.value)
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


def u32v(h, addr):
    b = rd(h, addr, 4)
    return struct.unpack("<I", b)[0] if b and len(b) == 4 else None


def wr_u32(h, addr, value):
    return bool(k32.WriteProcessMemory(h, ctypes.c_void_p(addr),
                                       struct.pack("<I", value), 4,
                                       ctypes.byref(ctypes.c_size_t())))


def hash_of(name):
    return zlib.crc32(name.lower().encode()) & 0x7FFFFFFF


def clear_flags(h, mod):
    """Снять наши флаги катсценного меню (вернуть состояние движка как было)."""
    d0 = u32v(h, mod + STA) or 0
    d1 = u32v(h, mod + STA + 4) or 0
    wr_u32(h, mod + STA, d0 & ~SOFT_EVENT)
    wr_u32(h, mod + STA + 4, d1 & ~SKIP_OK)


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("--watch", default="P370_IN", help="подфаза сцены (монолог)")
    p.add_argument("--next", default="P370_EVENT", help="куда переводить по скипу")
    p.add_argument("--timeout", type=float, default=1800.0)
    p.add_argument("--hz", type=float, default=10.0)
    a = p.parse_args(argv)
    api.setup_stdout()
    h, mod = open_game()
    watch_h = hash_of(a.watch)
    print(f"module base = 0x{mod:08X}; сцена {a.watch} (0x{watch_h:08X}) → "
          f"по скипу в {a.next}", flush=True)
    t0 = time.perf_counter()
    armed = False
    flags_set = False
    prev = None
    prev_confirm = False
    while time.perf_counter() - t0 < a.timeout:
        cur = u32v(h, mod + SUB_HASH)
        menu = u32v(h, mod + MENU)
        down = u32v(h, mod + INPUT_UNIT + BUTTONS_DOWN)
        pressed = u32v(h, mod + INPUT_UNIT + BUTTONS_PRESSED)
        if cur is None or menu is None or down is None or pressed is None:
            time.sleep(0.5)
            continue
        confirm = bool((down & CONFIRM_BIT) or (pressed & CONFIRM_BIT))
        confirm_edge = confirm and not prev_confirm
        prev_confirm = confirm
        in_scene = cur == watch_h
        if in_scene and not flags_set:
            d0 = u32v(h, mod + STA) or 0
            d1 = u32v(h, mod + STA + 4) or 0
            wr_u32(h, mod + STA, d0 | SOFT_EVENT)
            wr_u32(h, mod + STA + 4, d1 | SKIP_OK)
            flags_set = True
            print(f"[{time.perf_counter() - t0:6.1f}s] флаги катсценного меню "
                  f"выставлены (SOFT_EVENT + SKIP_OK)", flush=True)
        elif in_scene and flags_set and (time.perf_counter() * a.hz) % 3 < 1:
            # игра может сбрасывать флаги — поддерживаем их
            d0 = u32v(h, mod + STA) or 0
            d1 = u32v(h, mod + STA + 4) or 0
            if not (d0 & SOFT_EVENT) or not (d1 & SKIP_OK):
                wr_u32(h, mod + STA, d0 | SOFT_EVENT)
                wr_u32(h, mod + STA + 4, d1 | SKIP_OK)
        elif not in_scene and flags_set:
            clear_flags(h, mod)
            flags_set = False
            print(f"[{time.perf_counter() - t0:6.1f}s] сцена кончилась — флаги "
                  f"снял", flush=True)
        key = (menu, in_scene)
        if key != prev:
            print(f"[{time.perf_counter() - t0:6.1f}s] menu={menu} "
                  f"sub=0x{cur:08X} in_scene={in_scene}", flush=True)
            prev = key
        if menu == MENU_CUTSCENE:
            if not armed:
                print(f"[{time.perf_counter() - t0:6.1f}s] КОНСОЛЬНОЕ МЕНЮ "
                      f"ОТКРЫТО (Cutscene Pause) — ждём confirm", flush=True)
                armed = True
            if confirm_edge:
                print(f"[{time.perf_counter() - t0:6.1f}s] confirm в "
                      f"катсценном меню → заказ {a.next}", flush=True)
                try:
                    resp = api.http(api.DEFAULT_URL, "/order", "POST",
                                    {"name": a.next, "arg": 1,
                                     "clear_event": True})
                    print("  ответ /order:", resp, flush=True)
                except Exception as e:  # noqa: BLE001
                    print("  /order ошибка:", e, flush=True)
                # закрыть меню, иначе игра остаётся на паузе и загрузка не пойдёт
                try:
                    api.run_script({"name": "close-cutscene-menu",
                                    "commands": [{"t": 0, "duration": 3,
                                                  "input": {"pause": True}}]})
                    print("  меню закрываю (бит pause)", flush=True)
                except Exception as e:  # noqa: BLE001
                    print("  pause ошибка:", e, flush=True)
                for i in range(10):
                    time.sleep(3)
                    try:
                        st = api.state()
                        pl = st.get("player") or {}
                        print(f"  [{3 * (i + 1):3d}s] menu={st.get('menu_status')} "
                              f"phase={st.get('mission_name')} "
                              f"found={pl.get('found')} pos={pl.get('pos')}",
                              flush=True)
                    except Exception as e:  # noqa: BLE001
                        print(f"  [{3 * (i + 1):3d}s] /state: {e}", flush=True)
                # уборка перед выходом: снять наши флаги (иначе катсценное меню
                # останется включённым и в следующих сценах)
                clear_flags(h, mod)
                print("  флаги катсценного меню сняты (уборка)", flush=True)
                break
        else:
            armed = False
        time.sleep(1.0 / a.hz)
    else:
        print("таймаут — confirm в катсценном меню не случился", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
