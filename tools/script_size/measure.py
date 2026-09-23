"""Замер размера скриптов: JSON, текст .tas, сжатие и ответ /logs.

Моделирует три представления скрипта (docs/SCRIPT_DSL.md) и один ответ API,
чтобы ответить на вопрос «сколько влезает в лимиты мода»:

* ``POST /script/run``   — JSON, лимит тела ``src/api.rs::MAX_BODY_BYTES`` 64 КиБ,
  тайминги ограничены ``MAX_SCRIPT_FRAMES`` (`replay-types`) — страховкой, а
  практический ограничитель — ``MAX_BODY_BYTES`` 64 КиБ;
* ``.tas``              — тот же скрипт текстом (канонический ``Write``);
* ``GET /logs``         — кольцевой буфер ``src/api.rs::RING_CAPACITY`` 3600 кадров.

Проверки (assert) сверяют схему с реальным кодом мода: имена полей, индексы
команд, квантование ``ScriptInput`` и формат текста. Мод не импортируется —
модели повторяют то, что описано в docs/API.md §4 и SCRIPT_DSL.md §3–§5.

Запуск: ``python tools/script_size/measure.py`` (см. README.md).
"""

from __future__ import annotations

import argparse
import brotli
import gzip
import json
import math
import time
import zstandard
from collections import OrderedDict
from pathlib import Path

HERE = Path(__file__).resolve().parent
DEFAULT_OUT = HERE / "examples"
DEFAULT_FRAMES = (3600, 3600 * 60)

# --- лимиты мода (источники — в README.md, раздел «Откуда цифры») ----------
MAX_BODY_BYTES = 64 * 1024
# Верхняя страховка длительности скрипта (`replay-types::script`); практический
# ограничитель — размер тела, поэтому замер идёт на такте в 3600 кадров.
MAX_SCRIPT_FRAMES = 1_000_000
# Размер такта: столько кадров в кольцевом буфере логов и столько же был
# прежний потолок скрипта — удобная единица для таблиц.
TAKT_FRAMES = 3600
RING_CAPACITY = 3600
# Максимум кадров в ответе /logs (`src/api.rs::MAX_LOG_LIMIT`).
MAX_LOG_LIMIT = 5000
MAX_NAME_CHARS = 64
# Размер `LogFrame` (`src/api.rs`) — пришпилен тестом
# `api::ring_tests::log_frame_stays_216_bytes`; если он падает, переизмерять
# `std::mem::size_of::<LogFrame>()` и править цифру здесь и в README.
LOG_FRAME_BYTES = 216
# 10 имён декодирует src/api.rs::decode_buttons — схема лога совпадает с модом.
LOG_BUTTON_NAMES = 10

# --- порядок полей и порядок команд ----------------------------------------
# Порядок сериализации ScriptInput = порядок объявления в replay-types/src/script.rs
# (serde печатает поля в порядке объявления), после него — left_stick.
INPUT_FIELD_ORDER = [
    "forward", "backward", "left", "right", "jump", "light_attack", "heavy_attack",
    "camera", "ripper", "blade", "ninja_run", "walk", "dodge", "lock_on",
    "subweapon", "item", "ar_mode", "weapon_select", "codec", "zandatsu",
    "camera_reset", "pause", "confirm", "menu_up", "menu_down", "menu_left",
    "menu_right", "raw_key", "dik_key", "left_stick",
]
# Колонки таблицы команд (docs/SCRIPT_DSL.md §3.1) — в этом порядке `Write`
# печатает токены (после стиков).
TOKEN_COLUMNS = [
    "a", "b", "x", "y", "lt", "rt", "lb", "rb", "r", "lr", "ax", "du", "dd",
    "dl", "mu", "md", "ml", "mr", "ok", "esc", "cd",
]
TOKEN_TO_KEY = {
    "a": "jump", "b": "zandatsu", "x": "light_attack", "y": "heavy_attack",
    "lt": "blade", "rt": "ninja_run", "lb": "subweapon", "rb": "lock_on",
    "r": "camera_reset", "lr": "ripper", "ax": "dodge", "du": "ar_mode",
    "dd": "item", "dl": "weapon_select", "mu": "menu_up", "md": "menu_down",
    "ml": "menu_left", "mr": "menu_right", "ok": "confirm", "esc": "pause",
    "cd": "codec", "wk": "walk",
}
# Входы, которые mod подаёт фронтом на первом кадре команды; duration игнорируется
# (docs/API.md §4.2) — в тексте они всегда печатаются без «:кадры».
DEFAULT_DURATION_ONE = {"ripper", "lock_on", "subweapon", "item", "codec",
                        "camera_reset", "zandatsu"}
# Один кадр рестарта в терминах текста: dik_key стрелки вверх (@=0xC8), confirm.
RESTART_UP_INPUT = {"dik_key": 200, "confirm": True}

# Зоны корректности: значения пада (int16, docs/API.md §4.2) и очевидные
# инварианты входа.
ANALOG_ABS_MAX = 32767.0
INPUT_KEYS = set(INPUT_FIELD_ORDER)


class Script:
    """Скрипт = тело POST /script/run (без валидации — её делает мод)."""

    def __init__(self, name: str, commands: list[dict]):
        self.name = name
        self.commands = commands

    def to_json_dict(self):
        out = OrderedDict()
        out["name"] = self.name
        out["commands"] = [
            OrderedDict((k, v) for k, v in [
                ("t", c["t"]),
                ("duration", c["duration"]),
                ("input", self._ordered_input(c["input"])),
            ])
        for c in self.commands]
        return out

    def to_json(self) -> str:
        return json.dumps(self.to_json_dict(), separators=(",", ":"))

    @staticmethod
    def _ordered_input(inp: dict) -> OrderedDict:
        rest = {k: v for k, v in inp.items() if k not in INPUT_FIELD_ORDER}
        assert not rest, f"неизвестные входы: {rest}"
        return OrderedDict(
            (k, inp[k]) for k in INPUT_FIELD_ORDER if k in inp
        )

    # --- текст .tas --------------------------------------------------------
    def to_tas(self) -> str:
        # Один булев вход = самая длинная длительность (docs/SCRIPT_DSL.md §5).
        # Здесь у каждой команды свой кадр, поэтому длительности не сливаются,
        # но правило выражено явно: одна команда — одна строка, неизвестный
        # токен не сворачивается.
        lines: list[str] = []
        by_frame = self._commands_by_frame()
        for frame in sorted(by_frame):
            tokens: list[str] = []
            for kind, axis, value, duration in self._stick_tokens(by_frame[frame]):
                tokens.append(fmt_stick(kind, axis, value, duration))
            for token, duration in self._button_tokens(by_frame[frame]):
                tokens.append(token if duration == 1 else f"{token}:{duration}")
            lines.append(str(frame) if not tokens else f"{frame} " + " ".join(tokens))
        return "\n".join(lines) + "\n"

    def _commands_by_frame(self) -> dict[int, list[dict]]:
        out: dict[int, list[dict]] = {}
        for cmd in self.commands:
            bucket = out.setdefault(cmd["t"], [])
            bucket.append(cmd)
            assert len(bucket) <= 2, f"кадр {cmd['t']}: больше двух команд"
        return out

    def _stick_tokens(self, cmds: list[dict]):
        """До двух токенов стиков: (ls|rs, xy, value, duration) — иначе ошибка."""
        best: dict[str, tuple] = {}
        for cmd in cmds:
            for src, dst in (("left_stick", "ls"), ("camera", "rs")):
                if src not in cmd["input"]:
                    continue
                x, y = cmd["input"][src]
                duration = cmd["duration"]
                if dst in best:
                    prev = best[dst]
                    assert prev[3] == duration and prev[2] == (x, y), (
                        f"кадр {cmd['t']}: один стик на сторону двумя формами")
                best[dst] = (dst, (x, y), (x, y), duration)
        return [
            self._pick_stick_form(key, best[key]) for key in ("ls", "rs") if key in best
        ]

    @staticmethod
    def _pick_stick_form(kind: str, entry: tuple):
        """Полное нажатие — углом, всё остальное — осями (docs/SCRIPT_DSL.md §5)."""
        _, (x, y), _, duration = entry
        angle = full_stick_angle(x, y)
        if angle is not None:
            return (kind, "angle", angle, duration)
        if x == 0 and y == 0:
            return (kind + "x", "value", 0, duration)
        return (kind + "x", "value", x, duration)

    def _button_tokens(self, cmds: list[dict]):
        longest: dict[str, int] = {}
        for cmd in cmds:
            duration = cmd["duration"]
            for key, value in cmd["input"].items():
                if key in ("left_stick", "camera") or key == "dik_key":
                    continue
                if key == "walk":
                    assert value, "walk только как флаг"
                    continue
                token = KEY_TO_TOKEN.get(key)
                if token is None:
                    raise AssertionError(f"{key}: вход без текстового токена")
                if key in DEFAULT_DURATION_ONE:
                    duration = 1
                longest[token] = max(longest.get(token, 0), duration)
        tail: list[tuple[str, int]] = []
        if "walk" in cmds[0]["input"]:
            tail.append(("wk", 1))
        tail += [(t, longest[t]) for t in TOKEN_COLUMNS if t in longest]
        return tail


KEY_TO_TOKEN = {v: k for k, v in TOKEN_TO_KEY.items()}


def round3(value: float) -> float:
    return round(value, 3)


def fmt_num(value) -> str:
    """Кратчайшая запись числа (docs/SCRIPT_DSL.md §5): int без «.0»."""
    if isinstance(value, float) and value.is_integer():
        return str(int(value))
    return str(value)


def fmt_stick(kind: str, axis: str, value, duration: int) -> str:
    head = f"{kind}:{fmt_num(value)}" if axis == "angle" else f"{kind}:{fmt_num(value)}"
    return head if duration == 1 else f"{head}:{duration}"


def full_stick_angle(x: float, y: float):
    """Угол «компаса», если стик — полное нажатие, иначе None.

    Компас: 0 — вперёд, по часовой; x = 1000·sin, y = −1000·cos
    (docs/SCRIPT_DSL.md §4). Ищем целый угол или угол с шагом 0.001°
    (столько держит точность формат).
    """
    if x == 0 and y == 0:
        return None
    mag = math.hypot(x, y)
    if abs(mag - 1000.0) > 0.5:
        return None
    angle = round3(math.degrees(math.atan2(x, -y)) % 360.0)
    ax = 1000.0 * math.sin(math.radians(angle))
    ay = -1000.0 * math.cos(math.radians(angle))
    if abs(ax - x) > 0.5 or abs(ay - y) > 0.5:
        return None
    return angle


def stick(angle: float, duration: int) -> dict:
    """Поля входа для полного нажатия стика под углом (без `duration`)."""
    rad = math.radians(angle)
    return {
        "left_stick": [round(1000.0 * math.sin(rad), 3),
                       round(-1000.0 * math.cos(rad), 3)],
    }


# --- генераторы скриптов ----------------------------------------------------

# Такт (`TAKT_FRAMES`, объявлен выше) — единица замера: столько кадров был
# прежний потолок скрипта и столько же вмещает кольцевой буфер логов. Размер
# скрипта линеен по кадрам, поэтому «час» моделируется как профиль на такт × 60.


def takt_count(total_frames: int) -> int:
    """Сколько тактов по `TAKT_FRAMES` кадров в `total_frames` (для отчёта)."""
    return max(1, total_frames // TAKT_FRAMES)


def time_label(total_frames: int) -> str:
    seconds = round(total_frames / 60.0)
    if seconds < 60:
        return f"{seconds} с"
    return f"{seconds // 60} мин"


def gen_benign(total_frames: int) -> Script:
    """Бег по кругу (60 кадров на угол) + редкие нажатия + рестарт-хвост.

    Ровно `TAKT_FRAMES` кадров: такт (0–3560) + рестарт-хвост (3561–3600).
    """
    cmds: list[dict] = []
    angles = [round(a * 2.0, 3) for a in range(0, 180)]
    tail_frames = 40
    k = 0
    while k < TAKT_FRAMES - tail_frames:
        duration = min(60, TAKT_FRAMES - tail_frames - k)
        angle = angles[(k // 60) % len(angles)]
        cmds.append({"t": k, "duration": duration, "input": stick(angle, duration)})
        cmds.append({"t": k + 30, "duration": 2, "input": {"jump": True}})
        if k % 240 == 0:
            cmds.append({"t": k + 45, "duration": 2,
                         "input": {"light_attack": True}})
        k += duration
    # Рестарт-хвост: 7 «меню»-кадров (стрелка вверх + confirm).
    tail_t = TAKT_FRAMES - tail_frames
    for i in range(7):
        t0 = tail_t + i
        cmds.append({"t": t0, "duration": 6, "input": dict(RESTART_UP_INPUT)})
        t1 = tail_t + i + 8
        cmds.append({"t": t1, "duration": 2, "input": {"confirm": True}})
    return Script("r03-barrier-circle", cmds)


def gen_rot_split(total_frames: int) -> Script:
    """Каждая команда — свой кадр: стик гуляет каждый кадр (худший случай)."""
    cmds: list[dict] = []
    for k in range(TAKT_FRAMES):
        angle = round3((k * 360.0 / 120.0) % 360.0)  # круг за 120 кадров
        inp = stick(angle, 1)
        if k % 120 == 119:
            inp["jump"] = True
        if k % 480 == 0:
            inp["light_attack"] = True
        cmds.append({"t": k, "duration": 1, "input": inp})
    return Script("worst-case-stick", cmds)


def gen_sticky(total_frames: int) -> Script:
    """Одна команда на длинный отрезок: стик стоит, направление меняется редко.

    Профиль «мало команд на длинный прогон»: три сегмента стика с редким
    нажатием.
    """
    cmds: list[dict] = []
    segment = TAKT_FRAMES // 3
    k = 0
    while k < TAKT_FRAMES:
        duration = min(segment, TAKT_FRAMES - k)
        angle = k // segment * 120.0
        inp = stick(angle, duration)
        if k == 0:
            inp["light_attack"] = True
        cmds.append({"t": k, "duration": duration, "input": inp})
        k += duration
    return Script("one-hour-stick", cmds)


def gen_restart_heavy(restarts: int = 4) -> Script:
    """Фаза рестарта + маленький полёт: видно цену restart-команд в теле."""
    cmds: list[dict] = []
    t = 0
    for _ in range(restarts):
        for i in range(7):
            cmds.append({"t": t + i, "duration": 6, "input": dict(RESTART_UP_INPUT)})
            cmds.append({"t": t + i + 8, "duration": 2, "input": {"confirm": True}})
        t += 15 + 60
        cmds.append({"t": t, "duration": 60,
                     "input": {"left_stick": [0.0, -1000.0]}})
    return Script("restart-heavy", cmds)


# --- модели ответа /logs (схема LogFrameJson / InputJson, src/api.rs) -------

def fmt_f32(value: float) -> str:
    """Кратчайшее представление f32 (serde_json печатает так же)."""
    import struct
    v = struct.unpack("f", struct.pack("f", value))[0]
    for text in (repr(v), f"{v:.1f}", f"{v:.2f}", f"{v:.3f}"):
        if struct.unpack("f", struct.pack("f", float(text)))[0] == v:
            return text
    return repr(v)


def rand_seq(i: int, lo: float, hi: float) -> float:
    """Псевдослучайное в диапазоне — детерминированный LCG, без random."""
    x = (i * 1664525 + 1013904223) & 0xFFFFFFFF
    x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
    return lo + (hi - lo) * (x / 0xFFFFFFFF)


def log_frame_json(i: int) -> str:
    buttons = LOG_BUTTON_NAMES if i % 7 == 0 else 2
    names = ["forward", "jump", "light_attack", "heavy_attack", "ninja_run",
             "blade", "pause", "backward", "left", "right"][:buttons]
    base_ms = i * 16
    return (
        '{{"t_ms":{t_ms},"frame":{frame},"script_id":{sid},"menu_status":"In Game",'
        '"script_phase":"running","enemy":{{"pos":[{ex},{ey},{ez}],"blade_y":{bz},'
        '"r_anim":0,"frame":{ef},"hp":120,"found":1}},"fed_down_bits":4194368,'
        '"fed_pressed_bits":16,"fed_left_stick":[0.0,-500.0],"pos":[{px},{py},{pz}],'
        '"rot":[0.0,{ry},0.0],"vel":[0.0,0.0,0.0],"hp":100,"r_anim":0,"ripper":0,'
        '"blade":0,"camera_pos":[{px},{py},{pz}],"camera_look_at":[{px},{py},{lz}],'
        '"camera_rot":[1.57,0.0,0.0],"input":{{"buttons":{buttons},"down_bits":4194368,'
        '"pressed_bits":16,"left_stick":[0.0,-500.0],"right_stick":[0.0,0.0]}}}}'
    ).format(
        t_ms=base_ms, frame=i, sid=1,
        ex=fmt_f32(rand_seq(i, -30, 30)), ey=fmt_f32(rand_seq(i + 1, 0, 15)),
        ez=fmt_f32(rand_seq(i + 2, 100, 140)), bz=fmt_f32(rand_seq(i + 3, -1, 3)),
        ef=i % 40,
        px=fmt_f32(rand_seq(i + 4, -30, 30)), py=fmt_f32(rand_seq(i + 5, 0, 15)),
        pz=fmt_f32(rand_seq(i + 6, 100, 140)), ry=fmt_f32(rand_seq(i + 7, -3.2, 3.2)),
        lz=fmt_f32(rand_seq(i + 8, 99, 139)),
        buttons=json.dumps(names, separators=(",", ":")),
    )


def gen_logs_json(frames: int) -> str:
    head = ('{"from_ms":0,"to_ms":%d,"count":%d,"frames":[' % (frames * 16, frames))
    body = ",".join(log_frame_json(i) for i in range(frames))
    return head + body + "]}"


# --- замер ------------------------------------------------------------------

def sizes_of(payload: str) -> dict:
    """Размер и сжатие полезной нагрузки.

    ⚠️ Уровни сжатия — **практичные**, а не «максимум». На часе логов
    (216000 кадров, ~170 МБ) brotli 11 и zstd 19 считаются десятки минут и
    памяти, а разница с уровнем ниже — проценты. Здесь gzip 9, zstd 3
    (дефолт CLI), brotli 5; «сколько бы сжал лучший кодек» из отчёта
    снимается однократно скриптом `--high-effort`.
    """
    raw = payload.encode("utf-8")
    gz = gzip.compress(raw, compresslevel=9, mtime=0)
    zs = zstandard.ZstdCompressor(level=3).compress(raw)
    br = brotli.compress(raw, quality=5)
    return {
        "raw": len(raw),
        "gzip": len(gz),
        "zstd": len(zs),
        "brotli": len(br),
        "gzip_ratio": len(raw) / len(gz),
        "zstd_ratio": len(raw) / len(zs),
        "brotli_ratio": len(raw) / len(br),
    }


def validate_script(script: Script) -> None:
    """Повторяет пост-валидацию мода (src/api.rs::parse_script)."""
    assert len(script.name) <= MAX_NAME_CHARS, "name > 64"
    assert script.commands, "commands пуст"
    for i, c in enumerate(script.commands):
        assert c["duration"] >= 1, f"commands[{i}]: duration < 1"
        assert c["t"] <= MAX_SCRIPT_FRAMES and c["duration"] <= MAX_SCRIPT_FRAMES, \
            f"commands[{i}]: t/duration > {MAX_SCRIPT_FRAMES} (t={c['t']}, " \
            f"duration={c['duration']}, input={c['input']})"
        assert c["t"] + c["duration"] <= MAX_SCRIPT_FRAMES, \
            f"commands[{i}]: t+duration > {MAX_SCRIPT_FRAMES} (t={c['t']}, " \
            f"duration={c['duration']}, input={c['input']})"
        assert set(c["input"]) <= INPUT_KEYS, f"commands[{i}]: неизвестный вход"
        assert len(c["input"]) <= len(INPUT_FIELD_ORDER), "слишком много ключей"
        if "left_stick" in c["input"]:
            x, y = c["input"]["left_stick"]
            assert abs(x) <= ANALOG_ABS_MAX and abs(y) <= ANALOG_ABS_MAX


def check_roundtrip(script: Script) -> None:
    """Текст → разбор → те же кадры/длительности (без битов направлений)."""
    parsed: dict[int, dict] = {}
    for line in script.to_tas().splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        frame = int(parts[0])
        bucket = parsed.setdefault(frame, {})
        for token in parts[1:]:
            name, _, dur = token.partition(":")
            if name in ("ls", "rs"):
                angle, _, dur = dur.partition(":")
                rad = math.radians(float(angle))
                bucket["stick_" + name] = (round(1000 * math.sin(rad), 0),
                                           round(-1000 * math.cos(rad), 0))
            elif name in ("lsx", "lsx", "rsx"):
                val, _, dur = dur.partition(":")
                bucket["stick_" + name[:2] + "x"] = float(val)
            else:
                bucket[name] = int(dur) if dur else 1
    # Каждый кадр исходного скрипта с командами присутствует в тексте.
    frames = {c["t"] for c in script.commands}
    assert frames <= set(parsed), "текст потерял кадры"
    # Длительности токенов совпадают с длительностями команд (для однокадровых
    # скриптов это 1 — проверяем там, где токен не фронтовый).
    if all(c["duration"] == 1 for c in script.commands):
        for frame, tokens in parsed.items():
            for name, dur in tokens.items():
                if name.startswith("stick_"):
                    continue
                assert dur == 1, f"кадр {frame}: токен {name} длиной {dur}"


def _high_effort_sizes(payload: str) -> dict:
    """Максимальные уровни: gzip 9, zstd 19, brotli 11 (медленно на больших)."""
    raw = payload.encode("utf-8")
    gz = gzip.compress(raw, compresslevel=9, mtime=0)
    zs = zstandard.ZstdCompressor(level=19).compress(raw)
    br = brotli.compress(raw, quality=11)
    return {
        "raw": len(raw), "gzip": len(gz), "zstd": len(zs), "brotli": len(br),
        "gzip_ratio": len(raw) / len(gz), "zstd_ratio": len(raw) / len(zs),
        "brotli_ratio": len(raw) / len(br),
    }


def _frames_of(payload: str) -> int:
    """Сколько кадров покрывает готовый скрипт (JSON или .tas).

    Для JSON — `max(t + duration)`, для текста — последний номер строки +1.
    """
    text = payload.lstrip()
    if text.startswith("{"):
        try:
            data = json.loads(payload)
            return max((c["t"] + c["duration"]) for c in data["commands"])
        except (KeyError, TypeError, ValueError, json.JSONDecodeError) as err:
            raise SystemExit(f"не разобрал JSON скрипта: {err}") from err
    last = 0
    for line in payload.splitlines():
        line = line.strip()
        if not line or line.startswith(("#", "!")):
            continue
        last = max(last, int(line.split()[0]) + 1)
    return max(last, 1)


def report_for(name: str, payload: str, sizes_fn=sizes_of) -> dict:
    s = sizes_fn(payload)
    s["name"] = name
    s["body_limit"] = MAX_BODY_BYTES
    return s


def main() -> None:
    ap = argparse.ArgumentParser(description="Замер размера скриптов (JSON/.tas/сжатие)")
    ap.add_argument("--frames", default=",".join(str(f) for f in DEFAULT_FRAMES),
                    help="кадры через запятую (по умолчанию 3600,216000)")
    ap.add_argument("--out", default=str(DEFAULT_OUT), help="каталог примеров")
    ap.add_argument("--gen", action="store_true", help="только сгенерировать примеры")
    ap.add_argument("--high-effort", action="store_true",
                    help="уровни сжатия по максимуму (gzip 9, zstd 19, brotli 11) — "
                         "на часе логов считается десятки минут")
    ap.add_argument("--logs-frames", default=",".join(str(f) for f in DEFAULT_FRAMES),
                    help="кадры для замеров /logs (по умолчанию как --frames)")
    ap.add_argument("--file", action="append", default=[],
                    help="посчитать готовый скрипт (JSON или .tas): путь к файлу; "
                         "можно несколько раз")
    args = ap.parse_args()

    sizes_fn = _high_effort_sizes if args.high_effort else sizes_of

    frames = [int(x) for x in args.frames.split(",") if x.strip()]
    logs_frames = [int(x) for x in args.logs_frames.split(",") if x.strip()]
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    rows: list[dict] = []
    writers: list[tuple[str, str]] = []

    def emit(filename: str, payload: str, script: Script | None = None):
        path = out_dir / filename
        path.write_text(payload, encoding="utf-8", newline="\n")
        if script is not None:
            validate_script(script)
            check_roundtrip(script)
        writers.append((filename, payload))

    # --- скрипты (всегда один такт = MAX_SCRIPT_FRAMES кадров) ---
    for label, gen in (("benign", gen_benign), ("rotsplit", gen_rot_split),
                       ("sticky", gen_sticky)):
        script = gen(TAKT_FRAMES)
        json_text, tas_text = script.to_json(), script.to_tas()
        emit(f"{label}.json", json_text, script)
        emit(f"{label}.tas", tas_text, script)
        row = _script_row(f"{label}", json_text, TAKT_FRAMES, len(script.commands),
                          sizes_fn)
        _attach_tas(row, tas_text)
        for total in frames:
            _attach_takt(row, total)
        rows.append(row)
    rscript = gen_restart_heavy()
    emit("restart_heavy.json", rscript.to_json(), rscript)
    emit("restart_heavy.tas", rscript.to_tas(), rscript)
    rrow = _script_row("restart_heavy", rscript.to_json(), 600,
                       len(rscript.commands), sizes_fn)
    rrow["kind"] = "restart"
    rrow["restarts"] = 4
    _attach_tas(rrow, rscript.to_tas())
    rows.append(rrow)

    # --- ответ /logs ---
    if not args.file:
        for total in logs_frames:
            print(f"  сжатие logs_{total} …", flush=True)
            t0 = time.time()
            logs = gen_logs_json(total)
            emit(f"logs_{total}.json", logs)
            rows.append(_logs_row(f"logs_{total}", logs, total, sizes_fn))
            print(f"    готово за {time.time() - t0:.1f} с", flush=True)

    # --- готовые скрипты из --file ---
    if args.file:
        for path_str in args.file:
            path = Path(path_str)
            payload = path.read_text(encoding="utf-8")
            frames_n = _frames_of(payload)
            row = report_for(path.name, payload, sizes_fn)
            row.update({"kind": "external", "frames": frames_n,
                        "bytes_per_frame": row["raw"] / max(frames_n, 1),
                        "tas_raw": None})
            rows.append(row)
            print(f"  {path.name}: {human(row['raw'])} JSON, "
                  f"{row['raw'] / max(frames_n, 1):.1f} B/кадр", flush=True)

    if args.gen:
        print(f"сгенерировано: {len(writers)} файлов в {out_dir}")
        return

    write_report(rows, out_dir, frames, writers, high_effort=args.high_effort)
    print(f"отчёт: {HERE / 'report.md'}  (примеры: {out_dir})")


def _script_row(name: str, payload: str, frames: int, commands: int,
                sizes_fn=sizes_of) -> dict:
    raw = len(payload.encode())
    row = report_for(name, payload, sizes_fn)
    row.update({"kind": "script", "frames": frames, "commands": commands,
                "bytes_per_frame": raw / max(frames, 1),
                "fits_body": raw <= MAX_BODY_BYTES})
    return row


def _attach_tas(row: dict, tas_text: str) -> None:
    """Дописывает к строке скрипта размер его текста."""
    row["tas_raw"] = len(tas_text.encode("utf-8"))


def _attach_takt(row: dict, total_frames: int) -> None:
    """Размер за прогон длиной `total_frames`: такт × число тактов."""
    n = takt_count(total_frames)
    row.setdefault("takts", {})[total_frames] = {
        "takts": n,
        "json": row["raw"] * n,
        "gzip": row["gzip"] * n,
        "zstd": row["zstd"] * n,
        "brotli": row["brotli"] * n,
        "tas": row["tas_raw"] * n,
    }


def _logs_row(name: str, payload: str, frames: int, sizes_fn=sizes_of) -> dict:
    row = report_for(name, payload, sizes_fn)
    row.update({"kind": "logs", "frames": frames,
                "bytes_per_frame": len(payload.encode()) / max(frames, 1),
                "fits_body": None})
    return row


def human(n: float) -> str:
    if n < 1024:
        return f"{n:.0f} B"
    if n < 1024 * 1024:
        return f"{n / 1024:.1f} KiB"
    return f"{n / 1024 / 1024:.1f} MiB"


def logs_memory_note() -> list[str]:
    """Память кольцевого буфера в процессе игры.

    `LogFrame` (`src/api.rs`) — POD-структура без указателей: позиция/поворот/
    скорость, HP и анимация, поза камеры, `InputUnit` (48 B), `EnemyState`
    (32 B), биты поданного ввода и два `&'static str` статусов. Размер
    **216 B** измерен `std::mem::size_of::<LogFrame>()` (проверка — в
    `LOG_FRAME_BYTES`; переизмерять вставкой теста в `src/api.rs`).
    `VecDeque<LogFrame>` создаётся один раз через `with_capacity`, поэтому
    ёмкость и память фиксированы и не зависят от времени работы.
    """
    ring = LOG_FRAME_BYTES * RING_CAPACITY
    return [
        f"- `LogFrame`: {LOG_FRAME_BYTES} B (измерено).",
        f"- Кольцевой буфер 3600 кадров (`VecDeque` с `with_capacity`): "
        f"**~{human(ring)}** ({ring / 1024 / 1024:.2f} МиБ) — столько и на 60 с, "
        f"и на час работы: буфер не растёт, старые кадры вытесняются.",
        f"- Память буфера + копия ответа `GET /logs` при `limit={MAX_LOG_LIMIT}`: "
        f"до {human(MAX_LOG_LIMIT * 788)} на один запрос (по замеру 788 B/кадр "
        f"в JSON) — временный `Vec<LogFrameJson>`, на ёмкость буфера не влияет.",
    ]


def write_report(rows: list[dict], out_dir: Path, frames: list[int],
                 writers: list[tuple[str, str]], high_effort: bool = False) -> None:
    scripts = [r for r in rows if r["kind"] == "script"]
    lines: list[str] = []
    lines.append("<!-- сгенерировано: python tools/script_size/measure.py -->")
    lines.append("")
    lines.append("# Замер размера скриптов и логов")
    lines.append("")
    lines.append(f"Каталог примеров: `{out_dir.name}/`. Проверки схемы — "
                 f"`measure.py` (assert'ы по `ScriptInput`, порядку команд и "
                 f"синтаксису `.tas`).")

    lines.append("")
    lines.append("## Скрипт: `POST /script/run`")
    lines.append("")
    lines.append("Таблица — это **один такт** (3600 кадров = 60 с): столько кадров"
                 " вмещает кольцевой буфер логов и столько же был прежний потолок"
                 " скрипта. Практический ограничитель скрипта — размер тела запроса"
                 f" ({human(MAX_BODY_BYTES)}), а `MAX_SCRIPT_FRAMES`"
                 f" ({MAX_SCRIPT_FRAMES}) осталась верхней страховкой; размер"
                 " скрипта линеен по кадрам, поэтому час = такт × 60.")
    lines.append("")
    lines.append("| Профиль | Команд | JSON | JSON/кадр | gzip | zstd | brotli | "
                 ".tas | .tas/кадр | влезает в 64 КиБ |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|")
    for r in scripts:
        lines.append(
            f"| {r['name']} | {r['commands']} | {human(r['raw'])} | "
            f"{r['bytes_per_frame']:.1f} B | {human(r['gzip'])} | {human(r['zstd'])} | "
            f"{human(r['brotli'])} | {human(r['tas_raw'])} | "
            f"{r['tas_raw'] / TAKT_FRAMES:.1f} B | "
            f"{'да' if r['fits_body'] else '**НЕТ**'} |"
        )

    lines.append("")
    lines.append("## Прогон длиной N: 60 тактов = час")
    lines.append("")
    lines.append("| Профиль | кодек | " + " | ".join(time_label(f) for f in frames) + " |")
    lines.append("|---|---|" + "---|" * len(frames))
    for r in scripts:
        for codec in ("json", "gzip", "zstd", "brotli", "tas"):
            cells = " | ".join(human(r["takts"][f][codec]) for f in frames)
            lines.append(f"| {r['name']} | {codec} | {cells} |")
    lines.append("")
    lines.append(f"Такт: {TAKT_FRAMES} кадров. `{time_label(frames[-1])}` = "
                 f"{takt_count(frames[-1])} тактов = столько `POST /script/run` "
                 f"подряд (лимит тела 64 КиБ проверяется на каждом).")

    lines.append("")
    lines.append("## Рестарт-фаза отдельно (нет своего лимита размера)")
    lines.append("")
    lines.append("| Профиль | Кадров | Команд | JSON | КБ на рестарт | .tas |")
    lines.append("|---|---|---|---|---|---|")
    for r in rows:
        if r["kind"] != "restart":
            continue
        restarts = r["restarts"]
        lines.append(
            f"| {r['name']} | {r['frames']} | {r['commands']} | {human(r['raw'])} | "
            f"{r['raw'] / restarts / 1024:.2f} | {human(r['tas_raw'])} |"
        )

    lines.append("")
    lines.append("## Ответ `GET /logs` (кольцевой буфер)")
    lines.append("")
    lines.append("| Выборка | Кадров | JSON | JSON/кадр | gzip | zstd | brotli |")
    lines.append("|---|---|---|---|---|---|---|")
    for r in rows:
        if r["kind"] != "logs":
            continue
        lines.append(
            f"| {r['name']} | {r['frames']} | {human(r['raw'])} | "
            f"{r['bytes_per_frame']:.1f} B | {human(r['gzip'])} | {human(r['zstd'])} | "
            f"{human(r['brotli'])} |"
        )
    lines.append("")
    lines.append("## Память кольцевого буфера в RAM")
    lines.append("")
    lines.extend(logs_memory_note())
    lines.append("")
    lines.append("Выборка на час — это 60 запросов `GET /logs?limit=3600` подряд, "
                 "буфера на час в моде нет: `RING_CAPACITY` = 3600 кадров (60 с) "
                 "вытесняет всё старше.")

    lines.append("")
    lines.append("## Таблица команд (.tas)")
    lines.append("")
    lines.append("| Файл | строк (кадров) | токенов |")
    lines.append("|---|---|---|")
    for name, payload in writers:
        if not name.endswith(".tas"):
            continue
        body = [l for l in payload.splitlines()
                if l and not l.startswith(("#", "!"))]
        segments = sum(max(0, len(l.split()) - 1) for l in body)
        lines.append(f"| {name} | {len(body)} | {segments} |")

    lines.append("")
    lines.append("## Про кодек на проводе")
    lines.append("")
    levels = "gzip 9, brotli 11, zstd 19 (`--high-effort`)" if high_effort else \
        "gzip 9, brotli 5, zstd 3"
    lines.append(f"- Уровни сжатия в этом прогоне: {levels}. Практичные уровни "
                 f"выбраны из-за объёма: на часе логов максимум считается "
                 f"десятки минут, а разница — проценты.")
    lines.append("- `Content-Encoding: gzip` мод принимает (`src/api.rs`), и "
                 "лимит тела считается по сжатым байтам: `gzip`-колонка — это "
                 "уже не потолок выигрыша, а то, что реально влезает в 64 КиБ. "
                 "Сквозная проверка приёма — `check_gzip_e2e.py`.")
    lines.append("- Скрипты текстово-однородны (повторяющиеся ключи, сотни "
                 "похожих команд), поэтому gzip/brotli жмут их в разы сильнее "
                 "логов: сжатие снимает как раз потолок размера тела.")
    lines.append("- Логи наоборот — почти случайные числа, энтропия высокая; "
                 "выигрыш от сжатия у них невелик, а решает там не 64 КиБ "
                 "тела, а размер ответа и частота запросов.")

    lines.append("")
    lines.append("## Готовые скрипты (`--file`)")
    lines.append("")
    lines.append("| Файл | Кадров | JSON | JSON/кадр | gzip | zstd | brotli | "
                 "влезает в 64 КиБ |")
    lines.append("|---|---|---|---|---|---|---|---|")
    for r in rows:
        if r["kind"] != "external":
            continue
        fits = "да" if r["raw"] <= MAX_BODY_BYTES else "**НЕТ**"
        lines.append(
            f"| {r['name']} | {r['frames']} | {human(r['raw'])} | "
            f"{r['bytes_per_frame']:.1f} B | {human(r['gzip'])} | {human(r['zstd'])} | "
            f"{human(r['brotli'])} | {fits} |"
        )

    lines.append("")
    lines.append("## Что мерить на живой записи")
    lines.append("")
    lines.append("Порядок — `tools/dbdump/README.md` (`--script`): выгрузить "
                 "записанный прогон в JSON и подать здесь же как `--file`, "
                 "чтобы получить реальный `bytes/кадр` (эталонные `.tas` "
                 "находятся в `tas-editor-cs/TasEditorCs.Tests/Fixtures/`).")
    (HERE / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
