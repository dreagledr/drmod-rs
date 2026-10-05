# -*- coding: utf-8 -*-
"""Покоординатный подбор таймингов в **суффиксной** модели (общий для комбо).

Каждый ввод смотрит только на предыдущий: ручка — «вставить N кадров перед
i-м вводом», что двигает i-й ввод **и все последующие** (относительные зазоры
внутри хвоста сохраняются). Базой служит готовый скрипт (JSON-документ, напр.
`out\\combo\\5x.json`); сортируем команды по кадру и оптимизируем сдвиги.

Состояние (`offsets`) пишется в `--state`; `--max-runs` ограничивает прогоны
одного вызова, повтор команды продолжит с места и кэша. Мир как в
`combo_height.py`.

Пример:
    py -3 tools\\python tools... (см. ниже)
    py -3 tools\\script_tuning\\combo_suffix_opt.py --script out\\combo\\5x.json \\
        --span 6 --step 3 --max-runs 20 --state out\\combo\\5x_suffix.json
"""
import argparse
import json
import os
import sys
import types

import combo_height as ch
import drmod_api as api

EPS = 0.005


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--script", required=True, help="JSON-документ-база")
    p.add_argument("--end", type=int, default=420)
    p.add_argument("--seed", type=lambda s: int(s, 0), default=1)
    p.add_argument("--no-freeze", dest="freeze", action="store_false")
    p.add_argument("--no-fixed-dt", dest="fixed_dt", action="store_false")
    p.add_argument("--cap", default="off")
    p.add_argument("--span", type=int, default=6)
    p.add_argument("--step", type=int, default=3)
    p.add_argument("--passes", type=int, default=1)
    p.add_argument("--skip-first", dest="first_on", action="store_false",
                   help="не трогать первый ввод (сдвиг всех = трансляция)")
    p.add_argument("--max-runs", type=int, default=20)
    p.add_argument("--state", default=None)
    p.add_argument("--timeout", type=float, default=40.0)
    p.add_argument("--rearm-wait", type=float, default=14.0)
    args = p.parse_args(argv)

    doc = json.load(open(args.script, encoding="utf-8"))
    base = [c for c in doc["commands"] if set(c["input"]) != {"camera"}]
    base.sort(key=lambda c: c["t"])
    n = len(base)
    print(f"вводов: {n}")
    for i, c in enumerate(base):
        print(f"  [{i}] t={c['t']:4} dur={c['duration']:2} {list(c['input'])}")
    state_path = args.state or r"out\combo\suffix_opt_state.json"
    state = {"offsets": {str(i): 0 for i in range(n)},
             "cache": {}, "runs": 0, "height": 0.0}
    if os.path.exists(state_path):
        with open(state_path, encoding="utf-8") as f:
            state.update(json.load(f))
    for i in range(n):
        state["offsets"].setdefault(str(i), 0)

    def build(offsets):
        cum, cmds = 0, []
        for i, c in enumerate(base):
            cum += offsets.get(str(i), 0)
            t = c["t"] + cum
            if t < 0:
                raise ValueError(f"ввод {i} уехал в t={t}")
            cmds.append({"t": t, "duration": c["duration"], "input": c["input"]})
        total = max(c["t"] + c["duration"] for c in cmds)
        if args.end < total:
            raise ValueError(f"end={args.end} < конца {total}")
        if args.end > total:
            cmds.append({"t": total, "duration": args.end - total,
                         "input": {"camera": [0.0, 0.0]}})
        return {"name": doc.get("name", "combo") + "-suffix",
                "trigger": {"ticks": 0}, "restart": {"ups": 1}, "commands": cmds}

    run_args = types.SimpleNamespace(freeze=args.freeze, dump=None,
                                     timeout=args.timeout,
                                     rearm_wait=args.rearm_wait)
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
        with open(state_path, "w", encoding="utf-8") as f:
            json.dump(state, f, ensure_ascii=False, indent=1)

    def evaluate(offsets, label=""):
        key = json.dumps(sorted(offsets.items()))
        if key in state["cache"]:
            return state["cache"][key]
        if state["runs"] >= args.max_runs:
            raise Budget()
        api.focus_and_settle()
        api.ensure_gameplay()
        r = ch.run_once(build(offsets), args.seed, run_args)
        state["runs"] += 1
        if r is not None:
            state["cache"][key] = {"h": round(r["height"], 4),
                                   "max_y": round(r["max_y"], 4)}
        save()
        h = state["cache"].get(key, {}).get("h", "ERR")
        print(f"  [{label or '?'}] высота={h} (runs={state['runs']})", flush=True)
        return state["cache"].get(key)

    offsets = {k: int(v) for k, v in state["offsets"].items()}
    start = evaluate(offsets, "текущее")
    current = start["h"] if start else state.get("height", 0.0)
    print(f"старт: offsets={offsets} высота={current:.3f}")

    candidates = []
    for d in range(args.step, args.span + 1, args.step):
        candidates += [d, -d]

    order = list(range(n)) if args.first_on else list(range(1, n))
    try:
        for p in range(args.passes):
            print(f"\n== проход {p + 1}/{args.passes} ==")
            for i in order:
                best_d, best = 0, current
                for d in candidates:
                    cand = dict(offsets)
                    cand[str(i)] = offsets.get(str(i), 0) + d
                    try:
                        r = evaluate(cand, f"i{i}{d:+d}")
                    except ValueError:
                        continue
                    if r and r["h"] > best + EPS:
                        best, best_d = r["h"], d
                if best_d:
                    offsets[str(i)] = offsets.get(str(i), 0) + best_d
                    current = best
                    state["offsets"] = dict(offsets)
                    save()
                    print(f"  -> ввод {i}: {best_d:+d}, высота={current:.3f}")
                else:
                    print(f"  -> ввод {i}: без улучшения ({current:.3f})")
    except Budget:
        print(f"\nбюджет {args.max_runs} прогонов исчерпан — повторите команду.")

    state["height"] = current
    save()
    print(f"\nитог: offsets={offsets} высота={current:.3f} (runs={state['runs']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
