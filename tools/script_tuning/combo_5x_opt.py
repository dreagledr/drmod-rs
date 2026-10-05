# -*- coding: utf-8 -*-
"""Покоординатный подбор кадра каждого из 5 лёгких ударов (и `lt`).

Каждый `x` — отдельный рычаг: перебирается его кадр при фиксированных
остальных (порядок сохраняется, `lt` — за последним `x`). Префикс —
прыжок@50, хэви@75, риппер@107, хэви@108.

Состояние пишется в `--state`; `--max-runs` ограничивает прогоны одного вызова,
повтор команды продолжит с сохранённого места и кэша. Мир как в
`combo_height.py` (freeze + ticks:0 + restart + /dt fixed + cap off).

Пример:
    py -3 tools\\script_tuning\\combo_5x_opt.py --state out\\combo\\5x_state.json \\
        --span 6 --step 3 --max-runs 18
"""
import argparse
import json
import os
import sys
import types

import combo_5x as c5
import combo_height as ch
import drmod_api as api

PREFIX = c5.PREFIX
PREFIX_END = c5.PREFIX_END
COUNT = 5
EPS = 0.005


def build(frames, lt, x_dur=1, lt_dur=7, end=420):
    cmds = [{"t": t, "duration": d, "input": dict(ch.TOKEN_INPUT[tok])}
            for t, tok, d in PREFIX]
    for f in frames:
        cmds.append({"t": f, "duration": x_dur,
                     "input": {"light_attack": True}})
    cmds.append({"t": lt, "duration": lt_dur, "input": {"blade": True}})
    total = max(c["t"] + c["duration"] for c in cmds)
    if end > total:
        cmds.append({"t": total, "duration": end - total,
                     "input": {"camera": [0.0, 0.0]}})
    return {"name": f"combo-{len(frames)}x-opt", "trigger": {"ticks": 0},
            "restart": {"ups": 1}, "commands": cmds}


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--x0", type=int, default=150)
    p.add_argument("--gap", type=int, default=17)
    p.add_argument("--lt", type=int, default=221)
    p.add_argument("--x-dur", type=int, default=1)
    p.add_argument("--lt-dur", type=int, default=7)
    p.add_argument("--end", type=int, default=420)
    p.add_argument("--seed", type=lambda s: int(s, 0), default=1)
    p.add_argument("--no-freeze", dest="freeze", action="store_false")
    p.add_argument("--no-fixed-dt", dest="fixed_dt", action="store_false")
    p.add_argument("--cap", default="off")
    p.add_argument("--span", type=int, default=6)
    p.add_argument("--step", type=int, default=3)
    p.add_argument("--passes", type=int, default=1)
    p.add_argument("--max-runs", type=int, default=18)
    p.add_argument("--state", default=r"out\combo\5x_state.json")
    p.add_argument("--timeout", type=float, default=40.0)
    p.add_argument("--rearm-wait", type=float, default=14.0)
    args = p.parse_args(argv)

    run_args = types.SimpleNamespace(freeze=args.freeze, dump=None,
                                     timeout=args.timeout,
                                     rearm_wait=args.rearm_wait)
    os.makedirs(os.path.dirname(args.state) or ".", exist_ok=True)
    state = {"frames": [args.x0 + i * args.gap for i in range(COUNT)],
             "lt": args.lt, "cache": {}, "runs": 0, "height": 0.0}
    if os.path.exists(args.state):
        with open(args.state, encoding="utf-8") as f:
            state.update(json.load(f))
    frames = list(state["frames"])
    lt = state["lt"]

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

    class Budget(Exception):
        pass

    def save():
        state["frames"] = list(frames)
        state["lt"] = lt
        with open(args.state, "w", encoding="utf-8") as f:
            json.dump(state, f, ensure_ascii=False, indent=1)

    def evaluate(fs, l, label=""):
        key = json.dumps([fs, l])
        if key in state["cache"]:
            return state["cache"][key]
        if state["runs"] >= args.max_runs:
            raise Budget()
        api.focus_and_settle()
        api.ensure_gameplay()
        script = build(fs, l, args.x_dur, args.lt_dur, args.end)
        r = ch.run_once(script, args.seed, run_args)
        state["runs"] += 1
        if r is not None:
            state["cache"][key] = {"h": round(r["height"], 4),
                                   "max_y": round(r["max_y"], 4)}
        save()
        h = state["cache"].get(key, {}).get("h", "ERR")
        print(f"  [{label or '?'}] высота={h} (runs={state['runs']})", flush=True)
        return state["cache"].get(key)

    cur = evaluate(frames, lt, "текущее")
    current = cur["h"] if cur else state.get("height", 0.0)
    print(f"старт: x={frames} lt={lt} высота={current:.3f}")

    candidates = []
    for d in range(args.step, args.span + 1, args.step):
        candidates.append(d)
        candidates.append(-d)

    def bounds(i, fs, l):
        lo = PREFIX_END if i == 0 else fs[i - 1] + 1
        hi = (fs[i + 1] - 1) if i < len(fs) - 1 else l - 1
        return lo, hi

    try:
        for p in range(args.passes):
            print(f"\n== проход {p + 1}/{args.passes} ==")
            for i in range(COUNT):
                lo, hi = bounds(i, frames, lt)
                best_d, best = 0, current
                for d in candidates:
                    cand = list(frames)
                    cand[i] = frames[i] + d
                    if cand[i] < lo or cand[i] > hi:
                        continue
                    r = evaluate(cand, lt, f"x{i + 1}{d:+d}->{cand[i]}")
                    if r and r["h"] > best + EPS:
                        best, best_d = r["h"], d
                if best_d:
                    frames[i] += best_d
                    current = best
                    save()
                    print(f"  -> x{i + 1}: {best_d:+d} (кадр {frames[i]}), "
                          f"высота={current:.3f}")
                else:
                    print(f"  -> x{i + 1}: без улучшения ({current:.3f})")
            # lt
            best_d, best = 0, current
            for d in candidates:
                cand = lt + d
                if cand <= frames[-1]:
                    continue
                r = evaluate(frames, cand, f"lt{d:+d}->{cand}")
                if r and r["h"] > best + EPS:
                    best, best_d = r["h"], d
            if best_d:
                lt += best_d
                current = best
                save()
                print(f"  -> lt: {best_d:+d} (кадр {lt}), высота={current:.3f}")
            else:
                print(f"  -> lt: без улучшения ({current:.3f})")
    except Budget:
        print(f"\nбюджет {args.max_runs} прогонов исчерпан — повторите команду.")

    state["height"] = current
    save()
    print(f"\nитог: x={frames} lt={lt} высота={current:.3f} (runs={state['runs']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
