# -*- coding: utf-8 -*-
"""Короткий вариант приёма `… x lt`: префикс оптимума + один `lt`, ищем кадр.

Префикс — начало оптимума (прыжок@50, хэви@75, риппер@107, хэви@108,
хэви-спам@120–131, один лёгкий `x`@150). Вместо серии `x… lt` подаётся один
`lt` (Blade Mode); перебирается **кадр старта `lt`** (и, опционально,
длительность). Метрика — высота `max_y − y0`, мир как у `combo_height.py`.

Пример:
    py -3 tools\\script_tuning\\combo_short.py --starts 152:248:8 --lt-durs 7
"""
import argparse
import sys
import types

import combo_height as ch
import drmod_api as api

#: Оптимум (суффиксный) до первого лёгкого включительно: (кадр, токен, длит.).
#: Хэви только два (`y`@75 и `y`@108) — `y`-спам из записи (кадры 120–131)
#: нейтрален по высоте и выброшен (проверено: без него те же 5.47/4.92).
PREFIX = [
    (50, "a", 12),
    (75, "y", 10),
    (107, "lr", 1),
    (108, "y", 12),
    (150, "x", 2),
]
PREFIX_END = max(t + d for t, _, d in PREFIX)


def build_short(lt_start, lt_dur, end=420):
    if lt_start < PREFIX_END:
        raise ValueError(f"lt@{lt_start} перекрывает префикс (конец {PREFIX_END})")
    cmds = [{"t": t, "duration": d, "input": dict(ch.TOKEN_INPUT[tok])}
            for t, tok, d in PREFIX]
    cmds.append({"t": lt_start, "duration": lt_dur, "input": {"blade": True}})
    total = max(c["t"] + c["duration"] for c in cmds)
    if end > total:
        cmds.append({"t": total, "duration": end - total,
                     "input": {"camera": [0.0, 0.0]}})
    return {"name": f"combo-short-lt{lt_start}", "trigger": {"ticks": 0},
            "restart": {"ups": 1}, "commands": cmds}


def range_list(text):
    if ":" in text:
        lo, _, rest = text.partition(":")
        if rest.count(":") == 1:
            hi, _, step = rest.partition(":")
        else:
            hi, step = rest, "1"
        return list(range(int(lo), int(hi) + 1, int(step)))
    return [int(x) for x in text.split(",")]


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--starts", default="152:248:8",
                   help="кадры старта lt: диапазон LO:HI:STEP или список")
    p.add_argument("--lt-durs", default="7", help="длительности lt через запятую")
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
    starts = range_list(args.starts)
    durs = [int(x) for x in str(args.lt_durs).split(",")]

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
    for dur in durs:
        for start in starts:
            api.focus_and_settle()
            api.ensure_gameplay()
            try:
                script = build_short(start, dur, args.end)
            except ValueError as e:
                print(f"  lt@{start} d{dur}: пропуск — {e}")
                continue
            r = ch.run_once(script, args.seed, run_args)
            if r is None:
                print(f"  lt@{start} d{dur}: не удалось")
                continue
            rows.append((start, dur, r))
            print(f"  lt@{start} d{dur}: высота={r['height']:6.2f} "
                  f"(max_y={r['max_y']:6.2f})  вперёд dx={r['dx']:6.2f} "
                  f"dz={r['dz']:6.2f} |{r['horiz']:5.2f}| "
                  f"max|{r['horiz_max']:5.2f}|  посадка y={r['y_end']:5.2f}",
                  flush=True)

    print("\n-- топ по высоте --")
    for start, dur, r in sorted(rows, key=lambda x: x[2]["height"],
                                reverse=True)[:10]:
        print(f"  lt@{start} d{dur}: высота={r['height']:.3f} "
              f"вперёд={r['horiz']:.2f}")
    print("\n-- топ по сдвигу вперёд --")
    for start, dur, r in sorted(rows, key=lambda x: x[2]["horiz"],
                                reverse=True)[:10]:
        print(f"  lt@{start} d{dur}: вперёд={r['horiz']:.2f} "
              f"высота={r['height']:.3f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
