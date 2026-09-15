"""Замер ускорения от headless-режима (`POST /render`).

Прогоняет один и тот же мир в нескольких конфигурациях отрисовки и печатает
темп кадров движка и тиков симуляции — то, что реально определяет скорость
прогона TAS-скрипта.

Почему так: главный цикл игры делает **одну итерацию = один тик симуляции**
(`updateFrameTime` `0xA03970` → … → тик `0xA4F560` → `EndScene` `0xB9BE20` →
кадровый рендер `0x651080` → пацер `0xB98070` → `Present` `0xB97F90`), поэтому
снятая отрисовка ускоряет прогон, не меняя подачу кадров скрипта (она идёт по
тикам, `api::feed_tick`). Разбор — `docs/HEADLESS.md`.

Мир замера: фиксированный шаг (`/dt {"fixed":true}`) + снятый кап кадров
(`/fps {"cap":"off"}`) — так ускорение видно без оглядки на пацер игры; в конце
инструмент возвращает и отрисовку, и кап, который был до замера. По умолчанию
каждый выключатель меряется отдельно; `--only`/`--skip` сужают набор.

Боевой режим прогона (все выключатели + снятый кап + авто-возврат по концу
прогона скрипта) — это `POST /render {"headless": true}`; строка `all` тут даёт
то же ускорение, но кап снимает сам замер.

Запуск (игра должна быть запущена, мод инжектирован, игрок в геймплее):

    python tools/script_tuning/headless_bench.py --seconds 3
"""
import argparse
import sys
import time

import drmod_api as api


def apply_config(cfg):
    """Ставит конфигурацию отрисовки: cfg — набор из overlay/present/draw."""
    return api.render_skip(
        skip_overlay="overlay" in cfg,
        skip_present="present" in cfg,
        skip_draw="draw" in cfg,
    )


def restore_fps_cap(cap, base):
    """Возвращает кап кадров по строке из `/state` (`game` / `off` / `N fps`)."""
    if cap == "off":
        api.fps_cap(cap="off", base=base)
    elif cap.endswith(" fps"):
        api.fps_cap(fps=int(cap.split()[0]), base=base)
    else:
        api.fps_cap(cap="game", base=base)


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--seconds", type=float, default=3.0,
                   help="окно замера на конфигурацию, с (по умолчанию 3)")
    p.add_argument("--only", default=None,
                   help="мерить только эти конфигурации (через запятую: "
                        "base,overlay,present,draw,all)")
    p.add_argument("--no-world", dest="no_world", action="store_true",
                   help="не трогать /dt и /fps (мерить в текущем мире игры)")
    p.add_argument("--base", default=api.DEFAULT_URL)
    args = p.parse_args(argv)

    api.setup_stdout()
    api.http(args.base, "/health")  # падаем сразу, если API/игра недоступны

    configs = [
        ("base", frozenset()),
        ("overlay", frozenset({"overlay"})),
        ("present", frozenset({"present"})),
        ("draw", frozenset({"draw"})),
        ("all", frozenset({"overlay", "present", "draw"})),
    ]
    if args.only:
        wanted = {c.strip() for c in args.only.split(",") if c.strip()}
        unknown = wanted - {name for name, _ in configs}
        if unknown:
            print(f"неизвестные конфигурации: {', '.join(sorted(unknown))}")
            return 2
        configs = [(name, cfg) for name, cfg in configs if name in wanted]

    if not args.no_world:
        # Мир для замера: стабильный шаг и снятый кап — иначе пацер игры
        # упирает всё в 60 FPS и разница между конфигурациями исчезает.
        prev_cap = api.state(args.base)["fps_cap"]["cap"]
        api.fixed_dt(True, base=args.base)
        api.fps_cap(cap="off", base=args.base)
    else:
        prev_cap = None

    print(f"\nокно замера: {args.seconds:g} с на конфигурацию"
          f"{'' if args.no_world else ' (мир: /dt fixed + /fps off)'}\n")
    print(f"{'конфигурация':<12} {'кадров/с':>10} {'тиков/с':>9} {'×base':>7}")
    print("-" * 42)

    results = []
    try:
        for name, cfg in configs:
            api.render_skip(reset=True, base=args.base)
            if cfg:
                apply_config(cfg)
                # skip_draw ставит заглушки лениво, из render-цикла — даём кадр.
                time.sleep(0.5)
            st = api.state(args.base).get("render", {})
            frames, ticks = api.frame_rate(args.seconds, base=args.base)
            results.append((name, frames, ticks, st.get("draw_hooked", False)))
            print(f"{name:<12} {frames:>10.1f} {ticks:>9.1f}")
    except KeyboardInterrupt:
        print("\nпрервано")
    finally:
        # Возвращаем и отрисовку, и кап, который был до замера (иначе игра
        # осталась бы без капа — это уже не «побочный эффект замера»).
        api.render_skip(reset=True, base=args.base)
        cap_note = ""
        if prev_cap is not None:
            restore_fps_cap(prev_cap, args.base)
            cap_note = f", кап={api.state(args.base)['fps_cap']['cap']}"
        print(f"\nотрисовка возвращена (reset{cap_note})")

    if not results:
        return 1
    base_frames = results[0][1] or 1.0
    print("\nсводка (×base по кадрам движка):")
    for name, frames, ticks, hooked in results:
        note = ""
        if name == "draw":
            note = f"  заглушки: {'поставлены' if hooked else 'НЕ поставлены'}"
        print(f"  {name:<10} {frames / base_frames:>5.2f}×  ({frames:.1f} кадров/с, "
              f"{ticks:.1f} тиков/с){note}")
    print("\nбоевой режим (все выключатели + снятый кап + авто-возврат по концу "
          "прогона) — `POST /render {\"headless\": true}`; строка `all` выше — то же "
          "самое, но кап снимает сам замер.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
