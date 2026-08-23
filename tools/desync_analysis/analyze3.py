# -*- coding: utf-8 -*-
"""Сравнение playback'ов между собой: детерминирован ли playback при одинаковом вводе?"""
import sys
import pandas as pd
import numpy as np

DIR = sys.argv[1] if len(sys.argv) > 1 else r"C:\temp\dbdump_out_release"
INPUT_COLS = [
    "buttons_down", "buttons_pressed", "buttons_released", "buttons_alternated",
    "left_stick_x", "left_stick_y", "right_stick_x", "right_stick_y",
    "left_trigger", "right_trigger", "valid_input", "repeat_count",
]

def load(rid):
    return pd.read_csv(f"{DIR}\\run_{rid}_playback.csv")

def dpos(a, b):
    return np.sqrt((b["pos_x"]-a["pos_x"])**2 + (b["pos_y"]-a["pos_y"])**2 + (b["pos_z"]-a["pos_z"])**2)

PLAY_IDS = [int(a) for a in sys.argv[2:]] if len(sys.argv) > 2 else [78, 81, 82]
p = {rid: load(rid) for rid in PLAY_IDS}
# длины могут различаться (вариант D: дубли добавляют кадры) — обрезаем до минимума
n0 = min(len(v) for v in p.values())
p = {rid: v.iloc[:n0] for rid, v in p.items()}

# Ввод у всех одинаковый?
a = p[PLAY_IDS[0]][INPUT_COLS].astype("float64").to_numpy()
for rid in PLAY_IDS[1:]:
    b = p[rid][INPUT_COLS].astype("float64").to_numpy()
    same = (a == b).all()
    print(f"ввод {PLAY_IDS[0]} == ввод {rid}: {same}")
    if not same:
        diff = (a != b).any(axis=1)
        print(f"  первый отличающийся кадр: {np.argmax(diff)}")

print()
for i in range(len(PLAY_IDS)):
    for j in range(i + 1, len(PLAY_IDS)):
        x, y = PLAY_IDS[i], PLAY_IDS[j]
        d = dpos(p[x], p[y])
        print(f"|Δpos| между playback {x} и {y}: макс {d.max():.3f} м на fi={d.idxmax()}, "
              f"первый fi >0.1: {(d>0.1).idxmax() if (d>0.1).any() else 'нет'}")

# Окно: r_anim и dpos первых двух playback
x0, y0 = PLAY_IDS[0], PLAY_IDS[1]
ids_str = " ".join(str(r) for r in PLAY_IDS[:3])
print(f"\nfi | r_anim {ids_str} | dpos({x0},{y0}) | b_down {x0} | b_down {y0}")
for fi in range(170, 261):
    ra = [p[r]["r_anim"].iloc[fi] for r in PLAY_IDS[:3]]
    d = dpos(p[x0], p[y0]).iloc[fi]
    bdx = int(p[x0]["buttons_down"].iloc[fi]) & 0xFFFFFFFF
    bdy = int(p[y0]["buttons_down"].iloc[fi]) & 0xFFFFFFFF
    mark = ""
    if len(set(ra)) > 1:
        mark = " <== r_anim различается"
    if fi in (170, 171, 172, 173, 183, 184, 233, 234, 235, 236, 248, 249, 259, 260, 264):
        mark = " <-fi"
    print(f"{fi:3d} | {ra[0]:3d} {ra[1]:3d} {ra[2]:3d} | {d:7.3f} | 0x{bdx:08X} 0x{bdy:08X}{mark}")
