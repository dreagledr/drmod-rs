# -*- coding: utf-8 -*-
"""Анализ камеры: record 73 vs playbacks 78/81/82.

Сравниваем cam_pos, cam_look_at, cam_yaw, cam_pitch, cam_roll.
yaw/pitch — производные от pos->look_at (см. dbdump dump.rs).
"""
import sys
import pandas as pd
import numpy as np

DIR = sys.argv[1] if len(sys.argv) > 1 else r"C:\temp\dbdump_out_release"

def load(rid, kind):
    return pd.read_csv(f"{DIR}\\run_{rid}_{kind}.csv")

def dvec(a, b, px, py, pz):
    return np.sqrt((b[px]-a[px])**2 + (b[py]-a[py])**2 + (b[pz]-a[pz])**2)

def angle_diff(a, b):
    """Угловая разница в градусах с учётом переноса ±180/360."""
    d = (b - a + 180) % 360 - 180
    return d

def cam_metrics(rec, play):
    dpos = dvec(rec, play, "pos_x", "pos_y", "pos_z")
    dcam = dvec(rec, play, "cam_pos_x", "cam_pos_y", "cam_pos_z")
    dlook = dvec(rec, play, "cam_look_at_x", "cam_look_at_y", "cam_look_at_z")
    dyaw = angle_diff(rec["cam_yaw"] * 180/np.pi, play["cam_yaw"] * 180/np.pi).abs()
    dpitch = angle_diff(rec["cam_pitch"] * 180/np.pi, play["cam_pitch"] * 180/np.pi).abs()
    droll = angle_diff(rec["cam_roll"] * 180/np.pi, play["cam_roll"] * 180/np.pi).abs()
    return dpos, dcam, dlook, dyaw, dpitch, droll

REC_ID = int(sys.argv[2]) if len(sys.argv) > 2 else 73
PLAY_IDS = [int(a) for a in sys.argv[3:]] if len(sys.argv) > 3 else [78, 81, 82]

for pid in PLAY_IDS:
    rec = load(REC_ID, "record")
    play = load(pid, "playback")
    n = min(len(rec), len(play))
    rec = rec.iloc[:n]
    play = play.iloc[:n]
    dpos, dcam, dlook, dyaw, dpitch, droll = cam_metrics(rec, play)
    print("=" * 78)
    print(f"Record {REC_ID} vs Playback {pid}")
    print(f"  |Δpos игрока|   : макс {dpos.max():.3f} м, 1-й fi >0.1: {(dpos>0.1).idxmax() if (dpos>0.1).any() else '-'}")
    print(f"  |Δcam_pos|      : макс {dcam.max():.3f} м, 1-й fi >0.1: {(dcam>0.1).idxmax() if (dcam>0.1).any() else '-'}, >0.5: {(dcam>0.5).idxmax() if (dcam>0.5).any() else '-'}")
    print(f"  |Δlook_at|      : макс {dlook.max():.3f} м, 1-й fi >0.5: {(dlook>0.5).idxmax() if (dlook>0.5).any() else '-'}")
    print(f"  |Δyaw|   (град): макс {dyaw.max():.1f}, медиана {dyaw.median():.2f}, 1-й fi >1°: {(dyaw>1).idxmax() if (dyaw>1).any() else '-'}, >5°: {(dyaw>5).idxmax() if (dyaw>5).any() else '-'}")
    print(f"  |Δpitch| (град): макс {dpitch.max():.1f}, медиана {dpitch.median():.2f}, 1-й fi >1°: {(dpitch>1).idxmax() if (dpitch>1).any() else '-'}")
    print(f"  |Δroll|  (град): макс {droll.max():.1f}, медиана {droll.median():.2f}")
    # доля кадров с |Δyaw|>1 и >5
    print(f"  доля кадров |Δyaw|>1°: {(dyaw>1).mean()*100:.1f}%, >5°: {(dyaw>5).mean()*100:.1f}%")

# Playback vs playback по камере
print("\n" + "=" * 78)
print("Камера между playback'ами (ввод идентичен):")
p = {rid: load(rid, "playback") for rid in PLAY_IDS}
for i in range(len(PLAY_IDS)):
    for j in range(i + 1, len(PLAY_IDS)):
        x, y = PLAY_IDS[i], PLAY_IDS[j]
        _, dcam, _, dyaw, dpitch, _ = cam_metrics(p[x], p[y])
        print(f"  {x} vs {y}: |Δcam_pos| макс {dcam.max():.3f} м, |Δyaw| макс {dyaw.max():.1f}°, медиана {dyaw.median():.2f}°, 1-й fi >5°: {(dyaw>5).idxmax() if (dyaw>5).any() else '-'}")

# Окно 220-260: детально yaw/pitch/roll и dpos
x0, y0 = PLAY_IDS[0], PLAY_IDS[1]
print("\n" + "=" * 78)
print(f"Окно fi=220..260: cam_yaw/cam_pitch (градусы), |Δ|, dpos (record vs {x0} и vs {y0}):")
rec = load(REC_ID, "record")
p0 = p[x0]
p1 = p[y0]
n = min(len(rec), len(p0))
rec = rec.iloc[:n]
p0 = p0.iloc[:n]
p1 = p1.iloc[:n]
dpos0 = dvec(rec, p0, "pos_x", "pos_y", "pos_z")
dpos1 = dvec(rec, p1, "pos_x", "pos_y", "pos_z")
print(f"  fi | yaw_r yaw_{x0} yaw_{y0} | pitch_r pit_{x0} pit_{y0} | |Δyaw{x0}| |Δyaw{y0}| | dpos{x0} dpos{y0}")
for fi in range(220, 261):
    yr = rec["cam_yaw"].iloc[fi] * 180/np.pi
    y0v = p0["cam_yaw"].iloc[fi] * 180/np.pi
    y1v = p1["cam_yaw"].iloc[fi] * 180/np.pi
    pr = rec["cam_pitch"].iloc[fi] * 180/np.pi
    pt0 = p0["cam_pitch"].iloc[fi] * 180/np.pi
    pt1 = p1["cam_pitch"].iloc[fi] * 180/np.pi
    dy0 = abs(angle_diff(yr, y0v))
    dy1 = abs(angle_diff(yr, y1v))
    print(f"  {fi} | {yr:7.1f} {y0v:7.1f} {y1v:7.1f} | {pr:7.1f} {pt0:7.1f} {pt1:7.1f} | {dy0:6.1f} {dy1:6.1f} | {dpos0.iloc[fi]:.3f} {dpos1.iloc[fi]:.3f}")
