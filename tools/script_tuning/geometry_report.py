# -*- coding: utf-8 -*-
"""Сводка по сериям `parry_geometry.py`: парирования, перелёты, геометрия контакта.

Читает run-CSV (`--out`) и печатает по файлу одну строку: сколько прогонов,
сколько парирований, сколько взяли барьер, разброс `post_gain` и угол клинка в
контакте (по замеру сильные подбросы идут при |угол| ≲ 30°, перестоп — при
60–70°, и тогда подброса нет).

    py -3 tools\\script_tuning\\geometry_report.py out\\pg_s6.csv out\\pg_s14.csv
    py -3 tools\\script_tuning\\geometry_report.py "out/pg_n*.csv" --barrier 20
"""
import argparse
import csv
import glob
import statistics as st
import sys


def load(path):
    with open(path, encoding="utf-8") as f:
        rows = list(csv.DictReader(f))
    out = []
    for r in rows:
        rec = {}
        for k, v in r.items():
            if v in ("", "None", None):
                rec[k] = None
            else:
                try:
                    rec[k] = float(v)
                except ValueError:
                    rec[k] = v
        out.append(rec)
    return out


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("paths", nargs="+", help="CSV или glob-маска")
    p.add_argument("--barrier", type=float, default=20.0)
    a = p.parse_args(argv)
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

    files = []
    for pat in a.paths:
        files += sorted(glob.glob(pat)) if any(c in pat for c in "*?[") else [pat]
    if not files:
        print("нет файлов")
        return 1
    print(f"{'файл':<28} {'n':>3} {'пар':>7} {'>=бар':>7} "
          f"{'post_gain':<28} {'угол в контакте':<24} дист(удар)")
    for path in files:
        try:
            rows = load(path)
        except OSError as e:
            print(f"{path:<28} не прочитан: {e}")
            continue
        gains = [r["post_gain"] for r in rows if r.get("post_gain") is not None]
        cleared = [r for r in rows if (r.get("max_y") or 0) >= a.barrier]
        angles = [r["p_angle_deg"] for r in rows
                  if r.get("p_angle_deg") is not None]
        adists = [r["a_dist_h"] for r in rows if r.get("a_dist_h") is not None]
        name = path.replace("\\", "/").split("/")[-1]
        gains_s = (f"{min(gains):.1f}..{max(gains):.1f} "
                   f"(медиана {st.median(gains):.1f})") if gains else "—"
        ang_s = (f"{min(angles):.0f}..{max(angles):.0f}° "
                 f"(медиана {st.median(angles):.0f}°)") if angles else "—"
        dist_s = (f"{min(adists):.2f}..{max(adists):.2f}") if adists else "—"
        print(f"{name:<28} {len(rows):>3} {len(gains):>3}/{len(rows):<3} "
              f"{len(cleared):>3}/{len(rows):<3} {gains_s:<28} {ang_s:<24} {dist_s}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
