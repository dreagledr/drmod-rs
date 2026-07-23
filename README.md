# drmod-rs

Мод-инжектор и HUD-оверлей для **Metal Gear Rising: Revengeance**. Внедряет DLL в процесс игры, рендерит ImGui-оверлей поверх DirectX 9, читает игровую память в реальном времени.

- Сегментный автосплиттер с сохранением результатов в SQLite
- Призрак лучшего сегмента (ghost replay)
- Мультиплеер (TCP + UDP) — синхронизация позиций игроков

## Зависимости

| Проект | Назначение |
|--------|------------|
| [hudhook](https://github.com/veeenu/hudhook) | DirectX 9 hooking и инжекция DLL |
| [imgui-rs](https://github.com/imgui-rs/imgui-rs) | ImGui-биндинги для Rust |

## Референс-проекты

В `ref/` лежат git submodule — read-only референсы для анализа и сверки:

| Проект | Источник | Для чего |
|--------|----------|----------|
| [mgr-plugin-sdk](https://github.com/Frouk3/mgr-plugin-sdk) | `ref/mgr-plugin-sdk/` | 529 reverse-engineered заголовков игры |
| [livesplit_asl_mgrr](https://github.com/hau5test/livesplit_asl_mgrr) | `ref/livesplit_asl_mgrr/` | Эталонный автосплиттер для сверки чекпойнтов |
| [MGR-RedTrainer](https://github.com/Baromir19/MGR-RedTrainer) | `ref/MGR-RedTrainer/` | Референсный трейнер на C++ |
| [mmultiplayer](https://github.com/softsoundd/mmultiplayer) | `ref/mmultiplayer/` | Мультиплеерный мод Mirror's Edge |

## Клонирование

```bash
git clone --recurse-submodules https://github.com/dreagledr/drmod-rs.git
```

## Сборка

```bash
cargo build --release
```

Требуется Rust с таргетом `i686-pc-windows-msvc` (32-bit MSVC).
