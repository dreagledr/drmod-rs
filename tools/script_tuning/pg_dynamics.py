# -*- coding: utf-8 -*-
"""Динамика контакта: скорость сближения и вертикальная скорость по окну.

Геометрия (высоты, дистанция) величину подброса между сериями не объяснила,
а импульс физически задаёт ОТНОСИТЕЛЬНАЯ скорость в контакте. Считаем её по
кадровым срезам окна: `v_сближения = -(dist_h[rel] - dist_h[rel-1])` за кадр и
`v_y` игрока, плюс сами dist_h/player_y на rel = -2, -1, 0.

    py -3 tools\\script_tuning\\pg_dynamics.py out\\pg_f08_win.csv out\\pg_l00_win.csv
"""
import csv
import sys

RELS = (-2, -1, 0)


def load(path):
    with open(path, encoding="utf-8") as f:
        rows = list(csv.DictReader(f))
    by_run = {}
    for r in rows:
        by_run.setdefault(r["run"], {})[int(r["rel"])] = r
    return by_run


def num(rec, key):
    v = (rec or {}).get(key)
    return None if v in (None, "", "None") else float(v)


def main(paths):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    for path in paths:
        by_run = load(path)
        runs = [r for r, rels in by_run.items() if 0 in rels]
        if not runs:
            continue
        runs.sort(key=lambda r: -float(by_run[r][0]["post_gain"]))
        print(f"\n{path}")
        print(f"  {'run':>3} {'gain':>6} | {'dist -2':>8} {'-1':>7} {'0':>7} "
              f"{'сближ':>7} | {'y -2':>7} {'y 0':>7} {'v_y':>7}")
        for run in runs:
            rels = by_run[run]
            gain = float(rels[0]["post_gain"])
            d2, d1, d0 = (num(rels.get(r), "dist_h") for r in RELS)
            close = (d1 - d0) if None not in (d1, d0) else None
            y2, y0 = num(rels.get(-2), "player_y"), num(rels.get(0), "player_y")
            vy = num(rels.get(-1), "player_vy")
            print(f"  {run:>3} {gain:6.1f} | "
                  f"{d2 if d2 is not None else float('nan'):8.3f} "
                  f"{d1 if d1 is not None else float('nan'):7.3f} "
                  f"{d0 if d0 is not None else float('nan'):7.3f} "
                  f"{close if close is not None else float('nan'):7.3f} | "
                  f"{y2 if y2 is not None else float('nan'):7.3f} "
                  f"{y0 if y0 is not None else float('nan'):7.3f} "
                  f"{vy if vy is not None else float('nan'):7.3f}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:] or ["out/parry_geometry_window.csv"]))
