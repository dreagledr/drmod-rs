# -*- coding: utf-8 -*-
"""График air-атаки для всех ранов в одном PNG: сетка subplot'ов, по одному
на каждый ран (record + playbacks). В каждом: позиция Y игрока, высота меча
врага (enemy_blade_y), маркеры прыжка/атаки игрока (по вводу) и начала атаки
врага (переход enemy_anim в 65545 «прыжок» / 19 «выпад»).

Запуск: py -3 tools/desync_analysis/plot_enemy.py [DIR] REC_ID [PLAY_ID...]
Сохраняет run_<REC_ID>_enemy.png рядом с CSV.
"""
import sys
import os
import pandas as pd
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

DIR = sys.argv[1] if len(sys.argv) > 1 else r"out\run142"
REC_ID = int(sys.argv[2]) if len(sys.argv) > 2 else 142
PLAY_IDS = [int(a) for a in sys.argv[3:]]

JUMP = 0x10
ATTACK = 0x40 | 0x80
# Атакующие анимации врага: 65545 — «прыжок на игрока», 19 — выпад по земле
ENEMY_ATTACK_ANIMS = {65545, 19}


def load(rid, kind):
    df = pd.read_csv(f"{DIR}\\run_{rid}_{kind}.csv")
    return df.reset_index(drop=True)


def attack_starts(df):
    """Кадры начала атаки врага. Возвращает список (fi, anim):
    - переход в 65545 («прыжок на игрока») — из любого состояния (в т.ч. из
      выпада 19: это и есть начало прыжка);
    - переход в 19 (выпад) — только из не-атакующего состояния (спавн/пауза).
    Переход с fi=0 не считается (спавн-состояние)."""
    anim = df["enemy_anim"].astype("int64")
    starts = []
    for i in range(1, len(df)):
        a = anim.iloc[i]
        prev = anim.iloc[i - 1]
        if a == 65545 and prev != 65545:
            starts.append((i, a))
        elif a == 19 and prev not in ENEMY_ATTACK_ANIMS:
            starts.append((i, a))
    return starts


def plot_one(ax, df, title):
    x = df["frame_index"]
    ax.plot(x, df["pos_y"], color="#1f77b4", lw=1.0, label="pos_y игрока")
    ax.plot(x, df["enemy_blade_y"], color="#d62728", lw=1.0, label="enemy_blade_y")

    pressed = df["buttons_pressed"].astype("int64")
    for fi in x[pressed & JUMP > 0]:
        ax.axvline(fi, color="#2ca02c", lw=0.8, alpha=0.7)
    for fi in x[pressed & ATTACK > 0]:
        ax.axvline(fi, color="#ff7f0e", lw=1.4, alpha=0.9)
    for fi, a in attack_starts(df):
        # 65545 — «прыжок на игрока» (фиолетовый), 19 — выпад (оранжевый пунктир)
        color = "#9467bd" if a == 65545 else "#ff7f0e"
        ax.axvline(fi, color=color, lw=1.4, alpha=0.9, ls=":")

    ax.set_title(title, fontsize=9)
    ax.set_ylim(-1, 26)
    ax.grid(alpha=0.3)
    ax.tick_params(labelsize=7)


def main():
    runs = [(REC_ID, "record", load(REC_ID, "record"))]
    runs += [(pid, "playback", load(pid, "playback")) for pid in PLAY_IDS]

    n = len(runs)
    cols = 3
    rows = (n + cols - 1) // cols
    fig, axes = plt.subplots(rows, cols, figsize=(17, 4.2 * rows), squeeze=False)
    for ax, (rid, kind, df) in zip(axes.flat, runs):
        plot_one(ax, df, f"run {rid} ({kind})")
    for ax in axes.flat[n:]:
        ax.set_visible(False)

    from matplotlib.lines import Line2D
    legend = [
        Line2D([0], [0], color="#1f77b4", lw=1.0, label="pos_y игрока"),
        Line2D([0], [0], color="#d62728", lw=1.0, label="enemy_blade_y"),
        Line2D([0], [0], color="#2ca02c", lw=0.8, label="JUMP pressed (игрок)"),
        Line2D([0], [0], color="#ff7f0e", lw=1.4, label="ATTACK pressed (игрок)"),
        Line2D([0], [0], color="#9467bd", lw=1.4, ls=":", label="начало атаки врага: прыжок 65545"),
        Line2D([0], [0], color="#ff7f0e", lw=1.4, ls=":", label="начало атаки врага: выпад 19"),
    ]
    fig.legend(handles=legend, loc="lower center", ncol=6, fontsize=9,
               bbox_to_anchor=(0.5, -0.02))
    fig.suptitle(f"Record {REC_ID} + playbacks: pos_y игрока vs enemy_blade_y, фазы атаки",
                 fontsize=13)
    fig.tight_layout(rect=(0, 0, 1, 0.96))
    out_png = os.path.join(DIR, f"run_{REC_ID}_enemy.png")
    fig.savefig(out_png, dpi=110, bbox_inches="tight")
    plt.close(fig)
    print(f"Сохранено: {out_png} ({n} ранов)")


if __name__ == "__main__":
    main()