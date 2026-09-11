# -*- coding: utf-8 -*-
"""Прогон JSON-скрипта из `test_inputs/` через HTTP API мода.

Печатает ход фаз (restart → armed → running → done), переходы `menu_status` и
сводку кадров (позиция старта/финиша, max Y) — удобно проверять сценарии вида
«рестарт миссии + отложенный скрипт по спавну».

Пример:
    py -3 tools\\script_tuning\\run_json.py test_inputs\\r01_beach_restart_test.json
"""
import argparse
import json
import sys
import time

import drmod_api as api


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("path", help="JSON-файл скрипта (тело POST /script/run)")
    p.add_argument("--timeout", type=float, default=60.0, help="с — ждать завершения")
    p.add_argument("--focus", action="store_true",
                   help="активировать окно игры (нужно для фазы restart)")
    p.add_argument("--restart", action="store_true",
                   help="добавить в скрипт поле restart ({} — дефолты): мод сам "
                        "перезапустит миссию и взведёт скрипт после loading")
    p.add_argument("--url", default=api.DEFAULT_URL)
    a = p.parse_args(argv)
    api.setup_stdout()

    with open(a.path, encoding="utf-8") as f:
        script = json.load(f)
    if a.restart and script.get("restart") is None:
        script["restart"] = {}
    print(f"скрипт: {a.path} (name={script.get('name')}, "
          f"restart={'да' if script.get('restart') is not None else 'нет'}, "
          f"trigger={'да' if script.get('trigger') else 'нет'}, "
          f"команд {len(script.get('commands') or [])})")

    st = api.state(a.url)
    print(f"до: menu={st.get('menu_status')} pos={(st.get('player') or {}).get('pos')}")

    if a.focus:
        print(f"фокус окна игры: {'OK' if api.focus_and_settle() else 'НЕ ПОЛУЧИЛСЯ'}")
    if a.restart and not api.ensure_gameplay(a.url):
        print("не удалось выйти из меню в геймплей — фаза рестарта не сработает")
        return 2

    res = api.run_script(script, a.url)
    sid = res["script_id"]
    print(f"script {sid}: запушен, стартовый статус {res['status']}")

    start = time.monotonic()
    last = None
    while time.monotonic() - start < a.timeout:
        cur = api.http(a.url, f"/script/{sid}")
        status = cur["status"]
        if status != last:
            print(f"  {status} (кадр {cur['frame']}/{cur['total_frames']})", flush=True)
            last = status
        if status in ("done", "stopped"):
            break
        time.sleep(0.05)
    else:
        api.http(a.url, "/script/stop", "POST")
        print(f"  таймаут {a.timeout} с — скрипт снят")

    time.sleep(0.5)
    frames = api.logs(a.url, script_id=sid)
    print(f"кадров в /logs: {len(frames)}")
    flight = [f for f in frames if f.get("script_phase") == "running"]
    if flight and len(flight) != len(frames):
        print(f"  из них в фазе running (полёт): {len(flight)}")
    if flight:
        ys = [fr["pos"][1] for fr in flight]
        print(f"  полёт: старт {[round(v, 2) for v in flight[0]['pos']]} "
              f"→ финиш {[round(v, 2) for v in flight[-1]['pos']]}, "
              f"max_y={max(ys):.2f} (кадр {ys.index(max(ys))})")
    if frames:
        ys = [fr["pos"][1] for fr in frames]
        print(f"  старт: {[round(v, 2) for v in frames[0]['pos']]} "
              f"(menu={frames[0]['menu_status']})")
        print(f"  финиш: {[round(v, 2) for v in frames[-1]['pos']]} "
              f"(menu={frames[-1]['menu_status']})")
        print(f"  max_y={max(ys):.2f} при кадре {ys.index(max(ys))}")
    print("переходы menu_status:")
    for frame, ms in api.menu_status_transitions(frames):
        print(f"  f{frame}: {ms}")
    last_state = api.state(a.url)
    print(f"после: menu={last_state.get('menu_status')} "
          f"pos={(last_state.get('player') or {}).get('pos')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
