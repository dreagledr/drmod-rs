# -*- coding: utf-8 -*-
"""Демонстрация готового TAS первого сегмента R-03.

Запуск без аргументов:

    py -3 tools\\demo\\demo_r03_tas.py

Скрипт сам выставляет мир (фиксированный шаг 1/60, заморозка RNG seed 1,
тиковый триггер), рестартует миссию и прогоняет TAS (`--preset ls8` —
восемь lightning strike подряд) **три раза подряд**, печатая результат каждого
прогона: серию анимаций и точку остановки.

Headless-прогон — то же самое, но без отрисовки и без капа кадров:

    py -3 tools\\demo\\demo_r03_tas.py --headless

* `--headless` включает headless-прогон мода (`POST /render {"headless": true,
  "hold": true}`): снимается отрисовка (overlay мода, вывод кадра, геометрия
  игры) **и** кап кадров — без снятого капа прогон упирается в пацер игры.
  `hold` тут потому, что прогонов несколько: конец одного прогона — не конец
  сессии, поэтому в конце демонстрация сама возвращает рендер и прежний кап
  (`POST /render {"reset": true}`). Для одиночного прогона `hold` не нужен — мод
  вернёт всё сам по концу прогона скрипта;
* `--uncapped` — только снять кап кадров, отрисовку оставить (A/B: видно, что
  даёт кап, а что — снятая отрисовка);
* `--skip overlay,present,draw` — снять выбранные выключатели по одному (плюс
  кап), чтобы понять, какой из них портит прогон. Диагностический режим.

Каждый прогон печатается с настенным временем, а в конце — суммарное время.
⚠️ 2026-09-15: прогон с `--headless` довёл игру до падения в `d3d9.dll` на
втором прогоне (подробности и разбор — `docs/HEADLESS.md`, §5); для проверки по
одному выключателю и есть `--skip`. Возврат мира и капа — всегда, в том числе
при ошибке. Разбор и оговорки — `docs/HEADLESS.md`.
"""
import argparse
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "script_tuning"))

import drmod_api as api  # noqa: E402  (путь добавляем выше)
import r03_baseline as tas  # noqa: E402  (путь добавляем выше)

ARGS = ["--preset", "ls8", "--from", "9999"]


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--runs", type=int, default=3, help="сколько прогонов подряд")
    p.add_argument("--headless", action="store_true",
                   help="headless-прогон: без отрисовки и без капа кадров")
    p.add_argument("--uncapped", action="store_true",
                   help="только снять кап кадров (отрисовку оставить)")
    p.add_argument("--skip", default=None,
                   help="проверка по одному выключателю: overlay,present,draw "
                        "(снятый кап при этом тоже ставится, чтобы прогоны были "
                        "сравнимы)")
    args = p.parse_args(argv)

    api.setup_stdout()
    levers = set()
    if args.skip:
        levers = {s.strip() for s in args.skip.split(",") if s.strip()}
        unknown = levers - {"overlay", "present", "draw"}
        if unknown:
            print(f"неизвестные выключатели: {', '.join(sorted(unknown))}")
            return 2

    accelerated = args.headless or args.uncapped or bool(levers)
    before = api.frame_rate(2.0) if accelerated else (0.0, 0.0)
    if accelerated:
        print(f"до:    {before[0]:6.1f} кадров/с, {before[1]:6.1f} тиков/с"
              f"   кап={api.state()['fps_cap']['cap']}")

    try:
        if args.headless:
            st = api.render_skip(headless=True, hold=True)
            print(f"headless: вкл (skip overlay={st['skip_overlay']}, "
                  f"present={st['skip_present']}, draw={st['skip_draw']}, hold={st['hold']}, "
                  f"кап={api.state()['fps_cap']['cap']})")
        elif levers:
            st = api.render_skip(skip_overlay="overlay" in levers,
                                 skip_present="present" in levers,
                                 skip_draw="draw" in levers)
            api.fps_cap(cap="off")
            print(f"skip={sorted(levers)}: overlay={st['skip_overlay']}, "
                  f"present={st['skip_present']}, draw={st['skip_draw']}, "
                  f"кап={api.state()['fps_cap']['cap']}")
        elif args.uncapped:
            api.fps_cap(cap="off")
        if accelerated:
            after = api.frame_rate(2.0)
            ratio = after[1] / before[1] if before[1] > 0 else 0.0
            print(f"после: {after[0]:6.1f} кадров/с, {after[1]:6.1f} тиков/с"
                  f"   (×{ratio:.2f} по тикам симуляции)")

        total = 0.0
        for i in range(1, args.runs + 1):
            t0 = time.monotonic()
            print(f"\n=== прогон {i} из {args.runs} ===")
            rc = tas.main(list(ARGS))
            run_s = time.monotonic() - t0
            total += run_s
            print(f"--- прогон {i}: {run_s:.1f} с настенного времени")
            if rc != 0:
                print(f"прогон {i} не удался (код {rc})")
                return rc
    finally:
        if args.headless or levers:
            api.render_skip(reset=True)
            if levers:
                api.fps_cap(cap="game")
            print(f"\nрендер и кап вернулись (reset): кап="
                  f"{api.state()['fps_cap']['cap']}")
        elif args.uncapped:
            api.fps_cap(cap="game")

    print(f"\nдемонстрация завершена: {args.runs} прогона TAS первого сегмента R-03"
          f"{f', всего {total:.1f} с' if accelerated else ''}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
