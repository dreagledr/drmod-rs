# -*- coding: utf-8 -*-
"""Консольное меню скипа катсцены: Esc → PAUSE/SKIP → наш скип.

Задача, из которой всё выросло: в in-engine сцене (монолог Монсуна, `P370_IN`)
PC-версия не даёт скипнуть — сцена висит, пока не доиграет звук. Консольное
катсценное меню включается парой флагов, но его собственный пункт Skip на PC
инертен (см. ниже), поэтому решение выполняет наш код, а меню даёт консольный UX.

Что делает инструмент, пока идёт заданная подфаза (по умолчанию `P370_IN`):

1. держит выставленными `STA_SOFT_EVENT` (код 4) и `STA_SOFT_EVENT_SKIP_OK`
   (код 37) в `staFlags` (`base + 0x17EA060`) — от них Esc открывает
   **консольное катсценное меню** (`GameMenuStatus` = 6, класс
   `cEventPauseMenu`), а не обычную паузу;
2. читает живой объект меню (`base + 0x17EA140`): `+0x04` состояние машины
   (`-2` — решение принято), `+0x38` курсор, `+0x3C` индекс подтверждённого
   пункта (`-1` — ещё не подтверждён; 0 = CONTINUE, 1 = SKIP);
3. по подтверждению пункта убирает меню **штатным путём движка**
   (`GameMenuStatus` = 6 + шаг `0x17EA118` = 6 → движок сам уничтожает объект и
   уводит статус 6 → 12 → 1) и снимает `STA_PAUSE` (код 19) — без этого сцена
   остаётся стоять;
4. если подтверждён **SKIP** (индекс 1) — заказывает следующую подфазу через
   `POST /order` (движковая `request_subphase` с `clear_event`): движок реально
   выгружает текущую сцену и загружает заказанную. Если **CONTINUE** (индекс 0) —
   просто возвращает сцену, как на консоли.

⚠️ Заказ подфазы грузит сцену только когда игра **не на паузе**: `STA_PAUSE`
стопорит машину загрузки, и заявка меняет лишь подпись (проверено 2026-09-12:
с открытым катсценным меню сцена стояла на месте, а без паузы заказ `P370_EVENT`
сменил сцену и кадр).

⚠️ Пункт Skip катсценного меню на PC сам по себе ничего не делает: подтверждение
пишет `[obj+0x3C]` и `[obj+0x04] = -2`, но дальше этих значений никто не читает
(в exe 5 ссылок на объект меню, все внутри обработчика, деструктор скип не
выполняет), а шаг 5 обработчика (`RVA 0x816B8E`) ждёт `[0x1DC203C] <= 0` — там
элемент массива флагов, равный `1.0f`, поэтому меню висит насмерть. Отсюда шаг 3:
шаг 6 обходим, выставляя его снаружи.

⚠️ Меню открывает физический Esc игрока (движку нужен фронт, а клавиатура даёт
только бит `0x100` в `InputUnit.down`) — инструмент это видит и дополнительно
подаёт `pause`-скрипт, чтобы фронт появился.

При выходе из сцены флаги снимаются (состояние движка возвращается как было).

    py -3 -u tools\\script_tuning\\cutscene_skip.py
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
MENU = 0x17E9F9C            # GameMenuStatus
STA = 0x17EA060             # Trigger::staFlags: word0 (коды 0..31) + word1
MENU_STEP = 0x17EA118       # шаг жизненного цикла меню 6 (0..6)
MENU_OBJ = 0x17EA140        # указатель на живой объект cEventPauseMenu
# ⚠️ В дизассемблере эти глобалы видны как `[0x1BEA118]`/`[0x1BEA140]` — это
# **file VA** (ImageBase 0x400000 + RVA), поэтому RVA = 0x17EA118/0x17EA140.
# По 0x1BEA140 лежит чужой мусор (указатель на строку hkReferenceObject).
SUB_HASH = 0x14B9178        # текущая подфаза: хэш (объект состояния +0x38)
INPUT_KEYS = 0x19D06F8      # cInput::ms_InputKeys (DIK-буфер клавиатуры)
DIK_ESCAPE = 0x01
SOFT_EVENT = 0x08000000     # код 4 (word0)
SKIP_OK = 0x04000000        # код 37 (word1)
STA_PAUSE = 0x00001000      # код 19 (word0): игра на паузе — стопорит загрузку
MENU_CUTSCENE = 6           # GameMenuStatus::CutscenePause
MENU_IN_GAME = 1            # GameMenuStatus::InGame
OBJ_STATE = 0x04            # состояние машины меню; -2 = решение принято
OBJ_STATE_DECIDED = -2
OBJ_CURSOR = 0x38           # индекс курсора в пунктах меню
OBJ_CONFIRMED = 0x3C        # индекс подтверждённого пункта (-1 — ещё нет)
ITEM_CONTINUE = 0
ITEM_SKIP = 1

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


def set_flags(h, mod, on):
    """Включить/снять наши флаги катсценного меню (SOFT_EVENT + SKIP_OK)."""
    d0 = u32v(h, mod + STA) or 0
    d1 = u32v(h, mod + STA + 4) or 0
    if on:
        wr_u32(h, mod + STA, d0 | SOFT_EVENT)
        wr_u32(h, mod + STA + 4, d1 | SKIP_OK)
    else:
        wr_u32(h, mod + STA, d0 & ~SOFT_EVENT)
        wr_u32(h, mod + STA + 4, d1 & ~SKIP_OK)


def close_menu(h, mod, log):
    """Убрать катсценное меню штатным путём движка и снять паузу.

    Шаг 5 обработчика (`RVA 0x816B8E`) ждёт `[0x1DC203C] <= 0`, а там элемент
    массива флагов, равный `1.0f`, — этот шаг не проходится никогда. Поэтому
    сразу выставляем шаг 6: движок сам уничтожает объект меню, играет
    `core_se_sys_se_resume` и уводит статус 6 → 12 → 1. Пауза (`STA_PAUSE`,
    которую ставит шаг 0) при этом остаётся — её снимаем сами, иначе сцена стоит.
    """
    wr_u32(h, mod + MENU, MENU_CUTSCENE)
    wr_u32(h, mod + MENU_STEP, 6)
    deadline = time.perf_counter() + 5.0
    while time.perf_counter() < deadline:
        obj = u32v(h, mod + MENU_OBJ)
        status = u32v(h, mod + MENU)
        if obj == 0 and status not in (MENU_CUTSCENE, 12):
            log(f"меню убрано движком (status={status}, объект уничтожен)")
            break
        time.sleep(0.05)
    else:
        log("⚠️ меню не убралось за 5 с — снимаю паузу как есть")
    d0 = u32v(h, mod + STA) or 0
    if d0 & STA_PAUSE:
        wr_u32(h, mod + STA, d0 & ~STA_PAUSE)
        log(f"снял STA_PAUSE (было 0x{d0:08X}) — сцена снова идёт")


def order_subphase(h, mod, next_name, log, timeout=15.0):
    """Заказать подфазу штатной функцией движка и дождаться смены сцены."""
    try:
        resp = api.http(api.DEFAULT_URL, "/order", "POST",
                        {"name": next_name, "arg": 1, "clear_event": True})
        log(f"заказ /order: {resp}")
    except Exception as e:  # noqa: BLE001
        log(f"⚠️ /order ошибка: {e}")
        return False
    target = hash_of(next_name)
    deadline = time.perf_counter() + timeout
    while time.perf_counter() < deadline:
        cur = u32v(h, mod + SUB_HASH)
        if cur == target:
            log(f"✅ сцена загружена: подфаза {next_name} (0x{target:08X})")
            return True
        time.sleep(0.2)
    cur = u32v(h, mod + SUB_HASH)
    log(f"⚠️ подфаза не сменилась за {timeout:.0f} с "
        f"(сейчас 0x{cur or 0:08X}, ждали 0x{target:08X})")
    return False


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("--watch", default="P370_RESTART,P370_IN",
                   help="подфазы сцены через запятую (сцена идёт от RESTART к IN; "
                        "флаги держатся, пока мы внутри набора)")
    p.add_argument("--next", default="P370_EVENT",
                   help="куда переводить по подтверждённому Skip")
    p.add_argument("--timeout", type=float, default=1800.0,
                   help="сколько секунд ждать скипа, прежде чем выйти")
    p.add_argument("--hz", type=float, default=50.0,
                   help="частота опроса (клавиатурный Esc держится недолго)")
    a = p.parse_args(argv)
    api.setup_stdout()
    h, mod = open_game()
    watch_h = {hash_of(n) for n in a.watch.split(",") if n.strip()}
    print(f"module base = 0x{mod:08X}; сцена {a.watch} "
          f"({', '.join(f'0x{x:08X}' for x in watch_h)}) → "
          f"по Skip в {a.next}", flush=True)
    t0 = time.perf_counter()
    flags_set = False
    prev = None
    prev_obj = None
    prev_esc = False
    last_pause = 0.0
    while time.perf_counter() - t0 < a.timeout:
        cur = u32v(h, mod + SUB_HASH)
        menu = u32v(h, mod + MENU)
        keys = rd(h, mod + INPUT_KEYS, 256) or b""
        if cur is None or menu is None:
            time.sleep(0.5)
            continue
        esc = len(keys) > DIK_ESCAPE and keys[DIK_ESCAPE] != 0
        esc_edge = esc and not prev_esc
        prev_esc = esc
        in_scene = cur in watch_h
        if in_scene and not flags_set:
            set_flags(h, mod, True)
            flags_set = True
            print(f"[{time.perf_counter() - t0:6.1f}s] флаги катсценного меню "
                  f"выставлены (SOFT_EVENT + SKIP_OK)", flush=True)
        elif in_scene and flags_set:
            # игра может сбрасывать флаги (например, снимает SKIP_OK, когда пауза
            # нажата без SOFT_EVENT) — поддерживаем их
            d0 = u32v(h, mod + STA) or 0
            d1 = u32v(h, mod + STA + 4) or 0
            if not (d0 & SOFT_EVENT) or not (d1 & SKIP_OK):
                set_flags(h, mod, True)
        elif not in_scene and flags_set:
            set_flags(h, mod, False)
            flags_set = False
            print(f"[{time.perf_counter() - t0:6.1f}s] сцена кончилась — флаги "
                  f"снял", flush=True)
        # Физическому Esc движок не даёт фронта (клавиатура выставляет только бит
        # 0x100 в InputUnit.down), а проверка паузы хочет фронт — подаём pause.
        now = time.perf_counter()
        if (esc_edge and in_scene and menu == MENU_IN_GAME
                and now - last_pause >= 0.5):
            last_pause = now
            try:
                api.run_script({"name": "esc-as-pause",
                                "commands": [{"t": 0, "duration": 2,
                                              "input": {"pause": True}}]})
                print(f"[{now - t0:6.1f}s] Esc игрока → подаю pause-бит "
                      f"(движку нужен фронт)", flush=True)
            except Exception as e:  # noqa: BLE001
                print(f"  Esc→pause ошибка: {e}", flush=True)
        key = (menu, in_scene)
        if key != prev:
            print(f"[{time.perf_counter() - t0:6.1f}s] menu={menu} "
                  f"sub=0x{cur:08X} in_scene={in_scene}", flush=True)
            prev = key
        if menu == MENU_CUTSCENE:
            obj = u32v(h, mod + MENU_OBJ)
            state = i32v(h, obj + OBJ_STATE) if obj else None
            snap = (obj, state,
                    i32v(h, obj + OBJ_CURSOR) if obj else None,
                    i32v(h, obj + OBJ_CONFIRMED) if obj else None)
            if snap != prev_obj:
                print(f"[{time.perf_counter() - t0:6.1f}s] меню 0x{snap[0] or 0:08X}: "
                      f"state={snap[1]} курсор={snap[2]} подтверждён={snap[3]}",
                      flush=True)
                prev_obj = snap
            confirmed = snap[3]
            # Решение принято, когда машина меню ушла в -2 и записан индекс
            # пункта: обработчик ввода (`RVA 0x5A5930`) пишет их вместе.
            if (obj and state == OBJ_STATE_DECIDED
                    and confirmed is not None and confirmed >= 0):
                item = ("SKIP" if confirmed == ITEM_SKIP
                        else "CONTINUE" if confirmed == ITEM_CONTINUE
                        else f"#{confirmed}")
                print(f"[{time.perf_counter() - t0:6.1f}s] игрок подтвердил "
                      f"пункт {item} — убираю меню движком", flush=True)
                close_menu(h, mod, lambda s: print(f"  {s}", flush=True))
                if confirmed == ITEM_SKIP:
                    order_subphase(h, mod, a.next,
                                   lambda s: print(f"  {s}", flush=True))
                else:
                    print("  CONTINUE — сцена возвращается, заказ не нужен",
                          flush=True)
                set_flags(h, mod, False)
                print("  флаги катсценного меню сняты (уборка)", flush=True)
                break
        else:
            prev_obj = None
        time.sleep(1.0 / a.hz)
    else:
        print("таймаут — подтверждения в катсценном меню не случилось", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
