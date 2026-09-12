# -*- coding: utf-8 -*-
"""Перебор таймингов под детерминированный прогон (freeze RNG + тиковый триггер).

Отличие от `sweep.py`: прогоны идут в **детерминированном** мире —

1. `POST /rng {"pin":"freeze","seed":S}` — состояние LCG решений ИИ врага
   замораживается на первом тике скрипта: значения перестают зависеть от
   порядка бросков между потоками (см. `docs/API.md` §3.10);
2. `trigger: {"ticks": 0}` вместо зоны по позиции — старт ровно на первом тике
   геймплея после загрузки, поэтому фаза врага одинакова от прогона к прогону
   (`docs/API.md` §3.3).

В таком мире одинаковый конфиг воспроизводится бит-в-бит, и по таблице
«кадр атаки → max_y» можно выбирать тайминги осмысленно (повтор в конце
проверяет это явно).

Примеры:
    # лестница по кадру тяжёлой атаки (эталонный перелёт ≥20 м)
    py -3 tools\\script_tuning\\timing_tune.py --seed 1 --trigger-ticks 0 \\
        --attacks 76,78,79,80,82 --ripper 112

    # стабильность выбранного варианта: 4 повтора
    py -3 tools\\script_tuning\\timing_tune.py --seed 1 --trigger-ticks 0 \\
        --attacks 80 --ripper 112 --repeat-best 4

Метрики на прогон: `max_y` (пик игрока), кадр прыжка врага (`anim 65545`),
`post_gain` (пик после удара), `hp` врага (для сверки фазы).
"""
import argparse
import sys
import time

import core117
import drmod_api as api

# Старт «сразу после загрузки» = отсчёт тиков геймплея с нуля.
TRIGGER_TICKS = 0


def stop_script():
    try:
        api.http(path="/script/stop", method="POST")
    except Exception:  # noqa: BLE001
        pass


def run_once(seed, attack, args, tries=3):
    """Один прогон; None — если рестарт не сработал и повторы исчерпаны."""
    for _ in range(tries):
        if args.freeze:
            api.http(path="/rng", method="POST",
                     body={"pin": "freeze", "seed": seed})
        script = core117.build(jump=args.jump, run_frames=args.run_frames,
                               attack=attack, dur_attack=args.dur_attack,
                               ripper=args.ripper,
                               release_tail=args.release_tail,
                               ninja=not args.no_ninja,
                               ninja_flight=not args.no_ninja_flight)
        if args.trigger_ticks is not None:
            script["trigger"] = {"ticks": args.trigger_ticks}
        script["restart"] = {"ups": 1}
        sid = api.run_script(script)["script_id"]

        # Ждём конца прогона по `/script/{id}` (статус done/stopped), а не по
        # исчезновению `script` из `/state` — там запись остаётся, и ожидание
        # висит до таймаута. Если рестарт не сработал, фаза `running` не
        # наступит за REARM_WAIT — отменяем и повторяем.
        rearm_wait = args.rearm_wait
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
            if not started and time.monotonic() - t0 > rearm_wait:
                break
            time.sleep(0.05)
        if not started:
            stop_script()
            time.sleep(1.0)
            api.ensure_gameplay()
            continue

        frames = [f for f in api.logs(script_id=sid, limit=900)
                  if f.get("script_phase") == "running"]
        max_y = max((f["pos"][1] for f in frames), default=0.0)
        jump = next((i for i, f in enumerate(frames)
                     if (f.get("enemy") or {}).get("r_anim") == 65545), None)
        post = max((f["pos"][1] for f in frames[attack + 10:]), default=0.0)
        hp = next(((f.get("enemy") or {}).get("hp") for f in frames
                   if (f.get("enemy") or {}).get("found")), None)
        return max_y, jump, post, hp
    return None


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--seed", type=lambda s: int(s, 0), default=1,
                   help="сид заморозки LCG (freeze)")
    p.add_argument("--no-freeze", dest="freeze", action="store_false",
                   help="не трогать RNG (обычный недетерминированный прогон)")
    p.add_argument("--attacks", default="76,78,79,80,82",
                   help="кадры тяжёлой атаки через запятую")
    p.add_argument("--jump", type=int, default=39)
    p.add_argument("--run-frames", type=int, default=6)
    p.add_argument("--dur-attack", type=int, default=24)
    p.add_argument("--ripper", type=int, default=112,
                   help="кадр риппера (должен быть >= attack + dur_attack)")
    p.add_argument("--release-tail", type=int, default=6,
                   help="зазор без ввода перед атакой («стоп в воздухе»)")
    p.add_argument("--no-ninja", dest="no_ninja", action="store_true")
    p.add_argument("--no-ninja-flight", dest="no_ninja_flight",
                   action="store_true")
    p.add_argument("--trigger-ticks", type=int, default=TRIGGER_TICKS,
                   help="старт через N тиков геймплея (0 — сразу после загрузки); "
                        "без флага — позиционный триггер из core117")
    p.add_argument("--repeat-best", type=int, default=0,
                   help="сколько раз повторить лучший вариант (проверка "
                        "детерминизма)")
    p.add_argument("--timeout", type=float, default=40.0)
    p.add_argument("--rearm-wait", type=float, default=14.0,
                   help="сколько ждать фазу running, прежде чем счесть рестарт "
                        "неудачным и повторить")
    args = p.parse_args(argv)

    api.setup_stdout()
    api.focus_and_settle()
    if not api.ensure_gameplay():
        print("не в геймплее")
        return 2

    print(f"seed=0x{args.seed:X} freeze={args.freeze} "
          f"trigger_ticks={args.trigger_ticks} jump={args.jump} "
          f"ripper={args.ripper} dur_attack={args.dur_attack} "
          f"release_tail={args.release_tail}")

    rows = []
    for attack in [int(x) for x in args.attacks.split(",")]:
        api.focus_and_settle()
        if not api.ensure_gameplay():
            print("не в геймплее")
            break
        try:
            r = run_once(args.seed, attack, args)
        except ValueError as e:
            print(f"  атака@{attack}: пропуск — {e}")
            continue
        if r is None:
            print(f"  атака@{attack}: не удалось (рестарт)")
            continue
        max_y, jump, post, hp = r
        rows.append((attack, max_y))
        print(f"  атака@{attack}: max_y={max_y:6.2f} прыжок врага={jump} "
              f"post_gain={post:6.2f} hp врага={hp}")

    if rows and args.repeat_best:
        best = max(rows, key=lambda r: r[1])[0]
        print(f"\nповтор атаки@{best} ×{args.repeat_best} (проверка детерминизма)")
        for k in range(args.repeat_best):
            api.focus_and_settle()
            api.ensure_gameplay()
            r = run_once(args.seed, best, args)
            if r is None:
                print(f"  повтор {k + 1}: не удалось")
                continue
            same = abs(r[0] - dict(rows)[best]) < 0.01
            print(f"  повтор {k + 1}: max_y={r[0]:.2f} прыжок={r[1]} "
                  f"hp врага={r[3]} {'СОВПАЛ' if same else 'РАЗОШЁЛСЯ'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
