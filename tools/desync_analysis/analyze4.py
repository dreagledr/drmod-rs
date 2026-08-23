# -*- coding: utf-8 -*-
"""Проверка недетерминизма: blade/ripper, стартовые кадры, остальные поля."""
import sys
import pandas as pd
import numpy as np

DIR = sys.argv[1] if len(sys.argv) > 1 else r"C:\temp\dbdump_out_release"

def load(rid, kind):
    return pd.read_csv(f"{DIR}\\run_{rid}_{kind}.csv")

rec = load(73, "record")
p = {rid: load(rid, "playback") for rid in (78, 81, 82)}

# blade_down / ripper_pressed / raw у playback
print("blade_down / ripper_pressed / raw-поля (не входят в InputUnit):")
for rid in (78, 81, 82):
    bd = p[rid]["blade_down"]
    rp = p[rid]["ripper_pressed"]
    print(f"  {rid}: blade_down уникальных: {bd.unique().tolist()}, ripper_pressed уникальных: {rp.unique().tolist()}")
print(f"  record 73: blade_down: {rec['blade_down'].unique().tolist()}, ripper_pressed: {rec['ripper_pressed'].unique().tolist()}")
print(f"  raw_down_0 у playback: {p[78]['raw_down_0'].isna().all()} (все NULL?)")

# Стартовые кадры 0-5: pos, hp, r_anim
print("\nСтартовые кадры (fi 0-3):")
cols = ["frame_index", "pos_x", "pos_y", "pos_z", "hp", "r_anim", "sword_state", "sword_hidden"]
print("fi | record pos          | 78 pos              | 81 pos              | 82 pos")
for fi in range(4):
    row = []
    for label, df in (("R", rec), ("78", p[78]), ("81", p[81]), ("82", p[82])):
        r = df.iloc[fi]
        row.append(f"{r['pos_x']:9.3f} {r['pos_y']:8.3f} {r['pos_z']:9.3f}")
    print(f"{fi} | {row[0]} | {row[1]} | {row[2]} | {row[3]}")
print("hp/r_anim fi=0:", [ (df.iloc[0]['hp'], df.iloc[0]['r_anim']) for df in (rec, p[78], p[81], p[82]) ])

# Полные поля ввода на fi=0..2
print("\nВсе поля ввода на fi=0..2 (record vs 78):")
in_cols = ["buttons_down","buttons_pressed","buttons_released","buttons_alternated",
           "left_stick_x","left_stick_y","right_stick_x","right_stick_y",
           "left_trigger","right_trigger","valid_input","repeat_count"]
for fi in range(3):
    print(f"  fi={fi}: rec={rec[in_cols].iloc[fi].to_dict()}")
    print(f"        p78={p[78][in_cols].iloc[fi].to_dict()}")

# Когда r_anim начинает расходиться между playback'ами (первые отличия)
print("\nПервые кадры, где r_anim различается между playback'ами:")
n = len(rec)
diff_78_81 = np.where(p[78]["r_anim"].to_numpy() != p[81]["r_anim"].to_numpy())[0]
diff_78_82 = np.where(p[78]["r_anim"].to_numpy() != p[82]["r_anim"].to_numpy())[0]
print(f"  78 vs 81: {diff_78_81[:15].tolist()}")
print(f"  78 vs 82: {diff_78_82[:15].tolist()}")

# Состояние игрока в моменты первых расхождений r_anim между playback: velocity
print("\nfi=170: состояние (vel, pos) 78/81/82:")
for rid in (78, 81, 82):
    r = p[rid].iloc[170]
    print(f"  {rid}: vel=({r['velocity_x']:.2f},{r['velocity_y']:.2f},{r['velocity_z']:.2f}) "
          f"pos=({r['pos_x']:.3f},{r['pos_y']:.3f},{r['pos_z']:.3f}) r_anim={r['r_anim']}")
