# -*- coding: utf-8 -*-
"""Анализ расхождений Record(73) vs Playback(78/81/82).

1) Первый кадр, где ввод отличается (по любому полю InputUnit).
2) Первый кадр, где |dpos| превышает пороги.
3) Профиль длительностей кадров (дропы FPS) по обоим прогонам.
4) Окно вокруг начала расхождения ввода.
"""
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
    df = pd.read_csv(f"{DIR}\\run_{rid}_{kind}.csv")
    df["replay_id"] = df["replay_id"].astype(int)
    return df

def dpos(rec, play):
    d = np.sqrt(
        (play["pos_x"] - rec["pos_x"]) ** 2
        + (play["pos_y"] - rec["pos_y"]) ** 2
        + (play["pos_z"] - rec["pos_z"]) ** 2
    )
    return d

def hex32(v):
    return f"0x{int(v) & 0xFFFFFFFF:08X}"

def frame_dur(df):
    """Длительность кадра = разность накопленного времени."""
    return df["frame_duration_ms"].diff().fillna(0.0)

def analyze(rec_id, play_id):
    rec = load(rec_id, "record").reset_index(drop=True)
    play = load(play_id, "playback").reset_index(drop=True)
    n = min(len(rec), len(play))
    print("=" * 78)
    print(f"Record {rec_id} ({len(rec)} кадров) vs Playback {play_id} ({len(play)} кадров)")
    print(f"  total elapsed: record {rec['frame_duration_ms'].iloc[-1]:.0f} ms, "
          f"playback {play['frame_duration_ms'].iloc[-1]:.0f} ms")

    # --- 1. Первый кадр расхождения ввода ---
    input_diff = None
    first_input_diff = None
    for c in INPUT_COLS:
        diff = (rec[c].astype("float64") != play[c].astype("float64"))
        if diff.any():
            idx = diff.idxmax()  # первый True (idxmax даёт первый максимум)
            if first_input_diff is None or idx < first_input_diff:
                first_input_diff = idx
                input_diff = c
    print(f"\n[1] Первый кадр расхождения ВВОДА: fi={first_input_diff} (колонка {input_diff})")
    if first_input_diff is not None:
        for c in INPUT_COLS:
            diff = (rec[c].astype("float64") != play[c].astype("float64"))
            if diff.any() and diff.idxmax() <= first_input_diff + 5:
                rv, pv = rec[c].iloc[first_input_diff], play[c].iloc[first_input_diff]
                rv2, pv2 = rec[c].iloc[first_input_diff + 1], play[c].iloc[first_input_diff + 1] \
                    if first_input_diff + 1 < n else (None, None)
                print(f"    {c}: record={rv} playback={pv} | next: rec={rv2} play={pv2}")

    # --- 2. Первый кадр по порогам dpos ---
    d = dpos(rec, play)
    print("\n[2] |Δpos| по кадрам:")
    for th in (0.01, 0.1, 0.5, 1.0):
        over = (d > th).idxmax() if (d > th).any() else -1
        # idxmax возвращает первый индекс максимального значения -> для bool это первый True
        print(f"    первый fi с |Δpos|>{th}: {over if over >= 0 else 'нет'}")
    print(f"    макс |Δpos|: {d.max():.3f} м на fi={d.idxmax()}")
    print(f"    |Δpos| на первом расхождении ввода: {d.iloc[first_input_diff]:.3f} м" if first_input_diff is not None else "")

    # --- 3. Дропы длительности кадра ---
    rd = frame_dur(rec)
    pdur = frame_dur(play)
    print("\n[3] Длительности кадров (дропы FPS):")
    for name, dd, src in (("record", rd, rec), ("playback", pdur, play)):
        big = dd[dd > 25.0]
        print(f"    {name}: кадров {len(dd)}, макс {dd.max():.1f} мс, "
              f"кадров >25 мс: {len(big)} ({100*len(big)/len(dd):.1f}%)")
        if len(big):
            print(f"      >25 мс на fi: {big.index[:12].tolist()}")
    # Совпадают ли длительности кадров
    same_dur = np.allclose(rd, pdur, atol=0.01)
    print(f"    длительности record==playback по всем кадрам: {same_dur}")
    if not same_dur:
        diff_dur = (rd - pdur).abs()
        print(f"    макс |разность длительности|: {diff_dur.max():.1f} мс на fi={diff_dur.idxmax()}")

    # --- 4. Окно вокруг первого расхождения ввода ---
    if first_input_diff is not None:
        w0 = max(0, first_input_diff - 8)
        w1 = min(n - 1, first_input_diff + 12)
        print(f"\n[4] Окно fi={w0}..{w1} (первое расхождение ввода на {first_input_diff}):")
        print("  fi | dur_r dur_p | b_down_r b_down_p | rsx_r rsx_p | r_anim r_anim | dpos")
        for fi in range(w0, w1 + 1):
            mark = " <==" if fi == first_input_diff else ""
            print(
                f"  {fi} | {rd.iloc[fi]:6.1f} {pdur.iloc[fi]:6.1f} | "
                f"{hex32(rec['buttons_down'].iloc[fi])} {hex32(play['buttons_down'].iloc[fi])} | "
                f"{rec['right_stick_x'].iloc[fi]:6.0f} {play['right_stick_x'].iloc[fi]:6.0f} | "
                f"{rec['r_anim'].iloc[fi]:3.0f} {play['r_anim'].iloc[fi]:3.0f} | {d.iloc[fi]:.3f}{mark}"
            )

for pid in (78, 81, 82):
    analyze(73, pid)
