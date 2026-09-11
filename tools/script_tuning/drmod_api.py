# -*- coding: utf-8 -*-
"""Общий клиент HTTP API мода drmod-rs для инструментов script_tuning.

Два неочевидных момента, из-за которых тут свой клиент, а не `urllib`:

* Сокету **нельзя** выставлять таймаут: `SO_RCVTIMEO` переводит стек в
  неблокирующий режим, и ответ теряется (сервер мода закрывает соединение
  сразу после ответа, `Connection: close`) — WinError 10053 в ~2/3 случаев.
  Замер: без таймаута 8/8, с таймаутом 5/8. Ждём через `select`.
* Меню паузы принимает клавиши только когда окно игры **в фокусе**: игра
  опрашивает клавиатуру (`DirectInput` `GetDeviceState`) в функции
  `base+0x9D9670` и выходит раньше, если окно не foreground. Поэтому перед
  подачей меню-ввода окно активируется (`activate_window`).
"""
import ctypes
import json
import select
import socket
import sys
import time
import urllib.parse
from ctypes import wintypes

DEFAULT_URL = "http://127.0.0.1:5223"
GAME_TITLE = "METAL GEAR RISING: REVENGEANCE"


def setup_stdout():
    """UTF-8 + построчная буферизация для stdout инструментов.

    Иначе вывод идёт в локальной кодировке (cp1251 при перенаправлении в файл,
    cp866 в консоли) и русский текст в логах читается кракозябрами, а символы
    вне cp1251 (`→`, `≥`, `✓`) вообще роняют печать уже после записи CSV.
    """
    sys.stdout.reconfigure(encoding="utf-8", errors="replace",
                           line_buffering=True)

# Коды клавиш DirectInput (DIK): их мод подмешивает в `ms_InputKeys` после
# опроса устройства. Стрелки/Enter/Esc — то, что читает меню.
DIK_ESCAPE = 0x01
DIK_RETURN = 0x1C
DIK_UP = 0xC8
DIK_DOWN = 0xD0
DIK_LEFT = 0xCB
DIK_RIGHT = 0xCD

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
SW_RESTORE = 9


def http(base=DEFAULT_URL, path="/", method="GET", body=None, wait=5.0, tries=3):
    """HTTP-запрос к API мода на блокирующем сокете (см. модуль docstring).

    Одиночные обрывы случаются (сервер живёт в render-цикле игры и под нагрузкой,
    например на загрузке, может сбросить соединение) — повторяем запрос.
    """
    for attempt in range(tries):
        try:
            return _http_once(base, path, method, body, wait)
        except (ConnectionError, OSError) as e:
            if attempt == tries - 1:
                raise RuntimeError(f"{method} {path} → {e}") from e
            time.sleep(0.2 * (attempt + 1))


def _http_once(base, path, method, body, wait):
    u = urllib.parse.urlsplit(base)
    host, port = u.hostname, u.port or 80
    data = json.dumps(body, ensure_ascii=False).encode("utf-8") if body is not None else None
    req = (f"{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\n"
           f"User-Agent: drmod-script-tuning/1\r\nConnection: close\r\n").encode("ascii")
    if data is not None:
        req += b"Content-Type: application/json\r\n"
        req += f"Content-Length: {len(data)}\r\n".encode("ascii")
    req += b"\r\n" + (data or b"")

    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        s.connect((host, port))
        s.sendall(req)
        raw, deadline = b"", time.monotonic() + wait
        while True:
            left = deadline - time.monotonic()
            if left <= 0:
                raise RuntimeError(f"{method} {path}: таймаут ответа ({wait} с)")
            ready, _, _ = select.select([s], [], [], left)
            if not ready:
                continue
            chunk = s.recv(65536)
            if not chunk:
                break
            raw += chunk
    finally:
        s.close()
    head, _, payload = raw.partition(b"\r\n\r\n")
    if not raw:
        # Сервер мода живёт в render-цикле игры и под нагрузкой (загрузка миссии)
        # иногда закрывает соединение, не ответив — это retry-кейс, а не ошибка.
        raise ConnectionError(f"{method} {path}: пустой ответ")
    status = int(head.split(b" ")[1]) if head.startswith(b"HTTP/") else 0
    if status != 200:
        raise RuntimeError(f"{method} {path} → HTTP {status}: "
                           f"{payload.decode('utf-8', 'replace')}")
    return json.loads(payload) if payload else {}


def state(base=DEFAULT_URL):
    return http(base, "/state")


def logs(base=DEFAULT_URL, script_id=None, limit=2000):
    path = f"/logs?limit={limit}"
    if script_id is not None:
        path += f"&script_id={script_id}"
    return http(base, path, wait=10.0).get("frames", [])


def run_script(script, base=DEFAULT_URL):
    """Запускает скрипт, при 409 (уже есть активный) снимает старый и повторяет."""
    try:
        return http(base, "/script/run", "POST", script)
    except RuntimeError as e:
        if "409" not in str(e):
            raise
        http(base, "/script/stop", "POST")
        time.sleep(0.2)
        return http(base, "/script/run", "POST", script)


def wait_script(script_id, base=DEFAULT_URL, timeout=30.0, quiet=False):
    """Ждёт завершения скрипта, печатает смены статуса. Возвращает последний статус."""
    start = time.monotonic()
    last = None
    while time.monotonic() - start < timeout:
        st = http(base, f"/script/{script_id}")
        if st["status"] != last:
            if not quiet:
                print(f"  script {script_id}: {st['status']} "
                      f"({st['frame']}/{st['total_frames']})", flush=True)
            last = st["status"]
        if st["status"] in ("done", "stopped"):
            return st["status"]
        time.sleep(0.05)
    http(base, "/script/stop", "POST")
    return "timeout"


def activate_window(title=GAME_TITLE, tries=3):
    """Переводит окно игры в фокус (иначе игра не опрашивает клавиатуру).

    `SetForegroundWindow` работает только если вызывающий процесс уже в фокусе,
    поэтому используем приём с `AttachThreadInput` — подключение к потоку окна
    игры снимает ограничение. Фокус могут перехватывать другие окна (за машиной
    работает человек), поэтому пробуем несколько раз.
    """
    hwnd = user32.FindWindowW(None, title)
    if not hwnd:
        return False
    for attempt in range(tries):
        user32.ShowWindow(hwnd, SW_RESTORE)
        if user32.GetForegroundWindow() == hwnd:
            return True
        target_thread = user32.GetWindowThreadProcessId(hwnd, None)
        current_thread = kernel32.GetCurrentThreadId()
        user32.AttachThreadInput(current_thread, target_thread, True)
        try:
            user32.SetForegroundWindow(hwnd)
            user32.BringWindowToTop(hwnd)
        finally:
            user32.AttachThreadInput(current_thread, target_thread, False)
        if user32.GetForegroundWindow() == hwnd:
            return True
        time.sleep(0.3 * (attempt + 1))
    return user32.GetForegroundWindow() == hwnd


def focus_and_settle(title=GAME_TITLE, delay=0.35):
    """Активирует окно и даёт игре несколько кадров перехватить фокус."""
    ok = activate_window(title)
    time.sleep(delay)
    return ok


def menu_status_transitions(frames):
    """Список (frame_index, status) смен `menu_status` из кадров `/logs`."""
    out, prev = [], None
    for fr in frames:
        ms = fr.get("menu_status", "?")
        if ms != prev:
            out.append((fr["frame"], ms))
            prev = ms
    return out


#: Статусы меню, означающие перезагрузку/переход (рестарт миссии идёт через них).
LOADING_STATUSES = {"NONE", "Loading Into Mission", "Loading Into Boss Mission",
                    "LoadingIntoMission", "MainMenuLoad"}

#: Статусы, из которых игра сама не выйдет: игрок убит, идёт fail-меню.
FAIL_STATUSES = {"Mission Fail", "Mission Failed", "Game Over"}


def _is_loading(menu_status):
    return menu_status in LOADING_STATUSES or "loading" in menu_status.lower()


def watch_state(base=DEFAULT_URL, duration=15.0, baseline=None, interval=0.15):
    """Наблюдает `/state`: пишет смены статуса меню и позиции игрока.

    Рестарт миссии виден не в кадрах скрипта (loading наступает позже), а в
    состоянии: статус уходит в loading-статус, игрок пересоздаётся в другой
    точке. Возвращает `{"timeline", "last", "restarted"}`.
    """
    baseline_pos = (baseline or {}).get("player", {}).get("pos") if baseline else None
    timeline, last, prev_status, prev_pos = [], None, None, None
    restarted = False
    deadline = time.monotonic() + duration
    while time.monotonic() < deadline:
        st = state(base)
        last = st
        ms = st.get("menu_status", "?")
        player = st.get("player") or {}
        pos = player.get("pos")
        if ms != prev_status:
            timeline.append(f"menu: {prev_status} → {ms}")
            prev_status = ms
        if _is_loading(ms):
            restarted = True
        if pos and prev_pos and any(abs(a - b) > 0.5 for a, b in zip(pos, prev_pos)):
            timeline.append(f"pos: {[round(v, 2) for v in prev_pos]} → "
                            f"{[round(v, 2) for v in pos]}")
        if baseline_pos and pos and any(
                abs(a - b) > 0.5 for a, b in zip(pos, baseline_pos)):
            restarted = True
        prev_pos = pos
        # Как только увидели loading и вернулись в геймплей — выходим.
        if restarted and ms == "In Game":
            timeline.append("миссия перезагружена, снова In Game")
            break
        time.sleep(interval)
    return {"timeline": timeline, "last": last or {}, "restarted": restarted}


def open_menu(base=DEFAULT_URL):
    """Открывает меню паузы (бит START) — игра встаёт на паузу.

    Нужно в паузах между экспериментами: пока разбираешь логи, враг в игре
    живой и может убить игрока (Mission Fail ломает следующий прогон).
    """
    if state(base).get("menu_status") == "In Game":
        script = {"name": "pause",
                  "commands": [{"t": 0, "duration": 3, "input": {"pause": True}}]}
        sid = run_script(script, base)["script_id"]
        wait_script(sid, base, 10.0, quiet=True)
    return state(base).get("menu_status")


def build_restart_script(ups=1, downs=0, hold=6, open_gap=20, gap=10,
                         confirms=2, confirm_gap=25, tail=60):
    """Скрипт рестарта миссии: pause → стрелки → confirm ×N.

    * `open_gap` — пауза после `pause`: меню должно успеть открыться (короткая
      пауза — стрелка приходит в анимацию меню и теряется, проверено);
    * `confirms=2` — пункт Restart открывает диалог «You will lose all unsaved
      progress. Restart from last checkpoint?» (YES уже выбран), поэтому нужен
      второй `confirm` по диалогу.
    """
    cmds = [{"t": 0, "duration": 3, "input": {"pause": True}}]
    t = 3 + open_gap
    for _ in range(ups):
        cmds.append({"t": t, "duration": hold, "input": {"dik_key": DIK_UP}})
        t += hold + gap
    for _ in range(downs):
        cmds.append({"t": t, "duration": hold, "input": {"dik_key": DIK_DOWN}})
        t += hold + gap
    for i in range(confirms):
        cmds.append({"t": t, "duration": 3, "input": {"confirm": True}})
        t += 3 + (tail if i + 1 == confirms else confirm_gap)
    return {"name": "restart", "commands": cmds}, t


def ensure_gameplay(base=DEFAULT_URL, timeout=10.0, settle=3.0):
    """Приводит игру в геймплей: закрывает меню паузы, отрабатывает fail-меню.

    Переход идёт через ProcessOutOfPause, поэтому статус становится In Game не
    сразу — ждём, а не проверяем один раз. Fail-меню (игрок убит) само не
    уходит: без отдельного шага серия прогонов упиралась в «игра не в
    геймплее» и теряла прогон за прогоном (проверено 2026-09-11: после паузы
    между сериями игрока убивают).
    """
    menu = state(base).get("menu_status")
    if menu == "In Game":
        return True
    if menu in FAIL_STATUSES:
        # Выход из fail-меню быстрый (Mission Fail → In Game одним confirm,
        # проверено живьём) — ждать дольше пары секунд не нужно.
        return bool(recover_fail(base, timeout=12.0).get("ok"))
    script = {"name": "close-menu",
              "commands": [{"t": 0, "duration": 3, "input": {"pause": True}}]}
    sid = run_script(script, base)["script_id"]
    wait_script(sid, base, timeout, quiet=True)
    deadline = time.monotonic() + settle
    while time.monotonic() < deadline:
        if state(base).get("menu_status") == "In Game":
            return True
        time.sleep(0.1)
    return False


def recover_fail(base=DEFAULT_URL, timeout=12.0, settle=0.3):
    """Выход из Mission Fail: `confirm` (в fail-меню выбран «Retry»).

    Возвращает `{"ok", "before", "timeline"}`: траектория статусов нужна, чтобы
    отличить «не сработал confirm» от «меню ведёт себя иначе» (например, ушло
    в главное меню — тогда поднимать надо меню заголовка, а не этот путь).
    """
    before = state(base).get("menu_status")
    script = {"name": "fail-retry",
              "commands": [{"t": 0, "duration": 3, "input": {"confirm": True}}]}
    sid = run_script(script, base)["script_id"]
    wait_script(sid, base, 10.0, quiet=True)
    timeline, prev = [], None
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        ms = state(base).get("menu_status", "?")
        if ms != prev:
            timeline.append(ms)
            prev = ms
        if ms == "In Game":
            return {"ok": True, "before": before, "timeline": timeline}
        time.sleep(settle)
    return {"ok": False, "before": before, "timeline": timeline}


def restart_mission(base=DEFAULT_URL, focus=True, watch=15.0, **kwargs):
    """Рестарт миссии через меню паузы (см. `build_restart_script`).

    Возвращает `{"ok", "sid", "script_status", "drawer"}`: `ok` — увидели
    loading/пересоздание игрока. Требует фокуса окна игры: без него игра не
    опрашивает клавиатуру (`DirectInput`) и меню не двигается.
    """
    if focus:
        focus_and_settle()
    if not ensure_gameplay(base):
        return {"ok": False, "reason": "не удалось выйти из меню в геймплей"}
    baseline = state(base)
    script, _total = build_restart_script(**kwargs)
    sid = run_script(script, base)["script_id"]
    script_status = wait_script(sid, base, timeout=30.0, quiet=True)
    watcher = watch_state(base, duration=watch, baseline=baseline)
    return {"ok": watcher["restarted"], "sid": sid, "script_status": script_status,
            "watch": watcher}


if __name__ == "__main__":
    print("health:", http("/health"))
    print("focus:", activate_window())
    print("state:", json.dumps(state(), ensure_ascii=False)[:200])
