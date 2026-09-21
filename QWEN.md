# drmod-rs

## Project Overview

A Rust-based mod injector and HUD overlay for **Metal Gear Rising: Revengeance**.

- **Binary (`drmod`)**: инжектит DLL в запущенный процесс игры
- **Library (`drmod_rs_lib`)**: DX9-хук + overlay на ImGui, читает память игры в реальном времени
- **Server (`server/`)**: мультиплеерный relay (axum 0.8 + tokio, Docker, 64-bit)
- **Protocol (`protocol/`)**: общие типы TCP (JSON) и UDP (бинарный `PositionPacket`)
- **Replay-types (`replay-types/`)**: общие replay-DTO (`InputUnit`/`PlayerState`/`CameraState`/`EnemyState`, `#[repr(C)]`) + `to_bytes`/`from_bytes` — on-disk layout replay-BLOB'ов; `input_bits` — биты действий в `InputUnit`; `key_codes` — кодировка игровых кодов клавиш в словах `m_aKeysDown` (порядок бит обратный: `0x8000_0000 >> (code & 31)`)
- **dbdump (`tools/dbdump/`)**: CLI-экспорт кадров Record/Replay из `runs.db` в CSV/Parquet (90 плоских колонок) + режим `--script` (запись → JSON-скрипт HTTP API)
- **TAS Editor (`tas-editor/`)**: десктопный TAS-редактор на WinUI 3 (`windows-reactor`, Rust, self-contained x64) — пока мок интерфейса
- **Мод `mods/cutscene_skip/`**: самостоятельный крейт — скип in-engine катсцены (лаунчер + встроенная DLL, без imgui/сети)

Features:
- Segment-based autosplitter with SQLite persistence and ghost replay
- Multiplayer position sync (TCP + UDP)
- World-to-screen projection (camera matrix, D3D viewport)
- Debug panel with live game state (debug builds only)
- HTTP automation API (`127.0.0.1:5223`): input scripts, game state, ring-buffer logs
- Headless-прогон (`POST /render`) — снять отрисовку (overlay/Present/геометрия игры), сохранив всю логику кадра
- TAS-рычаги воспроизводимости: фиксированный шаг времени (`POST /dt`), пин RNG решений ИИ (`POST /rng`), кап кадров (`POST /fps`)

Built with [hudhook](https://github.com/veeenu/hudhook) for DirectX hooking and [imgui-rs](https://github.com/imgui-rs/imgui-rs) for the UI.

## Карта документации

Разборы и хроники живут в `docs/` и README тулов — здесь только сводка и адреса.

| Тема | Где разобрано |
|------|---------------|
| HTTP API, пацер, кап кадров, скрипты, окно Settings | `docs/API.md` |
| Headless-прогон (`POST /render`), замеры ускорения | `docs/HEADLESS.md` |
| Record/Playback: механика ввода, этапы реализации | `docs/REPLAY.md` |
| Проверенные гипотезы ввода и грабли | `docs/REPLAY_FINDINGS.md` |
| Что найдено точкой останова на запись | `docs/REPLAY_CROSS_REVIEW.md` |
| Статус входов API (что ✅, что открыто) | `docs/INPUT_STATUS.md` |
| Десинк Record→Playback | `docs/DESYNC_ANALYSIS.md` |
| Враги: адреса, иерархия частей, ИИ и RNG | `docs/ENEMY_TRACKING.md` |
| Фазы/подфазы, консольное меню со Skip | `docs/PHASE.md` |
| Lightning strike (`вперёд-вперёд-хэви`, `anim 110`) | `docs/LIGHTNING_STRIKE.md` |
| Тюнинг core-скрипта (хроника, «что не сработало») | `docs/SCRIPT_TUNING.md` |
| Сводка «что не работает» по проекту | `docs/PITFALLS.md` |
| dbdump: колонки, `--script` | `tools/dbdump/README.md` |
| script_tuning: инструменты, эталонный рецепт | `tools/script_tuning/README.md` |
| Мод cutscene_skip: лаунчер, флаги, статус | `mods/cutscene_skip/README.md` |
| TAS Editor: интерфейс, поставка, грабли Reactor | `tas-editor/README.md` |

## Architecture

```
src/
├── main.rs          # Injector binary — finds game process, injects DLL
├── lib.rs           # HUD library — DX9 hook, ImGui overlay, game memory, main loop
├── api.rs           # HTTP API (127.0.0.1:5223) — scripts, state, ring-buffer logs
├── segment.rs       # Segment tracking — start conditions, ASL-based finish triggers, DB cleanup
├── ui.rs            # ImGui windows — debug panel (debug only), multiplayer, settings
├── game/            # Сущности игры — игрок (Pl0000), камера (cCameraGame), статус меню, фазы, скип катсцены
│   ├── mod.rs       #   GameMenuStatus enum, is_readable_ptr, re-export Player/Camera/phase/cutscene_skip
│   ├── player.rs    #   Player — кэш объекта игрока, read_player_state/read_current_input/read_pl_input/read_enemies/read_skeleton
│   ├── camera.rs    #   Camera — read_camera_state/view_proj/pos
│   ├── phase.rs     #   Фазы/подфазы: hash_name + order_subphase (заказ смены подфазы = скип катсцены)
│   └── cutscene_skip.rs #  Скип in-engine катсцены «как на консоли» (кадровый автомат в render-цикле)
├── net.rs           # TCP + UDP client for multiplayer
├── overlay.rs       # world_to_screen projection, draw_world_pos
├── render_hooks.rs  # Headless-режим (`POST /render`): флаги скипа overlay/Present/геометрии игры + MinHook-заглушки DrawPrimitive* на vtable устройства
├── settings.rs      # User settings (ghost opacity, show ghost toggle, cutscene skip toggle)
├── d3d_render.rs    # CylinderRenderer, SphereRenderer for 3D overlays
├── skeleton.rs      # Bone/skeleton data structures
├── logger.rs        # Logging to %LOCALAPPDATA%\drmod\ (debug.log + buffered state.log)
├── tas/             # TAS (tool-assisted speedrun) — input record/replay
│   ├── addresses.rs #   Input memory addresses/constants
│   ├── db.rs        #   Replay SQLite tables, миграция колонок + bulk insert
│   ├── replay.rs    #   Record/playback logic, input override
│   ├── hooks.rs     #   MinHook input hooks (updateInputUnit/isKeybindPressed/isKeybindDown), обновитель кадра, randRange/randFloat, ripper/blade emulation, raw input readers
│   ├── watch.rs     #   (debug) аппаратная точка останова на запись: DR0 на все потоки + VEH, стек-чейн писателя
│   └── types.rs     #   Re-export DTO из replay-types + ReplayFrame/внутренние типы
server/              # Multiplayer server (axum 0.8 + tokio, 64-bit, Docker)
protocol/            # Shared protocol types (TCP JSON + UDP binary PositionPacket)
replay-types/        # Общие replay-DTO (InputUnit/PlayerState/CameraState/EnemyState) + to_bytes/from_bytes + input_bits
tools/
├── demo/            # Демонстрация TAS: demo_r03_tas.py — прогон первого сегмента R-03 3 раза подряд (флаги `--headless`/`--uncapped`)
├── dbdump/          # Экспорт replay-кадров в CSV/Parquet + --script (JSON для HTTP API) (x64, отдельный .cargo/config.toml)
├── desync_analysis/ # pandas-скрипты анализа десинка Record→Playback (CSV от dbdump)
├── script_tuning/   # Тайминги core-скрипта 117 (+ timing_tune.py, blade_tap_tune.py, lightning_strike.py, r03_baseline.py, headless_bench.py) и скип катсцен (cutscene_skip.py, order.py, order_on_event.py, subphase_now.py) (python)
└── disasm/          # Дизассемблирование: scan_srm.py (PE + поиск обращений к SRM), disasm.py (обёртка llvm-objdump по RVA), find_vtable.py (RTTI→vtable), find_strings.py, peek.py, mem_find_u32.py
mods/
└── cutscene_skip/   # Самостоятельный мод: скип in-engine катсцены (лаунчер + встроенная DLL, без imgui); свой [workspace]
tas-editor/          # TAS Editor: десктопный редактор на WinUI 3 (windows-reactor), self-contained; свой [workspace] и x64-конфиг
ref/                 # Git submodules — read-only reference projects
```

## Memory Offsets

All addresses are relative to the game module base (`GetModuleHandleA(null)`).

### Player object (Pl0000)

Static pointer: `base + 0x177B4A4` → dereference to get player object.

| Offset | Type | Field |
|--------|------|-------|
| `0x50` | `f32` | Position X |
| `0x54` | `f32` | Position Y |
| `0x58` | `f32` | Position Z |
| `0x870` | `i32` | Current HP |
| `0xB74` | `i32` | Sword hidden flag |
| `0x13FC` | `i32` | Sword state |

### PlayerManagerImplement

Static pointer: `base + 0x17EA100`.

| Offset | Type | Field |
|--------|------|-------|
| `0xE0` | `i32` | Main weapon ID |
| `0xE4` | `i32` | Custom weapon ID |
| `0xE8` | `i32` | Sub weapon ID |

### Game state

| Address | Type | Field |
|---------|------|-------|
| `base + 0x17E9F9C` | `i32` | GameMenuStatus (enum 0–18; 1 = InGame, 3 = PauseMenu, 6 = CutscenePause (`cEventPauseMenu`, консольное меню со Skip), 8 = Mission Fail, 12 = Pause1 (сразу ставит 1 = InGame, обработчика нет), 18 = ProcessOutOfPause; таблица статусов и разбор — `docs/PHASE.md`) |
| `base + 0x1764670` | `i32` | Current mission ID |
| `base + 0x1764674` | `*const i8` | Current mission name string |
| `base + 0x14B9181` | `*const i8` | gStr — game location string |
| `base + 0x14B91AD` | `*const i8` | gStr2 — game location string 2 |
| `base + 0x14B91A8` | `*const i8` | gStr4 — mission identifier ("P118", "EV60", etc.) |
| `base + 0x19D0814` | `u32` | Состояние глобального LCG решений ИИ (`randRange` `0x9DE2A0` / signed `0x9DE2D0` / `randFloat` `0x9DE300`; `state = state*214013 + 2531011`, берётся `state>>16`) — рычаг воспроизводимости, `POST /rng` |

### Frame pacer (кап кадров, `POST /fps`)

FPS держит **софтовый пацер**, а не vsync. Описание механизма, замеры ускорения и режимы — `docs/API.md` §3.13.

| Адрес | Тип | Field |
|-------|-----|-------|
| `base + 0x1B206EC` | `u32` | **Период кадра пацера** в единицах 3·мс: `3000·period_с` → `50` = 1/60 с (60 FPS, геймплей), `100` = 1/30 с (30 FPS, меню/ролики). `0` → пацер не ждёт |
| `base + 0x1B206F0` | `u32` | Метка прошлого кадра (те же 3·мс) |
| `base + 0x1B206D0` | `u32` | Режим кадра: `1` = 30 FPS, `0` = 60 FPS |
| `base + 0x1B206D4` | `*mut` | Живое `IDirect3DDevice9` (vtable — цель заглушек `skip_draw`); `IDirect3D9` — `base + 0x1B206D8`; оконные `D3DPRESENT_PARAMETERS` — `base + 0x1B20620`, фуллскрин — `base + 0x1B205E8` |
| `0xB98070` | fn | **Пацер** (зовётся из главного цикла `0xB9D650`); `Present` — `0xB97F90` (vtable `+0x44`) |
| `0xB98AD0` | fn | Сеттер периода (тот же код — `0xB98140`): `mode 1` → 1/30, иначе 1/60 |
| `.rdata` | const | `0.016666667` (1/60), `0.033333335` (1/30); `[0x16B6980]` = `3.0`, `[0x16C5358]` = `1000.0` — множители «сек → 3·мс» |

Мод (`api::apply_fps_cap`, вызов в начале render) перезаписывает `[0x1B206EC]` **каждый кадр**: render идёт внутри `Present`, то есть после сеттера и до ожидания пацера. Вместе с `POST /dt {"fixed":true}` снятый кап даёт ускорение прогонов (замер 2026-09-13: `period=0` → 238 кадров движка в секунду, 3.96× реального времени; кап игры → 58.9 и 0.98×).

### Camera

Static pointer: `base + 0x17EA1D0` (cCameraGame::Instance).

| Offset | Type | Field |
|--------|------|-------|
| `0x200` | `[f32; 16]` | View-projection matrix |

### Enemies (debug-панель, `read_enemies`)

Сущности сцены — из `EntitySystem`: `ms_Instance` = `base + 0x17E9A98`, список `m_EntityList` на `+0x38` (size `+0x0C`, первый узел `+0x14`, обход по `+0x08`). У `Entity`: имя `+0x04`, Behavior (m_pSceneModel) `+0x3C`, m_pInstance `+0x48`; у Behavior: позиция `+0x50`, HP `+0x870`, анимация `+0x618`, номер кадра анимации `+0x8B4`. Фильтр врагов: имена `Em*`/`Ba*`/`Pl001*` (без игрока), позиция не (0,0,0), HP 1..1 000 000.

Анимацию врага пишет сеттер `0x68CAF0`, его зовёт стейт-машина действия `0x739F00` (селектор `0x745C60`, диспетчер `0x740880`); единой «функции решения об атаке» нет, выбор действия случаен — глобальный LCG, состояние `base + 0x19D0814` (`POST /rng`). Полный разбор (иерархия частей, bladeY, сайты RNG, отсутствие vtable-геттера) — `docs/ENEMY_TRACKING.md`.

### Animation (Raiden)

3-level pointer chain: `base + 0x019C14C4 → +0x788 → +0x618`

| Level | Type | Field |
|-------|------|-------|
| Final | `i32` | rAnim — Raiden's current animation ID |

## Segment Tracking

### Start conditions

Position-gated: `START_CONDITIONS` table in `segment.rs`. Each mission has a spawn position with ±0.1m (XY) / ±1.0m (Y) tolerance.

### Finish conditions (ASL-based)

Derived from [livesplit_asl_mgrr](https://github.com/hau5test/livesplit_asl_mgrr) reference. Use gStr/gStr2/rAnim — no InMenu waiting:

| Mission | Trigger |
|---------|---------|
| R-00 | `gstr2: "" → "BEACH"` && `gstr == ""` |
| R-01 | `gstr: "MISTRAL03" → "MIST_RESU"` |
| R-02 | `gstr: "EVENT2"` + `rAnim` transition to 43 |
| R-03 | `gstr: "FINISH_QT" → "MON_RESUL"` |
| R-04 | `gstr: "QTE" → "SUN_RESUL"` |
| R-05 | `gstr: "STREET" → ""` |
| R-06 | `gstr: "BOSS" → "BOSS_END"` |
| R-07 | `rAnim: 70 → 297` (Armstrong QTE) |

### Database

SQLite at `%LOCALAPPDATA%\drmod\runs.db` (схемы и миграции — `src/tas/db.rs`).

- **Сегменты:** `runs` → `segments` (`mission_id`, `mission_name`, `started_at`, `duration_ms`) → `segment_positions` (позиция на кадр). При flush на `mission_id` остаётся только лучший (минимальный `duration_ms`) сегмент; ghost читает его через `load_best_ghost()`. WAL + NORMAL synchronous для быстрой bulk-вставки.
- **Record/Playback:** `replay_runs` (`kind` = `record`/`playback`, `source_replay_id` для playback) → `replay_record_frames` / `replay_playback_frames` (по кадру: `input_unit` BLOB 48 Б, `state` 88 Б, `camera` 92 Б, `enemy` 32 Б, `blade_down`, `ripper_pressed`, `raw_down`, `raw_pressed`). BLOB'ы — сырые байты структур `replay-types/`, layout версионируется размером (camera 76 Б = legacy до 2026-08-18, не читается dbdump). Миграция старых БД — `ensure_replay_frame_columns` (`ALTER TABLE`).
- Мод БД не читает (воспроизведение идёт из памяти сессии) — таблицы только для истории/аналитики; экспорт — `tools/dbdump`.

## Building and Running

### Prerequisites

- Rust toolchain with `i686-pc-windows-msvc` target (32-bit MSVC)
- MSVC C++ build tools

### Build

```bash
cargo build --release
```

The project is configured to compile for `i686-pc-windows-msvc` (32-bit), as specified in `.cargo/config.toml`. This is required because MGR:R is a 32-bit application.

### Run

```bash
cargo run --release
```

Or with a custom window name:

```bash
cargo run --release -- -n "Custom Window Name.exe"
```

### Output

- `target/i686-pc-windows-msvc/release/drmod.exe` — injector binary (DLL embedded via `include_bytes!`, extracted to `%TEMP%` at runtime)

### Smoke tests

- `test_connect.ps1` — multiplayer server (TCP connect/disconnect, dashboard)
- `test_api.ps1` — HTTP API (health/state/script run+get+stop/logs, error paths, 20 parallel requests, optional `-Eject` final step that unloads the DLL); requires the game running with the mod injected

## Development

### Commit Rules

- **Перед каждым коммитом** проверять актуальность `QWEN.md`: если изменения затрагивают архитектуру, зависимости, новые модули, референсы, или любую информацию из этого файла — обновить соответствующие секции (детали — в `docs/`, сюда добавлять только сводку и ссылку).

### Language & Edition

- Rust 2024 edition
- UI messages are in Russian

### Dependencies

| Crate | Purpose |
|-------|---------|
| `hudhook` (0.9.0, вендоренный форк — `vendor/hudhook`) | DirectX hooking and injection |
| `imgui` (0.12.0) | ImGui bindings for UI rendering |
| `windows` (0.62.2) | Windows API (UI windows, module loading) |
| `windows-numerics` (0.3) | Векторная/матричная математика для D3D-проекций |
| `rusqlite` (0.40.1, bundled) | SQLite for persisting run data |
| `chrono` (0.4.45) | Time formatting for run timestamps |
| `serde` / `serde_json` (1) | JSON serialization for multiplayer protocol and HTTP API |
| `drmod-protocol` | Shared types for client-server communication |
| `drmod-replay-types` | Shared replay DTOs (`InputUnit`/`PlayerState`/`CameraState`/`EnemyState`) + `to_bytes`/`from_bytes` + `input_bits` (биты InputUnit) |

Тул `tools/dbdump` дополнительно тянет (только для него, x64): `rusqlite`, `csv`, `arrow` + `parquet` (59.x) — экспорт в CSV/Parquet; `serde` + `serde_json` — режим `--script` (JSON для HTTP API).

### Notes

- **Thread safety**: `HelloHud` has `unsafe impl Send/Sync` because hudhook requires it for the render loop. This is safe since addresses are computed once in `new()` and never mutated. Все статические адреса считаются один раз при init, не покадрово.
- **Debug-only features** (`#[cfg(debug_assertions)]`): `DrmodDebug` window (record/playback status, segment timer, mission/menu status, compact player state), `Actions` window (numpad hotkey reference), numpad-хоткеи, saved position. Release builds keep only Multiplayer and Settings windows.
- **Окно Settings как пульт TAS-настроек** (`src/ui.rs` → `render_tas_controls`, обе сборки): фиксированный `dt`, пин RNG, кап кадров и три чекбокса headless — прямо в окне настроек. Контролы пишут тот же runtime-стейт, что и HTTP-ручки, через общие сеттеры (`api::set_fixed_dt`/`set_fixed_dt_ms`/`set_rng_pin`/`set_fps_cap`, `render_hooks::set_skip`), поэтому с API не рассинхронизируются. Детали — `docs/API.md` §2.
- **Headless-прогон** (`src/render_hooks.rs`, `POST /render`): три независимых выключателя — `skip_overlay`, `skip_present`, `skip_draw` (MinHook-заглушки `DrawPrimitive*` на vtable живого устройства). Боевой режим = `skip_overlay` + `skip_draw` + снятый кап; ⚠️ `skip_present` в него не входит — выигрыша не даёт, а со `skip_draw` роняет игру (AV в `d3d9.dll`). Заглушки дают ≈×1.25 к снятому капу и ≈×1.5 к обычному прогону, дальше упор в симуляцию (~9.7 мс/тик). ⚠️ При `skip_overlay` скрыто и окно Settings — вернуть отрисовку только `POST /render {"reset": true}`. Дизайн, падение и изоляция — `docs/HEADLESS.md`.
- **HTTP API** (`src/api.rs`, debug + release): собственный минимальный HTTP-сервер (raw `TcpListener`, без tiny_http) на `127.0.0.1:5223` — однопоточный, non-blocking accept, таймауты 1 с, `shutdown()` завершается за ограниченное время (eject не виснет). Ручки: `/script/run`, `/script/stop`, `/script/{id}`, `/state`, `/logs`, `/health`, `/eject`, `/order`, `/phase`, `/dt`, `/fps`, `/rng`, `/render`, `/watch`. Ring buffer 3600 кадров (60 с). **Кадр скрипта = тик симуляции** (подача из детура `updateInputUnit`, `api::feed_tick`). Полная спецификация — `docs/API.md`.
- **Скип катсцены «как на консоли»** (`src/game/cutscene_skip.rs`; включён по умолчанию, выключается галочкой в Settings): в сценах `P370_RESTART`/`P370_IN` держит в `staFlags` (`base + 0x17EA060`) флаги консольного меню, читает решение по объекту меню (`base + 0x17EA140`), убирает меню штатным путём движка (статус 6 + шаг `base + 0x17EA118` = 6), снимает `STA_PAUSE` и по подтверждённому SKIP заказывает `P370_EVENT`. ⚠️ Заказ подфазы грузит сцену только без паузы; ⚠️ пока флаги держатся, обычное меню паузы в сцене не откроется. Этап виден в `GET /state` → `cutscene_skip` (`off`/`armed`/`closing`/`skipped`). Разбор — `docs/PHASE.md`, порт — `mods/cutscene_skip/`.
- **Десинк Record→Playback** (`docs/DESYNC_ANALYSIS.md`): главный источник — **лаг подачи ввода на 1 кадр** (override из render(K) применяется тиком K+1, `play[fi]==rec[fi-1]` на 100%) + фазовая неопределённость Present↔тик. Решение: подача кадров из детура `updateInputUnit` (`replay::PLAYBACK_FEED` + `feed_playback`) + компенсация курса (`rsx_correction`) → **record 110 → 111/112/113/114: 4/4 успех, |Δpos| 0.7–1.0 м, |Δyaw| медиана 0.12–0.26°**.
- **Несколько копий игры на одной машине — нельзя**: вторая копия MGR:R умирает с кодом 0 через ~100 мс после загрузки `steam_api.dll`; сейвы — Steam Cloud, один файл на пару (steamid, appid). Разбор и варианты изоляции — `docs/PITFALLS.md`.
- **dbdump** (`tools/dbdump/`): экспорт кадров Record/Playback в CSV/Parquet (90 плоских колонок, включая ближайшего врага `enemy_*`) + режим `--script` (запись → JSON-скрипт для `POST /script/run`). Сборка — x64 в своей директории (arrow-rs только 64-bit, корневой `cargo build` тул не собирает). Детали — `tools/dbdump/README.md`.
- **script_tuning** (`tools/script_tuning/`, python): тайминги core-скрипта для перелёта барьера в `P310_RESTART`, ускорение прогонов (`/dt`+`/fps`), автоматика рестарта/меню/fail-recovery, скип катсцен. Инструменты и эталонный рецепт — `tools/script_tuning/README.md`; хроника и «что не сработало» — `docs/SCRIPT_TUNING.md`; грабли (креши DLL, фокус окна, UTF-8, «две копии игры») — `docs/PITFALLS.md`.
- **Мод `mods/cutscene_skip/`** (самостоятельный крейт: свой `[workspace]` и `target/`, в корневой workspace не входит): порт скипа катсцены без imgui/hudhook-dx9/API/сети. Лаунчер `cutscene_skip.exe` (ищет игру, инжектит или стартует; флаги `--kill-first`, `--no-launch`, `--follow`, `--timeout`) + встроенная DLL; пер-кадровая точка — MinHook на `updateFrameTime` (`0xA03970`). Детали, статус и вынос в отдельный репозиторий — `mods/cutscene_skip/README.md`.
- **TAS Editor `tas-editor/`** (самостоятельный крейт: свой `[workspace]`, `target/` и `.cargo/config.toml` с `x86_64-pc-windows-msvc` — корневой форсит i686, а WinUI 3 под него не собирается; лежит на уровне репо, не в `tools/`): WinUI 3 через `windows-reactor` **0.100**, декларативные компоненты без XAML, **self-contained** через `windows-reactor-setup` в `build.rs`. Запуск — `cd tas-editor && cargo run --release`; поставка — `pwsh -File pack.ps1 -Build -Zip` (~56 МБ, zip ≈20 МБ). Статус — мок интерфейса (список скриптов, полоса свойств, таймлайн-матрица, JSON-редактор); рабочая область на диске (`tas-editor/src/workspace.rs`) не подключена. ⚠️ Грабли self-contained (обрезанный `.nupkg` = «зелёная» сборка без runtime) и рендера Reactor — `tas-editor/README.md`.
- Error handling uses Windows `MessageBoxW` for user-facing errors.
- Library is compiled as both `cdylib` (for injection) and `rlib` (for the binary to link against).

### Reference Projects

In `ref/` as git submodules (read-only):

| Project | Source | For |
|---------|--------|-----|
| `mgr-plugin-sdk` | [Frouk3/mgr-plugin-sdk](https://github.com/Frouk3/mgr-plugin-sdk) | 529 reverse-engineered game headers (GPLv3) |
| `livesplit_asl_mgrr` | [hau5test/livesplit_asl_mgrr](https://github.com/hau5test/livesplit_asl_mgrr) | Reference autosplitter for checkpoint verification |
| `MGR-RedTrainer` | [Baromir19/MGR-RedTrainer](https://github.com/Baromir19/MGR-RedTrainer) | Reference trainer (C++) |
| `mmultiplayer` | [softsoundd/mmultiplayer](https://github.com/softsoundd/mmultiplayer) | Reference multiplayer mod for Mirror's Edge |

### Input Handling

The overlay supports keyboard input via hudhook's built-in WndProc hook — it intercepts `WM_KEYDOWN`/`WM_KEYUP` messages from the game window and feeds them to imgui-rs through `Io::add_key_event()`.

**Debug bindings** (`#[cfg(debug_assertions)]` only):

| Key | Action |
|-----|--------|
| `NumPad1` | +10m to player Y coordinate (direct memory write) |
| `NumPad2` | Save current position |
| `NumPad3` | Teleport to saved position |
| `NumPad4` | Script: run → jump → light attack → camera turn |
| `NumPad5` | Toggle record (arm → position trigger) |
| `NumPad6` | Toggle playback (arm → position trigger) |
| `NumPad7` | Emulate R (ripper) via isKeybindPressed, 1 frame |
| `NumPad8` | Blade mode (hold) toggle |

Memory writes use raw `*mut f32` pointers — since the DLL is injected, it has direct access to game memory.

### Multiplayer Protocol

- **TCP** (port 5222): JSON messages with `\0` delimiter — connect, disconnect, player list
- **UDP** (port 5222): Binary `PositionPacket` (28 bytes) — position + mission_id + HP, sent every frame
- Client: blocking TCP in separate `std::thread`, non-blocking UDP in render frame
- Server: axum-based, relays UDP to all clients in the same room
- Room concept: lobbies, no mission filtering on server side
