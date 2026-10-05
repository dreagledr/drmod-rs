# -*- coding: utf-8 -*-
"""Высота приёма `a y lr y x… lt` в детерминированном мире (freeze RNG + ticks).

База — запись пользователя (кадры 1617–1811): прыжок, хэви, риппер, хэви,
серия лёгких, Blade Mode. Стики в исходнике нулевые (левый 0,0 — игрок стоит),
поэтому они выброшены; камера (rsx/rsy) в высоту не входит.

Метрика — **высота**: `max_y − y0`, где `y0` — `pos.y` на первом тике приёма
(низ, до прыжка), `max_y` — пик `pos.y` за прогон.

Мир фиксируется сам, как в `timing_tune.py`:

1. `POST /rng {"pin":"freeze","seed":S}` — заморозка LCG решений ИИ;
2. `trigger: {"ticks": 0}` — старт ровно на первом тике геймплея после loading;
3. `restart: {"ups": 1}` — рестарт текущей миссии из меню паузы;
4. `POST /dt {"fixed": true}` — фиксированный шаг времени;
5. `POST /fps {"cap":"off"}` — без капа кадров (быстрее).

Приём ставится так, чтобы **прыжок был на заданном кадре** (`--base`, по
умолчанию 50): раньше игра ещё глотает ввод.

Примеры:
    # базовый замер (высота записи)
    py -3 tools\\script_tuning\\combo_height.py --base 50 --dump out\\combo\\base.json

    # сдвиг удара-лаунчера (`y` в окне кадров 101–113) на +1/+2
    py -3 tools\\script_tuning\\combo_height.py --shift 102:113 --deltas 0,1,2
"""
import argparse
import json
import sys
import time

import drmod_api as api

# Кадр записи, от которого отсчитываются сдвиги (первый ввод — прыжок).
REC_JUMP = 1617

# База: (кадр записи, токен, длительность). Стики/камера выброшены.
BASE = [
    (1617, "a", 12),
    (1636, "y", 10),
    (1668, "lr", 1),
    (1669, "y", 12),
    (1681, "y", 1),
    (1682, "y", 7),
    (1689, "y", 1),
    (1690, "y", 1),
    (1691, "y", 1),
    (1692, "y", 1),
    (1725, "x", 2),
    (1727, "x", 1),
    (1728, "x", 1),
    (1729, "x", 1),
    (1737, "x", 1),
    (1738, "x", 1),
    (1739, "x", 1),
    (1740, "x", 1),
    (1747, "x", 1),
    (1748, "x", 1),
    (1749, "x", 1),
    (1750, "x", 1),
    (1751, "x", 1),
    (1752, "x", 1),
    (1759, "x", 1),
    (1760, "x", 1),
    (1761, "x", 1),
    (1762, "x", 1),
    (1763, "x", 1),
    (1769, "x", 4),
    (1773, "x", 1),
    (1774, "x", 1),
    (1779, "x", 5),
    (1784, "x", 1),
    (1785, "x", 1),
    (1791, "x", 5),
    (1796, "x", 1),
    (1801, "x", 8),
    (1809, "x", 2),
    (1809, "lt", 2),
    (1811, "lt", 7),
]

#: токен → ключ `input` (см. docs/SCRIPT_DSL.md §3.1)
TOKEN_INPUT = {
    "a": {"jump": True},
    "y": {"heavy_attack": True},
    "x": {"light_attack": True},
    "lr": {"ripper": True},
    "lt": {"blade": True},
}

#: Группы команд для покоординатного перебора (сдвигаются целиком).
GROUPS = ["jump", "y1", "lr", "y2", "yspam", "x1", "xrest", "lt"]


def group_of(rec_frame, token):
    """К какой группе-рычагу относится команда записи."""
    if token == "a":
        return "jump"
    if token == "lr":
        return "lr"
    if token == "lt":
        return "lt"
    if token == "y":
        if rec_frame == 1636:
            return "y1"
        if rec_frame == 1669:
            return "y2"
        return "yspam"
    if token == "x":
        return "x1" if rec_frame == 1725 else "xrest"
    return token


def build_groups(base=50, end=420, offsets=None, durs=None, dur_deltas=None,
                 name=None):
    """Сборка скрипта из групповых сдвигов `offsets` (группа → кадры),
    переопределений длительности `durs` (группа → кадры, абсолютно) и прибавок
    к длительности `dur_deltas` (группа → ±кадры, ≥1)."""
    offsets = offsets or {}
    durs = durs or {}
    dur_deltas = dur_deltas or {}
    offset = base - REC_JUMP
    cmds = []
    for rec_frame, token, base_dur in BASE:
        group = group_of(rec_frame, token)
        t = rec_frame + offset + offsets.get(group, 0)
        dur = durs.get(group)
        if dur is None:
            dur = max(1, base_dur + dur_deltas.get(group, 0))
        if t < 0:
            raise ValueError(f"команда {token}@{rec_frame} уехала в t={t}")
        cmds.append({"t": t, "duration": dur, "input": dict(TOKEN_INPUT[token])})

    total = max(c["t"] + c["duration"] for c in cmds)
    if end < total:
        raise ValueError(f"end={end} меньше конца команд ({total}); задайте больше")
    if end > total:
        cmds.append({"t": total, "duration": end - total,
                     "input": {"camera": [0.0, 0.0]}})
    return {
        "name": name or f"combo-b{base}",
        "trigger": {"ticks": 0},
        "restart": {"ups": 1},
        "commands": cmds,
    }


#: порядок токенов в строке кадра (docs/SCRIPT_DSL.md §5, FlagKeys::ALL)
TAS_ORDER = ["a", "x", "y", "lr", "lt"]


def write_tas(script):
    """JSON-документ → канонический текст `.tas` (служебный хвост камеры
    пропускается — в комбо он не входит)."""
    from collections import OrderedDict
    lines = OrderedDict()
    for cmd in script["commands"]:
        inp = cmd["input"]
        if set(inp) == {"camera"}:
            continue
        for token, flag in TOKEN_INPUT.items():
            if any(inp.get(key) for key in flag):
                lines.setdefault(cmd["t"], []).append((token, cmd["duration"]))
    trig = script.get("trigger") or {}
    attrs = [f"name={script.get('name', 'script')}"]
    if "ticks" in trig:
        attrs.append(f"trig=ticks:{trig['ticks']}")
    if script.get("restart") is not None:
        attrs.append("restart")
    out = ["! " + " ".join(attrs)]
    for t in sorted(lines):
        toks = sorted(lines[t], key=lambda td: TAS_ORDER.index(td[0]))
        parts = [tok if dur == 1 else f"{tok}:{dur}" for tok, dur in toks]
        out.append(f"{t} " + " ".join(parts))
    return "\n".join(out) + "\n"


def build(base=50, end=400, levers=(), jump_dur=None, ripper_delta=0,
          jump_offset=0, name=None):
    """Собрать скрипт: команды записи, сдвинутые так, чтобы прыжок был на `base`.

    `levers` — список окон-рычагов `(lo, hi, delta)` в **базовых** (уже
    приведённых к `base`) кадрах: команды внутри окна сдвигаются на `delta`.
    Окна применяются по порядку к исходному кадру команды, всё вне — на месте.

    `jump_dur` — длительность удержания прыжка (`a`), по умолчанию 12 из записи.
    `ripper_delta` — сдвиг риппера (`lr`) относительно своего кадра (y2 = 102).
    """
    offset = base - REC_JUMP
    cmds = []
    for rec_frame, token, dur in BASE:
        t = rec_frame + offset
        for lo, hi, delta in levers:
            if lo <= t <= hi:
                t += delta
        if token == "a":
            t += jump_offset
            if jump_dur is not None:
                dur = jump_dur
        if token == "lr":
            t += ripper_delta
        if t < 0:
            raise ValueError(f"команда {token}@{rec_frame} уехала в t={t}")
        cmds.append({"t": t, "duration": dur, "input": dict(TOKEN_INPUT[token])})

    total = max(c["t"] + c["duration"] for c in cmds)
    if end < total:
        raise ValueError(f"end={end} меньше конца команд ({total}); задайте больше")
    # Хвост: держим камеру (right stick, нули) до `end`, чтобы скрипт дожил до
    # пика и посадки. На физику игрока не влияет (камеру core117 выбрасывал).
    if end > total:
        cmds.append({"t": total, "duration": end - total,
                     "input": {"camera": [0.0, 0.0]}})
    return {
        "name": name or f"combo-b{base}",
        "trigger": {"ticks": 0},
        "restart": {"ups": 1},
        "commands": cmds,
    }


def stop_script():
    try:
        api.http(path="/script/stop", method="POST")
    except Exception:  # noqa: BLE001
        pass


def run_once(script, seed, args, tries=4):
    """Один прогон; None — если рестарт не сработал и попытки исчерпаны."""
    for _ in range(tries):
        if args.freeze:
            api.http(path="/rng", method="POST",
                     body={"pin": "freeze", "seed": seed})
        sid = api.run_script(script)["script_id"]

        started = False
        t0 = time.monotonic()
        while time.monotonic() - t0 < args.timeout:
            try:
                st = api.http(path=f"/script/{sid}")
            except Exception:  # noqa: BLE001
                time.sleep(0.1)
                continue
            status = st["status"]
            if status == "running":
                started = True
            if status in ("done", "stopped"):
                break
            if not started and time.monotonic() - t0 > args.rearm_wait:
                break
            time.sleep(0.05)

        if not started:
            stop_script()
            time.sleep(1.0)
            api.ensure_gameplay()
            continue

        time.sleep(0.2)
        frames = api.logs(script_id=sid, limit=5000)
        if args.dump:
            with open(args.dump, "w", encoding="utf-8") as f:
                json.dump(frames, f, ensure_ascii=False, indent=1)
            print(f"  кадры → {args.dump}")

        flight = [f for f in frames if f.get("script_phase") == "running"]
        use = flight or frames
        if not use:
            continue
        ys = [f["pos"][1] for f in use]
        xs = [f["pos"][0] for f in use]
        zs = [f["pos"][2] for f in use]
        # Первые кадры после взвода объект игрока ещё не создан (pos 0,0,0) —
        # берём первый устойчивый низ, а не нуль.
        i0 = next((i for i, f in enumerate(use)
                   if any(abs(v) > 1e-6 for v in f["pos"])), 0)
        y0 = next((y for y in ys if abs(y) > 1e-6), ys[0])
        i_max = max(range(len(ys)), key=ys.__getitem__)
        dx, dz = xs[-1] - xs[i0], zs[-1] - zs[i0]
        horiz = (dx * dx + dz * dz) ** 0.5
        # только от появления игрока: кадры до него (pos 0,0,0) дают ложный выброс
        horiz_max = max(((xs[i] - xs[i0]) ** 2 + (zs[i] - zs[i0]) ** 2) ** 0.5
                        for i in range(i0, len(use)))
        return {
            "y0": y0,
            "max_y": ys[i_max],
            "height": ys[i_max] - y0,
            "t_max": i_max,
            "y_end": ys[-1],
            "x0": xs[i0], "z0": zs[i0], "x_end": xs[-1], "z_end": zs[-1],
            "dx": dx, "dz": dz, "horiz": horiz, "horiz_max": horiz_max,
            "n": len(use),
            "n_restart": len(frames) - len(flight),
        }
    return None


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--base", type=int, default=50,
                   help="кадр прыжка (первого ввода) в скрипте")
    p.add_argument("--end", type=int, default=400,
                   help="длина скрипта (кадров), чтобы дожить до посадки")
    p.add_argument("--seed", type=lambda s: int(s, 0), default=1)
    p.add_argument("--no-freeze", dest="freeze", action="store_false",
                   help="не трогать RNG (недетерминированный прогон)")
    p.add_argument("--no-fixed-dt", dest="fixed_dt", action="store_false",
                   help="не ставить /dt fixed")
    p.add_argument("--cap", default="off",
                   help="кап кадров: off (по умолчанию), game или число")
    p.add_argument("--shift", default=None,
                   help="окно рычага в виде LO:HI (базовые кадры)")
    p.add_argument("--deltas", default="0",
                   help="сдвиги окна через запятую, напр. 0,1,2")
    p.add_argument("--shift2", default=None,
                   help="второе окно рычага LO:HI, сдвиг фиксирован --delta2")
    p.add_argument("--delta2", type=int, default=0,
                   help="сдвиг второго окна (по умолчанию 0)")
    p.add_argument("--jump-durs", default=None,
                   help="длительности удержания прыжка через запятую, напр. 6,8,10,12")
    p.add_argument("--ripper-deltas", default=None,
                   help="сдвиг риппера относительно y2 через запятую, напр. -2,-1,0,1,2")
    p.add_argument("--jump-offsets", default=None,
                   help="сдвиги только прыжка (`a`) через запятую, напр. -9,-6,-3,0")
    p.add_argument("--repeat-best", type=int, default=0,
                   help="повторить лучший вариант (проверка детерминизма)")
    p.add_argument("--dump", default=None,
                   help="файл для сырых кадров последнего прогона")
    p.add_argument("--timeout", type=float, default=40.0)
    p.add_argument("--rearm-wait", type=float, default=14.0)
    args = p.parse_args(argv)

    api.setup_stdout()
    if args.fixed_dt:
        api.fixed_dt(True)
    if args.cap == "off":
        api.fps_cap(cap="off")
    elif args.cap == "game":
        api.fps_cap(cap="game")
    elif args.cap is not None:
        api.fps_cap(fps=int(args.cap))

    api.focus_and_settle()
    if not api.ensure_gameplay():
        print("не в геймплее")
        return 2

    def window(text):
        if not text:
            return None
        lo, _, hi = text.partition(":")
        return int(lo), int(hi or lo)

    shift1 = window(args.shift)
    shift2 = window(args.shift2)
    deltas = [int(x) for x in args.deltas.split(",")]
    jump_durs = ([int(x) for x in args.jump_durs.split(",")]
                 if args.jump_durs else [None])
    ripper_deltas = ([int(x) for x in args.ripper_deltas.split(",")]
                     if args.ripper_deltas else [0])
    jump_offsets = ([int(x) for x in args.jump_offsets.split(",")]
                    if args.jump_offsets else [0])

    def make_levers(delta):
        levers = []
        if shift1:
            levers.append((shift1[0], shift1[1], delta))
        if shift2:
            levers.append((shift2[0], shift2[1], args.delta2))
        return levers

    print(f"seed=0x{args.seed:X} freeze={args.freeze} base={args.base} "
          f"end={args.end} fixed_dt={args.fixed_dt} cap={args.cap} "
          f"shift={args.shift} shift2={args.shift2}+{args.delta2:+d} "
          f"jump_durs={args.jump_durs}")

    def run_arm(delta, jd, rd, jo, dump=False):
        api.focus_and_settle()
        if not api.ensure_gameplay():
            raise RuntimeError("не в геймплее")
        script = build(base=args.base, end=args.end,
                       levers=make_levers(delta), jump_dur=jd,
                       ripper_delta=rd, jump_offset=jo)
        return run_once(script, args.seed, args)

    rows = []
    for delta in deltas:
        for jd in jump_durs:
            for rd in ripper_deltas:
                for jo in jump_offsets:
                    label = (f"delta={delta:+d}"
                             + (f" jump_dur={jd}" if jd is not None else "")
                             + (f" ripper={rd:+d}" if rd else "")
                             + (f" jump={jo:+d}" if jo else ""))
                    try:
                        r = run_arm(delta, jd, rd, jo)
                    except Exception as e:  # noqa: BLE001
                        print(f"  {label}: {e}")
                        continue
                    if r is None:
                        print(f"  {label}: не удалось (рестарт)")
                        continue
                    rows.append({"key": (delta, jd, rd, jo),
                                 "height": r["height"]})
                    print(f"  {label}: y0={r['y0']:6.2f} max_y={r['max_y']:6.2f} "
                          f"высота={r['height']:6.2f} t_max={r['t_max']} "
                          f"y_end={r['y_end']:6.2f} n={r['n']}", flush=True)

    if rows and args.repeat_best:
        best = max(rows, key=lambda r: r["height"])
        delta, jd, rd, jo = best["key"]
        print(f"\nповтор delta={delta:+d} jump_dur={jd} ripper={rd:+d} "
              f"jump={jo:+d} ×{args.repeat_best} (проверка детерминизма)")
        for k in range(args.repeat_best):
            r = run_arm(delta, jd, rd, jo)
            if r is None:
                print(f"  повтор {k + 1}: не удалось")
                continue
            same = abs(r["height"] - best["height"]) < 0.01
            print(f"  повтор {k + 1}: высота={r['height']:.2f} "
                  f"{'СОВПАЛ' if same else 'РАЗОШЁЛСЯ'}")
    print("\n-- топ --")
    for row in sorted(rows, key=lambda r: r["height"], reverse=True)[:8]:
        print(f"  {row['key']}  высота={row['height']:.3f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
