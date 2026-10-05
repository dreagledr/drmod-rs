# -*- coding: utf-8 -*-
"""Комбо с ровно N лёгкими ударами (`x`) вместо спама: префикс оптимума +
N нажатий с шагом `gap` + один `lt`.

Префикс — прыжок@50, хэви@75, риппер@107, хэви@108 (без `y`-спама). Далее
`count` лёгких: первый в `x0`, следующие через `gap`, каждый длительностью
`x-dur`; затем `lt` через `lt-gap` после конца последнего `x`. Метрика — высота
(`max_y − y0`), плюс смещение; мир как у `combo_height.py`.

Пример:
    py -3 tools\\script_tuning\\combo_5x.py --count 5 --x0 150 --gaps 8:24:2
"""
import argparse
import sys
import types

import combo_height as ch
import drmod_api as api

#: префикс оптимума без лёгких: (кадр, токен, длительность)
PREFIX = [
    (50, "a", 12),
    (75, "y", 10),
    (107, "lr", 1),
    (108, "y", 12),
]
PREFIX_END = max(t + d for t, _, d in PREFIX)


def build_combo(x0, gap, count, x_dur, lt_gap, lt_dur, end=420):
    if x0 < PREFIX_END:
        raise ValueError(f"x0={x0} перекрывает префикс (конец {PREFIX_END})")
    cmds = [{"t": t, "duration": d, "input": dict(ch.TOKEN_INPUT[tok])}
            for t, tok, d in PREFIX]
    for i in range(count):
        cmds.append({"t": x0 + i * gap, "duration": x_dur,
                     "input": {"light_attack": True}})
    last_end = x0 + (count - 1) * gap + x_dur
    lt0 = last_end + lt_gap
    cmds.append({"t": lt0, "duration": lt_dur, "input": {"blade": True}})
    total = max(c["t"] + c["duration"] for c in cmds)
    if end > total:
        cmds.append({"t": total, "duration": end - total,
                     "input": {"camera": [0.0, 0.0]}})
    return {"name": f"combo-{count}x", "trigger": {"ticks": 0},
            "restart": {"ups": 1}, "commands": cmds}


def range_list(text):
    if ":" in text:
        parts = text.split(":")
        lo, hi = int(parts[0]), int(parts[1])
        step = int(parts[2]) if len(parts) > 2 else 1
        return list(range(lo, hi + 1, step))
    return [int(x) for x in text.split(",")]


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--count", type=int, default=5, help="число лёгких ударов")
    p.add_argument("--x0", type=int, default=150, help="кадр первого x")
    p.add_argument("--gaps", default="8,10,12,14,16,18,20,24",
                   help="шаги между x (список/диапазон)")
    p.add_argument("--x-durs", default="1", help="длительности x через запятую")
    p.add_argument("--lt-gap", type=int, default=2,
                   help="зазор от конца последнего x до lt")
    p.add_argument("--lt-dur", type=int, default=7)
    p.add_argument("--end", type=int, default=420)
    p.add_argument("--seed", type=lambda s: int(s, 0), default=1)
    p.add_argument("--no-freeze", dest="freeze", action="store_false")
    p.add_argument("--no-fixed-dt", dest="fixed_dt", action="store_false")
    p.add_argument("--cap", default="off")
    p.add_argument("--timeout", type=float, default=40.0)
    p.add_argument("--rearm-wait", type=float, default=14.0)
    args = p.parse_args(argv)

    run_args = types.SimpleNamespace(freeze=args.freeze, dump=None,
                                     timeout=args.timeout,
                                     rearm_wait=args.rearm_wait)
    gaps = range_list(args.gaps)
    x_durs = [int(x) for x in str(args.x_durs).split(",")]

    api.setup_stdout()
    if args.fixed_dt:
        api.fixed_dt(True)
    if args.cap == "off":
        api.fps_cap(cap="off")
    elif args.cap == "game":
        api.fps_cap(cap="game")
    elif args.cap:
        api.fps_cap(fps=int(args.cap))
    api.focus_and_settle()
    if not api.ensure_gameplay():
        print("не в геймплее")
        return 2

    rows = []
    for xd in x_durs:
        for gap in gaps:
            api.focus_and_settle()
            api.ensure_gameplay()
            try:
                script = build_combo(args.x0, gap, args.count, xd,
                                     args.lt_gap, args.lt_dur, args.end)
            except ValueError as e:
                print(f"  gap={gap} xd={xd}: пропуск — {e}")
                continue
            r = ch.run_once(script, args.seed, run_args)
            if r is None:
                print(f"  gap={gap} xd={xd}: не удалось")
                continue
            rows.append((gap, xd, r))
            print(f"  gap={gap:2} xd={xd}: высота={r['height']:6.2f} "
                  f"(max_y={r['max_y']:6.2f}) вперёд={r['horiz']:5.2f} "
                  f"t_max={r['t_max']}", flush=True)

    print(f"\n-- топ ({args.count} x) --")
    for gap, xd, r in sorted(rows, key=lambda x: x[2]["height"],
                             reverse=True)[:10]:
        print(f"  gap={gap:2} xd={xd}: высота={r['height']:.3f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
