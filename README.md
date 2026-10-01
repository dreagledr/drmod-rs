# drmod-rs

Мод-инжектор и HUD-оверлей для **Metal Gear Rising: Revengeance**. Внедряет DLL в процесс игры, рендерит ImGui-оверлей поверх DirectX 9, читает игровую память в реальном времени.

- Сегментный автосплиттер с сохранением результатов в SQLite
- Призрак лучшего сегмента (ghost replay)
- Мультиплеер (TCP + UDP) — синхронизация позиций игроков
- Record/Replay ввода (TAS) с хранением кадров в SQLite
- HTTP API автоматизации на `127.0.0.1:5223` — скрипты ввода, состояние, логи, диагностика
- Скип in-engine катсцен «как на консоли» (в DLL)

## Зависимости

| Проект | Назначение |
|--------|------------|
| [hudhook](https://github.com/veeenu/hudhook) | DirectX 9 hooking и инжекция DLL (вендоренный форк 0.9.0 — `drmod-hudhook`) |
| [imgui-rs](https://github.com/imgui-rs/imgui-rs) | ImGui-биндинги для Rust (`imgui` 0.12) |
| `rusqlite` (0.40, bundled) | SQLite: прогоны сегментов и record/replay |
| `windows` (0.62), `windows-numerics` (0.3) | WinAPI (окна, D3D9, загрузка модулей) |
| `chrono` | Таймстампы прогонов |
| `serde` / `serde_json` | JSON протокола мультиплеера и HTTP API |
| `drmod-protocol` (`drmod-protocol/`) | Общие типы TCP/UDP-протокола |
| `drmod-replay-types` (`drmod-replay-types/`) | Общие replay-DTO (`InputUnit`/`PlayerState`/`CameraState`/`EnemyState`) |

## Документация

| Документ | О чём |
|----------|-------|
| [`QWEN.md`](QWEN.md) | Обзор проекта, архитектура, адреса памяти, заметки по фичам |
| [`docs/API.md`](docs/API.md) | HTTP API (`127.0.0.1:5223`), механика ввода (в т.ч. меню) |
| [`docs/HEADLESS.md`](docs/HEADLESS.md) | Headless-прогон: разбор главного цикла игры, снятие отрисовки без потери логики |
| [`docs/PHASE.md`](docs/PHASE.md) | Фазы/подфазы, катсценное меню |
| [`docs/ENEMY_TRACKING.md`](docs/ENEMY_TRACKING.md) | Запись состояния ближайшего врага |
| [`docs/REPLAY.md`](docs/REPLAY.md) | Дизайн Record/Replay и подачи ввода |
| [`docs/PITFALLS.md`](docs/PITFALLS.md) | **История:** грабли и «что не работает» |
| [`docs/SCRIPT_TUNING.md`](docs/SCRIPT_TUNING.md) | **История:** тюнинг core-скрипта (перелёт через барьер) |
| [`docs/REPLAY_FINDINGS.md`](docs/REPLAY_FINDINGS.md) | **История:** гипотезы и грабли ввода |
| [`docs/INPUT_STATUS.md`](docs/INPUT_STATUS.md) | **История:** статус команд ввода |
| [`docs/DESYNC_ANALYSIS.md`](docs/DESYNC_ANALYSIS.md) | **История:** анализ десинка Record→Playback |
| [`docs/REPLAY_CROSS_REVIEW.md`](docs/REPLAY_CROSS_REVIEW.md) | **История:** кросс-ревью replay |
| `tools/*/README.md` | Инструменты |

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

Корень — воркспейс: код мода (только библиотека) лежит в `drmod-core/`,
инжектор-бинарник — в `drmod-injector/`, общие крейты — в
`drmod-protocol/` и `drmod-replay-types/`, сервер — в `drmod-server/`. Из
корневой сборки исключены x64-крейты `drmod-dbdump/` и `drmod-script-gen/`
(arrow-rs только 64-битный) — они собираются из своей директории, где лежит
собственный `.cargo/config.toml`.

## Сборка артефактов (`cargo xtask`)

Упаковка релиза — в `xtask/`, а не в PowerShell-скриптах. Всё, что публикуется,
складывается в `out/` в корне:

| Команда | Что делает |
|---------|------------|
| `cargo xtask build` | `out/drmod-rs.zip` (лаунчер) и `out/drmod-asi.zip` (ASI-набор с загрузчиком) |
| `cargo xtask build-tools` | `out/dbdump.exe` (x64) и `out/dump-replay-input.exe` (i686) |
| `cargo xtask pack-editor` | `out/drmod-tas-editor/` и `out/drmod-tas-editor.zip` |
| `cargo xtask test-api` | смок-тест HTTP API (нужна запущенная игра с инжектированным модом) |
| `cargo xtask test-connect` | смок-тест сервера мультиплеера |

Подробности и грабли — `QWEN.md`, §Build automation.
