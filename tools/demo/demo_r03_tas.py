# -*- coding: utf-8 -*-
"""Демонстрация готового TAS первого сегмента R-03.

Запуск без аргументов:

    py -3 tools\\demo\\demo_r03_tas.py

Скрипт сам выставляет мир (фиксированный шаг 1/60, заморозка RNG seed 1,
тиковый триггер), рестартует миссию и прогоняет TAS (`--preset ls8` —
восемь lightning strike подряд) **три раза подряд**, печатая результат каждого
прогона: серию анимаций и точку остановки.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "script_tuning"))

import r03_baseline as tas  # noqa: E402  (путь добавляем выше)

RUNS = 3
ARGS = ["--preset", "ls8", "--from", "9999"]


def main():
    for i in range(1, RUNS + 1):
        print(f"\n=== прогон {i} из {RUNS} ===")
        rc = tas.main(list(ARGS))
        if rc != 0:
            print(f"прогон {i} не удался (код {rc})")
            return rc
    print(f"\nдемонстрация завершена: {RUNS} прогона TAS первого сегмента R-03")
    return 0


if __name__ == "__main__":
    sys.exit(main())
