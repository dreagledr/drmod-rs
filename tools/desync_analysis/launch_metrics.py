# -*- coding: utf-8 -*-
"""Метрики ПЕРВОЙ air-атаки по ранам (окно [t_attack, t_attack+60]):
- t_attack = кадр ATTACK pressed (ввод одинаковый во всех ранах)
- t_hiS/t_hiE = первый/последний кадр blade_y > 2.0 (большая высота клинка)
- t_fall = кадр падения клинка (blade_y > 2.0 -> <= 2.0)
- t_launch = первый кадр рывка: прирост pos_y >= 0.2 И следующий кадр тоже
  растёт (>= 0.15) — одиночный «подскок» (dy=0.39 затем 0.00) не считается
- подготовка = t_launch - t_attack; длит_hi = t_hiE - t_hiS + 1
- дельта = t_launch - t_fall (отрицательная = лаунч ДО падения клинка)

Запуск: py -3 tools/desync_analysis/launch_metrics.py [DIR] REC_ID [PLAY_ID...]
"""
import sys
import pandas as pd

DIR = sys.argv[1] if len(sys.argv) > 1 else r"out\run142"
REC_ID = int(sys.argv[2]) if len(sys.argv) > 2 else 142
PLAY_IDS = [int(a) for a in sys.argv[3:]]

BLADE_HIGH = 2.0
LAUNCH_DY = 0.2
LAUNCH_DY2 = 0.15
WIN = 60


def metrics(df):
    p = df["buttons_pressed"].astype("int64")
    atk = df.index[p & (0x40 | 0x80) > 0]
    t_attack = int(atk[0]) if len(atk) else None
    if t_attack is None:
        return None
    lo, hi = t_attack, min(t_attack + WIN, len(df) - 1)

    blade = df["enemy_blade_y"].astype("float64").iloc[lo:hi + 1]
    high = blade > BLADE_HIGH
    t_hi_s = int(blade.index[high][0]) if high.any() else None
    t_hi_e = int(blade.index[high][-1]) if high.any() else None
    t_fall = None
    if t_hi_e is not None:
        for i in range(t_hi_e + 1, hi + 1):
            if df["enemy_blade_y"].iloc[i] <= BLADE_HIGH:
                t_fall = i
                break

    pos_y = df["pos_y"].astype("float64")
    t_launch = None
    for i in range(t_attack + 1, hi):
        dy = pos_y.iloc[i] - pos_y.iloc[i - 1]
        dy2 = pos_y.iloc[i + 1] - pos_y.iloc[i]
        if dy >= LAUNCH_DY and dy2 >= LAUNCH_DY2:
            t_launch = i
            break

    return {
        "t_attack": t_attack,
        "t_hi_s": t_hi_s,
        "t_hi_e": t_hi_e,
        "t_fall": t_fall,
        "t_launch": t_launch,
        "max_y": pos_y.iloc[lo:hi + 1].max(),
    }


def main():
    runs = [(REC_ID, "record")] + [(pid, "playback") for pid in PLAY_IDS]
    print(f"{'run':>4} {'kind':<8} {'maxY':>6} {'t_atk':>5} {'t_hiS':>5} {'t_hiE':>5} "
          f"{'t_fall':>6} {'t_lch':>5} {'подгот':>6} {'длит_hi':>7} {'дельта':>6}")
    for rid, kind in runs:
        df = pd.read_csv(f"{DIR}\\run_{rid}_{kind}.csv").reset_index(drop=True)
        m = metrics(df)
        if m is None:
            print(f"{rid:>4} {kind:<8} атак нет")
            continue
        prep = (m["t_launch"] - m["t_attack"]) if m["t_launch"] is not None else None
        dur_hi = (m["t_hi_e"] - m["t_hi_s"] + 1) if (m["t_hi_s"] is not None and m["t_hi_e"] is not None) else None
        delta = (m["t_launch"] - m["t_fall"]) if (m["t_launch"] is not None and m["t_fall"] is not None) else None
        f = lambda v: "-" if v is None else f"{v}"
        print(f"{rid:>4} {kind:<8} {m['max_y']:>6.2f} {f(m['t_attack']):>5} {f(m['t_hi_s']):>5} "
              f"{f(m['t_hi_e']):>5} {f(m['t_fall']):>6} {f(m['t_launch']):>5} {f(prep):>6} "
              f"{f(dur_hi):>7} {f(delta):>6}")


if __name__ == "__main__":
    main()