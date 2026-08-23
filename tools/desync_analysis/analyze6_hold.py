# -*- coding: utf-8 -*-
"""Анализ дублирования кадров ввода для компенсации отставания (along < 0).

Идея: в hold-окнах (ввод стабилен: down не меняется, pressed/released пусты,
стики равны) можно подать текущий кадр 2 тика подряд — персонаж пройдёт ещё
одно смещение кадра по вектору движения и догонит запись.

Считаем для каждого playback:
1. along (отставание вдоль вектора движения record).
2. дефицит: суммарное отставание вдоль.
3. hold-окна в RECORD (где дубль безопасен) + их кадры.
4. модель: сколько дублей нужно для компенсации дефицита и сколько доступно.
"""
import sys
import pandas as pd
import numpy as np

DIR = sys.argv[1] if len(sys.argv) > 1 else r"C:\temp\dbdump_out_91"
REC_ID = int(sys.argv[2]) if len(sys.argv) > 2 else 91
PLAY_IDS = [int(a) for a in sys.argv[3:]] if len(sys.argv) > 3 else [92, 93, 94, 95]

MOVING_ANIMS = {5, 71, 13, 14, 4, 3, 11}  # фазы с поступательным движением

def load(rid, kind):
    return pd.read_csv(f"{DIR}\\run_{rid}_{kind}.csv")

def hold_windows(rec):
    """Hold-окна в record: кадры, где дубль безопасен.
    Условия: down одинаковый со следующим кадром, НЕТ однокадровых фронтов
    pressed/released (ни в этом, ни в следующем кадре), left_stick (движение)
    стабилен, анимация — поступательное движение. Правый стик (камера) может
    меняться: дубль продлит поворот на 1 тик — для бега приемлемо."""
    n = len(rec)
    down = rec["buttons_down"].to_numpy()
    pressed = rec["buttons_pressed"].to_numpy()
    released = rec["buttons_released"].to_numpy()
    lsx = rec["left_stick_x"].to_numpy(); lsy = rec["left_stick_y"].to_numpy()
    anim = rec["r_anim"].to_numpy()
    stable = np.zeros(n, dtype=bool)
    for i in range(n - 1):
        if (down[i] == down[i + 1]
                and pressed[i] == 0 and released[i] == 0
                and pressed[i + 1] == 0 and released[i + 1] == 0
                and lsx[i] == lsx[i + 1] and lsy[i] == lsy[i + 1]
                and anim[i] in MOVING_ANIMS
                and np.hypot(lsx[i], lsy[i]) > 0):  # стик отклонён (движемся)
            stable[i] = True
    return stable

def along_series(rec, play):
    n = min(len(rec), len(play))
    dirv = np.zeros((n, 3))
    xyz = rec[["pos_x", "pos_y", "pos_z"]].to_numpy()[:n]
    dirv[:-1] = xyz[1:] - xyz[:-1]
    dirv[-1] = dirv[-2]
    dlen = np.linalg.norm(dirv, axis=1)
    e = dirv / dlen[:, None]
    d = play[["pos_x", "pos_y", "pos_z"]].to_numpy()[:n] - xyz
    return np.sum(d * e, axis=1)  # + обгон, - отставание

rec = load(REC_ID, "record")
stable = hold_windows(rec)
# смещение кадра (горизонталь) в каждом кадре record — «цена» одного дубля
h = np.sqrt(rec["pos_x"].diff() ** 2 + rec["pos_z"].diff() ** 2).to_numpy().copy()
h[0] = h[1] if len(h) > 1 else 0.0

n_hold = int(stable.sum())
print(f"Record {REC_ID}: {len(rec)} кадров; hold-окна (безопасный дубль): "
      f"{n_hold} кадров ({100 * n_hold / len(rec):.1f}%)")
# непрерывные hold-сегменты
segs = []
start = None
for i in range(len(stable)):
    if stable[i] and start is None:
        start = i
    elif not stable[i] and start is not None:
        segs.append((start, i - 1))
        start = None
if start is not None:
    segs.append((start, len(stable) - 1))
print(f"  сегментов удержания: {len(segs)}; суммарная длина: {sum(e - s + 1 for s, e in segs)}")
print(f"  среднее смещение за кадр в hold-кадрах: {np.nanmedian(h[stable]):.4f} м")

for pid in PLAY_IDS:
    play = load(pid, "playback")
    n = min(len(rec), len(play))
    along = along_series(rec, play)
    lag = -along  # отставание (положительное)
    valid = np.isfinite(lag)
    lag_clean = np.where(valid, lag, 0.0)  # NaN -> 0 (стояние, направление неопределено)
    deficit = lag_clean[lag_clean > 0].sum()
    print("\n" + "=" * 78)
    d = np.sqrt((play['pos_x'] - rec['pos_x'])**2 + (play['pos_y'] - rec['pos_y'])**2 + (play['pos_z'] - rec['pos_z'])**2)
    print(f"Playback {pid}: макс |Δpos|={d.max():.2f} м")
    # отставание
    lag_pos = lag_clean[lag_clean > 0.05]
    print(f"  отставание вдоль: кадров >5 см: {len(lag_pos)}/{n}, "
          f"среднее по отстающим кадрам: {lag_pos.mean():.3f} м, "
          f"макс: {lag_clean.max():.3f} м, интегральный дефицит: {deficit:.1f} м·кадр")
    # где отставание: диапазоны
    idx = np.where(lag_clean > 0.05)[0]
    if len(idx):
        ranges = []
        s = idx[0]
        for i in range(1, len(idx)):
            if idx[i] - idx[i - 1] > 3:
                ranges.append((s, idx[i - 1]))
                s = idx[i]
        ranges.append((s, idx[-1]))
        print(f"  диапазоны отставания >5 см: "
              f"{[(int(a), int(b), f'{lag_clean[a:b+1].mean():.2f}м') for a, b in ranges][:12]}")
    # модель компенсации
    candidate = np.where(stable[:n] & (lag_clean > 0.02))[0]
    # жадный выбор с мин. интервалом 3 (не раздувать расписание)
    chosen = []
    last = -10
    for i in candidate:
        if i - last >= 3:
            chosen.append(i)
            last = i
    gain = h[chosen].sum() if chosen else 0.0
    # среднее отставание после компенсации (приближённо: дубль сдвигает позицию
    # вдоль на h[i], все последующие кадры получают +h[i] к along)
    remaining = lag_clean.copy()
    for i in chosen:
        # дубль в кадре i даёт +h[i] ко всем последующим кадрам (позиция уезжает вперёд)
        remaining[i:] -= h[i]
    rem_mean = remaining[remaining > 0.05]
    print(f"  дубли доступно (hold + отставание>2см, интервал 3): {len(chosen)} кадров, "
          f"выигрыш по позиции: {gain:.2f} м")
    print(f"  после компенсации: кадров с отставанием >5 см: {len(rem_mean)}/{n}, "
          f"среднее: {rem_mean.mean():.3f} м" if len(rem_mean) else "  после компенсации: отставание >5 см не осталось")
