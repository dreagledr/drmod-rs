# -*- coding: utf-8 -*-
"""Геометрия в момент парирования → корреляция с подбросом.

Гипотеза: направление и сила подброса зависят от треугольника «игрок — клинок
врага — дистанция» в момент парирования. Инструмент гоняет N прогонов заданной
связки, на кадре парирования (враг ушёл в анимацию 1114113) и на кадре старта
подброса снимает геометрию и пишет CSV, затем считает корреляцию признаков с
`max_y`.

    py -3 tools\\script_tuning\\parry_geometry.py --runs 20
    py -3 tools\\script_tuning\\parry_geometry.py --runs 20 --jump 45 \\
        --attack-when-enemy "{\\"anim\\":[65545],\\"player_y_min\\":0.3,\\"player_y_max\\":0.8,\\"player_vy_max\\":0.0}"
"""
import argparse
import csv
import json
import math
import sys
import time

import core117
import drmod_api as api

PARRY_ANIM = 1114113
COLS = ["run", "max_y", "launch_frame", "parry_frame", "parry_anim", "player_y",
        "blade_y", "blade_dy", "dist3d", "dist_h", "angle_deg", "player_vy",
        "rise_per_frame", "blade_tip_dx", "blade_tip_dz"]


def geometry_at(frames, idx):
    """Геометрия кадра: игрок, клинок врага, дистанции и угол клинка."""
    fr = frames[idx]
    e = fr.get("enemy") or {}
    p = fr["pos"]
    ep = e.get("pos") or [0.0, 0.0, 0.0]
    blade_y = e.get("blade_y") or 0.0
    dx, dy, dz = ep[0] - p[0], blade_y - p[1], ep[2] - p[2]
    vy = (fr.get("vel") or [0, 0, 0])[1]
    return {
        "player_y": round(p[1], 3),
        "blade_y": round(blade_y, 3),
        "blade_dy": round(blade_y - p[1], 3),
        "dist3d": round(math.sqrt(dx * dx + dy * dy + dz * dz), 3),
        "dist_h": round(math.hypot(dx, dz), 3),
        "angle_deg": round(math.degrees(math.atan2(dy, math.hypot(dx, dz))), 1),
        "player_vy": round(vy, 3),
        "blade_tip_dx": round(dx, 3),
        "blade_tip_dz": round(dz, 3),
    }


def analyse_run(frames, run_no):
    ys = [f["pos"][1] for f in frames]
    max_y = max(ys)
    launch_frame, parry_frame = None, None
    for i in range(1, len(frames)):
        if parry_frame is None and (frames[i].get("enemy") or {}).get("r_anim") == PARRY_ANIM:
            parry_frame = i
        if launch_frame is None and ys[i] - ys[i - 1] >= 0.25:
            launch_frame = i
    row = {"run": run_no, "max_y": round(max_y, 2),
           "launch_frame": launch_frame, "parry_frame": parry_frame,
           "parry_anim": (frames[parry_frame].get("enemy") or {}).get("r_anim")
           if parry_frame is not None else None}
    idx = launch_frame if launch_frame is not None else parry_frame
    if idx is not None and idx > 0:
        row.update(geometry_at(frames, idx))
        row["rise_per_frame"] = round(ys[idx] - ys[idx - 1], 3)
    return row


def pearson(xs, ys):
    """Коэффициент корреляции Пирсона (без numpy)."""
    pairs = [(x, y) for x, y in zip(xs, ys) if x is not None and y is not None]
    n = len(pairs)
    if n < 3:
        return None
    mx = sum(x for x, _ in pairs) / n
    my = sum(y for _, y in pairs) / n
    num = sum((x - mx) * (y - my) for x, y in pairs)
    dx = math.sqrt(sum((x - mx) ** 2 for x, _ in pairs))
    dy = math.sqrt(sum((y - my) ** 2 for _, y in pairs))
    return num / (dx * dy) if dx and dy else None


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--runs", type=int, default=20)
    p.add_argument("--jump", type=int, default=45)
    p.add_argument("--attack", type=int, default=None)
    p.add_argument("--run-frames", type=int, default=6)
    p.add_argument("--attack-duration", type=int, default=24)
    p.add_argument("--attack-when-enemy", default=None,
                   help='JSON-условие удара (по умолчанию — рабочий рецепт)')
    p.add_argument("--out", default=r"out\parry_geometry.csv")
    p.add_argument("--timeout", type=float, default=12.0)
    p.add_argument("--url", default=api.DEFAULT_URL)
    a = p.parse_args(argv)
    sys.stdout.reconfigure(line_buffering=True)

    spec = (json.loads(a.attack_when_enemy) if a.attack_when_enemy else
            {"anim": [65545], "player_y_min": 0.3, "player_y_max": 0.8,
             "player_vy_max": 0.0})
    script = core117.build(
        jump=a.jump, attack=a.attack or core117.T_ATTACK,
        run_frames=a.run_frames, t_run=a.jump - a.run_frames,
        dur_attack=a.attack_duration, attack_when_enemy=spec)
    print(f"связка: jump={a.jump} run_frames={a.run_frames} "
          f"attack_dur={a.attack_duration}\nусловие: {spec}\nпрогонов {a.runs}\n")

    rows = []
    for n in range(1, a.runs + 1):
        # Один сбойный прогон (сеть, инжект, ребут мода) не должен ронять серию.
        try:
            api.focus_and_settle()
            if not api.ensure_gameplay(a.url):
                print(f"#{n}: игра не в геймплее")
                continue
            script["restart"] = {"ups": 1}
            sid = api.run_script(script, a.url)["script_id"]
            api.wait_script(sid, a.url, a.timeout, quiet=True)
            time.sleep(0.4)
            frames = [f for f in api.logs(a.url, script_id=sid, limit=1000)
                      if f.get("script_phase") == "running"]
        except (RuntimeError, OSError) as e:
            print(f"#{n}: прогон не удался — {e}")
            continue
        if not frames:
            print(f"#{n}: нет кадров")
            continue
        row = analyse_run(frames, n)
        rows.append(row)
        g = {k: row.get(k) for k in ("player_y", "blade_dy", "dist_h", "angle_deg")}
        print(f"#{n:>2} max_y={row['max_y']:6.2f} "
              f"парирование={'да' if row['parry_frame'] is not None else 'нет'} "
              f"подъём_кадр={row['launch_frame']} {g}")

    if not rows:
        print("нет данных")
        return 1
    with open(a.out, "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=COLS, extrasaction="ignore")
        w.writeheader()
        w.writerows(rows)
    print(f"\nCSV: {a.out}")

    max_ys = [r["max_y"] for r in rows]
    print(f"\nmax_y: медиана {sorted(max_ys)[len(max_ys) // 2]:.1f}, "
          f"макс {max(max_ys):.1f}, ≥20 м: {sum(1 for y in max_ys if y >= 20)}/{len(rows)}")
    print("\nкорреляция признаков (в момент подброса) с max_y:")
    for col in ("player_y", "blade_y", "blade_dy", "dist3d", "dist_h", "angle_deg",
                "player_vy", "rise_per_frame", "blade_tip_dx", "blade_tip_dz"):
        r = pearson([row.get(col) for row in rows], max_ys)
        if r is not None:
            bar = "#" * int(abs(r) * 20)
            print(f"  {col:<16} r={r:+.3f} {bar}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
