# -*- coding: utf-8 -*-
"""Геометрия в момент парирования и подброса → корреляция с высотой полёта.

Гипотеза: сила подброса зависит от треугольника «игрок — клинок врага —
дистанция» в момент парирования прыжка врага. Инструмент гоняет N прогонов
заданной связки и снимает геометрию по кадрам, чтобы посчитать корреляцию с
величиной подброса.

Два момента, из-за которых первая версия мерила не то (серия 11.09.2026):

* подброс — это подъём ПОСЛЕ парирования, а не отрыв прыжка. Порог «+0.25 м
  за кадр» срабатывает уже на отрыве (кадр ~51), парирование же приходит
  на 93-100 (0.7 с позже): мерилась геометрия разбега, где враг ещё в 9 м;
* поэтому подброс ищется как максимальный подъём за кадр ПОСЛЕ парирования
  (`boost_frame`), а «чистая» его величина — `post_gain = max_y - y(parry)`:
  `max_y` включает высоту самого прыжка (~1.5 м) и по нему сила лаунча видна
  хуже.

Геометрия пишется в двух срезах — на кадре парирования и на кадре подброса —
и, главное, оконным CSV: кадры `parry ± REL` с выравниванием по событию. Без
выравнивания корреляция мертва: джиттер тика Present↔симуляция сдвигает всю
картину на кадр, а за кадр игрок проходит ~0.1 м по высоте.

    py -3 tools\\script_tuning\\parry_geometry.py --runs 30 --jump 45
    py -3 tools\\script_tuning\\parry_geometry.py --analyse   # только по CSV
"""
import argparse
import csv
import json
import math
import os
import sys
import time

import core117
import drmod_api as api

PARRY_ANIM = 1114113
HEAVY_ATTACK = 0x80  # бит InputUnit тяжелого удара — им же видно кадр подачи удара
REL_MIN, REL_MAX = -25, 14
FEATURES = ("player_y", "blade_y", "blade_dy", "dist3d", "dist_h", "angle_deg",
            "player_vy", "blade_tip_dx", "blade_tip_dz")
RUN_COLS = (["run", "max_y", "post_gain", "parry_frame", "boost_frame",
             "boost_gain", "y_parry", "attack_frame", "rel_attack",
             "a_enemy_anim", "a_enemy_frame"]
            + [f"{pre}_{c}" for pre in ("a", "p", "b") for c in FEATURES])
WIN_COLS = ["run", "rel", "post_gain", "enemy_anim", "enemy_frame"] + list(FEATURES)


def geometry_at(fr):
    """Геометрия кадра: игрок, клинок врага, дистанции и угол клинка."""
    e = fr.get("enemy") or {}
    p = fr["pos"]
    ep = e.get("pos") or [0.0, 0.0, 0.0]
    blade_y = e.get("blade_y") or 0.0
    dx, dy, dz = ep[0] - p[0], blade_y - p[1], ep[2] - p[2]
    dist_h = math.hypot(dx, dz)
    return {
        "player_y": round(p[1], 3),
        "blade_y": round(blade_y, 3),
        "blade_dy": round(blade_y - p[1], 3),
        "dist3d": round(math.sqrt(dx * dx + dy * dy + dz * dz), 3),
        "dist_h": round(dist_h, 3),
        "angle_deg": round(math.degrees(math.atan2(dy, dist_h)), 1),
        "player_vy": round((fr.get("vel") or [0, 0, 0])[1], 3),
        "blade_tip_dx": round(dx, 3),
        "blade_tip_dz": round(dz, 3),
    }


def find_parry(frames):
    """Первый кадр парирования: враг ушёл в анимацию 1114113."""
    for i, fr in enumerate(frames):
        if (fr.get("enemy") or {}).get("r_anim") == PARRY_ANIM:
            return i
    return None


def find_attack(frames):
    """Кадр подачи тяжелого удара (`fed_down_bits`), то есть срабатывания условия.

    Именно этот момент мы и можем двигать: удар «спит» в команде до выполнения
    `when_enemy` и подаётся первым же подходящим тиком.
    """
    for i, fr in enumerate(frames):
        if (fr.get("fed_down_bits") or 0) & HEAVY_ATTACK:
            return i
    return None


def find_boost(ys, parry_frame):
    """Кадр подброса — максимальный подъём за кадр ПОСЛЕ парирования."""
    start = (parry_frame + 1) if parry_frame is not None else 1
    best, gain = None, 0.0
    for i in range(start, len(ys)):
        d = ys[i] - ys[i - 1]
        if d > gain:
            gain, best = d, i
    return best, round(gain, 3)


def analyse_run(frames, run_no):
    ys = [fr["pos"][1] for fr in frames]
    parry = find_parry(frames)
    attack = find_attack(frames)
    boost, gain = find_boost(ys, parry)
    if parry is None:
        gain = None
    y_parry = ys[parry] if parry is not None else None
    row = {
        "run": run_no,
        "max_y": round(max(ys), 2),
        "parry_frame": parry,
        "boost_frame": boost,
        "boost_gain": gain,
        "y_parry": round(y_parry, 3) if y_parry is not None else None,
        # сила лаунча без высоты самого прыжка
        "post_gain": round(max(ys) - y_parry, 2) if parry is not None else None,
        "attack_frame": attack,
        "rel_attack": (attack - parry) if None not in (attack, parry) else None,
    }
    if attack is not None:
        e = frames[attack].get("enemy") or {}
        row["a_enemy_anim"] = e.get("r_anim")
        row["a_enemy_frame"] = e.get("frame")
        row.update({f"a_{k}": v for k, v in geometry_at(frames[attack]).items()})
    if parry is not None:
        row.update({f"p_{k}": v for k, v in geometry_at(frames[parry]).items()})
    if boost is not None:
        row.update({f"b_{k}": v for k, v in geometry_at(frames[boost]).items()})
    return row


def window_rows(frames, run_no, post_gain):
    """Кадры `parry ± REL` — окно с выравниванием по событию."""
    parry = find_parry(frames)
    if parry is None:
        return []
    out = []
    for rel in range(REL_MIN, REL_MAX + 1):
        i = parry + rel
        if i < 0 or i >= len(frames):
            continue
        e = frames[i].get("enemy") or {}
        out.append({"run": run_no, "rel": rel, "post_gain": post_gain,
                    "enemy_anim": e.get("r_anim"), "enemy_frame": e.get("frame"),
                    **geometry_at(frames[i])})
    return out


def pearson(xs, ys):
    """Коэффициент корреляции Пирсона (без numpy)."""
    pairs = [(x, y) for x, y in zip(xs, ys) if x is not None and y is not None]
    n = len(pairs)
    if n < 4:
        return None
    mx = sum(x for x, _ in pairs) / n
    my = sum(y for _, y in pairs) / n
    num = sum((x - mx) * (y - my) for x, y in pairs)
    dx = math.sqrt(sum((x - mx) ** 2 for x, _ in pairs))
    dy = math.sqrt(sum((y - my) ** 2 for _, y in pairs))
    return num / (dx * dy) if dx and dy else None


def write_csv(path, cols, rows):
    directory = os.path.dirname(path)
    if directory:
        os.makedirs(directory, exist_ok=True)
    with open(path, "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=cols, extrasaction="ignore")
        w.writeheader()
        w.writerows(rows)


def bar(r):
    return "#" * int(abs(r) * 20)


def print_correlations(rows, win_rows, target="post_gain"):
    """Корреляции признаков с силой подброса: удар / парирование / окно rel."""
    vals = [r.get(target) for r in rows]
    if sum(1 for v in vals if v is not None) < 4:
        print(f"\n{target} пуст — парирований почти не было")
        return
    n = sum(1 for v in vals if v is not None)
    print(f"\nкорреляция с {target} ({n} прогонов с парированием):")
    groups = (("a", "в момент подачи удара (управляемый момент)"),
              ("p", "в момент парирования"), ("b", "в момент подброса (тавтология)"))
    for pre, title in groups:
        print(f"\n  {title}:")
        extra = ["a_enemy_frame", "a_enemy_anim"] if pre == "a" else []
        for col in list(FEATURES) + extra:
            r = pearson([row.get(f"{pre}_{col}" if col in FEATURES else col)
                         for row in rows], vals)
            if r is not None:
                print(f"    {col:<14} r={r:+.3f} {bar(r)}")
    if not win_rows:
        return
    print("\n  по окну относительно кадра парирования (лучший rel по |r|):")
    by_rel = {}
    for row in win_rows:
        by_rel.setdefault(int(row["rel"]), []).append(row)
    for col in FEATURES:
        best, r0 = None, None
        for rel, wrows in sorted(by_rel.items()):
            r = pearson([w.get(col) for w in wrows],
                        [w.get(target) for w in wrows])
            if r is None:
                continue
            if rel == 0:
                r0 = r
            if best is None or abs(r) > abs(best[1]):
                best = (rel, r)
        if best is None:
            continue
        tail = f"   (rel=0: r={r0:+.3f})" if r0 is not None else ""
        print(f"    {col:<14} лучший rel={best[0]:+3d} r={best[1]:+.3f} "
              f"{bar(best[1])}{tail}")
    print("\n  разброс признаков в окне (по кадрам всех прогонов):")
    for col in ("dist_h", "blade_dy", "player_y", "angle_deg"):
        vs = [w.get(col) for w in win_rows if w.get(col) is not None]
        if vs:
            print(f"    {col:<14} min={min(vs):7.3f}  max={max(vs):7.3f}")


def print_attack_table(rows):
    """Геометрия в момент подачи удара — вход для нового `when_enemy`."""
    hits = [r for r in rows if r.get("post_gain") is not None
            and r.get("a_dist_h") is not None]
    if not hits:
        return
    print("\nмомент подачи удара (по убыванию подброса):")
    print(f"  {'run':>3} {'gain':>5} {'rel':>4} {'игрок_y':>8} {'дист_h':>7} "
          f"{'угол':>6} {'vy':>6} {'враг_аним':>9} {'кадр':>5}")
    for r in sorted(hits, key=lambda r: -r["post_gain"]):
        print(f"  {r['run']:>3} {r['post_gain']:5.1f} {r['rel_attack']:>4} "
              f"{r['a_player_y']:>8.3f} {r['a_dist_h']:>7.3f} {r['a_angle_deg']:>6.1f} "
              f"{r['a_player_vy']:>6.3f} {r['a_enemy_anim']:>9} {r['a_enemy_frame']:>5}")


def load_csv(path, cols):
    """Числа из CSV — обратно в float (пусто/None → None)."""
    with open(path, encoding="utf-8") as f:
        rows = list(csv.DictReader(f))
    for row in rows:
        for c in cols:
            v = row.get(c)
            if v is None or v == "" or v == "None":
                row[c] = None
            else:
                try:
                    row[c] = float(v)
                except ValueError:
                    pass
    return rows


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--runs", type=int, default=30)
    p.add_argument("--jump", type=int, default=45)
    p.add_argument("--attack", type=int, default=None)
    p.add_argument("--run-frames", type=int, default=6)
    p.add_argument("--attack-duration", type=int, default=24)
    p.add_argument("--attack-when-enemy", default=None,
                   help='JSON-условие удара (по умолчанию — рабочий рецепт)')
    p.add_argument("--out", default=r"out\parry_geometry.csv")
    p.add_argument("--out-window", default=r"out\parry_geometry_window.csv")
    p.add_argument("--analyse", action="store_true",
                   help="не трогать игру: пересчитать корреляции по готовым CSV")
    p.add_argument("--timeout", type=float, default=12.0)
    p.add_argument("--url", default=api.DEFAULT_URL)
    a = p.parse_args(argv)
    api.setup_stdout()

    if a.analyse:
        rows = load_csv(a.out, RUN_COLS)
        win = load_csv(a.out_window, WIN_COLS) if os.path.exists(a.out_window) else []
        print(f"{a.out}: {len(rows)} прогонов, "
              f"{a.out_window}: {len(win)} кадров")
        print(f"парирований: {sum(1 for r in rows if r.get('post_gain') is not None)}"
              f"/{len(rows)}")
        print_correlations(rows, win)
        print_attack_table(rows)
        return 0

    spec = (json.loads(a.attack_when_enemy) if a.attack_when_enemy else
            {"anim": [65545], "player_y_min": 0.3, "player_y_max": 0.8,
             "player_vy_max": 0.0})
    script = core117.build(
        jump=a.jump, attack=a.attack or core117.T_ATTACK,
        run_frames=a.run_frames, t_run=a.jump - a.run_frames,
        dur_attack=a.attack_duration, attack_when_enemy=spec)
    print(f"связка: jump={a.jump} run_frames={a.run_frames} "
          f"attack_dur={a.attack_duration}\nусловие: {spec}\nпрогонов {a.runs}\n")

    rows, win = [], []
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
        win += window_rows(frames, n, row.get("post_gain"))
        print(f"#{n:>2} max_y={row['max_y']:6.2f} "
              f"парирование={'да' if row['parry_frame'] is not None else 'нет'}"
              f"@{row['parry_frame']} подброс="
              + (f"+{row['boost_gain']:.2f}@+{row['boost_frame'] - row['parry_frame']}"
                 if row["boost_gain"] else "—")
              + f" post_gain={row['post_gain']}"
              f" удар@кадр {row['attack_frame']}"
              f" (за {row['rel_attack']} до парирования)"
              f" удар_y={row.get('a_player_y')}"
              f" удар_дист={row.get('a_dist_h')}"
              f" враг_кадр={row.get('a_enemy_frame')}"
              f" | парирование: игрок_y={row.get('p_player_y')}"
              f" дист={row.get('p_dist_h')}"
              f" угол={row.get('p_angle_deg')}")

    if not rows:
        print("нет данных")
        return 1
    write_csv(a.out, RUN_COLS, rows)
    write_csv(a.out_window, WIN_COLS, win)
    print(f"\nCSV: {a.out}, {a.out_window}")

    max_ys = sorted(r["max_y"] for r in rows)
    parried = [r for r in rows if r["post_gain"] is not None]
    launched = [r for r in rows if r["max_y"] >= 20]
    print(f"\nmax_y: медиана {max_ys[len(max_ys) // 2]:.1f}, "
          f"макс {max_ys[-1]:.1f}, не ниже 20 м: {len(launched)}/{len(rows)}")
    print(f"парирований: {len(parried)}/{len(rows)}; post_gain: "
          + (", ".join(f"{r['post_gain']:.1f}" for r in parried) if parried else "—"))
    print_correlations(rows, win)
    print_attack_table(rows)
    return 0


if __name__ == "__main__":
    sys.exit(main())
