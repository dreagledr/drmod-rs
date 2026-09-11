# -*- coding: utf-8 -*-
"""Пробник: N прогонов одного варианта с выгрузкой таймлайна врага и max_y.

Нужен, чтобы понять, чем отличаются удачные прогоны (подброс) от неудачных по
состоянию врага — это основа для адаптивного удара (по анимации врага, а не по
фиксированному кадру).

    py -3 tools\\script_tuning\\probe_enemy_runs.py --runs 6
    py -3 tools\\script_tuning\\probe_enemy_runs.py --runs 6 --jump 45 --attack 75
"""
import argparse
import json
import sys
import time

import core117
import drmod_api as api

ENEMY_LUNGE = {19, 65545, 1114113, 131078, 131072, 24, 655370, 131074, 131081}
#: Анимация врага в момент парирования (запись 142 и прогоны с подбросом): именно
#: в неё уходит враг, когда удар игрока попал в окно его прыжка → подброс.
ENEMY_PARRY = 1114113


def enemy_summary(frames, max_y):
    """Сжатая сводка по врагу: последовательность (anim, первый кадр анимации)."""
    timeline, prev, lunge_frames, parried = [], None, [], False
    for i, fr in enumerate(frames):
        e = fr.get("enemy") or {}
        anim = e.get("r_anim")
        if anim != prev:
            timeline.append(f"i{i}:{anim}")
            prev = anim
        if anim == ENEMY_PARRY:
            parried = True
        if anim in ENEMY_LUNGE:
            lunge_frames.append(i)
    lunge_at_peak = None
    if frames:
        ys = [f["pos"][1] for f in frames]
        peak = ys.index(max(ys))
        near = [i for i in lunge_frames if abs(i - peak) <= 25]
        lunge_at_peak = near[-1] if near else (lunge_frames[-1] if lunge_frames else None)
    return " ".join(timeline[:14]), lunge_at_peak, parried


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--runs", type=int, default=6)
    p.add_argument("--jump", type=int, default=None)
    p.add_argument("--attack", type=int, default=None)
    p.add_argument("--run-frames", type=int, default=2)
    p.add_argument("--t-run", type=int, default=36)
    p.add_argument("--attack-when-enemy", default=None,
                   help='JSON-условие адаптивного удара, напр. '
                        '\'{"anim": [65545], "frame_max": 60, "dist_max": 2.5}\'')
    p.add_argument("--attack-duration", type=int, default=None,
                   help="кадров удержания атаки (по умолчанию 6)")
    p.add_argument("--timeout", type=float, default=12.0,
                   help="с — сколько ждать прогон (10 с хватает)")
    p.add_argument("--url", default=api.DEFAULT_URL)
    a = p.parse_args(argv)
    sys.stdout.reconfigure(line_buffering=True)

    script = core117.build(
        jump=a.jump or core117.T_JUMP, attack=a.attack or core117.T_ATTACK,
        run_frames=a.run_frames, t_run=a.t_run,
        dur_attack=a.attack_duration or core117.DUR_ATTACK,
        attack_when_enemy=(json.loads(a.attack_when_enemy)
                           if a.attack_when_enemy else None))
    print(f"вариант: jump={a.jump or core117.T_JUMP} attack={a.attack or core117.T_ATTACK} "
          f"run_frames={a.run_frames} t_run={a.t_run} "
          f"when_enemy={a.attack_when_enemy or '—'}; прогонов {a.runs}\n")

    for n in range(1, a.runs + 1):
        # Фокус перед КАЖДЫМ прогоном: без него игра не обрабатывает ввод
        # (меню/DirectInput — точно, и, похоже, override тоже) — прогон «пустой»,
        # и это выглядит как отсутствие лаунча (max_y 2-3 м).
        focused = api.focus_and_settle()
        if not focused:
            print(f"#{n}: окно игры не удалось активировать — прогон невалиден")
        if not api.ensure_gameplay(a.url):
            print(f"#{n}: игра не в геймплее")
            continue
        script["restart"] = {"ups": 1}
        sid = api.run_script(script, a.url)["script_id"]
        api.wait_script(sid, a.url, a.timeout, quiet=True)
        time.sleep(0.5)
        frames = [f for f in api.logs(a.url, script_id=sid, limit=1000)
                  if f.get("script_phase") == "running"]
        if not frames:
            print(f"#{n}: нет кадров")
            continue
        ys = [f["pos"][1] for f in frames]
        max_y = max(ys)
        line, lunge, parried = enemy_summary(frames, max_y)
        mark = "ЛАУНЧ" if max_y >= 20 else "  --  "
        parry_mark = "парирование ✓" if parried else "парирования нет"
        print(f"#{n} {mark} max_y={max_y:6.2f} (кадр {ys.index(max_y):>3}) "
              f"{parry_mark} | атака врага у пика: i={lunge}\n     враг: {line}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
