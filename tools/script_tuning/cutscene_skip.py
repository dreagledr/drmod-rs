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

⚠️ Заказ подфазы меняет только подпись (объект состояния `+0x38`) — сцену он НЕ
грузит: после `/order` позиция игрока стоит на месте (проверено 2026-09-12,
`out/cutscene_skip5.log`). Реальную загрузку делает либо собственный пункт Skip
меню (нажатие игрока), либо `--restart` (рестарт чекпойнта).

⚠️ Меню открывает только физический Esc игрока: подача паузы из мода
(`pause`/`dik_key`/`raw_key`) в сцене-событии меню не открывает (проверено
2026-09-12, `out/menu_input_test.py` — `GameMenuStatus` оставался 1).

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
CHK_HASH = 0x14B914C        # чекпойнт-запись: хэш подфазы (объект состояния +0x0C)
INPUT_UNIT = 0x177B850      # cInput::g_InputUnit0
INPUT_KEYS = 0x19D06F8      # cInput::ms_InputKeys (DIK-буфер клавиатуры)
MENU_OBJ = 0x1BEA140        # указатель на живой объект меню (cEventPauseMenu)
DIK_ESCAPE = 0x01
DIK_RETURN = 0x1C
BUTTONS_DOWN = 0x00         # m_nButtonsDown
BUTTONS_PRESSED = 0x04      # m_nButtonsPressed
CONFIRM_BIT = 0x10          # confirm == JUMP (бит BUTTON_A)
PAUSE_BIT = 0x100           # pause/START в нормализованном вводе игры
MENU_CUTSCENE = 6           # GameMenuStatus::CutscenePause
MENU_IN_GAME = 1            # GameMenuStatus::InGame
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


def i32v(h, addr):
    b = rd(h, addr, 4)
    return struct.unpack("<i", b)[0] if b and len(b) == 4 else None


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
    p.add_argument("--watch", default="P370_RESTART,P370_IN",
                   help="подфазы сцены через запятую (сцена идёт от RESTART к IN)")
    p.add_argument("--next", default="P370_EVENT", help="куда переводить по скипу")
    p.add_argument("--restart", action="store_true",
                   help="после заказа рестартовать чекпойнт: он-то и грузит "
                        "сцену, но загрузки/рестарты иногда роняют игру")
    p.add_argument("--observe", action="store_true",
                   help="ничего не заказывать: только логировать состояние меню "
                        "и объекта (курсор/подтверждённый пункт) — режим «игрок "
                        "сам жмёт Skip, а мы смотрим, что делает движок»")
    p.add_argument("--timeout", type=float, default=1800.0,
                   help="сколько секунд ждать скипа, прежде чем выйти")
    p.add_argument("--hz", type=float, default=50.0,
                   help="частота опроса (клавиатурный Esc держится недолго)")
    a = p.parse_args(argv)
    api.setup_stdout()
    h, mod = open_game()
    watch_h = {hash_of(n) for n in a.watch.split(",") if n.strip()}
    print(f"module base = 0x{mod:08X}; сцена {a.watch} "
          f"({', '.join(f'0x{h:08X}' for h in watch_h)}) → "
          f"по скипу в {a.next}", flush=True)
    t0 = time.perf_counter()
    armed = False
    flags_set = False
    prev = None
    prev_confirm = False
    prev_esc = False
    prev_obj = None
    last_pause = 0.0
    while time.perf_counter() - t0 < a.timeout:
        cur = u32v(h, mod + SUB_HASH)
        menu = u32v(h, mod + MENU)
        down = u32v(h, mod + INPUT_UNIT + BUTTONS_DOWN)
        pressed = u32v(h, mod + INPUT_UNIT + BUTTONS_PRESSED)
        keys = rd(h, mod + INPUT_KEYS, 256) or b""
        if cur is None or menu is None or down is None or pressed is None:
            time.sleep(0.5)
            continue
        esc = len(keys) > DIK_ESCAPE and keys[DIK_ESCAPE] != 0
        ret = len(keys) > DIK_RETURN and keys[DIK_RETURN] != 0
        esc_edge = esc and not prev_esc
        prev_esc = esc
        confirm = bool((down & CONFIRM_BIT) or (pressed & CONFIRM_BIT) or ret)
        confirm_edge = confirm and not prev_confirm
        prev_confirm = confirm
        # Клавиатурный Esc игра маппит в pause-бит 0x100 (виден в InputUnit.down),
        # но «фронта» (pressed) для клавиатуры движок не выставляет — а его
        # проверка паузы в событии хочет именно фронт. Поэтому смотрим бит 0x100
        # (или сырой Esc) и сами подаём pause-скрипт: он выставляет и down, и
        # фронт, и движок открывает консольное катсценное меню.
        pause_wanted = bool(down & PAUSE_BIT) or esc
        in_scene = cur in watch_h
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
        # Подаём pause-бит, пока игрок держит паузу (Esc), а меню ещё закрыто:
        # движок откроет своё консольное катсценное меню, и условие само
        # перестанет выполняться (меню != InGame) — естественный лимит.
        # ⚠️ Проверено (2026-09-12, out/menu_input_test.py): в сцене-событии
        # эта подача меню НЕ открывает — открывает только физический Esc игрока
        # (игрок должен нажать Esc сам, инструмент лишь держит флаги).
        now = time.perf_counter()
        if (pause_wanted and in_scene and menu == MENU_IN_GAME
                and now - last_pause >= 0.5):
            last_pause = now
            try:
                api.run_script({"name": "esc-as-pause",
                                "commands": [{"t": 0, "duration": 2,
                                              "input": {"pause": True}}]})
                print(f"[{now - t0:6.1f}s] пауза с клавиатуры → pause-бит "
                      f"(движок откроет консольное меню)", flush=True)
            except Exception as e:  # noqa: BLE001
                print(f"  Esc→pause ошибка: {e}", flush=True)
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
            # Живой объект меню — cEventPauseMenu (base + 0x1BEA140):
            # +0x38 курсор, +0x3C индекс подтверждённого пункта (до confirm -1).
            obj = u32v(h, mod + MENU_OBJ)
            snap = (obj, i32v(h, obj + 0x38) if obj else None,
                    i32v(h, obj + 0x3C) if obj else None)
            if snap != prev_obj:
                print(f"[{time.perf_counter() - t0:6.1f}s] меню 0x{snap[0] or 0:08X}: "
                      f"курсор={snap[1]} подтверждён={snap[2]}", flush=True)
                prev_obj = snap
            if confirm_edge:
                print(f"[{time.perf_counter() - t0:6.1f}s] confirm в "
                      f"катсценном меню (подтверждён пункт {snap[2]})", flush=True)
                if a.observe:
                    # Ничего не заказываем: смотрим, что сделает сам движок.
                    for i in range(20):
                        time.sleep(1)
                        obj = u32v(h, mod + MENU_OBJ)
                        print(f"  [{i + 1:2d}s] menu={u32v(h, mod + MENU)} "
                              f"sub=0x{u32v(h, mod + SUB_HASH) or 0:08X} "
                              f"chk=0x{u32v(h, mod + CHK_HASH) or 0:08X} "
                              f"объект=0x{obj or 0:08X} "
                              f"курсор={i32v(h, obj + 0x38) if obj else None} "
                              f"подтв={i32v(h, obj + 0x3C) if obj else None}",
                              flush=True)
                    break
                # Шаг 1: заказ подфазы. ⚠️ Он пишет ТОЛЬКО поле текущей подфазы
                # (сцену не грузит!) — загрузку делает либо пункт Skip меню
                # (нажатие игрока), либо рестарт чекпойнта (--restart).
                try:
                    resp = api.http(api.DEFAULT_URL, "/order", "POST",
                                    {"name": a.next, "arg": 1,
                                     "clear_event": True})
                    print("  ответ /order:", resp, flush=True)
                except Exception as e:  # noqa: BLE001
                    print("  /order ошибка:", e, flush=True)
                if a.restart:
                    # Шаг 2: ждём, пока чекпойнт-запись догонит целевой подфазу.
                    target_h = hash_of(a.next)
                    deadline = time.perf_counter() + 8.0
                    while time.perf_counter() < deadline:
                        chk = u32v(h, mod + CHK_HASH)
                        if chk == target_h:
                            break
                        time.sleep(0.2)
                    print(f"  чекпойнт: 0x{u32v(h, mod + CHK_HASH) or 0:08X} "
                          f"(ждём 0x{target_h:08X})", flush=True)
                    # Шаг 3: рестарт чекпойнта — он и загружает нужную сцену.
                    try:
                        res = api.restart_mission(api.DEFAULT_URL, focus=True,
                                                  watch=15.0)
                        print("  рестарт:", res, flush=True)
                    except Exception as e:  # noqa: BLE001
                        print("  рестарт ошибка:", e, flush=True)
                    for i in range(6):
                        time.sleep(3)
                        try:
                            st = api.state()
                            pl = st.get("player") or {}
                            print(f"  [{3 * (i + 1):3d}s] "
                                  f"menu={st.get('menu_status')} "
                                  f"phase={st.get('mission_name')} "
                                  f"player={pl.get('found')} pos={pl.get('pos')}",
                                  flush=True)
                        except Exception as e:  # noqa: BLE001
                            print(f"  [{3 * (i + 1):3d}s] /state: {e}",
                                  flush=True)
                else:
                    # Без --restart заказ только пишет поле подфазы: сцену
                    # грузит штатный пункт Skip меню (нажатие игрока).
                    print("  сцену грузит пункт Skip меню (без --restart)",
                          flush=True)
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
