# -*- coding: utf-8 -*-
"""Отчёт по `sweep.csv`: надёжность вариантов (сколько повторов взяли барьер).

    py -3 tools\\script_tuning\\sweep_report.py [CSV] [--barrier 20]
"""
import argparse
import collections
import csv
import sys


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("csv", nargs="?", default=r"out\tuning3\sweep.csv")
    p.add_argument("--barrier", type=float, default=20.0)
    a = p.parse_args(argv)
    sys.stdout.reconfigure(line_buffering=True)

    with open(a.csv, encoding="utf-8") as f:
        rows = list(csv.DictReader(f))
    if not rows:
        print("нет строк")
        return 1

    groups = collections.defaultdict(list)
    for r in rows:
        key = (int(r["jump"]), int(r["attack"]))
        if r.get("error"):
            groups[key].append(("err", r["error"][:40]))
            continue
        try:
            groups[key].append(float(r["max_y"]))
        except (TypeError, ValueError):
            groups[key].append(("err", "нет max_y"))

    print(f"{a.csv}: {len(rows)} прогонов, {len(groups)} вариантов, "
          f"барьер {a.barrier:g} м\n")
    print(f"{'jump':>4} {'atk':>4} | {'max_y по повторам':<34} | взяли")
    best = []
    for (j, atk), values in sorted(groups.items()):
        nums = [v for v in values if isinstance(v, float)]
        errs = [v for v in values if not isinstance(v, float)]
        cleared = sum(1 for v in nums if v >= a.barrier)
        spread = ", ".join(f"{v:.1f}" for v in sorted(nums, reverse=True))
        if errs:
            spread += f"  ({len(errs)} ошибок)"
        print(f"{j:>4} {atk:>4} | {spread:<34} | {cleared}/{len(values)}")
        if nums:
            best.append((cleared, nums[0], j, atk))

    print("\nлучшие по числу взятых повторов (потом по max_y):")
    for cleared, top, j, atk in sorted(best, key=lambda x: (-x[0], -x[1]))[:5]:
        print(f"  jump={j} attack={atk}: {cleared} повторов ≥ {a.barrier:g} м, "
              f"лучший {top:.1f} м")
    return 0


if __name__ == "__main__":
    sys.exit(main())
