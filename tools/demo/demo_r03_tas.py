# -*- coding: utf-8 -*-
"""Демонстрация готового TAS первого сегмента R-03.

Запуск без аргументов:

    py -3 tools\\demo\\demo_r03_tas.py

Скрипт сам выставляет мир (фиксированный шаг 1/60, заморозка RNG seed 1,
тиковый триггер), рестартует миссию и прогоняет TAS (`--preset ls8` —
восемь lightning strike подряд) **три раза подряд**, печатая результат каждого
прогона: серию анимаций и точку остановки.

Headless-прогон — то же самое, но без отрисовки и без капа кадров:

    py -3 tools\\demo\\demo_r03_tas.py --headless --uncapped

* `--headless` снимает отрисовку (`POST /render`: overlay мода, вывод кадра,
  геометрия игры) — логика кадра при этом не меняется: кадр скрипта подаётся по
  тикам симуляции (`api::feed_tick`), поэтому результат прогона должен совпасть
  с обычным, а прогон идёт быстрее;
* `--uncapped` снимает кап кадров игры (`POST /fps`) — именно он ограничивает
  скорость, когда отрисовка уже снята.

Оба флага печатают темп кадров движка и тиков до/после и возвращают всё как было
в конце (в том числе при ошибке). Разбор и оговорки — `docs/HEADLESS.md`.
"""
import argparse
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "script_tuning"))

import drmod_api as api  # noqa: E402  (путь добавляем выше)
import r03_baseline as tas  # noqa: E402  (путь добавляем выше)

ARGS = ["--preset", "ls8", "--from", "9999"]


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--runs", type=int, default=3, help="сколько прогонов подряд")
    p.add_argument("--headless", action="store_true",
                   help="снять отрисовку на время прогонов (POST /render)")
    p.add_argument("--uncapped", action="store_true",
                   help="снять кап кадров игры (POST /fps cap=off)")
    args = p.parse_args(argv)

    api.setup_stdout()
    accelerated = args.headless or args.uncapped
    before = api.frame_rate(2.0) if accelerated else (0.0, 0.0)
    if accelerated:
        print(f"до:    {before[0]:6.1f} кадров/с, {before[1]:6.1f} тиков/с")

    try:
        if args.uncapped:
            api.fps_cap(cap="off")
        if args.headless:
            st = api.render_skip(skip_overlay=True, skip_present=True, skip_draw=True)
            print(f"headless: overlay={st['skip_overlay']} "
                  f"present={st['skip_present']} draw={st['skip_draw']}")
        if accelerated:
            after = api.frame_rate(2.0)
            ratio = after[0] / before[0] if before[0] > 0 else 0.0
            print(f"после: {after[0]:6.1f} кадров/с, {after[1]:6.1f} тиков/с"
                  f"   (×{ratio:.2f} по кадрам движка)")

        for i in range(1, args.runs + 1):
            print(f"\n=== прогон {i} из {args.runs} ===")
            rc = tas.main(list(ARGS))
            if rc != 0:
                print(f"прогон {i} не удался (код {rc})")
                return rc
    finally:
        if args.headless:
            api.render_skip(reset=True)
            print("\nотрисовка возвращена (reset)")
        if args.uncapped:
            api.fps_cap(cap="game")

    print(f"\nдемонстрация завершена: {args.runs} прогона TAS первого сегмента R-03")
    return 0


if __name__ == "__main__":
    sys.exit(main())
