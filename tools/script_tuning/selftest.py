# -*- coding: utf-8 -*-
"""Оффлайн-проверка sweep.py: зеркало in_zone и метрики перелёта.

Прогоняется без игры: `py -3 tools\\script_tuning\\selftest.py`.
Проверяет только то, что не зависит от живого API: допуски триггера
(должны совпадать с `segment::in_zone`) и метрики перелёта.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import sweep  # noqa: E402

# cp1251-консоль не кодирует «→»/«≥» из сводок — иначе проверка падает на печати.
sys.stdout.reconfigure(errors="replace")

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

# Проверки порядка взвода (arm_when_outside) убраны вместе с самой функцией:
# взвод теперь делает мод (поле `restart` в скрипте + `segment::in_zone`),
# а зеркалом допусков остаётся `in_zone` выше.

print(f"\nпровалено: {len(fail)}" + (" → " + ", ".join(fail) if fail else ""))
sys.exit(1 if fail else 0)
