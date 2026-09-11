# -*- coding: utf-8 -*-
"""Очищенный core-скрипт записи 117: разгон → прыжок → хэви у земли → риппер.

Из записи (618 кадров, 360 команд генератора dbdump) выброшено:
- камера и right_stick — 175 однокадровых команд с битом `right` 0x100000
  (fi 131-558); замер по кадрам: за весь полёт cam_yaw ушёл 2.90 → 3.12,
  rot_y не менялся вообще, |Δyaw| между записью и плейбэками < 1°
  (out/desync_cam_117.png, панель 2) — на перелёт не влияло ни разу;
- шумовые одиночные нажатия backward/left на fi 191 и fi 238-284 —
  сделаны, когда игрок стоял застрявшим в геометрии.

Остаётся связка, которая и даёт перелёт (пик записи 24.20 м при барьере 20 м):
forward@40 → jump@45 (6 кадров) → heavy_attack@76 (6 кадров, y≈0.51) →
ripper@104 (keybind 11, однокадровый фронт; лаунч стартует на fi 96,
за 8 кадров ДО риппера — высоту даёт не он).

Сетка таймингов: `t_jump` сдвигает всю связку (разгон до прыжка всегда
`RUNUP` кадров, как в записи — иначе длина разгона стала бы третьей
переменной), `t_attack` задаётся абсолютным кадром.
"""
import argparse
import json
import sys

# --- опорные кадры записи 117 (проверено по кадрам, см. out/_tmp117/flight.py) ---
SPAWN = (2.86, -0.0009459257, 70.91)  # позиция первого кадра → trigger
T_FORWARD = 40
T_JUMP = 45
DUR_JUMP = 6
T_ATTACK = 76
DUR_ATTACK = 6
T_RIPPER = 104
END = 240  # посадка записи на fi 234 (+6 кадров запаса)
RUNUP = T_JUMP - T_FORWARD  # 5


def build(jump=T_JUMP, attack=T_ATTACK, ripper=T_RIPPER, dur_jump=DUR_JUMP,
          dur_attack=DUR_ATTACK, runup=RUNUP, end=END, name=None,
          run_frames=None, t_run=None, air_forward=True, attack_forward=True):
    """Собрать очищенный скрипт. Кадры — абсолютные, от старта скрипта.

    `run_frames` — длина разгона до прыжка (по умолчанию `runup` = 5 как в
    записи). `air_forward=False` отпускает бег в прыжке/полёте: прыжок делает
    «короткий бег» и не удлиняется, а игрок остаётся ближе к спавну (подброс
    даёт парирование атаки врага, а не разбег). `t_run` задаёт абсолютный кадр
    первого ввода (по умолчанию `jump - run_frames`) — сдвиг «самого первого
    ввода» относительно прыжка.
    """
    run_up = run_frames if run_frames is not None else runup
    if jump < run_up:
        raise ValueError(f"jump={jump} меньше длины разгона {run_up}")
    if attack < jump + dur_jump:
        raise ValueError(
            f"attack={attack} перекрывает удержание прыжка ({jump}+{dur_jump})")
    if end < attack + dur_attack:
        raise ValueError(f"end={end} меньше конца атаки ({attack + dur_attack})")

    start = t_run if t_run is not None else jump - run_up
    if start < 0 or start > jump:
        raise ValueError(f"t_run={start} вне диапазона [0, jump={jump}]")

    cmds = []

    def add(t, dur, **inp):
        if dur > 0:
            cmds.append({"t": t, "duration": dur, "input": inp})

    add(start, jump - start, forward=True)
    add(jump, dur_jump, forward=air_forward, jump=True)
    add(jump + dur_jump, attack - jump - dur_jump, forward=air_forward)
    add(attack, dur_attack, forward=attack_forward, heavy_attack=True)

    after = attack + dur_attack
    if ripper is not None and after <= ripper < end:
        # forward разрезается вокруг риппера — ровно как в выводе dbdump --script
        add(after, ripper - after, forward=True)
        add(ripper, 1, forward=True, ripper=True)
        add(ripper + 1, end - ripper - 1, forward=True)
    else:
        add(after, end - after, forward=True)

    return {
        "name": name or f"core117-j{jump}-a{attack}-f{start}",
        "trigger": {"pos": list(SPAWN)},
        "commands": cmds,
    }


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("out", nargs="?", help="файл JSON (по умолчанию — stdout)")
    p.add_argument("--jump", type=int, default=T_JUMP)
    p.add_argument("--attack", type=int, default=T_ATTACK)
    p.add_argument("--ripper", type=int, default=T_RIPPER)
    p.add_argument("--end", type=int, default=END)
    p.add_argument("--run-frames", type=int, default=None,
                   help="длина разгона до прыжка (по умолчанию 5, как в записи)")
    p.add_argument("--t-run", type=int, default=None,
                   help="абсолютный кадр первого ввода (по умолчанию jump - разгон)")
    p.add_argument("--no-air-forward", action="store_true",
                   help="отпустить бег в прыжке/полёте (короткий прыжок)")
    p.add_argument("--pretty", action="store_true")
    a = p.parse_args(argv)

    script = build(jump=a.jump, attack=a.attack, ripper=a.ripper, end=a.end,
                   run_frames=a.run_frames, t_run=a.t_run,
                   air_forward=not a.no_air_forward)
    text = json.dumps(script, ensure_ascii=False, indent=2 if a.pretty else None)
    if a.out:
        with open(a.out, "w", encoding="utf-8") as f:
            f.write(text)
        print(f"{a.out}: {len(script['commands'])} команд, "
              f"протяжённость {max(c['t'] + c['duration'] for c in script['commands'])}")
    else:
        print(text)


if __name__ == "__main__":
    sys.exit(main())
