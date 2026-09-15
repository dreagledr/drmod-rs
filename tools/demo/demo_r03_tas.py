# -*- coding: utf-8 -*-
"""Демонстрация готового TAS первого сегмента R-03.

Запуск без аргументов:

    py -3 tools\\demo\\demo_r03_tas.py

Скрипт сам выставляет мир (фиксированный шаг 1/60, заморозка RNG seed 1,
тиковый триггер), рестартует миссию и прогоняет TAS (`--preset ls8` —
восемь lightning strike подряд) **три раза подряд**, печатая результат каждого
прогона: серию анимаций и точку остановки.

Headless-прогон — то же самое, но без отрисовки и без капа **на время самого
прогона**:

    py -3 tools\\demo\\demo_r03_tas.py --headless

* `--headless` передаёт прогону `--headless-run`: headless включается, когда
  скрипт уже стартовал (`running`), то есть **рестарт миссии и загрузка уровня
  проходят с обычной отрисовкой**, а по концу прогона мод возвращает рендер и
  прежний кап сам — до следующего рестарта отрисовка уже на месте. Так проверяем
  гипотезу 2026-09-15: падение в `d3d9.dll` было на прогоне, где хуки стояли и
  во время загрузки;
* `--uncapped` — только снять кап кадров на всю сессию (отрисовку оставить);
* `--skip overlay,present,draw` — диагностика: снять выключатели на всю сессию
  (плюс кап), чтобы изолировать виновника падения.

Каждый прогон печатается с настенным временем, в конце — среднее. ⚠️ Прогон
2026-09-15 с headless «на всю сессию» довёл игру до падения в `d3d9.dll`
(разбор — `docs/HEADLESS.md` §5); текущий режим `--headless` как раз это
исключает.
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
                   help="headless на самом прогоне (рестарт и загрузка — с "
                        "обычной отрисовкой, возврат сразу по концу прогона)")
    p.add_argument("--uncapped", action="store_true",
                   help="снять кап кадров на всю сессию (отрисовку оставить)")
    p.add_argument("--skip", default=None,
                   help="диагностика: снять выключатели на всю сессию — "
                        "overlay,present,draw (кап тоже снимается, чтобы прогоны "
                        "были сравнимы)")
    p.add_argument("--skip-run", dest="skip_run", default=None,
                   help="диагностика: на время прогона снять выбранные рычаги "
                        "(overlay,present,draw; cap — только снять кап), "
                        "рестарт и загрузка — с обычной отрисовкой")
    args = p.parse_args(argv)

    api.setup_stdout()
    run_args = list(ARGS)
    if args.headless:
        run_args.append("--headless-run")
    if args.skip_run is not None:
        run_args += ["--skip-run", args.skip_run]

    levers = set()
    if args.skip:
        levers = {s.strip() for s in args.skip.split(",") if s.strip()}
        unknown = levers - {"overlay", "present", "draw"}
        if unknown:
            print(f"неизвестные выключатели: {', '.join(sorted(unknown))}")
            return 2

    # На всю сессию выключатели ставит только диагностический --skip; штатный
    # headless — это режим прогона (его включает сам r03_baseline на старте
    # прогона и снимает по концу), поэтому тут ничего не трогаем.
    session_wide = args.uncapped or bool(levers)
    before = api.frame_rate(2.0) if session_wide else (0.0, 0.0)
    if session_wide:
        print(f"до:    {before[0]:6.1f} кадров/с, {before[1]:6.1f} тиков/с"
              f"   кап={api.state()['fps_cap']['cap']}")

    try:
        if levers:
            st = api.render_skip(skip_overlay="overlay" in levers,
                                 skip_present="present" in levers,
                                 skip_draw="draw" in levers)
            api.fps_cap(cap="off")
            print(f"skip={sorted(levers)}: overlay={st['skip_overlay']}, "
                  f"present={st['skip_present']}, draw={st['skip_draw']}, "
                  f"кап={api.state()['fps_cap']['cap']}")
        elif args.uncapped:
            api.fps_cap(cap="off")
        if session_wide:
            after = api.frame_rate(2.0)
            ratio = after[1] / before[1] if before[1] > 0 else 0.0
            print(f"после: {after[0]:6.1f} кадров/с, {after[1]:6.1f} тиков/с"
                  f"   (×{ratio:.2f} по тикам симуляции)")
        if args.headless:
            print("headless: включит сам прогон (рестарт миссии и загрузка "
                  "уровня пройдут с обычной отрисовкой)")

        times = []
        for i in range(1, args.runs + 1):
            t0 = time.monotonic()
            print(f"\n=== прогон {i} из {args.runs} ===")
            rc = tas.main(list(run_args))
            run_s = time.monotonic() - t0
            times.append(run_s)
            print(f"--- прогон {i}: {run_s:.1f} с настенного времени")
            if rc != 0:
                print(f"прогон {i} не удался (код {rc})")
                return rc
    finally:
        if session_wide:
            api.render_skip(reset=True)
            api.fps_cap(cap="game")
            print(f"\nрендер и кап вернулись: кап={api.state()['fps_cap']['cap']}")

    total = sum(times)
    print(f"\nдемонстрация завершена: {args.runs} прогона TAS первого сегмента R-03"
          f" (всего {total:.1f} с"
          f"{f', в среднем {total / len(times):.1f} с на прогон' if times else ''})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
