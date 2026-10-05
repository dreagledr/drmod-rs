# -*- coding: utf-8 -*-
"""Покоординатный подъём высоты приёма в **суффиксной** модели таймингов.

Сдвиг группы — это вставленная перед ней пауза: он двигает **и саму группу, и
всё, что идёт за ней** (относительные зазоры внутри сохраняются). Так «задержать
y1 на +6» = `jump@50` остаётся, а `y1` и все последующие удары едут на +6 —
именно это и есть ручное «ввод на 50, а остальное подвинуть».

Группы (`combo_height.group_of`): `jump`, `y1`, `lr`, `y2`, `yspam`, `x1`,
`xrest`, `lt`. Помимо сдвигов перебираются **длительности** групп
(`--dur-groups`, прибавка к длительности каждой команды группы, ≥1).

Состояние пишется в `--state`; `--max-runs` ограничивает прогоны одного вызова —
повтор команды продолжит с сохранённого места и кэша.

Мир как у `combo_height.py`: `freeze` RNG + `trigger.ticks=0` + `restart` +
`/dt fixed` + `fps cap off`.
"""
import argparse
import json
import os
import sys
import types

import combo_height as ch
import drmod_api as api

GROUPS = ch.GROUPS
ANCHOR = {g: min(rec for rec, tok, _ in ch.BASE if ch.group_of(rec, tok) == g)
          for g in GROUPS}
EPS = 0.005


def build_script(base, end, offsets, durs, dur_deltas):
    """Суффиксная сборка: t = rec + base-сдвиг + Σ сдвигов групп, начавшихся до rec."""
    shift0 = base - ch.REC_JUMP
    ordered = sorted(GROUPS, key=lambda g: ANCHOR[g])
    cmds = []
    for rec, token, base_dur in ch.BASE:
        group = ch.group_of(rec, token)
        shift = shift0 + sum(offsets.get(g, 0) for g in ordered if ANCHOR[g] <= rec)
        t = rec + shift
        dur = durs.get(group)
        if dur is None:
            dur = max(1, base_dur + dur_deltas.get(group, 0))
        if t < 0:
            raise ValueError(f"команда {token}@{rec} уехала в t={t}")
        cmds.append({"t": t, "duration": dur, "input": dict(ch.TOKEN_INPUT[token])})
    total = max(c["t"] + c["duration"] for c in cmds)
    if end < total:
        raise ValueError(f"end={end} меньше конца команд ({total})")
    if end > total:
        cmds.append({"t": total, "duration": end - total,
                     "input": {"camera": [0.0, 0.0]}})
    return {"name": f"combo-b{base}", "trigger": {"ticks": 0},
            "restart": {"ups": 1}, "commands": cmds}


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--base", type=int, default=50)
    p.add_argument("--end", type=int, default=420)
    p.add_argument("--seed", type=lambda s: int(s, 0), default=1)
    p.add_argument("--no-freeze", dest="freeze", action="store_false")
    p.add_argument("--no-fixed-dt", dest="fixed_dt", action="store_false")
    p.add_argument("--cap", default="off")
    p.add_argument("--jump-dur", type=int, default=12)
    p.add_argument("--groups", default="jump,y1,y2,yspam,x1,xrest,lt",
                   help="группы сдвигов (порядок перебора)")
    p.add_argument("--dur-groups", default="y1,y2,yspam,x1,xrest,lt",
                   help="группы, у которых перебирается длительность")
    p.add_argument("--no-offsets", dest="offsets_on", action="store_false")
    p.add_argument("--no-durs", dest="durs_on", action="store_false")
    p.add_argument("--span", type=int, default=6)
    p.add_argument("--step", type=int, default=3)
    p.add_argument("--direction", choices=["both", "plus", "minus"], default="both")
    p.add_argument("--passes", type=int, default=1)
    p.add_argument("--max-runs", type=int, default=12)
    p.add_argument("--state", default=r"out\combo\suffix_state.json")
    p.add_argument("--timeout", type=float, default=40.0)
    p.add_argument("--rearm-wait", type=float, default=14.0)
    args = p.parse_args(argv)

    groups = [g.strip() for g in args.groups.split(",") if g.strip()]
    dur_groups = [g.strip() for g in args.dur_groups.split(",") if g.strip()]
    for g in groups + dur_groups:
        if g not in GROUPS:
            print(f"неизвестная группа '{g}' (есть: {', '.join(GROUPS)})")
            return 2

    run_args = types.SimpleNamespace(freeze=args.freeze, dump=None,
                                     timeout=args.timeout,
                                     rearm_wait=args.rearm_wait)

    os.makedirs(os.path.dirname(args.state) or ".", exist_ok=True)
    state = {"offsets": {}, "dur_delta": {}, "durs": {"jump": args.jump_dur},
             "cache": {}, "runs": 0, "height": 0.0}
    if os.path.exists(args.state):
        with open(args.state, encoding="utf-8") as f:
            state.update(json.load(f))
    for g in groups:
        state["offsets"].setdefault(g, 0)
    for g in dur_groups:
        state["dur_delta"].setdefault(g, 0)

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
        with open(args.state, "w", encoding="utf-8") as f:
            json.dump(state, f, ensure_ascii=False, indent=1)

    def key_of(offsets, durs, dur_delta):
        return json.dumps([tuple(sorted(offsets.items())),
                           tuple(sorted(durs.items())),
                           tuple(sorted(dur_delta.items()))])

    def evaluate(offsets, durs, dur_delta, label=""):
        key = key_of(offsets, durs, dur_delta)
        if key in state["cache"]:
            return state["cache"][key]
        if state["runs"] >= args.max_runs:
            raise Budget()
        api.focus_and_settle()
        api.ensure_gameplay()
        script = build_script(args.base, args.end, offsets, durs, dur_delta)
        r = ch.run_once(script, args.seed, run_args)
        state["runs"] += 1
        if r is not None:
            state["cache"][key] = {"h": round(r["height"], 4),
                                   "max_y": round(r["max_y"], 4)}
        save()
        h = state["cache"].get(key, {}).get("h", "ERR")
        print(f"  [{label or '?'}] высота={h} (runs={state['runs']})", flush=True)
        return state["cache"].get(key)

    offsets = dict(state["offsets"])
    durs = {"jump": state["durs"].get("jump", args.jump_dur)}
    dur_delta = dict(state["dur_delta"])
    start = evaluate(offsets, durs, dur_delta, "текущее")
    current = start["h"] if start else state.get("height", 0.0)
    print(f"старт: offsets={offsets} dur_delta={dur_delta} высота={current:.3f}")

    candidates = []
    for d in range(args.step, args.span + 1, args.step):
        if args.direction in ("both", "plus"):
            candidates.append(d)
        if args.direction in ("both", "minus"):
            candidates.append(-d)

    try:
        for p in range(args.passes):
            print(f"\n== проход {p + 1}/{args.passes} ==")
            if args.offsets_on:
                for g in groups:
                    best_delta, best = 0, current
                    for d in candidates:
                        cand = dict(offsets)
                        cand[g] = offsets.get(g, 0) + d
                        r = evaluate(cand, durs, dur_delta, f"shift {g}{d:+d}")
                        if r and r["h"] > best + EPS:
                            best, best_delta = r["h"], d
                    if best_delta:
                        offsets[g] = offsets.get(g, 0) + best_delta
                        current = best
                        state["offsets"] = dict(offsets)
                        save()
                        print(f"  -> сдвиг {g}: {best_delta:+d}, "
                              f"высота={current:.3f}")
                    else:
                        print(f"  -> сдвиг {g}: без улучшения ({current:.3f})")
            if args.durs_on:
                for g in dur_groups:
                    best_delta, best = 0, current
                    for d in candidates:
                        cand = dict(dur_delta)
                        cand[g] = dur_delta.get(g, 0) + d
                        r = evaluate(offsets, durs, cand, f"dur {g}{d:+d}")
                        if r and r["h"] > best + EPS:
                            best, best_delta = r["h"], d
                    if best_delta:
                        dur_delta[g] = dur_delta.get(g, 0) + best_delta
                        current = best
                        state["dur_delta"] = dict(dur_delta)
                        save()
                        print(f"  -> dur {g}: {best_delta:+d}, "
                              f"высота={current:.3f}")
                    else:
                        print(f"  -> dur {g}: без улучшения ({current:.3f})")
    except Budget:
        print(f"\nбюджет {args.max_runs} прогонов исчерпан — повторите команду.")

    state["height"] = current
    save()
    print(f"\nитог: offsets={offsets} dur_delta={dur_delta} durs={durs} "
          f"высота={current:.3f} (runs={state['runs']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
