# -*- coding: utf-8 -*-
"""Lightning strike (`anim 110`) вживую: свипы таймингов и зацикливание.

Приём — `вперёд, вперёд, хэви`. Пара ввода: `тап forward+blade` (2 кадра) →
`+4` кадра → `тап forward+heavy` (`heavy` держать ~6). Пара даёт `blade` →
Blade Mode (`anim 69`) → сам выходит → `anim 110`.

Режимы:
    --gaps "2,4,6"        пауза между тапами (2 → `81`, ≥4 → `110`)
    --offsets "10,14,18"  кадр второй пары (поиск зацикливания)
    --periods "44,52,60"  серия пар: одна пара на цикл при периоде ≥44

Мир — как есть (рестарта нет), перед каждым вариантом ждём `anim 0` и скорость
≈ 0 (в стане `197/208` приёмы не выходят). Перемещение НЕ мерим — критерий
тайминговый: приём штатно делают и в стену. Подробности и цифры —
`docs/LIGHTNING_STRIKE.md`.
"""
import argparse
import sys
import time

import drmod_api as api

FWD = {"forward": True}
FWD_H = {"forward": True, "heavy_attack": True}


def build(gap, heavy_dur, blade_first=False):
    """Одна пара: тап `forward` (при `--blade-first` — с `blade`), пауза, удар."""
    first = {"forward": True, "blade": True} if blade_first else FWD
    return [{"t": 0, "duration": 2, "input": first},
            {"t": gap, "duration": heavy_dur, "input": FWD_H}]


def build_loop(off, heavy_dur, blade_first=True):
    """Две пары: вторая со сдвигом `off` от начала скрипта."""
    first = {"forward": True, "blade": True} if blade_first else FWD
    return [{"t": 0, "duration": 2, "input": first},
            {"t": 4, "duration": heavy_dur, "input": FWD_H},
            {"t": off, "duration": 2, "input": first},
            {"t": off + 4, "duration": heavy_dur, "input": FWD_H}]


def build_chain(period, repeats, heavy_dur, blade_first=True):
    """Серия пар с периодом `period` (одна пара на цикл при период ≥44)."""
    first = {"forward": True, "blade": True} if blade_first else FWD
    cmds, t = [], 0
    for _ in range(repeats):
        cmds.append({"t": t, "duration": 2, "input": first})
        cmds.append({"t": t + 4, "duration": heavy_dur, "input": FWD_H})
        t += period
    return cmds


def settle(timeout=20.0, need=4):
    """Ждём устойчивую нейтраль: геймплей, `anim 0`, скорость ≈ 0."""
    t0, streak = time.monotonic(), 0
    while time.monotonic() - t0 < timeout:
        try:
            st = api.state()
        except Exception:  # noqa: BLE001
            streak = 0
            time.sleep(0.2)
            continue
        p = st.get("player") or {}
        v = p.get("vel") or [0, 0, 0]
        if (st.get("menu_status") == "In Game" and p.get("r_anim") == 0
                and all(abs(x) < 0.05 for x in v)):
            streak += 1
            if streak >= need:
                return True
        else:
            streak = 0
        time.sleep(0.2)
    return False


def series_of(frames):
    """Сжатая серия анимаций: [[anim, длина], ...]."""
    out = []
    for f in frames:
        if not out or out[-1][0] != f["r_anim"]:
            out.append([f["r_anim"], 1])
        else:
            out[-1][1] += 1
    return out


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--gaps", default="2,4,6,8,10,12,16")
    p.add_argument("--offsets", default=None, help="кадры второй пары")
    p.add_argument("--periods", default=None, help="периоды серии пар")
    p.add_argument("--repeats", type=int, default=5, help="пар в серии")
    p.add_argument("--heavy-dur", type=int, default=6)
    p.add_argument("--blade-first", dest="blade_first", action="store_true",
                   help="к первому тапу forward добавить тап blade")
    p.add_argument("--timeout", type=float, default=15.0)
    a = p.parse_args(argv)
    api.setup_stdout()
    api.focus_and_settle()
    if not api.ensure_gameplay():
        print("не в геймплее")
        return 2

    if a.offsets is not None:
        plans = [(f"off{off}", build_loop(off, a.heavy_dur))
                 for off in [int(x) for x in a.offsets.split(",")]]
    elif a.periods is not None:
        plans = [(f"период {per:2d}", build_chain(per, a.repeats, a.heavy_dur))
                 for per in [int(x) for x in a.periods.split(",")]]
    else:
        plans = [(f"пауза {gap:2d}", build(gap, a.heavy_dur, a.blade_first))
                 for gap in [int(x) for x in a.gaps.split(",")]]

    print("вариант → серия анимаций (сжатая) → вердикт")
    for name, cmds in plans:
        api.focus_and_settle()
        api.ensure_gameplay()
        if not settle():
            print(f"  {name}: не удалось дождаться нейтрали")
            continue
        sid = api.run_script({"name": f"ls-{name}", "commands": cmds})["script_id"]
        api.wait_script(sid, timeout=a.timeout, quiet=True)
        frames = [f for f in api.logs(script_id=sid, limit=1500)
                  if f.get("script_phase") == "running"]
        if not frames:
            print(f"  {name}: кадров нет")
            continue
        series = series_of(frames)
        txt = " ".join(f"{an}x{n}" for an, n in series)
        first = next((an for an, _ in series if an in (110, 81, 94, 107)), None)
        verdict = {110: "ПРИЁМ 110", 81: "просто 81", 94: "ЛАУНЧ 94",
                   107: "107", None: "ничего"}.get(first, str(first))
        n110 = sum(1 for an, _ in series if an == 110)
        tail = f"  ← зациклилось: 110-отрезков {n110}" if n110 >= 2 else \
               f"  (110-отрезков: {n110})"
        print(f"  {name}: {txt}   → {verdict}{tail}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
