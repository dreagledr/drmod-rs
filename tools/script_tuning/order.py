# -*- coding: utf-8 -*-
"""Ручной заказ подфазы через `POST /order` (штатная функция движка).

    py -3 tools\\script_tuning\\order.py P370_EVENT                  # снять STA_EVENT и заказать
    py -3 tools\\script_tuning\\order.py P370_EVENT --no-clear-event # без снятия флага (в событии проигнорируется)
    py -3 tools\\script_tuning\\order.py P370_EVENT --arg 1
"""
import argparse
import json
import sys
import time

import drmod_api as api


def main(argv=None):
    p = argparse.ArgumentParser()
    p.add_argument("name")
    p.add_argument("--arg", type=lambda s: int(s, 0), default=1)
    p.add_argument("--no-clear-event", dest="clear_event", action="store_false")
    p.set_defaults(clear_event=True)
    a = p.parse_args(argv)
    api.setup_stdout()
    body = {"name": a.name, "arg": a.arg, "clear_event": a.clear_event}
    print("заказ:", body)
    print("ответ:", api.http(api.DEFAULT_URL, "/order", "POST", body))
    for i in range(8):
        time.sleep(2)
        st = api.state()
        pl = st.get("player") or {}
        print(f"  [{2 * (i + 1):2d}s] menu={st.get('menu_status')} "
              f"phase={st.get('mission_name')} pos={pl.get('pos')} "
              f"found={pl.get('found')}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
