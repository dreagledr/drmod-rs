# -*- coding: utf-8 -*-
"""Выход из Mission Fail (игрок убит) — диагностика и восстановление.

Зачем: fail-меню игра сама не покидает, а мод в этом состоянии отдаёт
`menu_status: Mission Fail`. Серия прогонов на этом встаёт («игра не в
геймплее» на каждом прогоне), поэтому выход нужен автоматический: `confirm`
по пункту Retry (в fail-меню он выбран по умолчанию).

С `--wait` инструмент сначала даёт игроку умереть (снимает паузу и ждёт статус
из `FAIL_STATUSES`) — так проверялся сам выход 2026-09-11.

    py -3 tools\\script_tuning\\fail_recovery.py --wait 420
    py -3 tools\\script_tuning\\fail_recovery.py --wait 0   # если уже в fail-меню
"""
import argparse
import sys
import time

import drmod_api as api


def wait_for_fail(base, timeout, interval=5.0):
    """Ждёт, пока игрок умрёт: статус меню уходит в fail-список."""
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        st = api.state(base)
        ms = st.get("menu_status")
        if ms != last:
            print(f"  menu={ms}", flush=True)
            last = ms
        if ms in api.FAIL_STATUSES:
            return ms
        time.sleep(interval)
    return None


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--wait", type=float, default=420.0,
                   help="с — сколько ждать смерти игрока (0 — не ждать)")
    p.add_argument("--timeout", type=float, default=12.0,
                   help="с — сколько ждать выхода после подачи confirm")
    p.add_argument("--no-focus", action="store_true")
    p.add_argument("--url", default=api.DEFAULT_URL)
    a = p.parse_args(argv)
    api.setup_stdout()

    if not a.no_focus:
        print(f"фокус окна игры: {'OK' if api.focus_and_settle() else 'НЕ ПОЛУЧИЛСЯ'}")

    before = api.state(a.url)
    print(f"до: menu={before.get('menu_status')} "
          f"hp={(before.get('player') or {}).get('hp')}")

    if a.wait > 0 and before.get("menu_status") not in api.FAIL_STATUSES:
        # Снять паузу: пока игра на паузе, игрок не умирает никогда.
        if before.get("menu_status") != "In Game":
            print(f"  закрываю меню паузы: {api.ensure_gameplay(a.url)}")
        print(f"жду смерти игрока (до {a.wait:.0f} с)...")
        ms = wait_for_fail(a.url, a.wait)
        if ms is None:
            print("игрок не умер — нечего проверять")
            return 2
        print(f"fail-статус получен: {ms}")

    res = api.recover_fail(a.url, timeout=a.timeout)
    print(f"\nвыход из fail-меню: {'OK' if res['ok'] else 'НЕ СРАБОТАЛ'}")
    print(f"  было: {res['before']}")
    print(f"  траектория статусов: {' → '.join(res['timeline']) or '—'}")
    after = api.state(a.url)
    pl = after.get("player") or {}
    print(f"  стало: menu={after.get('menu_status')} hp={pl.get('hp')} "
          f"поз={[round(v, 1) for v in (pl.get('pos') or [])]}")
    return 0 if res["ok"] else 1


if __name__ == "__main__":
    sys.exit(main())
