# -*- coding: utf-8 -*-
"""Таймлайн врага и игрока по кадрам прогона (`GET /logs`).

Зачем: подброс игрока в P310_RESTART даёт **парирование прыжка врага** (запись
142: враг anim 19 → 1114113 на кадре 99, игрок в ударе `r_anim 94`, подъём до
22.1 м). Поэтому sweep'у важно видеть, в какой кадр попадает удар относительно
анимации врага.

Печатает: смены анимации врага (найден/anim/кадр анимации/дистанция/HP),
высоту и анимацию игрока, а также «окно» — кадры, где враг в анимации атаки.

    py -3 tools\\script_tuning\\enemy_timeline.py            # последний скрипт
    py -3 tools\\script_tuning\\enemy_timeline.py 4          # конкретный script_id
"""
import argparse
import sys

import drmod_api as api

#: Анимации врага, важные для парирования (из docs/ENEMY_TRACKING.md и записи 142).
ENEMY_LUNGE = {19, 65545, 1114113, 131078, 131072}


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("script_id", nargs="?", type=int, help="id скрипта (по умолчанию — последний)")
    p.add_argument("--all-phases", action="store_true",
                   help="не фильтровать по фазе running")
    p.add_argument("--url", default=api.DEFAULT_URL)
    a = p.parse_args(argv)
    sys.stdout.reconfigure(line_buffering=True)

    sid = a.script_id
    if sid is None:
        cur = api.state(a.url).get("script") or {}
        sid = cur.get("id")
        if sid is None:
            print("нет данных о последнем скрипте — укажите script_id")
            return 2
        print(f"последний скрипт: {sid} ({cur.get('name')}, {cur.get('status')})")

    frames = api.logs(a.url, script_id=sid, limit=2000)
    if not a.all_phases:
        frames = [f for f in frames if f.get("script_phase") == "running"]
    print(f"кадров: {len(frames)}")
    if not frames:
        print("кадров нет (скрипт не отыграл фазу running?)")
        return 1

    print(f"\n{'i':>4} {'y':>7} {'z':>8} {'anim':>5} | {'enf':>4} {'en_anim':>8} "
          f"{'en_fr':>6} {'edz':>7} {'ehp':>5}  событие")
    prev_enemy, prev_anim, launch_seen = None, None, False
    for i, fr in enumerate(frames):
        e = fr.get("enemy") or {}
        pos = fr["pos"]
        anim = fr.get("r_anim")
        enemy_key = (e.get("found"), e.get("r_anim"))
        notes = []
        if enemy_key != prev_enemy:
            notes.append("смена анимации врага" if prev_enemy else "враг найден")
            prev_enemy = enemy_key
            if e.get("r_anim") in ENEMY_LUNGE:
                notes.append("ВРАГ В АТАКЕ")
        if anim != prev_anim:
            notes.append("смена анимации игрока")
            prev_anim = anim
        if i > 0:
            dy = pos[1] - frames[i - 1]["pos"][1]
            if dy >= 0.2:
                notes.append(f"подъём +{dy:.2f}/кадр")
                launch_seen = True
        if notes:
            ep = e.get("pos") or [0, 0, 0]
            print(f"{i:>4} {pos[1]:>7.2f} {pos[2]:>8.2f} {anim if anim is not None else -1:>5} | "
                  f"{e.get('found', 0):>4} {e.get('r_anim', 0):>8} {e.get('frame', 0):>6} "
                  f"{ep[2] - pos[2]:>7.2f} {e.get('hp', 0):>5}  {', '.join(notes)}")

    ys = [f["pos"][1] for f in frames]
    print(f"\nmax_y={max(ys):.2f} на кадре {ys.index(max(ys))}; "
          f"подъём {'был' if launch_seen else 'НЕ было'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
