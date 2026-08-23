# -*- coding: utf-8 -*-
"""График 6 панелей: |Δpos|/|Δcam_pos|, |Δyaw|, along, perp, углы,
прыжки (pos_y + фронты jump-бита 0x10 + зоны подъёма)."""
import sys
import pandas as pd
import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

DIR = sys.argv[1] if len(sys.argv) > 1 else r"C:\temp\dbdump_out_release"
OUT = sys.argv[2] if len(sys.argv) > 2 else r"D:\pet\drmod-rs\out\desync_cam.png"
REC_ID = int(sys.argv[3]) if len(sys.argv) > 3 else 73
PLAY_IDS = [int(a) for a in sys.argv[4:]] if len(sys.argv) > 4 else [78, 81, 82]
JUMP_BIT = 0x10

def load(rid, kind):
    return pd.read_csv(f"{DIR}\\run_{rid}_{kind}.csv")

def angle_diff(a, b):
    return (b - a + 180) % 360 - 180

def motion_decompose(rec, play):
    """Разложение вектора отклонения d = play.pos - rec.pos на компоненты
    вдоль направления движения record (разность позиций) и перпендикулярно.
    Возвращает also: угол между d и направлением движения, и курсовой угол
    (между направлениями движения record и playback)."""
    n = len(rec)
    rec_xyz = rec[["pos_x", "pos_y", "pos_z"]].to_numpy()
    play_xyz = play[["pos_x", "pos_y", "pos_z"]].to_numpy()
    dirv = np.zeros((n, 3))
    dirv[:-1] = rec_xyz[1:] - rec_xyz[:-1]
    dirv[-1] = dirv[-2]
    dlen = np.linalg.norm(dirv, axis=1)
    # направление неопределено при ~нулевом смещении кадра
    e = dirv / dlen[:, None]
    valid = dlen > 0.001
    e[~valid] = np.nan

    d = play_xyz - rec_xyz
    along = np.sum(d * e, axis=1)          # со знаком: + обгон, - отставание
    proj = along[:, None] * e
    perp = np.linalg.norm(d - proj, axis=1)

    # Угол между вектором отклонения d и вектором движения e (0..180°).
    # Не определён при |d| ~ 0 (отклонения нет).
    dnorm = np.linalg.norm(d, axis=1)
    cos_a = np.sum(d * e, axis=1) / dnorm
    angle_off = np.degrees(np.arccos(np.clip(cos_a, -1.0, 1.0)))
    angle_off[dnorm < 0.001] = np.nan

    # Курсовой угол: между направлениями движения record и playback.
    play_dir = np.zeros((n, 3))
    play_dir[:-1] = play_xyz[1:] - play_xyz[:-1]
    play_dir[-1] = play_dir[-2]
    p_len = np.linalg.norm(play_dir, axis=1)
    cos_c = np.sum(dirv * play_dir, axis=1) / (dlen * p_len)
    angle_course = np.degrees(np.arccos(np.clip(cos_c, -1.0, 1.0)))
    angle_course[(dlen < 0.001) | (p_len < 0.001)] = np.nan

    return along, perp, dlen, angle_off, angle_course

rec = load(REC_ID, "record")
series = {}
for pid in PLAY_IDS:
    play = load(pid, "playback")
    n = min(len(rec), len(play))
    r = rec.iloc[:n]
    p = play.iloc[:n]
    dpos = np.sqrt(
        (p["pos_x"] - r["pos_x"])**2
        + (p["pos_y"] - r["pos_y"])**2
        + (p["pos_z"] - r["pos_z"])**2
    )
    dcam = np.sqrt(
        (p["cam_pos_x"] - r["cam_pos_x"])**2
        + (p["cam_pos_y"] - r["cam_pos_y"])**2
        + (p["cam_pos_z"] - r["cam_pos_z"])**2
    )
    dyaw = angle_diff(r["cam_yaw"] * 180/np.pi, p["cam_yaw"] * 180/np.pi).abs()
    along, perp, dlen, angle_off, angle_course = motion_decompose(r, p)
    jump_down = (p["buttons_down"].astype("int64") & JUMP_BIT) != 0
    jump_pressed = (p["buttons_pressed"].astype("int64") & JUMP_BIT) != 0
    series[pid] = (dpos, dcam, dyaw, along, perp, angle_off, angle_course,
                   p["pos_y"].to_numpy(), jump_down, jump_pressed)
    # числовая сводка
    a_valid = np.abs(along[~np.isnan(along)])
    print(f"playback {pid}: |along| медиана {np.nanmedian(a_valid):.3f} м, "
          f"макс {np.nanmax(a_valid):.3f} м, смещение {np.nanmean(along):+.3f} м (среднее)")
    perp_bool = perp > 0.5
    first_perp = int(np.argmax(perp_bool)) if perp_bool.any() else -1
    print(f"           perp медиана {np.nanmedian(perp):.3f} м, макс {np.nanmax(perp):.3f} м, "
          f"1-й fi perp>0.5: {first_perp if first_perp >= 0 else '-'}")
    ao = angle_off[~np.isnan(angle_off)]
    ac = angle_course[~np.isnan(angle_course)]
    print(f"           угол(отклонение, движение): медиана {np.median(ao):.1f}°, "
          f"макс {np.max(ao):.1f}°, доля >45°: {(ao>45).mean()*100:.1f}%, >90°: {(ao>90).mean()*100:.1f}%")
    print(f"           курсовой угол: медиана {np.median(ac):.1f}°, "
          f"макс {np.max(ac):.1f}°, 1-й fi >5°: {int(np.argmax(ac>5)) if (ac>5).any() else '-'}")

# Прыжки: record (эталон)
n0 = min(len(rec), *(len(v[0]) for v in series.values()))
r0 = rec.iloc[:n0]
jump_down_rec = (r0["buttons_down"].astype("int64") & JUMP_BIT) != 0
jump_pressed_rec = (r0["buttons_pressed"].astype("int64") & JUMP_BIT) != 0
jf_rec = np.where(jump_pressed_rec)[0]
rise = np.diff(r0["pos_y"].to_numpy())
rise_frames = np.where(np.pad(rise > 0.05, (0, 1)))[0]  # подъём >5 см/кадр
print(f"\nПрыжки (фронт jump 0x10): record {REC_ID}: {len(jf_rec)} фронтов "
      f"{jf_rec.tolist()[:30]}")
print(f"  зоны подъёма высоты (>0.05 м/кадр) в record: {len(rise_frames)}, "
      f"кадры {np.where(rise > 0.05)[0].tolist()[:30]}")
for pid in PLAY_IDS:
    jf = np.where(series[pid][9])[0]
    close = sum(1 for f in jf if np.any(np.abs(jf_rec - f) <= 1))
    print(f"  playback {pid}: {len(jf)} фронтов {jf.tolist()[:30]}; "
          f"совпадают с record ±1 кадр: {close}/{len(jf_rec)}")

fig, axes = plt.subplots(6, 1, figsize=(13, 17.5), sharex=True,
                         gridspec_kw={"hspace": 0.12})
palette = ["#d62728", "#ff7f0e", "#2ca02c", "#1f77b4", "#9467bd"]
colors = {pid: palette[i % len(palette)] for i, pid in enumerate(PLAY_IDS)}
labels = {pid: f"playback {pid}" for pid in PLAY_IDS}

ax1, ax2, ax3, ax4, ax5, ax6 = axes
for pid, (dpos, dcam, dyaw, along, perp, angle_off, angle_course, *_rest) in series.items():
    ax1.plot(dpos, color=colors[pid], lw=1.1, ls="--", label=f"{labels[pid]} |Δpos игрока|")
    ax1.plot(dcam, color=colors[pid], lw=0.9, label=f"{labels[pid]} |Δcam_pos|")
    ax2.plot(dyaw, color=colors[pid], lw=0.9, label=labels[pid])
    ax3.plot(along, color=colors[pid], lw=0.9, label=labels[pid])
    ax4.plot(perp, color=colors[pid], lw=0.9, label=labels[pid])
    ax5.plot(angle_off, color=colors[pid], lw=1.0, label=f"{labels[pid]} угол(откл., движ.)")
    ax5.plot(angle_course, color=colors[pid], lw=0.7, ls=":", label=f"{labels[pid]} курсовой угол")

# Панель 6: прыжки — высота + фронты jump-бита
pos_y = r0["pos_y"].to_numpy()
ax6.plot(pos_y, color="black", lw=1.2, label=f"record {REC_ID} pos_y")
for pid in PLAY_IDS:
    ax6.plot(series[pid][7], color=colors[pid], lw=0.8, ls="--", alpha=0.75,
             label=f"{labels[pid]} pos_y")
# Зоны подъёма высоты (детекция прыжка по record)
for fr in np.where(rise > 0.05)[0]:
    ax6.axvspan(fr - 0.5, fr + 0.5, color="#7fbf7f", alpha=0.20)
# Фронты jump pressed: record — линии от pos_y вверх, playbacks — от pos_y вниз
ax6.vlines(jf_rec, pos_y[jf_rec], pos_y[jf_rec] + 0.35, color="black", lw=1.1,
           label=f"record jump pressed")
for pid in PLAY_IDS:
    jf = np.where(series[pid][9])[0]
    py = series[pid][7]
    ax6.vlines(jf, py[jf], py[jf] - 0.35, color=colors[pid], lw=0.8,
               label=f"{labels[pid]} jump pressed")

for ax in axes:
    ax.axvspan(233, 248, color="gray", alpha=0.18, label="вход в ninja run (233–248)")
    ax.axvline(27, color="black", lw=0.6, ls=":", alpha=0.7, label="старт лага ввода (27)")
ax3.axhline(0, color="black", lw=0.5, alpha=0.5)

ax1.set_ylabel("|Δ|, м")
ax1.set_title(f"Record {REC_ID} → playback {'/'.join(map(str, PLAY_IDS))}: позиция, камера, yaw и разложение отклонения "
              "по вектору движения")
ax1.legend(loc="upper left", fontsize=8, ncol=2)
ax1.grid(alpha=0.3)

ax2.set_ylabel("|Δyaw|, град")
ax2.legend(loc="upper left", fontsize=8, ncol=2)
ax2.grid(alpha=0.3)

ax3.set_ylabel("вдоль вектора, м\n(+ обгон / − отставание)")
ax3.legend(loc="upper left", fontsize=8, ncol=2)
ax3.grid(alpha=0.3)

ax4.set_ylabel("перпендикулярно, м")
ax4.set_xlabel("frame_index")
ax4.legend(loc="upper left", fontsize=8, ncol=2)
ax4.grid(alpha=0.3)

ax5.set_ylabel("угол(отклонение,\nдвижение), град")
ax5.set_xlabel("frame_index")
ax5.set_ylim(0, 180)
ax5.legend(loc="upper left", fontsize=8, ncol=2)
ax5.grid(alpha=0.3)

ax6.set_ylabel("pos_y, м\n(прыжки)")
ax6.set_xlabel("frame_index")
ax6.legend(loc="upper left", fontsize=7, ncol=3)
ax6.grid(alpha=0.3)

plt.savefig(OUT, dpi=120)
print(f"saved: {OUT}")
