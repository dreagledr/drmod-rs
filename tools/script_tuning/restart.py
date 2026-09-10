# -*- coding: utf-8 -*-
"""Рестарт миссии скриптом (меню паузы) — CLI над `drmod_api.restart_mission`.

Схема (проверено live 2026-09-10, миссия P118_BEACH):

1. меню паузы открывается битом InputUnit START (`pause`, бит 0x100);
2. курсор ходит клавишами DirectInput: мод подмешивает DIK-биты в
   `ms_InputKeys` после опроса устройства (`dik:0xC8` — вверх, `dik:0xD0` — вниз);
3. пункт Restart — нижний, от верхнего это одно нажатие **вверх**;
4. Restart открывает диалог «You will lose all unsaved progress. Restart from
   last checkpoint?» (YES уже выбран) — нужен второй `confirm`;
5. манифест: игра опрашивает клавиатуру только когда окно в фокусе, поэтому
   инструмент активирует окно игры (`--no-focus` для отладки).

Примеры:
    py -3 tools\\script_tuning\\restart.py            # рестарт
    py -3 tools\\script_tuning\\restart.py --dry      # только довести курсор
    py -3 tools\\script_tuning\\restart.py --down 2   # курсор вниз ×2
"""
import argparse
import sys

import drmod_api as api


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--up", type=int, default=1, help="нажатий «вверх» (Restart = нижний пункт)")
    p.add_argument("--down", type=int, default=0, help="нажатий «вниз»")
    p.add_argument("--hold", type=int, default=6, help="кадров удержания стрелки")
    p.add_argument("--open-gap", type=int, default=20,
                   help="кадров после pause — меню должно открыться")
    p.add_argument("--gap", type=int, default=10, help="кадров между стрелками и confirm")
    p.add_argument("--confirms", type=int, default=2,
                   help="подтверждений подряд (2: пункт Restart + диалог YES)")
    p.add_argument("--confirm-gap", type=int, default=25,
                   help="кадров между подтверждениями (диалог должен появиться)")
    p.add_argument("--tail", type=int, default=60, help="кадров хвоста после последнего confirm")
    p.add_argument("--watch", type=float, default=15.0, help="с — сколько наблюдать /state")
    p.add_argument("--dry", action="store_true", help="без confirm (только довести курсор)")
    p.add_argument("--no-focus", action="store_true", help="не активировать окно игры")
    p.add_argument("--url", default=api.DEFAULT_URL)
    a = p.parse_args(argv)
    sys.stdout.reconfigure(line_buffering=True)

    before = api.state(a.url)
    print(f"до: menu={before.get('menu_status')} mission={before.get('mission_name')} "
          f"pos={(before.get('player') or {}).get('pos')}")

    if a.dry:
        if not a.no_focus:
            print(f"фокус окна игры: {'OK' if api.focus_and_settle() else 'НЕ ПОЛУЧИЛСЯ'}")
        script, total = api.build_restart_script(a.up, a.down, a.hold, a.open_gap,
                                                 a.gap, confirms=0, tail=a.tail)
        print(f"пробный прогон: pause → стрелки ({total} кадров)")
        sid = api.run_script(script, a.url)["script_id"]
        api.wait_script(sid, a.url, 30.0)
        print(f"после: menu={api.state(a.url).get('menu_status')} "
              f"(курсор доведён, confirm не отправлялся)")
        return 0

    res = api.restart_mission(a.url, focus=not a.no_focus, watch=a.watch,
                              ups=a.up, downs=a.down, hold=a.hold,
                              open_gap=a.open_gap, gap=a.gap, confirms=a.confirms,
                              confirm_gap=a.confirm_gap, tail=a.tail)
    if "reason" in res:
        print(f"ИТОГ: {res['reason']}")
        return 2
    for line in res["watch"]["timeline"]:
        print(f"    {line}")
    last = res["watch"]["last"]
    print(f"после: menu={last.get('menu_status')} "
          f"pos={(last.get('player') or {}).get('pos')}")
    if res["script_status"] != "done":
        print(f"ИТОГ: скрипт {res['script_status']}")
        return 2
    if res["ok"]:
        print("ИТОГ: рестарт выполнен (loading/пересоздание игрока)")
        return 0
    print("ИТОГ: рестарт НЕ подтверждён — возможно, курсор не на пункте Restart")
    return 1


if __name__ == "__main__":
    sys.exit(main())
