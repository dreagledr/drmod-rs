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
    for _ in range(a.strikes):
        cmds.append({"t": t, "duration": 2, "input": FWD_B})
        cmds.append({"t": t + 4, "duration": a.heavy_dur, "input": FWD_H})
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
    p.add_argument("--pair-start", type=int, default=210)
    p.add_argument("--strikes", type=int, default=2,
                   help="пар: 1 — приём, 2 — приём + кансел/фоллинг")
    p.add_argument("--period", type=int, default=56)
    p.add_argument("--heavy-dur", type=int, default=6)
    p.add_argument("--ripper-after", type=int, default=30,
                   help="кадров от удара до риппера (-1 — без риппера)")
    p.add_argument("--tail", type=int, default=0,
                   help="кадр маркера вдали (продлить лог до посадки)")
    p.add_argument("--seed", type=lambda s: int(s, 0), default=1)
    p.add_argument("--trigger-ticks", type=int, default=0)
    p.add_argument("--every", type=int, default=6)
    p.add_argument("--from", dest="frm", type=int, default=0)
    p.add_argument("--timeout", type=float, default=45.0)
    p.add_argument("--rearm-wait", type=float, default=14.0)
    a = p.parse_args(argv)
    api.setup_stdout()
    api.focus_and_settle()
    api.fixed_dt(True)
    if not api.ensure_gameplay():
        print("не в геймплее")
        return 2
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
