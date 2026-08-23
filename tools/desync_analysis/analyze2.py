# -*- coding: utf-8 -*-
"""Глубокий анализ: лаг ввода, точка десинка, сравнение 78/81 vs 82."""
import sys
import pandas as pd
import numpy as np

DIR = sys.argv[1] if len(sys.argv) > 1 else r"C:\temp\dbdump_out_release"

INPUT_COLS = [
    "buttons_down", "buttons_pressed", "buttons_released", "buttons_alternated",
    "left_stick_x", "left_stick_y", "right_stick_x", "right_stick_y",
    "left_trigger", "right_trigger", "valid_input", "repeat_count",
]

def load(rid, kind):
    return pd.read_csv(f"{DIR}\\run_{rid}_{kind}.csv")

def dpos(rec, play):
    return np.sqrt(
        (play["pos_x"] - rec["pos_x"]) ** 2
        + (play["pos_y"] - rec["pos_y"]) ** 2
        + (play["pos_z"] - rec["pos_z"]) ** 2
    )

def input_vec(df):
    return df[INPUT_COLS].astype("float64").to_numpy()

def lag_match(rec_in, play_in, lag):
    """Доля кадров, где play[fi] == rec[fi-lag] (сравнение по индексу кадра)."""
    if lag == 0:
        n = len(rec_in)
        eq = (rec_in == play_in).all(axis=1)
    else:
        n = len(rec_in) - lag
        eq = (rec_in[:n] == play_in[lag:]).all(axis=1)
    return eq.sum() / n

REC_ID = int(sys.argv[2]) if len(sys.argv) > 2 else 73
PLAY_IDS = [int(a) for a in sys.argv[3:]] if len(sys.argv) > 3 else [78, 81, 82]

for pid in PLAY_IDS:
    rec = load(REC_ID, "record")
    play = load(pid, "playback")
    n = min(len(rec), len(play))
    rec = rec.iloc[:n]
    play = play.iloc[:n]
    rec_in = input_vec(rec)
    play_in = input_vec(play)
    d = dpos(rec, play)
    print("=" * 78)
    print(f"Record {REC_ID} vs Playback {pid}")
    print(f"  лаг 0: совпадение ввода {lag_match(rec_in, play_in, 0)*100:.1f}% кадров")
    print(f"  лаг 1: совпадение ввода (play[fi]==rec[fi-1]) {lag_match(rec_in, play_in, 1)*100:.1f}% кадров")
    print(f"  лаг 2: {lag_match(rec_in, play_in, 2)*100:.1f}%")

    # Производная |Δpos| — ищем начало ускоренного роста
    dd = np.diff(d)
    # первый fi где рост начался и держится (10 кадров подряд рост)
    run = 0
    desync_start = -1
    for i in range(len(dd)):
        if dd[i] > 0.005:
            run += 1
            if run >= 10 and desync_start < 0:
                desync_start = i - 9
        else:
            run = 0
    print(f"  начало устойчивого роста |Δpos| (10 кадров подряд +): fi={desync_start}")

    # Точки смены анимации в record (границы r_anim)
    anim_changes_rec = np.where(rec["r_anim"].to_numpy() != np.roll(rec["r_anim"].to_numpy(), 1))[0][1:]
    anim_changes_play = np.where(play["r_anim"].to_numpy() != np.roll(play["r_anim"].to_numpy(), 1))[0][1:]
    print(f"  смены r_anim в record: {anim_changes_rec.tolist()[:40]}")
    print(f"  смены r_anim в playback: {anim_changes_play.tolist()[:40]}")

    # Максимальные расхождения
    print(f"  макс |Δpos| {d.max():.3f} м на fi={d.idxmax()}")

print("\n" + "=" * 78)
print(f"Окно fi=225..270: record {REC_ID} vs playback {PLAY_IDS[0]} и {PLAY_IDS[1]} (r_anim, кнопки, dpos)")
rec = load(REC_ID, "record")
for pid in (PLAY_IDS[0], PLAY_IDS[1]):
    play = load(pid, "playback")
    n = min(len(rec), len(play))
    rec = rec.iloc[:n]
    play = play.iloc[:n]
    d = dpos(rec, play)
    print(f"\n--- playback {pid} ---")
    print("  fi | r_anim rec|play | b_down rec|play (hex)          | rsx rec|play | dpos")
    for fi in range(225, 271):
        ra_r, ra_p = rec["r_anim"].iloc[fi], play["r_anim"].iloc[fi]
        bd_r, bd_p = int(rec["buttons_down"].iloc[fi]) & 0xFFFFFFFF, int(play["buttons_down"].iloc[fi]) & 0xFFFFFFFF
        mark = ""
        if ra_r != ra_p:
            mark += " *anim"
        if bd_r != bd_p:
            mark += " *btn"
        print(f"  {fi} | {ra_r:3.0f} {ra_p:3.0f} | 0x{bd_r:08X} 0x{bd_p:08X} | "
              f"{rec['right_stick_x'].iloc[fi]:5.0f} {play['right_stick_x'].iloc[fi]:5.0f} | {d.iloc[fi]:.3f}{mark}")
