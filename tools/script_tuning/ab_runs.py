# -*- coding: utf-8 -*-
"""Чередующийся прогон нескольких вариантов связки в ОДНОЙ сессии.

Зачем: серии, снятые по отдельности, расходятся между собой сильнее, чем
варианты внутри серии (одна и та же связка в двух сериях дала 2/8 и 5/8
парирований — 2026-09-11). Поэтому варианты надо чередовать: один прогон —
вариант A, следующий — B, и так по кругу. Тогда любой дрейф (состояние
миссии, FPS, кадр контакта) бьёт по всем вариантам одинаково.

Варианты задаются JSON'ом в `--arm` (ключи — параметры `core117.build`, поле
`name` — имя варианта для отчёта):

    py -3 tools\\script_tuning\\ab_runs.py --runs 8 \\
        --arm '{"name":"j45-n","jump":45,"release_tail":6,"ninja":true}' \\
        --arm '{"name":"j41-nf","jump":41,"release_tail":6,"ninja":true,"ninja_flight":false}'

CSV — как у `parry_geometry.py` плюс колонка `arm`; в конце печатается сводка
по вариантам (парирования, перелёты, разброс `post_gain`, угол в контакте).
"""
import argparse
import json
import sys
import time

import core117
import drmod_api as api
import parry_geometry as pg

ARM_COLS = ["arm"] + list(pg.RUN_COLS)


def build_script(arm):
    """Скрипт варианта: ключи `arm` — параметры `core117.build` (без `name`)."""
    kw = {k: v for k, v in arm.items() if k != "name"}
    kw.setdefault("attack", core117.T_ATTACK)
    kw.setdefault("run_frames", 6)
    kw.setdefault("dur_attack", 24)
    kw["t_run"] = kw["jump"] - kw["run_frames"]
    return core117.build(**kw)


def run_once(script, url, timeout):
    """Один прогон: рестарт+полёт одним скриптом, кадры фазы `running`."""
    script["restart"] = {"ups": 1}
    sid = api.run_script(script, url)["script_id"]
    api.wait_script(sid, url, timeout, quiet=True)
    time.sleep(0.4)
    return [f for f in api.logs(url, script_id=sid, limit=1000)
            if f.get("script_phase") == "running"]


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--arm", action="append", required=True,
                   help='JSON варианта, напр. \'{"name":"j45-n","jump":45,'
                        '"release_tail":6,"ninja":true}\'')
    p.add_argument("--runs", type=int, default=8, help="прогонов на вариант")
    p.add_argument("--out", default=r"out\ab_runs.csv")
    p.add_argument("--out-window", default=r"out\ab_runs_window.csv")
    p.add_argument("--timeout", type=float, default=12.0)
    p.add_argument("--fixed-dt", dest="fixed_dt", action="store_true",
                   default=None,
                   help="включить фиксированный шаг времени движка (POST /dt)")
    p.add_argument("--no-fixed-dt", dest="fixed_dt", action="store_false",
                   help="выключить фиксированный шаг времени движка")
    p.add_argument("--url", default=api.DEFAULT_URL)
    a = p.parse_args(argv)
    api.setup_stdout()

    if a.fixed_dt is not None:
        res = api.fixed_dt(a.fixed_dt, a.url)
        print(f"фиксированный dt: {'вкл' if res.get('fixed') else 'выкл'} "
              f"({res.get('addr')})")
    dt_now = api.state(a.url).get("dt") or {}
    print(f"шаг времени движка: fixed={dt_now.get('fixed')} "
          f"frame_ms={dt_now.get('frame_ms')} rate={dt_now.get('rate')}")

    arms = []
    for i, spec in enumerate(a.arm, 1):
        arm = json.loads(spec)
        arm.setdefault("name", f"arm{i}")
        arms.append((arm["name"], arm, build_script(arm)))
    print("варианты (чередуются через прогон):")
    for name, arm, _ in arms:
        print(f"  {name}: {json.dumps({k: v for k, v in arm.items() if k != 'name'}, ensure_ascii=False)}")
    print(f"прогонов на вариант: {a.runs}\n")

    rows, win = [], []
    n = 0
    for rep in range(1, a.runs + 1):
        # Порядок вариантов вращается — чтобы систематический дрейф внутри
        # сессии не приписывался первому варианту списка.
        order = arms[rep % len(arms):] + arms[:rep % len(arms)]
        for name, _arm, script in order:
            n += 1
            try:
                api.focus_and_settle()
                if not api.ensure_gameplay(a.url):
                    print(f"#{n} {name}: игра не в геймплее")
                    continue
                frames = run_once(script, a.url, a.timeout)
            except (RuntimeError, OSError) as e:
                print(f"#{n} {name}: прогон не удался — {e}")
                continue
            if not frames:
                print(f"#{n} {name}: нет кадров")
                continue
            row = pg.analyse_run(frames, n)
            row["arm"] = name
            rows.append(row)
            win += pg.window_rows(frames, n, row.get("post_gain"))
            print(f"#{n:>3} {name:<10} max_y={row['max_y']:6.2f} "
                  f"пар={'да' if row['parry_frame'] is not None else 'нет'}"
                  f"@{row['parry_frame']} post_gain={row['post_gain']}"
                  f" угол={row.get('p_angle_deg')} "
                  f"дист(удар)={row.get('a_dist_h')} y(удар)={row.get('a_player_y')}")

    if not rows:
        print("нет данных")
        return 1
    pg.write_csv(a.out, ARM_COLS, rows)
    pg.write_csv(a.out_window, pg.WIN_COLS, win)
    print(f"\nCSV: {a.out}")
    print(f"\n{'вариант':<12} {'n':>3} {'пар':>6} {'>=20':>6} "
          f"{'post_gain':<24} {'угол в контакте':<20} дист(удар)")
    for name, _arm, _ in arms:
        sub = [r for r in rows if r["arm"] == name]
        gains = [r["post_gain"] for r in sub if r.get("post_gain") is not None]
        cleared = sum(1 for r in sub if (r.get("max_y") or 0) >= 20)
        angles = [r["p_angle_deg"] for r in sub if r.get("p_angle_deg") is not None]
        adist = [r["a_dist_h"] for r in sub if r.get("a_dist_h") is not None]
        g = (f"{min(gains):.1f}..{max(gains):.1f}") if gains else "—"
        ang = (f"{min(angles):.0f}..{max(angles):.0f}°") if angles else "—"
        d = (f"{min(adist):.2f}..{max(adist):.2f}") if adist else "—"
        print(f"{name:<12} {len(sub):>3} {len(gains):>3}/{len(sub):<3} "
              f"{cleared:>3}/{len(sub):<3} {g:<24} {ang:<20} {d}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
