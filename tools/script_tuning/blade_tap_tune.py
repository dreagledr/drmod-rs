# -*- coding: utf-8 -*-
"""Перебор кадра тапа Blade Mode после риппера — детерминированный мир.

Мир фиксируется так, как требует исследование:

1. `POST /dt {"fixed": true}` — шаг симуляции ровно 1/60 с (движок замедляется,
   зато окна анимации/физики стабильны, см. `docs/API.md` §3.8);
2. `POST /rng {"pin":"freeze","seed":S}` — заморозка LCG решений ИИ врага (§3.10),
   вместе с `trigger: {"ticks": 0}` (§3.3) прогон воспроизводится бит-в-бит;
3. кап кадров **не трогаем** — игра идёт с тем FPS, что есть.

Поверх эталонной связки `core117` (jump=39, run_frames=6, attack=80,
dur_attack=24, ripper=112, release_tail=6, ninja, ниninja_flight=False) кладётся
команда `blade` — «тап» Blade Mode на кадре `--frames`, который и перебирается.

Примеры:
    # базовая точка без тапа + таймлайн анимаций игрока (ориентир по фазам)
    py -3 tools\\script_tuning\\blade_tap_tune.py --baseline --dump

    # плавающее окно тапа: кадры 116..170 через 2, длительность 3
    py -3 tools\\script_tuning\\blade_tap_tune.py --frames 116:170:2 --bm-dur 3

    # проверить лучший кадр на воспроизводимость
    py -3 tools\\script_tuning\\blade_tap_tune.py --frames 140 --repeat-best 3
"""
import argparse
import csv
import os
import sys
import time

import core117
import drmod_api as api

#: Бит Blade Mode в InputUnit (см. `docs/API.md` §4.2 — `blade` это 0x800).
BLADE_BIT = 0x800
#: Старт точно на первом тике геймплея после загрузки.
TRIGGER_TICKS = 0
#: Z спавна — база для дальности перелёта (`dz`).
SPAWN_Z = core117.SPAWN[2]


def parse_frames(spec):
    """`116:170:2` / `116,120,140` / `140` → отсортированный список кадров."""
    out = []
    for part in spec.split(","):
        part = part.strip()
        if not part:
            continue
        bits = part.split(":")
        if len(bits) == 1:
            out.append(int(bits[0]))
        elif len(bits) in (2, 3):
            lo, hi = int(bits[0]), int(bits[1])
            step = int(bits[2]) if len(bits) == 3 else 1
            if step <= 0:
                raise ValueError(f"шаг должен быть > 0: {part}")
            out.extend(range(lo, hi + 1, step))
        else:
            raise ValueError(f"не разобрал кадры: {part}")
    return sorted(set(out))


def build_script(bm, bm_dur, args):
    """Эталонная связка + тап `blade` на кадре `bm` (`bm=None` — без тапа)."""
    end = args.end
    if bm is not None:
        # Тап должен уложиться в скрипт (иначе игра не увидит команду).
        end = max(end, bm + bm_dur + 5)
    script = core117.build(jump=args.jump, run_frames=args.run_frames,
                           attack=args.attack, dur_attack=args.dur_attack,
                           ripper=args.ripper, release_tail=args.release_tail,
                           ninja=args.ninja, ninja_flight=args.ninja_flight,
                           end=end)
    if bm is not None:
        # Отдельная команда поверх хвоста `forward` — активные команды
        # объединяются по ИЛИ (§4.3), тап не режет бег.
        script["commands"].append({"t": bm, "duration": bm_dur,
                                   "input": {"blade": True}})
    script["name"] = f"blade-tap-bm{bm if bm is not None else 'off'}"
    script["trigger"] = {"ticks": args.trigger_ticks}
    script["restart"] = {"ups": 1}
    return script


def stop_script():
    try:
        api.http(path="/script/stop", method="POST")
    except Exception:  # noqa: BLE001
        pass


def run_once(bm, bm_dur, args, tries=3):
    """Один прогон. Возвращает `(summary, frames)`; `(None, None)` — рестарт не удался."""
    for _ in range(tries):
        api.http(path="/rng", method="POST",
                 body={"pin": "freeze", "seed": args.seed})
        script = build_script(bm, bm_dur, args)
        sid = api.run_script(script)["script_id"]

        started = False
        t0 = time.monotonic()
        while time.monotonic() - t0 < args.timeout:
            try:
                st = api.http(path=f"/script/{sid}")
            except Exception:  # noqa: BLE001
                time.sleep(0.1)
                continue
            if st["status"] == "running":
                started = True
            if st["status"] in ("done", "stopped"):
                break
            if not started and time.monotonic() - t0 > args.rearm_wait:
                break
            time.sleep(0.05)
        if not started:
            stop_script()
            time.sleep(1.0)
            api.ensure_gameplay()
            continue

        frames = [f for f in api.logs(script_id=sid, limit=1400)
                  if f.get("script_phase") == "running"]
        if not frames:
            return None, None
        ys = [f["pos"][1] for f in frames]
        max_y = max(ys)
        t_max = ys.index(max_y)
        jump = next((i for i, f in enumerate(frames)
                     if (f.get("enemy") or {}).get("r_anim") == 65545), None)
        post = max(ys[args.attack + 10:], default=0.0)
        last = frames[-1]["pos"]
        # Посадка: первый кадр после пика, где игрок снова у земли. Без этого
        # «дальность» меряется по обрезанному логу (скрипт кончается раньше
        # приземления — особенно когда BM тормозит падение).
        t_land = next((i for i in range(t_max, len(frames)) if ys[i] <= 0.5), None)
        z_land = round(frames[t_land]["pos"][2], 2) if t_land is not None else ""
        hp = next(((f.get("enemy") or {}).get("hp") for f in frames
                   if (f.get("enemy") or {}).get("found")), None)

        def at(i, key, default=None):
            return frames[i].get(key, default) if 0 <= i < len(frames) else default

        def fed_blade(i):
            return bool((at(i, "fed_down_bits", 0) | at(i, "fed_pressed_bits", 0))
                        & BLADE_BIT)

        win = range(bm, bm + bm_dur) if bm is not None else range(0)
        summary = {
            "bm": bm if bm is not None else "",
            "bm_dur": bm_dur if bm is not None else 0,
            "max_y": round(max_y, 2),
            "t_max": t_max,
            "cleared": int(max_y >= 20.0),
            "jump_enemy": jump,
            "post_gain": round(post, 2),
            "hp": hp,
            "x_end": round(last[0], 2),
            "y_end": round(last[1], 2),
            "z_end": round(last[2], 2),
            "t_land": t_land if t_land is not None else "",
            "z_land": z_land,
            "dz": round(SPAWN_Z - z_land, 2) if t_land is not None else "",
            "y_at_bm": round(at(bm, "pos", [0, 0, 0])[1], 3) if bm is not None else "",
            "vy_at_bm": round(at(bm, "vel", [0, 0, 0])[1], 3) if bm is not None else "",
            "anim_at_bm": at(bm, "r_anim", "") if bm is not None else "",
            "ripper_at_bm": at(bm, "ripper", "") if bm is not None else "",
            # Реально ли ушёл бит Blade Mode в отрисовочные кадры тапа.
            "blade_fed": int(sum(1 for i in win if fed_blade(i))),
            # Состояние blade/blade_mode_type в окне тапа и сразу после.
            "blade_state": max((at(i, "blade", 0) or 0
                                for i in range(bm, bm + bm_dur + 8))
                               if bm is not None else [0]),
            "ripper_on": max((at(i, "ripper", 0) or 0 for i in range(len(frames))),
                             default=0),
        }
        return summary, frames
    return None, None


def dump_timeline(frames, path=None):
    """Компактный таймлайн: отрезки одной анимации игрока с высотой и флагами."""
    rows, seg = [], None
    for i, f in enumerate(frames):
        key = (f.get("r_anim"), f.get("ripper"), f.get("blade"))
        if seg is None or seg[0] != key:
            if seg is not None:
                rows.append(seg)
            seg = [key, i, i, f["pos"][1], f["pos"][1],
                   (f.get("enemy") or {}).get("r_anim")]
        seg[2] = i
        seg[3] = min(seg[3], f["pos"][1])
        seg[4] = max(seg[4], f["pos"][1])
    if seg is not None:
        rows.append(seg)
    print("  кадры    анимация  риппер блейд   y_min..y_max    враг")
    for key, a, b, ylo, yhi, eanim in rows:
        print(f"  {a:4d}-{b:4d}  {str(key[0]):>7}  {key[1]:>6} {key[2]:>5}   "
              f"{ylo:6.2f}..{yhi:6.2f}   {eanim}")
    if path:
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        with open(path, "w", newline="", encoding="utf-8") as fh:
            w = csv.writer(fh)
            w.writerow(["idx", "frame", "t_ms", "menu", "r_anim", "ripper", "blade",
                        "y", "vy", "fed_down", "fed_pressed", "enemy_anim",
                        "enemy_frame", "enemy_hp"])
            for i, f in enumerate(frames):
                e = f.get("enemy") or {}
                w.writerow([i, f.get("frame"), f.get("t_ms"), f.get("menu_status"),
                            f.get("r_anim"), f.get("ripper"), f.get("blade"),
                            f["pos"][1], f["vel"][1], f.get("fed_down_bits"),
                            f.get("fed_pressed_bits"), e.get("r_anim"),
                            e.get("frame"), e.get("hp")])
        print(f"  кадры → {path}")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--frames", default="",
                   help="кадры тапа: '116:170:2', '116,140' (по умолчанию — только база)")
    p.add_argument("--baseline", action="store_true",
                   help="прогон без тапа (проверка, что перелёт есть в этом мире)")
    p.add_argument("--bm-dur", type=int, default=3,
                   help="кадров удержания Blade Mode в тапе")
    p.add_argument("--seed", type=lambda s: int(s, 0), default=1)
    p.add_argument("--trigger-ticks", type=int, default=TRIGGER_TICKS)
    p.add_argument("--jump", type=int, default=39)
    p.add_argument("--run-frames", type=int, default=6)
    p.add_argument("--attack", type=int, default=80)
    p.add_argument("--dur-attack", type=int, default=24)
    p.add_argument("--ripper", type=int, default=112)
    p.add_argument("--release-tail", type=int, default=6)
    p.add_argument("--no-ninja", dest="no_ninja", action="store_true")
    p.add_argument("--ninja-flight", dest="ninja_flight", action="store_true",
                   help="держать ninja и в полёте (в эталоне README — отпущен)")
    p.add_argument("--end", type=int, default=240)
    p.add_argument("--repeat-best", type=int, default=0)
    p.add_argument("--out", default="out\\blade_tap.csv", help="CSV со сводкой")
    p.add_argument("--dump", action="store_true",
                   help="печать таймлайна анимаций (для базы/лучшего кадра)")
    p.add_argument("--timeout", type=float, default=40.0)
    p.add_argument("--rearm-wait", type=float, default=14.0)
    a = p.parse_args(argv)
    a.ninja = not a.no_ninja

    api.setup_stdout()
    api.focus_and_settle()

    # Мир: тик 1/60 фиксирован, RNG заморожен, кап кадров не трогаем.
    api.fixed_dt(True)
    st = api.state()
    print(f"мир: dt.fixed={st['dt']['fixed']} ms={st['dt']['fixed_ms']:.3f} "
          f"ticks={st['dt']['ticks']} | rng={st.get('rng_pin')} seed={a.seed} | "
          f"fps_cap={st.get('fps_cap')} fps={st.get('fps', 0):.1f}")
    print(f"связка: jump={a.jump} run_frames={a.run_frames} attack={a.attack} "
          f"dur_attack={a.dur_attack} ripper={a.ripper} "
          f"release_tail={a.release_tail} ninja={a.ninja} "
          f"ninja_flight={a.ninja_flight} trigger_ticks={a.trigger_ticks}")
    if not api.ensure_gameplay():
        print("не в геймплее")
        return 2

    runs = []  # (bm, bm_dur) к прогону
    if a.baseline or not a.frames:
        runs.append((None, a.bm_dur))
    runs.extend((f, a.bm_dur) for f in parse_frames(a.frames))

    rows, best, best_frames = [], None, None
    for bm, dur in runs:
        api.focus_and_settle()
        if not api.ensure_gameplay():
            print("не в геймплее")
            break
        summary, frames = run_once(bm, dur, a)
        if summary is None:
            print(f"  тап@{bm}: не удалось (рестарт)")
            continue
        rows.append(summary)
        print(f"  тап@{bm if bm is not None else 'off':>4}"
              f"{('+' + str(dur)) if bm is not None else '':>4}: "
              f"max_y={summary['max_y']:6.2f} t_max={summary['t_max']:3d} "
              f"cleared={summary['cleared']} прыжок врага="
              f"{summary['jump_enemy']} post_gain={summary['post_gain']:6.2f} "
              f"hp={summary['hp']} посадка z={summary['z_land']}@t"
              f"{summary['t_land']} dz={summary['dz']}"
              + (f" | y@тап={summary['y_at_bm']} vy={summary['vy_at_bm']} "
                 f"anim={summary['anim_at_bm']} ripper={summary['ripper_at_bm']} "
                 f"blade_fed={summary['blade_fed']}/{dur} blade={summary['blade_state']}"
                 if bm is not None else ""))
        if bm is None and a.dump:
            print("  таймлайн базы:")
            dump_timeline(frames, os.path.splitext(a.out)[0] + "_base_frames.csv")
        if bm is not None and (best is None or summary["max_y"] > best["max_y"]):
            best, best_frames = summary, frames

    if rows:
        os.makedirs(os.path.dirname(a.out) or ".", exist_ok=True)
        with open(a.out, "w", newline="", encoding="utf-8") as fh:
            w = csv.DictWriter(fh, fieldnames=list(rows[0].keys()))
            w.writeheader()
            w.writerows(rows)
        print(f"\nсводка → {a.out}")

    if best is not None and a.dump:
        print(f"\nтаймлайн лучшего (тап@{best['bm']}):")
        dump_timeline(best_frames)

    if best is not None and a.repeat_best:
        print(f"\nповтор тапа@{best['bm']} ×{a.repeat_best} (детерминизм)")
        for k in range(a.repeat_best):
            api.focus_and_settle()
            api.ensure_gameplay()
            summary, _ = run_once(best["bm"], a.bm_dur, a)
            if summary is None:
                print(f"  повтор {k + 1}: не удалось")
                continue
            same = (abs(summary["max_y"] - best["max_y"]) < 0.01
                    and summary["jump_enemy"] == best["jump_enemy"])
            print(f"  повтор {k + 1}: max_y={summary['max_y']:.2f} "
                  f"прыжок={summary['jump_enemy']} hp={summary['hp']} "
                  f"{'СОВПАЛ' if same else 'РАЗОШЁЛСЯ'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
