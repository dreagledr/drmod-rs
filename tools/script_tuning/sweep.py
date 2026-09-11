# -*- coding: utf-8 -*-
"""Сетка таймингов core-скрипта 117 и прогон через HTTP API мода.

Методология: высоту перелёта даёт связка «разгон → прыжок → хэви у земли»
(лаунч на fi 96, root motion анимации). Значит, находка «надежного» скрипта —
это поиск по двум осям: `t_jump` (когда оторвались) и `t_attack` (в какой
фазе падения прилетела атака). Критерий успеха — `max_y` ≥ 20 м (барьер),
надёжность — повторяемость на N прогонах подряд.

Без `--run` только генерирует варианты в `--out` (можно гонять вручную).
С `--run` шлёт по одному `POST /script/run`, ждёт `armed → running → done`,
снимает `GET /logs?script_id=`, считает метрики и пишет `sweep.csv`.

Порядок прогона (автоматический, с 2026-09-10): один `POST /script/run` несёт
и рестарт, и полёт — поле `restart` (`{...dik:0xC8...}`) заставляет мод сначала
отыграть меню паузы (pause → вверх → confirm ×2), дождаться loading, взвести
скрипт по триггеру спавна и стартовать на первой же позиции в зоне. Одним
скриптом это делается потому, что активный скрипт в моде ровно один (второй
`POST /script/run` → 409). Фаза рестарта требует фокуса окна игры: без него игра
не опрашивает клавиатуру (`DirectInput`) и меню не двигается — поэтому `--run`
активирует окно игры.

Взводить, стоя в зоне, нельзя (триггер — проверка уровня `segment::in_zone` в
каждом тике): такой скрипт стартует ближайшим же тиком, и `frame 0` перестаёт
совпадать с началом миссии. Поле `restart` решает это тем, что взводится только
после фактического loading новой миссии.
"""
import argparse
import csv
import json
import os
import sys
import time

import drmod_api as api

from core117 import build, END, SPAWN, T_ATTACK, T_JUMP

BARRIER = 20.0     # м — высота барьера
LAUNCH_DY = 0.20   # порог подъёма за кадр (два подряд — лаунч, одиночный хоп не считается)
LAUNCH_DY2 = 0.15
# допуски триггера — зеркало segment::in_zone (src/segment.rs:206)
ZONE_XY = 0.1
ZONE_Y = 1.0


# --- взвод ------------------------------------------------------------------

def in_zone(pos, target):
    """Зеркало `segment::in_zone`: ±0.1 м по X/Z, ±1.0 м по Y; без позиции — нет."""
    if pos is None:
        return False
    return (abs(pos[0] - target[0]) <= ZONE_XY
            and abs(pos[1] - target[1]) <= ZONE_Y
            and abs(pos[2] - target[2]) <= ZONE_XY)


def fmt_pos(pos):
    return "—" if pos is None else "(%.2f, %.2f, %.2f)" % tuple(pos)


def wait_done(base, sid, arm_timeout, run_timeout, label):
    t_start = time.time()
    t_run = None
    last = None
    while True:
        st = api.http(base, f"/script/{sid}")
        s = st["status"]
        if s != last:
            print(f"  [{label}] script {sid}: {last or '?'} → {s} "
                  f"(кадр {st['frame']}/{st['total_frames']})", flush=True)
            last = s
        if s == "running" and t_run is None:
            t_run = time.time()
        if s in ("done", "stopped"):
            return s
        now = time.time()
        if t_run is None and now - t_start > arm_timeout:
            raise TimeoutError(f"не дождался триггера за {arm_timeout} с")
        if t_run is not None and now - t_run > run_timeout:
            api.http(base, "/script/stop", "POST")
            raise TimeoutError(f"прогон не завершился за {run_timeout} с")
        time.sleep(0.05)


# --- метрики ----------------------------------------------------------------

def flight_frames(frames):
    """Кадры фазы `running` (полёт). В /logs попадают и кадры фазы рестарта
    (меню, loading) — по ним метрики полёта считать нельзя."""
    flight = [f for f in frames if f.get("script_phase") == "running"]
    return flight or frames


def parried(frames):
    """Было ли парирование (враг ушёл в анимацию 1114113 → подброс).

    Одиночный удар попадает в узкое окно парирования лишь в ~3-4 прогонах из 10
    (джиттер симуляции ±2 кадра), поэтому для перебора таймингов имеет смысл
    best-of-N: перезапускать вариант, пока парирование не случится.
    """
    return any((f.get("enemy") or {}).get("r_anim") == 1114113 for f in frames)


def metrics(frames):
    frames = flight_frames(frames)
    if not frames:
        return None
    ys = [f["pos"][1] for f in frames]
    xs = [f["pos"][0] for f in frames]
    zs = [f["pos"][2] for f in frames]
    i_max = max(range(len(ys)), key=ys.__getitem__)

    launch = None
    for k in range(1, len(ys) - 1):
        if ys[k] - ys[k - 1] >= LAUNCH_DY and ys[k + 1] - ys[k] >= LAUNCH_DY2:
            launch = k
            break

    return {
        "n": len(frames),
        "max_y": round(ys[i_max], 2),
        "cleared": int(ys[i_max] >= BARRIER),
        # скрипт стартовал на спавне (а не подобрал игрока мимо зоны): иначе
        # прогон невалиден и max_y ничего не значит
        "spawn_ok": int(in_zone(frames[0]["pos"], SPAWN)),
        "t_max": i_max,
        "launch": launch,
        "y_end": round(ys[-1], 2),
        "x_end": round(xs[-1], 2),
        "z_end": round(zs[-1], 2),
        "dz": round(zs[-1] - zs[0], 2),
    }


# --- CLI --------------------------------------------------------------------

def parse_range(s):
    lo, _, hi = s.partition(":")
    return int(lo), int(hi or lo)


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--jump", default=f"{T_JUMP - 2}:{T_JUMP + 2}",
                   help="диапазон t_jump, напр. 42:48")
    p.add_argument("--attack", default=f"{T_ATTACK - 3}:{T_ATTACK + 6}",
                   help="диапазон t_attack, напр. 72:82")
    p.add_argument("--step", type=int, default=2, help="шаг сетки (1 — точный перебор)")
    p.add_argument("--end", type=int, default=END)
    p.add_argument("--ripper", type=int, default=104)
    p.add_argument("--out", default=r"out\tuning", help="каталог для вариантов и sweep.csv")
    p.add_argument("--run", action="store_true", help="гнать через API, а не только генерировать")
    p.add_argument("--repeat", type=int, default=1, help="прогонов на вариант (проверка надёжности)")
    p.add_argument("--max-attempts", type=int, default=1,
                   help="best-of-N: до N перезапусков варианта, пока не случится "
                        "парирование (окно узкое, одиночный удар попадает в ~3-4/10)")
    p.add_argument("--require-parry", action="store_true",
                   help="считать прогон валидным только при парировании (враг 1114113)")
    p.add_argument("--attack-duration", type=int, default=None,
                   help="кадров удержания атаки (по умолчанию 6; для позы, "
                        "доживающей до контакта с врагом — 20-30)")
    p.add_argument("--run-frames", type=int, default=None,
                   help="длина разгона до прыжка (по умолчанию 5, как в записи; "
                        "для «короткого бега» 1-2)")
    p.add_argument("--t-run", type=int, default=None,
                   help="абсолютный кадр первого ввода (по умолчанию jump - разгон)")
    p.add_argument("--no-air-forward", action="store_true",
                   help="отпустить бег сразу после прыжка (вертикальный прыжок)")
    p.add_argument("--release-tail", type=int, default=0,
                   help="отпустить бег за N кадров до атаки")
    p.add_argument("--attack-when-enemy", default=None,
                   help='JSON-условие адаптивного удара (по врагу), напр. '
                        '\'{"anim": [65545], "dist_max": 2.5}\'')
    p.add_argument("--url", default="http://127.0.0.1:5223")
    p.add_argument("--arm-timeout", type=float, default=300.0)
    p.add_argument("--run-timeout", type=float, default=10.0,
                   help="с — сколько ждать прогон после старта (10 с хватает)")
    a = p.parse_args(argv)

    j_lo, j_hi = parse_range(a.jump)
    atk_lo, atk_hi = parse_range(a.attack)
    grid = [(j, atk) for j in range(j_lo, j_hi + 1, a.step)
            for atk in range(atk_lo, atk_hi + 1, a.step)]

    os.makedirs(a.out, exist_ok=True)
    variants = []
    for j, atk in grid:
        try:
            s = build(jump=j, attack=atk, ripper=a.ripper, end=a.end,
                      run_frames=a.run_frames, t_run=a.t_run,
                      air_forward=not a.no_air_forward,
                      release_tail=a.release_tail,
                      dur_attack=a.attack_duration or 6,
                      attack_when_enemy=(json.loads(a.attack_when_enemy)
                                         if a.attack_when_enemy else None))
        except ValueError as e:
            print(f"пропуск j{j} a{atk}: {e}")
            continue
        path = os.path.join(a.out, f"j{j:03d}_a{atk:03d}.json")
        with open(path, "w", encoding="utf-8") as f:
            json.dump(s, f, ensure_ascii=False)
        variants.append((j, atk, path, s))

    print(f"сетка: {len(variants)} вариантов "
          f"(jump {j_lo}..{j_hi}, attack {atk_lo}..{atk_hi}, шаг {a.step}) → {a.out}")
    if not a.run:
        print("без --run прогон не выполняется; файлы готовы к ручному запуску.")
        return 0

    try:
        api.http(a.url, "/health", wait=3.0)
    except Exception as e:  # noqa: BLE001 — сообщение важнее типа
        print(f"API недоступен: {a.url} ({e})")
        print("запустите игру с заинжекченным модом и повторите.")
        return 2
    print(f"фокус окна игры: {'OK' if api.focus_and_settle() else 'НЕ ПОЛУЧИЛСЯ'} "
          f"(нужен фазе рестарта: без фокуса игра не опрашивает клавиатуру)")

    rows = []
    for idx, (j, atk, path, script) in enumerate(variants, 1):
        for rep in range(1, a.repeat + 1):
            label = f"j{j} a{atk} #{rep} ({idx}/{len(variants)})"
            print(f"\n=== {label} ===")
            row = {"jump": j, "attack": atk, "run": rep, "status": "", "error": "",
                   "attempts": 0, "parry": 0}
            for attempt in range(1, a.max_attempts + 1):
                try:
                    # Фокус перед каждым прогоном: без него игра не обрабатывает
                    # ввод (меню — точно, и override, похоже, тоже) — прогон пустой.
                    if not api.focus_and_settle():
                        raise RuntimeError("окно игры не удалось активировать")
                    # Мод должен быть в геймплее: если игра в меню (после рестарта
                    # или падения), фаза restart начнёт с `pause` по открытому меню.
                    if not api.ensure_gameplay(a.url):
                        raise RuntimeError("игра не в геймплее (меню/загрузка)")
                    # Рестарт и полёт — одним запросом (поле `restart`): мод сначала
                    # отыгрывает меню паузы, дожидается loading, взводится по спавну
                    # и стартует там же — то есть с начала миссии.
                    script["restart"] = {"ups": 1}
                    res = api.run_script(script, a.url)
                    sid = res["script_id"]
                    print(f"  [{label}] попытка {attempt}: script {sid}, "
                          f"статус {res['status']}")
                    row["status"] = wait_done(a.url, sid, a.arm_timeout, a.run_timeout, label)
                    frames = api.logs(a.url, script_id=sid, limit=1000)
                    row["attempts"] = attempt
                    row["parry"] = int(parried(frames))
                    m = metrics(frames)
                    row.update(m or {"error": "нет кадров в логе"})
                    if m and (not a.require_parry or row["parry"]):
                        break
                except Exception as e:  # noqa: BLE001
                    row["error"] = str(e)
            row["cleared"] = row.get("cleared", 0)
            rows.append(row)
            print(f"  → max_y={row.get('max_y')} cleared={row['cleared']} "
                  f"spawn_ok={row.get('spawn_ok')} launch={row.get('launch')} "
                  f"парирование={row['parry']} попыток={row['attempts']} "
                  f"{row['error'] or ''}", flush=True)

    csv_path = os.path.join(a.out, "sweep.csv")
    cols = ["jump", "attack", "run", "status", "attempts", "parry", "max_y", "cleared",
            "spawn_ok", "t_max", "launch", "y_end", "x_end", "z_end", "dz", "n", "error"]
    with open(csv_path, "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=cols, extrasaction="ignore")
        w.writeheader()
        w.writerows(rows)
    print(f"\nотчёт: {csv_path}")

    valid = [r for r in rows if r.get("spawn_ok")]
    ok = [r for r in valid if r.get("cleared")]
    bad = [r for r in rows if not r.get("spawn_ok") and not r.get("error")]
    print("\n-- итог (по max_y) --")
    for r in sorted(rows, key=lambda r: r.get("max_y") or -1, reverse=True)[:10]:
        print(f"  j{r['jump']:>3} a{r['attack']:>3} #{r['run']}  "
              f"max_y={r.get('max_y')}  {'OK' if r.get('cleared') else '--'}  "
              f"spawn_ok={r.get('spawn_ok')}  {r['error']}")
    if bad:
        print(f"\nневалидных (старт не на спавне): {len(bad)} — "
              f"их max_y в расчёт не берётся")
    print(f"\nперелетели барьер: {len(ok)}/{len(valid)} валидных прогонов"
          f" (всего {len(rows)})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
