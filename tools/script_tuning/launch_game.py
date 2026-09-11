# -*- coding: utf-8 -*-
"""Запуск игры и внедрение мода — автоматизация цикла прогонов.

Что делает:

1. (по `--build`) пересобирает мод, чтобы внедрилась свежая DLL;
2. если игра уже запущена — не трогает её, только внедряет мод;
3. иначе запускает exe игры с её рабочим каталогом и ждёт появления окна;
4. внедряет `drmod.exe` (инжектор) с повтором: первая попытка после горячей
   выгрузки DLL иногда не проходит;
5. ждёт `GET /health` и печатает `GET /state`.

Примеры:
    py -3 tools\\script_tuning\\launch_game.py
    py -3 tools\\script_tuning\\launch_game.py --build
    py -3 tools\\script_tuning\\launch_game.py --kill-first
"""
import argparse
import ctypes
import os
import subprocess
import sys
import time

import drmod_api as api

GAME_EXE = (r"C:\Program Files (x86)\Steam\steamapps\common"
            r"\METAL GEAR RISING REVENGEANCE\METAL GEAR RISING REVENGEANCE.exe")
INJECTOR = os.path.join("target", "i686-pc-windows-msvc", "debug", "drmod.exe")
PROCESS_NAME = "METAL GEAR RISING REVENGEANCE"

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)


def game_running():
    """PID игры или None (через tasklist, без psutil)."""
    out = subprocess.run(
        ["tasklist", "/FI", f"IMAGENAME eq {PROCESS_NAME}.exe", "/FO", "CSV", "/NH"],
        capture_output=True, text=True, errors="replace",
    ).stdout
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) >= 2 and parts[0].lower().startswith(PROCESS_NAME.lower()):
            try:
                return int(parts[1])
            except ValueError:
                return 0
    return None


def wait_for_window(timeout=120.0):
    """Ждёт появления окна игры (по Process.MainWindowHandle через WMI-нет — API)."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        pid = game_running()
        if pid:
            hwnd = user32.FindWindowW(None, api.GAME_TITLE)
            if hwnd:
                return pid
        time.sleep(1.0)
    return None


def kill_game(pid=None):
    """Мягко завершает игру (после краша её обычно уже нет)."""
    pid = pid or game_running()
    if not pid:
        return False
    subprocess.run(["taskkill", "/PID", str(pid), "/F"], capture_output=True)
    time.sleep(2.0)
    return True


def inject(tries=3, wait_after=6.0):
    """Внедряет мод инжектором, проверяя `GET /health`."""
    for attempt in range(1, tries + 1):
        proc = subprocess.run([INJECTOR], capture_output=True, text=True,
                              errors="replace", cwd=os.getcwd())
        out = (proc.stdout or "").strip().replace("\n", " ")
        print(f"  инжект #{attempt}: {out or '(нет вывода)'}")
        deadline = time.monotonic() + wait_after
        while time.monotonic() < deadline:
            try:
                health = api.http(path="/health", wait=2.0)
                print(f"  мод отвечает: uptime={health.get('uptime_ms')} мс")
                return True
            except RuntimeError:
                time.sleep(0.5)
    return False


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--build", action="store_true", help="сначала `cargo build`")
    p.add_argument("--kill-first", action="store_true",
                   help="убить уже запущенную игру (чистый старт)")
    p.add_argument("--exe", default=GAME_EXE)
    p.add_argument("--timeout", type=float, default=120.0, help="с — ждать окно игры")
    p.add_argument("--no-inject", action="store_true")
    a = p.parse_args(argv)
    sys.stdout.reconfigure(line_buffering=True)

    if a.build:
        print("сборка мода (cargo build)...")
        r = subprocess.run(["cargo", "build"], capture_output=True, text=True,
                           errors="replace")
        if r.returncode != 0:
            print("сборка упала:")
            print((r.stderr or "")[-2000:])
            return 2
        print("  OK")

    pid = game_running()
    if pid and a.kill_first:
        print(f"завершаю запущенную игру (PID {pid})")
        kill_game(pid)
        pid = None
    if not pid:
        if not os.path.exists(a.exe):
            print(f"нет exe игры: {a.exe}")
            return 2
        print(f"запускаю игру: {a.exe}")
        subprocess.Popen([a.exe], cwd=os.path.dirname(a.exe))
        pid = wait_for_window(a.timeout)
        if not pid:
            print(f"окно игры не появилось за {a.timeout} с")
            return 2
        print(f"  игра запущена: PID {pid}")
    else:
        print(f"игра уже запущена: PID {pid}")

    if a.no_inject:
        return 0

    print("внедряю мод...")
    if not inject():
        print("мод не поднялся (нет /health)")
        return 3

    time.sleep(1.0)
    try:
        st = api.state()
        player = st.get("player") or {}
        print(f"состояние: menu={st.get('menu_status')} mission={st.get('mission_name')} "
              f"pos={player.get('pos')}")
        if st.get("menu_status") != "In Game":
            print("  ⚠ игра не в геймплее — загрузи миссию (или это титул/меню)")
    except RuntimeError as e:
        print(f"состояние не прочитано: {e}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
