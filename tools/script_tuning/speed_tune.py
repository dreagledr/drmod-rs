# -*- coding: utf-8 -*-
"""Сравнение скорости перемещения: ninja run vs прыжки (rt+a), с тюнингом
таймингов и длительностей.

Режимы:
  run      — только `ls:0` + `rt` (бег).
  jump     — бег + прыжки `a` каждые `period` кадров, удержание `dur`.
  release  — как jump, но `rt` отпускается на `rel-len` кадров через `rel-off`
             после начала каждого прыжка и снова включается (быстрая анимация
             приземления).

Мир: `freeze` RNG + `trigger.ticks=0` + `restart` + `/dt fixed` + `fps cap off`.
Метрика — пройденная дистанция (X/Z) на кадрах `--measure` от старта бега.

Пример:
    py -3 tools\\script_tuning\\speed_tune.py --mode jump --durs 16,20,24 \\
        --periods 36,40,44 --njumps 7
"""
import argparse
import sys
import types

import combo_height as ch
import drmod_api as api

END = 300
LSTICK = {"left_stick": [0, -1000]}
RT = {"ninja_run": True}


def rt_segments(end, release_windows):
    """Интервалы, где `rt` включён: [4, end] минус окна отпускания."""
    segs = [(4, end)]
    for lo, hi in release_windows:
        out = []
        for a, b in segs:
            if hi <= a or lo >= b:
                out.append((a, b))
                continue
            if a < lo:
                out.append((a, lo))
            if hi < b:
                out.append((hi, b))
        segs = out
    return segs


def build(mode, dur, period, njumps, t0=9, rel_off=30, rel_len=8, end=END):
    cmds = [{"t": 0, "duration": end, "input": dict(LSTICK)}]
    jumps = []
    t = t0
    for _ in range(njumps):
        if t + dur > end:
            break
        jumps.append(t)
        t += period
    releases = []
    if mode == "release":
        releases = [(j + rel_off, min(j + rel_off + rel_len, end)) for j in jumps]
    for a, b in rt_segments(end, releases):
        if b > a:
            cmds.append({"t": a, "duration": b - a, "input": dict(RT)})
    for j in jumps:
        cmds.append({"t": j, "duration": dur, "input": {"jump": True}})
    total = max(c["t"] + c["duration"] for c in cmds)
    if end > total:
        cmds.append({"t": total, "duration": end - total,
                     "input": {"camera": [0.0, 0.0]}})
    return {"name": f"{mode}", "trigger": {"ticks": 0}, "restart": {"ups": 1},
            "commands": cmds}


def dist_at(frames, frame):
    i0 = next((i for i, f in enumerate(frames)
               if any(abs(v) > 1e-6 for v in f["pos"])), 0)
    p0 = frames[i0]["pos"]
    p = frames[min(frame, len(frames) - 1)]["pos"]
    return ((p[0] - p0[0]) ** 2 + (p[2] - p0[2]) ** 2) ** 0.5


def run_once(script, args):
    api.focus_and_settle()
    api.ensure_gameplay()
    ch.run_once(script, 1, types.SimpleNamespace(
        freeze=True, dump=r"out\combo\sp_tmp.json",
        timeout=args.timeout, rearm_wait=args.rearm_wait))
    import json
    frames = [f for f in json.load(open(r"out\combo\sp_tmp.json", encoding="utf-8"))
              if f.get("script_phase") == "running"]
    return {m: round(dist_at(frames, m), 2) for m in args.measures}


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--mode", choices=["run", "jump", "release"], default="jump")
    p.add_argument("--durs", default="20", help="удержание прыжка, список")
    p.add_argument("--periods", default="40", help="период прыжков, список")
    p.add_argument("--njumps", type=int, default=7)
    p.add_argument("--t0", type=int, default=9, help="кадр первого прыжка")
    p.add_argument("--rel-off", type=int, default=30)
    p.add_argument("--rel-len", type=int, default=8)
    p.add_argument("--measures", default="180,240,270",
                   help="кадры замера дистанции")
    p.add_argument("--no-fixed-dt", dest="fixed_dt", action="store_false")
    p.add_argument("--timeout", type=float, default=40.0)
    p.add_argument("--rearm-wait", type=float, default=14.0)
    args = p.parse_args(argv)
    args.measures = [int(x) for x in args.measures.split(",")]
    durs = [int(x) for x in str(args.durs).split(",")]
    periods = [int(x) for x in str(args.periods).split(",")]

    api.setup_stdout()
    if args.fixed_dt:
        api.fixed_dt(True)
    api.fps_cap(cap="off")
    api.focus_and_settle()
    if not api.ensure_gameplay():
        print("не в геймплее")
        return 2

    print(f"mode={args.mode} njumps={args.njumps} measures={args.measures}")
    rows = []
    if args.mode == "run":
        r = run_once(build("run", 0, 0, 0), args)
        rows.append(("run", 0, 0, r))
        print(f"  run                  {r}")
    else:
        for period in periods:
            for dur in durs:
                r = run_once(build(args.mode, dur, period, args.njumps,
                                   args.t0, args.rel_off, args.rel_len), args)
                rows.append((args.mode, dur, period, r))
                print(f"  dur={dur:3} period={period:3} {r}", flush=True)

    last = args.measures[-1]
    print(f"\n-- ранжирование по дистанции @{last} --")
    for mode, dur, period, r in sorted(rows, key=lambda x: x[3][last],
                                       reverse=True):
        print(f"  {mode:8} dur={dur:3} period={period:3}  "
              f"dist@{last}={r[last]}  {r}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
