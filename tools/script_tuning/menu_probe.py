# -*- coding: utf-8 -*-
"""Живой пробник меню MGR:R через HTTP API мода.

Зачем: рестарт миссии из скрипта упирается в меню паузы — сколько пунктов,
какой бит открывает паузу, двигают ли курсор D-Pad-биты. Проверять это
визуально нельзя (агент не видит экран), поэтому пробник гоняет
последовательность вводов и печатает траекторию из `GET /logs`:
`menu_status` по кадрам (добавлен в кадр именно для этого), поданные
битмаски и позицию.

Примеры:
    # открыть паузу (Esc = бит 0x100) и посмотреть, что стало со статусом
    py -3 tools\\script_tuning\\menu_probe.py pause

    # пауза → курсор вниз ×2 → подтверждение
    py -3 tools\\script_tuning\\menu_probe.py pause down down confirm

    # удержание курсора стиком вместо D-Pad
    py -3 tools\\script_tuning\\menu_probe.py pause stick_down stick_down confirm
"""
import argparse
import json
import sys
import time

import drmod_api as api

DEFAULT_URL = api.DEFAULT_URL

# Шаги → поля ввода API (см. docs/API.md §4.2). stick_* — левый стик
# (курсор в меню двигается D-Pad-битами или стиком).
STEPS = {
    "pause": {"pause": True},
    "confirm": {"confirm": True},
    "weapon_select": {"weapon_select": True},
    "up": {"menu_up": True},
    "down": {"menu_down": True},
    "left": {"menu_left": True},
    "right": {"menu_right": True},
    "stick_up": {"left_stick": [0, -1000]},
    "stick_down": {"left_stick": [0, 1000]},
    "stick_left": {"left_stick": [-1000, 0]},
    "stick_right": {"left_stick": [1000, 0]},
}


def stop_active(base):
    try:
        api.http(base, "/script/stop", "POST")
        return True
    except RuntimeError:
        return False


def step_input(token):
    """Ввод для шага: имя из STEPS, `raw:<hex>` — игровой код клавиши
    (docs/REPLAY.md §2.1, через `isKeyDown`-детуры) или `dik:<hex>` — DIK-код
    DirectInput, который мод подмешивает после опроса устройства
    (`ms_InputKeys`). Примеры: `raw:8C`, `dik:0xD0` (стрелка вниз)."""
    if token in STEPS:
        return STEPS[token]
    if token.startswith("raw:"):
        return {"raw_key": int(token[4:], 16)}
    if token.startswith("dik:"):
        return {"dik_key": int(token[4:], 16)}
    raise SystemExit(f"неизвестный шаг: {token} (есть: {', '.join(sorted(STEPS))}, "
                     f"raw:8C, dik:0xD0)")


def build_script(steps, hold, gap, tail, name):
    cmds = []
    t = 0
    for s in steps:
        cmds.append({"t": t, "duration": hold, "input": step_input(s)})
        t += hold + gap
    total = t + tail
    return {"name": name, "commands": cmds}, total


def run(base, script, timeout):
    res = api.run_script(script, base)
    sid = res["script_id"]
    t0 = time.time()
    last = None
    while time.time() - t0 < timeout:
        st = api.http(base, f"/script/{sid}")
        if st["status"] != last:
            print(f"  script {sid}: {last or res.get('status')} → {st['status']} "
                  f"({st['frame']}/{st['total_frames']})", flush=True)
            last = st["status"]
        if st["status"] in ("done", "stopped"):
            break
        time.sleep(0.05)
    else:
        stop_active(base)
        print(f"  script {sid}: таймаут {timeout} с — снят")
    return sid


def show_log(base, sid, limit):
    frames = api.logs(base, script_id=sid, limit=limit)
    if not frames:
        print("  в /logs нет кадров этого скрипта")
        return frames
    print(f"\n  кадры скрипта {sid} ({len(frames)}):")
    print("    f   menu_status              fed_down fed_prs  cur_down cur_prs  fed_stick      pos")
    # `fed_*` — то, что подали мы (override); `cur_*` — что видит игра
    # (cur_in). В паузе cur_in может залипать — сравнение показывает, дошёл
    # ли override до игры.
    seen = object()
    for fr in frames:
        ms = fr.get("menu_status", "?")
        inp = fr.get("input") or {}
        stick = tuple(fr.get("fed_left_stick") or (0, 0))
        row = (ms, fr.get("fed_down_bits"), fr.get("fed_pressed_bits"),
               inp.get("down_bits"), inp.get("pressed_bits"), stick)
        marker = " " if row == seen else "*"
        seen = row
        p = fr.get("pos") or [0, 0, 0]
        print(f"  {marker}{fr['frame']:>4}  {ms:<21} {fr.get('fed_down_bits', 0):08X} "
              f"{fr.get('fed_pressed_bits', 0):08X}  {inp.get('down_bits', 0):08X} "
              f"{inp.get('pressed_bits', 0):08X}  {stick[0]:6.0f},{stick[1]:6.0f}  "
              f"({p[0]:7.2f},{p[1]:7.2f},{p[2]:7.2f})")
    print("\n  переходы menu_status:")
    prev = None
    for fr in frames:
        ms = fr.get("menu_status", "?")
        if ms != prev:
            print(f"    f{fr['frame']:>4}: {prev} → {ms}")
            prev = ms
    return frames


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("steps", nargs="+",
                   help="шаги ввода: имена (%s) или raw:<hex> — сырой код клавиши"
                        % ", ".join(sorted(STEPS)))
    p.add_argument("--hold", type=int, default=3, help="кадров удержания шага")
    p.add_argument("--gap", type=int, default=8, help="кадров паузы между шагами")
    p.add_argument("--tail", type=int, default=45, help="кадров хвоста после шагов")
    p.add_argument("--limit", type=int, default=1000)
    p.add_argument("--timeout", type=float, default=20.0)
    p.add_argument("--url", default=DEFAULT_URL)
    p.add_argument("--no-focus", action="store_true",
                   help="не активировать окно игры (без фокуса игра ввод в меню не видит)")
    a = p.parse_args(argv)
    sys.stdout.reconfigure(line_buffering=True)

    if not a.no_focus:
        print(f"фокус окна игры: {'OK' if api.focus_and_settle() else 'НЕ ПОЛУЧИЛСЯ'}")

    st = api.state(a.url)
    pl = st.get("player") or {}
    print(f"до запуска: menu={st.get('menu_status')} mission={st.get('mission_name')} "
          f"позиция={pl.get('pos')}")

    script, total = build_script(a.steps, a.hold, a.gap, a.tail, "probe-" + "-".join(a.steps))
    print(f"скрипт: {' → '.join(a.steps)} (hold={a.hold} gap={a.gap}, {total} кадров)")
    sid = run(a.url, script, a.timeout)
    time.sleep(0.3)
    try:
        show_log(a.url, sid, a.limit)
    except RuntimeError as e:
        print(f"  лог не получен: {e}")

    try:
        st = api.state(a.url)
        pl = st.get("player") or {}
        print(f"\nпосле: menu={st.get('menu_status')} mission={st.get('mission_name')} "
              f"позиция={pl.get('pos')}")
    except RuntimeError as e:
        print(f"\nпосле: /state недоступен ({e})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
