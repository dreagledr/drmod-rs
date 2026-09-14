# -*- coding: utf-8 -*-
"""R-03 TAS baseline: барьер -> BM-стойка -> lightning strike -> кансел ->
falling lightning -> риппер -> посадка.

Связка целиком (проверено живьём 2026-09-13, debug-сборка; кадры скрипта):

| кадр        | что                                                             |
|-------------|-----------------------------------------------------------------|
| 0-78        | `core117`: разгон, прыжок, тяжёлая атака (`ninja`) — лаунч `94`  |
| 174         | пик перелёта, `y ≈ 21.5`                                        |
| 186-196     | удержание BM (`blade`): посадка на барьер, стойка `y ≈ 20.04`    |
| 210 / +4    | пара: тап `вперёд`+`blade`, затем `вперёд`+`heavy`               |
| 222-264     | **lightning strike `110`** с барьера (`110 ≈ 48` кадров)         |
| 266 / 270   | вторая пара: тап `blade` = **кансел** `110`, `heavy` = фоллинг    |
| 276-…       | **falling lightning `94`** — летит вниз                          |
| +30 к удару | **риппер** (в замере — кадр 300, в полёте, `y ≈ 14.5`)           |
| ~346        | посадка на землю (`y ≈ 0.11`), анимация `94` продолжается        |
| итог        | игрок с барьера `(−7.2, 20.0, 47.4)` уезжает на `(−21.2, 0.1, 2.8)` |

Ключевое: риппер надо жать **в полёте** (~+30 кадров к тяжёлой) — он удлиняет
`falling lightning` до самой земли; при +60/+90 он попадает уже после посадки и
удлинения не даёт. Обязательное условие связки — `end` у `core117` кончается
*до* первой пары (хвост держит `ninja_run`+`forward`, а с ninja `forward`+`heavy`
даёт лаунч `94`, не приём `110`).

Мир: `POST /dt {"fixed":true}` + `POST /rng {"pin":"freeze","seed":1}` +
`trigger.ticks=0` + рестарт миссии (прогон воспроизводится бит-в-бит).
Подробности приёма — `docs/LIGHTNING_STRIKE.md`.
"""
import argparse
import sys
import time

import core117
import drmod_api as api

FWD = {"forward": True}
FWD_B = {"forward": True, "blade": True}
FWD_H = {"forward": True, "heavy_attack": True}
BLADE = {"blade": True}
RIPPER = {"ripper": True}


def build(a):
    """Команды связки (тайминги — из опций, по умолчанию проверенные)."""
    cmds = core117.build(jump=a.jump, run_frames=a.run_frames, attack=a.attack,
                         dur_attack=a.dur_attack, ripper=a.ripper,
                         release_tail=a.release_tail, ninja=True,
                         ninja_flight=False, end=a.end)["commands"]
    cmds.append({"t": a.bm_start, "duration": a.bm_end - a.bm_start,
                 "input": BLADE})
    t = a.pair_start
    for i in range(a.strikes):
        # Аналоговое направление: приём — два тапа стика в ОДНУ сторону, а
        # дискретные биты дают только ±45°, поэтому направление задаём стиком
        # (`left_stick` перекрывает стик от бита forward).
        sx = None
        if i == 0:
            sx = a.aim_x
        elif i == a.strikes - 1:
            sx = a.fall_x
        stick = [sx, -1000.0] if sx is not None else None
        tap = dict(FWD_B)
        heavy = dict(FWD_H)
        if stick:
            tap["left_stick"] = stick
            heavy["left_stick"] = stick
        dur = a.heavy_dur
        if a.strike_right and i == 0:
            # Дискретный поворот вправо (грубый: 0 кадров → x -16.4, ≥1 → +18.8).
            if a.strike_right in ("both", "tap"):
                tap["right"] = True
            if a.strike_right in ("both", "heavy"):
                heavy["right"] = True
        if a.fall_right and i == a.strikes - 1:
            heavy["right"] = True
            dur = a.fall_dur
        cmds.append({"t": t, "duration": 2, "input": tap})
        cmds.append({"t": t + 4, "duration": dur, "input": heavy})
        t += a.period
    if a.ripper_after >= 0:
        cmds.append({"t": a.pair_start + a.period * (a.strikes - 1) + 4
                     + a.ripper_after, "duration": 2, "input": RIPPER})
    if a.tail:
        # Маркер вдали: мод пишет кадры только пока идёт скрипт, без него лог
        # обрывается в воздухе и посадки не видно. `camera_reset` — безобидно и
        # не трогает состояние риппера.
        cmds.append({"t": a.tail, "duration": 2,
                     "input": {"camera_reset": True}})
    return cmds


def stop_script():
    try:
        api.http(path="/script/stop", method="POST")
    except Exception:  # noqa: BLE001
        pass


def run_once(a, tries=3):
    for _ in range(tries):
        api.http(path="/rng", method="POST",
                 body={"pin": "freeze", "seed": a.seed})
        sid = api.run_script({"name": "r03-baseline", "commands": build(a),
                              "trigger": {"ticks": a.trigger_ticks},
                              "restart": {"ups": 1}})["script_id"]
        started, t0 = False, time.monotonic()
        while time.monotonic() - t0 < a.timeout:
            try:
                st = api.http(path=f"/script/{sid}")
            except Exception:  # noqa: BLE001
                time.sleep(0.1)
                continue
            if st["status"] == "running":
                started = True
            if st["status"] in ("done", "stopped"):
                break
            if not started and time.monotonic() - t0 > a.rearm_wait:
                break
            time.sleep(0.05)
        if not started:
            stop_script()
            time.sleep(1.0)
            api.ensure_gameplay()
            continue
        return [f for f in api.logs(script_id=sid, limit=1500)
                if f.get("script_phase") == "running"]
    return None


def series_of(frames):
    out = []
    for f in frames:
        if not out or out[-1][0] != f["r_anim"]:
            out.append([f["r_anim"], 1])
        else:
            out[-1][1] += 1
    return out


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--jump", type=int, default=39)
    p.add_argument("--run-frames", type=int, default=6)
    p.add_argument("--attack", type=int, default=78)
    p.add_argument("--dur-attack", type=int, default=24)
    p.add_argument("--ripper", type=int, default=112)
    p.add_argument("--release-tail", type=int, default=6)
    p.add_argument("--end", type=int, default=206,
                   help="конец хвоста core117 (до первой пары!)")
    p.add_argument("--bm-start", type=int, default=186)
    p.add_argument("--bm-end", type=int, default=196)
    p.add_argument("--pair-start", type=int, default=214)
    p.add_argument("--strikes", type=int, default=2,
                   help="пар: 1 — приём, 2 — приём + кансел/фоллинг")
    p.add_argument("--period", type=int, default=52)
    p.add_argument("--heavy-dur", type=int, default=6)
    p.add_argument("--ripper-after", type=int, default=26,
                   help="кадров от удара до риппера (-1 — без риппера)")
    p.add_argument("--strike-right", dest="strike_right", default=None,
                   choices=["both", "tap", "heavy"],
                   help="первый приём forward+right: both/tap/heavy")
    p.add_argument("--fall-right", dest="fall_right", action="store_true",
                   help="последний удар (фоллинг-лайтнинг) жать forward+right+heavy")
    p.add_argument("--fall-dur", type=int, default=6,
                   help="длительность удержания последнего удара (право)")
    p.add_argument("--aim-x", type=float, default=190.0,
                   help="left_stick.x (аналоговое направление) для ОБОИХ тапов "
                        "первого приёма; forward = y -1000")
    p.add_argument("--fall-x", type=float, default=None,
                   help="left_stick.x для удара фоллинг-лайтнинга "
                        "(по умолчанию — как --aim-x)")
    p.add_argument("--aim-start", type=int, default=None,
                   help="кадр начала прицела (по умолчанию pair_start-4)")
    p.add_argument("--aim-dur", type=int, default=14,
                   help="кадров поворота камеры (прицел)")
    p.add_argument("--aims", default=None,
                   help="свип прицела, напр. '-600,-300,0,300,600'")
    p.add_argument("--target", default=None,
                   help="цель посадки для сравнения, 'x,y,z'")
    p.add_argument("--tail", type=int, default=0,
                   help="кадр маркера вдали (продлить лог до посадки)")
    p.add_argument("--seed", type=lambda s: int(s, 0), default=1)
    p.add_argument("--trigger-ticks", type=int, default=0)
    p.add_argument("--every", type=int, default=6)
    p.add_argument("--from", dest="frm", type=int, default=0)
    p.add_argument("--timeout", type=float, default=45.0)
    p.add_argument("--rearm-wait", type=float, default=14.0)
    a = p.parse_args(argv)
    if a.aim_start is None:
        a.aim_start = a.pair_start - 4
    if a.fall_x is None:
        a.fall_x = a.aim_x
    api.setup_stdout()
    api.focus_and_settle()
    api.fixed_dt(True)
    if not api.ensure_gameplay():
        print("не в геймплее")
        return 2
    if a.aims is not None:
        # Свип аналогового направления: печатаем точку посадки и Δ до цели.
        target = a.target
        tx, ty, tz = (float(v) for v in target.split(",")) \
            if target else (None, None, None)
        print("направление (stick x) → посадка → смещение от цели")
        for aim in [float(x) for x in a.aims.split(",")]:
            a.aim_x = aim
            a.fall_x = aim
            frames = run_once(a)
            if not frames:
                print(f"  aim {aim:+.0f}: прогон не удался")
                continue
            last = frames[-1]
            p = last["pos"]
            extra = ""
            if tx is not None:
                extra = (f"   Δ=({p[0] - tx:+.2f},{p[1] - ty:+.2f},"
                         f"{p[2] - tz:+.2f})")
            print(f"  aim {aim:+.0f}: pos=({p[0]:7.2f},{p[1]:6.2f},{p[2]:7.2f}) "
                  f"anim={last['r_anim']}{extra}")
        return 0
    frames = run_once(a)
    if not frames:
        print("прогон не удался")
        return 1
    for i in range(a.frm, len(frames), a.every):
        f = frames[i]
        print(f"  {i:3d}: anim={f['r_anim']:>4} "
              f"pos=({f['pos'][0]:7.2f},{f['pos'][1]:6.2f},"
              f"{f['pos'][2]:7.2f}) blade={f['blade']} "
              f"fed={f['fed_down_bits']:06X}/{f['fed_pressed_bits']:06X}")
    print("серия: " + " ".join(f"{an}x{n}" for an, n in series_of(frames)))
    if a.ripper_after >= 0:
        at = a.pair_start + a.period * (a.strikes - 1) + 4 + a.ripper_after
        if 0 <= at < len(frames):
            f = frames[at]
            print(f"риппер на кадре {at}: y={f['pos'][1]:.2f} anim={f['r_anim']}")
    last = frames[-1]
    print(f"конец: pos=({last['pos'][0]:.2f},{last['pos'][1]:.2f},"
          f"{last['pos'][2]:.2f}) anim={last['r_anim']} "
          f"ripper={last.get('ripper')} ({len(frames)} кадров)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
