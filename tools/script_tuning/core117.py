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
          run_frames=None, t_run=None, air_forward=True, release_tail=0,
          attack_forward=True, ninja=False, ninja_flight=True,
          attack_when_enemy=None):
    """Собрать очищенный скрипт. Кадры — абсолютные, от старта скрипта.

    `run_frames` — длина разгона до прыжка (по умолчанию `runup` = 5 как в
    записи; для «короткого бега» 1-2: прыжок не удлиняется).
    `t_run` — абсолютный кадр первого ввода (по умолчанию `jump - run_frames`).
    `air_forward=False` — отпустить бег сразу после прыжка (вертикальный прыжок).
    `release_tail=N` — отпустить бег за N кадров до атаки (прыжок и почти весь
    полёт летят вперёд, как в записи). Прыжок всегда начинается на бегу.
    `ninja=True` — держать `ninja_run` (бит 0x4000 + keybind 8) на разгоне,
    прыжке, полёте и в атаке: без него прыжок и удар в воздухе идут другими
    анимациями (проверено на игре 2026-09-11), а отпускание в `release_tail`
    глушит и ninja — «стоп в воздухе» перед атакой.
    `ninja_flight=False` — отпустить ninja сразу после прыжка и держать в полёте
    только `forward` (прыжок при этом был ninja-овым): анимация прыжка как на
    бегу, но скорость ниже.
    `attack_when_enemy` — условие атаки по врагу (адаптивный удар): словарь
    `{"anim": [...], "frame_min": .., "frame_max": .., "dist_max": ..}`;
    команда атаки «спит» до выполнения условия — подброс даёт парирование
    прыжка врага, а его состояние между прогонами плавает.
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

    def add(t, dur, when_enemy=None, **inp):
        # Команду без РЕАЛЬНЫХ входов не добавляем: API отвергает и пустой
        # `input`, и набор из одних `false` (`input is empty`). «Отпущенный бег» —
        # это как раз отсутствие активных команд: override снимается, и игра
        # видит реальный ввод (ничего).
        if dur > 0 and any(inp.values()):
            cmd = {"t": t, "duration": dur, "input": inp}
            if when_enemy is not None:
                cmd["when_enemy"] = when_enemy
            cmds.append(cmd)

    add(start, run_up, forward=True, ninja_run=ninja)
    # Пауза между разгоном и прыжком (если t_run раньше jump-run_up): первый
    # ввод сдвигается независимо от прыжка — так враг «видит» движение раньше,
    # а прыжок остаётся коротким (старт с места, forward включается в прыжке).
    # Прыжок начинается на бегу (forward в прыжке), иначе это прыжок на месте.
    add(jump, dur_jump, forward=True, jump=True, ninja_run=ninja)
    air_from = jump + dur_jump
    air_to = max(air_from, attack - release_tail)
    # Полёт: вперёд (по умолчанию) — отпускаем за `release_tail` кадров до атаки.
    add(air_from, air_to - air_from, forward=air_forward,
        ninja_run=ninja and ninja_flight)
    add(attack, dur_attack, when_enemy=attack_when_enemy,
        forward=attack_forward, heavy_attack=True, ninja_run=ninja)

    after = attack + dur_attack
    if ripper is not None and ripper < after:
        # Иначе риппер пропадал МОЛЧА: его кадр накрыт удержанием атаки (на
        # серии с `--attack 84 --attack-duration 24` так потерялся риппер
        # записи, и её роль в подбросе проверялась без него).
        raise ValueError(
            f"ripper={ripper} попадает внутрь удержания атаки ({attack}+"
            f"{dur_attack}={after}): выберите ripper >= {after} или меньший "
            f"dur_attack")
    if ripper is not None and after <= ripper < end:
        # forward разрезается вокруг риппера — ровно как в выводе dbdump --script
        add(after, ripper - after, forward=True, ninja_run=ninja)
        add(ripper, 1, forward=True, ripper=True, ninja_run=ninja)
        add(ripper + 1, end - ripper - 1, forward=True, ninja_run=ninja)
    else:
        add(after, end - after, forward=True, ninja_run=ninja)

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
                   help="отпустить бег сразу после прыжка (вертикальный прыжок)")
    p.add_argument("--release-tail", type=int, default=0,
                   help="отпустить бег за N кадров до атаки")
    p.add_argument("--ninja", action="store_true",
                   help="держать ninja_run (бит 0x4000 + keybind 8) на разгоне, "
                        "прыжке, в полёте и в атаке")
    p.add_argument("--no-ninja-flight", action="store_true",
                   help="отпустить ninja сразу после прыжка (в полёте только "
                        "forward), на удар включить снова")
    p.add_argument("--attack-when-enemy", default=None,
                   help='JSON-условие адаптивного удара, напр. '
                        '\'{"anim": [65545], "frame_max": 60, "dist_max": 2.5}\'')
    p.add_argument("--attack-duration", type=int, default=None,
                   help="кадров удержания атаки (по умолчанию 6; в записи поза "
                        "держится ~30 кадров до контакта с врагом)")
    p.add_argument("--pretty", action="store_true")
    a = p.parse_args(argv)

    script = build(jump=a.jump, attack=a.attack, ripper=a.ripper, end=a.end,
                   run_frames=a.run_frames, t_run=a.t_run,
                   air_forward=not a.no_air_forward, release_tail=a.release_tail,
                   ninja=a.ninja, ninja_flight=not a.no_ninja_flight,
                   dur_attack=a.attack_duration or DUR_ATTACK,
                   attack_when_enemy=(json.loads(a.attack_when_enemy)
                                      if a.attack_when_enemy else None))
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
