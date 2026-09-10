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
          dur_attack=DUR_ATTACK, runup=RUNUP, end=END, name=None):
    """Собрать очищенный скрипт. Кадры — абсолютные, от старта скрипта."""
    if jump < runup:
        raise ValueError(f"jump={jump} меньше длины разгона {runup}")
    if attack < jump + dur_jump:
        raise ValueError(
            f"attack={attack} перекрывает удержание прыжка ({jump}+{dur_jump})")
    if end < attack + dur_attack:
        raise ValueError(f"end={end} меньше конца атаки ({attack + dur_attack})")

    cmds = []

    def add(t, dur, **inp):
        if dur > 0:
            cmds.append({"t": t, "duration": dur, "input": inp})

    add(jump - runup, runup, forward=True)
    add(jump, dur_jump, forward=True, jump=True)
    add(jump + dur_jump, attack - jump - dur_jump, forward=True)
    add(attack, dur_attack, forward=True, heavy_attack=True)

    after = attack + dur_attack
    if ripper is not None and after <= ripper < end:
        # forward разрезается вокруг риппера — ровно как в выводе dbdump --script
        add(after, ripper - after, forward=True)
        add(ripper, 1, forward=True, ripper=True)
        add(ripper + 1, end - ripper - 1, forward=True)
    else:
        add(after, end - after, forward=True)

    return {
        "name": name or f"core117-j{jump}-a{attack}",
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
    p.add_argument("--pretty", action="store_true")
    a = p.parse_args(argv)

    script = build(jump=a.jump, attack=a.attack, ripper=a.ripper, end=a.end)
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
