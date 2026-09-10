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

Порядок прогона: скрипт взводится ПЕРВЫМ (игрок в этот момент должен быть вне
зоны спавна, а прошлый прогон — уже завершён), затем игрок жмёт рестарт миссии
(P310_RESTART) — на спавне игрок попадает в зону триггера, и скрипт стартует сам.
Взводить, стоя в зоне, нельзя: триггер — проверка уровня (`segment::in_zone` в
каждом тике), такой скрипт стартует ближайшим же тиком, и `frame 0` перестаёт
совпадать с началом миссии.
"""
import argparse
import csv
import json
import os
import sys
import time
import urllib.error
import urllib.request

from core117 import build, END, SPAWN, T_ATTACK, T_JUMP

BARRIER = 20.0     # м — высота барьера
LAUNCH_DY = 0.20   # порог подъёма за кадр (два подряд — лаунч, одиночный хоп не считается)
LAUNCH_DY2 = 0.15
# допуски триггера — зеркало segment::in_zone (src/segment.rs:206)
ZONE_XY = 0.1
ZONE_Y = 1.0


# --- HTTP -------------------------------------------------------------------

def http(base, path, method="GET", body=None, timeout=5.0):
    data = json.dumps(body, ensure_ascii=False).encode("utf-8") if body is not None else None
    headers = {"Content-Type": "application/json"} if data else {}
    req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"{method} {path} → HTTP {e.code}: {e.read().decode('utf-8', 'replace')}")
    return json.loads(raw) if raw else {}


def stop_active(base):
    """Снять зависший скрипт, иначе POST /script/run вернёт 409."""
    try:
        http(base, "/script/stop", "POST")
        return True
    except RuntimeError:
        return False


def start(base, script):
    try:
        return http(base, "/script/run", "POST", script)
    except RuntimeError as e:
        if "409" not in str(e):
            raise
        stop_active(base)
        time.sleep(0.2)
        return http(base, "/script/run", "POST", script)


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


def arm_when_outside(base, script, label, tries=20):
    """Взвести скрипт так, чтобы он НЕ стартовал в момент взвода.

    Триггер в `api.rs` — проверка уровня в каждом тике, поэтому взведённый
    скрипт стартует первым же тиком, когда игрок окажется в зоне спавна.
    Стоя в зоне, взводить нельзя: `frame 0` перестанет совпадать с началом
    миссии. Поэтому взводим вне зоны и убеждаемся, что статус остался `armed`.
    """
    for _ in range(tries):
        pl = http(base, "/state", timeout=3.0).get("player") or {}
        found = pl.get("found")
        pos = pl.get("pos") if found else None  # в меню/загрузке позиция не валидна
        if in_zone(pos, SPAWN):
            print(f"  [{label}] игрок в зоне триггера {fmt_pos(pos)} — "
                  f"взведённый скрипт стартует сразу.")
            input("  отойдите от спавна (или выйдите в меню) и нажмите Enter ")
            continue
        res = start(base, script)
        sid = res["script_id"]
        time.sleep(0.15)
        cur = http(base, "/state", timeout=3.0).get("script") or {}
        if cur.get("id") == sid and cur.get("status") == "running":
            stop_active(base)
            print(f"  [{label}] скрипт стартовал в момент взвода — снимаю, пробуем снова.")
            input("  отойдите от спавна и нажмите Enter ")
            continue
        print(f"  [{label}] взведён: script {sid} "
              f"({cur.get('status') or res.get('status')}), игрок "
              f"{'найден' if found else 'НЕ найден (меню/загрузка)'} {fmt_pos(pos)}")
        return sid
    raise RuntimeError("не удалось взвести скрипт вне зоны триггера")


def wait_done(base, sid, arm_timeout, run_timeout, label):
    t_start = time.time()
    t_run = None
    last = None
    while True:
        st = http(base, f"/script/{sid}")
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
            stop_active(base)
            raise TimeoutError(f"прогон не завершился за {run_timeout} с")
        time.sleep(0.05)


# --- метрики ----------------------------------------------------------------

def metrics(frames):
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
    p.add_argument("--url", default="http://127.0.0.1:5223")
    p.add_argument("--arm-timeout", type=float, default=300.0)
    p.add_argument("--run-timeout", type=float, default=20.0)
    a = p.parse_args(argv)

    j_lo, j_hi = parse_range(a.jump)
    atk_lo, atk_hi = parse_range(a.attack)
    grid = [(j, atk) for j in range(j_lo, j_hi + 1, a.step)
            for atk in range(atk_lo, atk_hi + 1, a.step)]

    os.makedirs(a.out, exist_ok=True)
    variants = []
    for j, atk in grid:
        try:
            s = build(jump=j, attack=atk, ripper=a.ripper, end=a.end)
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
        http(a.url, "/health", timeout=3.0)
    except Exception as e:  # noqa: BLE001 — сообщение важнее типа
        print(f"API недоступен: {a.url} ({e})")
        print("запустите игру с заинжекченным модом и повторите.")
        return 2

    rows = []
    for idx, (j, atk, path, script) in enumerate(variants, 1):
        for rep in range(1, a.repeat + 1):
            label = f"j{j} a{atk} #{rep} ({idx}/{len(variants)})"
            print(f"\n=== {label} ===")
            row = {"jump": j, "attack": atk, "run": rep, "status": "", "error": ""}
            try:
                # Взвод ДО рестарта: взведённый скрипт стартует тем тиком, каким
                # игрок окажется в зоне спавна, — то есть в начале миссии.
                sid = arm_when_outside(a.url, script, label)
                input(f"  [{label}] нажмите рестарт миссии (P310_RESTART) "
                      f"и сразу Enter ")
                row["status"] = wait_done(a.url, sid, a.arm_timeout, a.run_timeout, label)
                m = metrics(http(a.url, f"/logs?script_id={sid}&limit=1000").get("frames", []))
                row.update(m or {"error": "нет кадров в логе"})
            except Exception as e:  # noqa: BLE001
                row["error"] = str(e)
            row["cleared"] = row.get("cleared", 0)
            rows.append(row)
            print(f"  → max_y={row.get('max_y')} cleared={row['cleared']} "
                  f"spawn_ok={row.get('spawn_ok')} launch={row.get('launch')} "
                  f"{row['error'] or ''}", flush=True)

    csv_path = os.path.join(a.out, "sweep.csv")
    cols = ["jump", "attack", "run", "status", "max_y", "cleared", "spawn_ok", "t_max",
            "launch", "y_end", "x_end", "z_end", "dz", "n", "error"]
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
