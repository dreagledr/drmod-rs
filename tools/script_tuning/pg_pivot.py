# -*- coding: utf-8 -*-
"""Разбор окна парирования: геометрия по кадровым срезам (rel) и прогонам.

Показывает, что стоит за корреляциями `parry_geometry.py`: видно ли разницу
между большим и нулевым подбросом ЗА кадры до контакта (на управляемых
моментах) или только после него.

    py -3 tools\\script_tuning\\pg_pivot.py > out\\pg_pivot.txt
    py -3 tools\\script_tuning\\pg_pivot.py out\\parry_geometry_window.csv
"""
import csv
import sys

FEATS = ("player_y", "dist_h", "angle_deg", "enemy_anim", "enemy_frame")
#: кадры подачи удара (rel ≈ −18…−22), последние кадры до контакта и контакт
RELS = (-22, -20, -18, -5, -2, 0)


def main(path="out/parry_geometry_window.csv"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    with open(path, encoding="utf-8") as f:
        rows = list(csv.DictReader(f))
    by_rel, gains = {}, {}
    for r in rows:
        by_rel.setdefault(int(r["rel"]), {})[r["run"]] = r
        gains[r["run"]] = float(r["post_gain"])
    runs = sorted(by_rel.get(0, {}), key=lambda k: -gains[k])
    print("парирования по убыванию подброса; rel=0 — кадр контакта:")
    print("  run  gain " + " ".join(f"{f:>10}" for f in FEATS))
    for run in runs:
        r0 = by_rel[0][run]
        print(f"  {run:>3} {gains[run]:5.1f} "
              + " ".join(f"{r0.get(f, ''):>10}" for f in FEATS))
    for rel in RELS[1:]:
        print(f"\nrel={rel:+d}:")
        for run in runs:
            r = by_rel.get(rel, {}).get(run)
            if r is None:
                continue
            print(f"  {run:>3} {gains[run]:5.1f} "
                  + " ".join(f"{r.get(f, ''):>10}" for f in FEATS))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else
                  "out/parry_geometry_window.csv"))
