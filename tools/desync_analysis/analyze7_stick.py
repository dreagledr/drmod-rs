# -*- coding: utf-8 -*-
"""Чувствительность right_stick -> поворот камеры (cam_yaw/cam_pitch).

Считаем по record: Δyaw/Δpitch за кадр vs right_stick_x/y.
1. Град/кадр при заданном rsx (бина), базовый авто-поворот при rsx=0.
2. Чувствительность: Δyaw / rsx (град на единицу стика), по фазам.
3. Задержка: корреляция Δyaw[i] с rsx[i-k], k=0..3.
"""
import sys
import pandas as pd
import numpy as np

DIR = sys.argv[1] if len(sys.argv) > 1 else r"C:\temp\dbdump_out_91"
REC_ID = int(sys.argv[2]) if len(sys.argv) > 2 else 91

def load(rid):
    return pd.read_csv(f"{DIR}\\run_{rid}_record.csv")

def angle_diff(a, b):
    return (b - a + 180) % 360 - 180

df = load(REC_ID)
n = len(df)
rsx = df["right_stick_x"].to_numpy()
rsy = df["right_stick_y"].to_numpy()
yaw = df["cam_yaw"].to_numpy() * 180 / np.pi
pitch = df["cam_pitch"].to_numpy() * 180 / np.pi
anim = df["r_anim"].to_numpy()

dyaw = np.zeros(n)
dpitch = np.zeros(n)
dyaw[:-1] = angle_diff(yaw[:-1], yaw[1:])
dpitch[:-1] = angle_diff(pitch[:-1], pitch[1:])

print(f"Record {REC_ID}: {n} кадров")

# --- 1. Авто-поворот при rsx=0 (базовая линия) ---
z = rsx == 0
print(f"\n[1] База: Δyaw при rsx=0: медиана {np.median(dyaw[z]):.3f}°, "
      f"|Δyaw| медиана {np.median(np.abs(dyaw[z])):.3f}° (авто-поворот камеры)")
print(f"    Δpitch при rsy=0: |Δpitch| медиана {np.median(np.abs(dpitch[rsy == 0])):.3f}°")

# --- 2. Чувствительность по бинам rsx ---
print("\n[2] Δyaw по бинам right_stick_x (rsx != 0):")
for lo, hi in [(1, 100), (100, 300), (300, 600), (600, 1000), (1000, 2200)]:
    m = (np.abs(rsx) > lo) & (np.abs(rsx) <= hi)
    if m.sum() < 5:
        continue
    sens = dyaw[m] / rsx[m]  # град на единицу (со знаком)
    print(f"    |rsx| ({lo:4d},{hi:4d}]: n={m.sum():4d} | Δyaw медиана {np.median(dyaw[m]):6.3f}° | "
          f"чувствительность медиана {np.median(sens):.5f} °/ед, "
          f"средняя {sens.mean():.5f} °/ед")

print("\n[3] Δpitch по бинам right_stick_y (rsy != 0):")
for lo, hi in [(1, 100), (100, 300), (300, 600), (600, 1000), (1000, 2200)]:
    m = (np.abs(rsy) > lo) & (np.abs(rsy) <= hi)
    if m.sum() < 5:
        continue
    sens = dpitch[m] / rsy[m]
    print(f"    |rsy| ({lo:4d},{hi:4d}]: n={m.sum():4d} | Δpitch медиана {np.median(dpitch[m]):6.3f}° | "
          f"чувствительность медиана {np.median(sens):.5f} °/ед, "
          f"средняя {sens.mean():.5f} °/ед")

# --- 3. Задержка: корреляция Δyaw[i] с rsx[i-k] ---
print("\n[4] Задержка стика -> камера (линейная корреляция Δyaw[i] ~ rsx[i-k]):")
for k in range(0, 4):
    a = dyaw[k:] if k == 0 else dyaw[:-k]
    b = rsx if k == 0 else rsx[k:]
    c = np.corrcoef(a, b)[0, 1]
    print(f"    k={k}: r = {c:+.3f}")

# --- 4. По фазам: бег (5) и ninja run (71) ---
print("\n[5] Чувствительность по фазам (rsx != 0, |rsx| > 100):")
for a, name in [(5, "бег (5)"), (71, "ninja run (71)"), (13, "переход (13)"), (14, "переход (14)")]:
    m = (np.abs(rsx) > 100) & (anim == a)
    if m.sum() < 5:
        print(f"    {name}: n<5")
        continue
    sens = dyaw[m] / rsx[m]
    print(f"    {name}: n={m.sum():4d}, чувствительность медиана {np.median(sens):.5f} °/ед, "
          f"Δyaw медиана {np.median(dyaw[m]):+.3f}°")

# --- 5. Оценка: сколько градусов даёт стик 1000 за 1 кадр ---
print("\n[6] Итог: right_stick_x=1000 на 1 кадр => Δyaw ≈ "
      f"{np.median(dyaw[np.abs(rsx) > 500] / np.where(rsx == 0, 1, rsx)[np.abs(rsx) > 500]) * 1000:.2f}°")
print(f"    right_stick_y=1000 на 1 кадр => Δpitch ≈ "
      f"{np.median(dpitch[np.abs(rsy) > 500] / np.where(rsy == 0, 1, rsy)[np.abs(rsy) > 500]) * 1000:.2f}°")
