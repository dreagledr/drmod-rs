# -*- coding: utf-8 -*-
"""Оффлайн-проверка sweep.py: зеркало in_zone, метрики и логика взвода.

Прогоняется без игры: `py -3 tools\\script_tuning\\selftest.py`.
Проверяет только то, что не зависит от живого API, — допуски триггера
(должны совпадать с `segment::in_zone`), метрики перелёта и порядок взвода
(взводим вне зоны, при попадании в зону — промпт, а не старт).
"""
import builtins
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sweep  # noqa: E402

SPAWN = sweep.SPAWN
fail = []


def check(name, cond):
    print(("ok   " if cond else "FAIL ") + name)
    if not cond:
        fail.append(name)


# --- in_zone: зеркало segment::in_zone ---------------------------------------
check("in_zone: точка спавна", sweep.in_zone((2.86, 0.0, 70.91), SPAWN))
check("in_zone: внутри X (0.09)", sweep.in_zone((2.95, 0.0, 70.91), SPAWN))
check("in_zone: за границей X (0.11)", not sweep.in_zone((2.97, 0.0, 70.91), SPAWN))
check("in_zone: за границей Y (1.01)", not sweep.in_zone((2.86, 1.01, 70.91), SPAWN))
check("in_zone: внутри Y (0.9)", sweep.in_zone((2.86, 0.9, 70.91), SPAWN))
check("in_zone: None → False", not sweep.in_zone(None, SPAWN))
check("in_zone: далеко → False", not sweep.in_zone((-5.9, 39.0, 39.0), SPAWN))

# --- metrics -----------------------------------------------------------------
frames = [{"pos": [2.86, 0.0, 70.91 - k]} for k in range(20)]
frames += [{"pos": [2.86, 0.5 * (k - 19), 70.91 - k]} for k in range(20, 60)]
frames += [{"pos": [2.86, max(0.0, 20.5 - 0.5 * (k - 59)), 70.91 - k]} for k in range(60, 100)]
m = sweep.metrics(frames)
check(f"metrics: max_y={m['max_y']} ≥ 20", m["max_y"] >= 20)
check(f"metrics: cleared={m['cleared']} == 1", m["cleared"] == 1)
check(f"metrics: spawn_ok={m['spawn_ok']} == 1 (старт на спавне)", m["spawn_ok"] == 1)
check(f"metrics: launch={m['launch']} найден", m["launch"] is not None)
m2 = sweep.metrics([{"pos": [0.0, 0.0, 0.0]}, {"pos": [0.0, 1.0, 0.0]}])
check("metrics: чужой старт → spawn_ok=0", m2["spawn_ok"] == 0)
check("metrics: пустой лог → None", sweep.metrics([]) is None)

# --- arm_when_outside: промпт при игроке в зоне и ретрай ----------------------
real_http = sweep.http
real_input = builtins.input
state = {"n": 0}
asked = []


def fake_http_in_zone(base, path, method="GET", body=None, timeout=5.0):
    if path == "/script/run":
        return {"script_id": 7, "name": "core117-j45-a76", "total_frames": 240, "status": "armed"}
    if path == "/script/stop":
        return {"stopped": True}
    if path == "/state":
        state["n"] += 1
        if state["n"] == 1:  # первая попытка — игрок ещё в зоне спавна
            return {"player": {"found": True, "pos": [2.86, 0.0, 70.91]}, "script": None}
        return {"player": {"found": True, "pos": [-5.9, 39.0, 39.0]},
                "script": {"id": 7, "status": "armed"}}
    raise AssertionError(path)


builtins.input = lambda prompt="": asked.append(prompt) or ""
sweep.http = fake_http_in_zone
try:
    sid = sweep.arm_when_outside("http://x", {"commands": []}, "test")
finally:
    builtins.input = real_input
    sweep.http = real_http

check(f"arm: вернул sid={sid}", sid == 7)
check(f"arm: спросили про уход из зоны ({len(asked)} промпт)", len(asked) == 1)

# --- arm_when_outside: игрок не найден (меню) → взводим без промпта -----------
asked.clear()


def fake_http_menu(base, path, method="GET", body=None, timeout=5.0):
    if path == "/script/run":
        return {"script_id": 8, "name": "n", "total_frames": 240, "status": "armed"}
    if path == "/state":
        # позиция есть, но found=false — как в меню/на загрузке: верить нельзя
        return {"player": {"found": False, "pos": [2.86, 0.0, 70.91]},
                "script": {"id": 8, "status": "armed"}}
    raise AssertionError(path)


builtins.input = lambda prompt="": asked.append(prompt) or ""
sweep.http = fake_http_menu
try:
    sid = sweep.arm_when_outside("http://x", {"commands": []}, "test")
finally:
    builtins.input = real_input
    sweep.http = real_http
check(f"menu: взведено sid={sid} без промпта ({len(asked)})", sid == 8 and not asked)

print(f"\nпровалено: {len(fail)}" + (" → " + ", ".join(fail) if fail else ""))
sys.exit(1 if fail else 0)
