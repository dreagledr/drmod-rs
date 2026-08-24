# -*- coding: utf-8 -*-
"""Сопоставление кадров воздушной атаки игрока с состоянием врага
(ENEMY_TRACKING): record vs playbacks.

Находит в записи окна air-атаки (JUMP pressed → LIGHT_ATTACK pressed в воздухе),
выводит для каждого кадра окна состояние ближайшего врага (enemy_pos/blade_y/
anim/frame/hp) в record и каждом playback, а в момент удара — |Δenemy_pos|
и фазу анимации врага (65545 — «прыжок», 19 — выпад).

Запуск: py -3 tools/desync_analysis/analyze_enemy.py [DIR] [REC_ID PLAY_ID...]
"""
import sys
import pandas as pd
import numpy as np

DIR = sys.argv[1] if len(sys.argv) > 1 else r"C:\temp\dbdump_out_release"
JUMP = 0x10
# Air-атака — любой удар в воздухе (лёгкий или тяжёлый)
LIGHT_ATTACK = 0x40
HEAVY_ATTACK = 0x80
ATTACK = LIGHT_ATTACK | HEAVY_ATTACK
# Высота, ниже которой игрок считается на земле (конец air-окна)
GROUND_Y = 0.3

ENEMY_COLS = ["enemy_pos_x", "enemy_pos_y", "enemy_pos_z",
              "enemy_blade_y", "enemy_anim", "enemy_frame", "enemy_hp"]


def load(rid, kind):
    df = pd.read_csv(f"{DIR}\\run_{rid}_{kind}.csv")
    df["replay_id"] = df["replay_id"].astype(int)
    return df


def enemy_present(df):
    return df["enemy_hp"].fillna(0) > 0


def air_windows(df):
    """Окна air-атаки: (start, attack, end) — начало воздушной фазы
    (pos_y > GROUND_Y), первый удар в воздухе (LIGHT/HEAVY_ATTACK pressed),
    конец воздушной фазы (приземление). JUMP pressed не требуется — игрок
    мог прыгнуть с уступа или задержать атаку до конца прыжка."""
    pressed = df["buttons_pressed"].astype("int64")
    pos_y = df["pos_y"].astype("float64")
    in_air = pos_y > GROUND_Y
    windows = []
    i = 0
    n = len(df)
    while i < n:
        if in_air.iloc[i]:
            start = i
            j = i
            while j < n and in_air.iloc[j]:
                j += 1
            end = j - 1
            hits = pressed.iloc[start:end + 1]
            attack_idx = hits.index[hits & ATTACK > 0]
            if len(attack_idx):
                windows.append((start, attack_idx[0], end))
            i = j
        else:
            i += 1
    return windows


def enemy_row(df, fi):
    """Строка состояния врага в кадре fi (None — врага нет/нет данных)."""
    if fi >= len(df) or not enemy_present(df).iloc[fi]:
        return None
    r = df.iloc[fi]
    return (r["enemy_pos_x"], r["enemy_pos_y"], r["enemy_pos_z"],
            r["enemy_blade_y"], r["enemy_anim"], r["enemy_frame"], r["enemy_hp"])


def fmt_enemy(e):
    if e is None:
        return "  -  "
    return (f"({e[0]:7.2f},{e[1]:6.2f},{e[2]:7.2f}) "
            f"blade={e[3]:5.2f} anim={e[4]:5.0f} fr={e[5]:3.0f} hp={e[6]:4.0f}")


def denemy(rec_e, play_e):
    if rec_e is None or play_e is None:
        return None
    return np.sqrt((rec_e[0] - play_e[0]) ** 2 + (rec_e[1] - play_e[1]) ** 2
                   + (rec_e[2] - play_e[2]) ** 2)


def analyze(rec_id, play_id):
    rec = load(rec_id, "record").reset_index(drop=True)
    play = load(play_id, "playback").reset_index(drop=True)
    if "enemy_hp" not in rec.columns:
        print(f"Record {rec_id}: нет enemy-колонок (запись до ENEMY_TRACKING) — пропуск")
        return
    n = min(len(rec), len(play))
    print("=" * 100)
    print(f"Record {rec_id} ({len(rec)} кадров) vs Playback {play_id} ({len(play)} кадров)")

    windows = air_windows(rec)
    if not windows:
        print("  air-атак (JUMP→LIGHT_ATTACK в воздухе) в записи не найдено")
        return
    print(f"  air-атак: {len(windows)}")

    for wi, (start, attack, end) in enumerate(windows, 1):
        print(f"\n--- Air-атака #{wi}: прыжок fi={start}, удар fi={attack}, "
              f"приземление fi={end} ---")
        print("  fi | rec: pY   enemy(blade/anim/fr/hp)          | "
              "play: pY   enemy(blade/anim/fr/hp)          | dEnemy")
        for fi in range(start, min(end, n) + 1):
            rec_e = enemy_row(rec, fi)
            play_e = enemy_row(play, fi)
            d = denemy(rec_e, play_e)
            d_s = f"{d:.2f}" if d is not None else "  -  "
            print(f"  {fi:3d} | {rec['pos_y'].iloc[fi]:6.2f} {fmt_enemy(rec_e)} | "
                  f"{play['pos_y'].iloc[fi]:6.2f} {fmt_enemy(play_e)} | {d_s}")

        # Метрики в момент удара
        rec_e = enemy_row(rec, attack)
        play_e = enemy_row(play, attack)
        d = denemy(rec_e, play_e)
        print(f"  УДАР fi={attack}: rec {fmt_enemy(rec_e)} | play {fmt_enemy(play_e)}"
              + (f" | |Δenemy_pos|={d:.2f} м" if d is not None else ""))
        if rec_e is not None and play_e is not None:
            print(f"    blade_y: rec={rec_e[3]:.2f} play={play_e[3]:.2f} "
                  f"(Δ={play_e[3] - rec_e[3]:+.2f}) | anim: rec={rec_e[4]:.0f} "
                  f"play={play_e[4]:.0f} | frame: rec={rec_e[5]:.0f} play={play_e[5]:.0f}")


def main():
    if len(sys.argv) < 3:
        print("Использование: py -3 analyze_enemy.py [DIR] REC_ID PLAY_ID [PLAY_ID...]")
        sys.exit(1)
    rec_id = int(sys.argv[2])
    for pid in (int(a) for a in sys.argv[3:]):
        analyze(rec_id, pid)


if __name__ == "__main__":
    main()