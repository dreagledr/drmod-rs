# drmod-rs

## Project Overview

A Rust-based mod injector and HUD overlay for **Metal Gear Rising: Revengeance**. The project consists of:

- **Binary (`drmod`)**: Injects a DLL into the running game process
- **Library (`drmod_rs_lib`)**: Hooks into DirectX 9 to render an ImGui overlay that reads game memory in real-time
- **Server (`server/`)**: Multiplayer relay server (axum 0.8 + tokio, Docker, 64-bit)
- **Protocol (`protocol/`)**: Shared types for TCP (JSON) and UDP (binary) communication
- **Replay-types** (`replay-types/`): Shared replay DTOs (`InputUnit`/`PlayerState`/`CameraState`/`EnemyState`, `#[repr(C)]`) + `to_bytes`/`from_bytes` — on-disk layout replay BLOB'ов; `input_bits` — биты действий в `InputUnit` (общие для мода и dbdump); `key_codes` — кодировка игровых кодов клавиш в словах `m_aKeysDown` (порядок бит обратный: `0x8000_0000 >> (code & 31)`)
- **dbdump (`tools/dbdump/`)**: CLI-экспорт кадров Record/Replay из `runs.db` в CSV/Parquet (90 плоских колонок) для аналитики + режим `--script` (запись → JSON-скрипт HTTP API)

Features:
- Segment-based autosplitter with SQLite persistence and ghost replay
- Multiplayer position sync (TCP + UDP)
- World-to-screen projection (camera matrix, D3D viewport)
- Debug panel with live game state (debug builds only)
- HTTP automation API (127.0.0.1:5223) — input scripts, game state, ring-buffer logs (design: `docs/API.md`)
- Headless-прогон (`POST /render`) — снять отрисовку (overlay/Present/геометрия игры), сохранив всю логику кадра (design: `docs/HEADLESS.md`)

Built with [hudhook](https://github.com/veeenu/hudhook) for DirectX hooking and [imgui-rs](https://github.com/imgui-rs/imgui-rs) for the UI.

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
│   └── cutscene_skip.rs #  Скип in-engine катсцены «как на консоли»: держит флаги консольного меню в P370_*, по подтверждённому пункту убирает меню (статус 6 + шаг 6), снимает STA_PAUSE и заказывает P370_EVENT (кадровый автомат, живёт в render-цикле)
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
├── demo/            # Демонстрация TAS: demo_r03_tas.py — готовый прогон первого сегмента R-03 (пресет ls8) 3 раза подряд, с флагами `--headless`/`--uncapped` (снять отрисовку и кап кадров, печатая темп до/после)
├── dbdump/          # Экспорт replay-кадров в CSV/Parquet + --script (JSON для HTTP API) (x64, отдельный .cargo/config.toml)
├── desync_analysis/ # pandas-скрипты анализа десинка Record→Playback (CSV от dbdump)
├── script_tuning/   # Тайминги core-скрипта 117 (+ timing_tune.py: мир freeze+ticks; blade_tap_tune.py: свип кадра тапа Blade Mode; lightning_strike.py: приём «вперёд-вперёд-хэви»; r03_baseline.py: R-03 TAS baseline — барьер → BM-стойка → 110 → кансел → 94 → риппер; headless_bench.py: замер ускорения от `POST /render`) и скип катсцен (cutscene_skip.py: консольное меню → наш скип; order.py/order_on_event.py: заказ подфазы; subphase_now.py) (python)
└── disasm/          # Дизассемблирование: scan_srm.py (PE + поиск обращений к SRM), disasm.py (обёртка llvm-objdump по RVA), find_vtable.py (RTTI→vtable), find_strings.py, peek.py, mem_find_u32.py
mods/
└── cutscene_skip/   # Самостоятельный мод: скип in-engine катсцены (лаунчер + встроенная DLL, без imgui); свой [workspace]
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

FPS держит **софтовый пацер**, а не vsync (в оконном `D3DPRESENT_PARAMETERS` по
`base + 0x1B20620` стоит `PresentationInterval = D3DPRESENT_INTERVAL_IMMEDIATE`;
рядом `base + 0x1B205E8` — фуллскрин 3840×2160@119 с `INTERVAL_ONE`; устройство —
`base + 0x1B206D4`, `IDirect3D9` — `base + 0x1B206D8`):

| Адрес | Тип | Field |
|-------|-----|-------|
| `base + 0x1B206EC` | `u32` | **Период кадра пацера** в единицах 3·мс: `3000·period_с` → `50` = 1/60 с (60 FPS, геймплей), `100` = 1/30 с (30 FPS, меню/ролики). `0` → пацер не ждёт |
| `base + 0x1B206F0` | `u32` | Метка прошлого кадра (те же 3·мс) |
| `base + 0x1B206D0` | `u32` | Режим кадра: `1` = 30 FPS, `0` = 60 FPS |
| `0xB98070` | fn | **Пацер**: `Present` → `elapsed = now − [0x1B206F0]` → пока `elapsed < [0x1B206EC]`, `Sleep(остаток/3)` + перепроверка по QPC → `[0x1B206F0] = now`. Зовётся только из главного цикла (`0xB9D650`), `Present` — `0xB97F90` (vtable `+0x44`) |
| `0xB98AD0` | fn | Сеттер периода (тот же код — `0xB98140`): `mode 1` → 1/30, иначе 1/60; кладёт `[0x1B206D0]` и `[0x1B206EC]`. Зовётся из главного цикла (`0xA52510`) сразу после `updateFrameTime` |
| константы | `.rdata` | `0.016666667` (1/60) и `0.033333335` (1/30); `[0x16B6980]` = `3.0`, `[0x16C5358]` = `1000.0` — множители «сек → 3·мс» |

Режим 30 FPS включается по флагам: `staFlags(+0x24) & 0x200000` либо
`(staFlags(+4) & 0x4000) && (staFlags & 0x40000000)`. Из-за перекоса `Sleep`
(~1 мс) кап даёт 56–58 FPS вместо ровных 60, а кадры квантуются 16.67/33.3 мс.
Мод (`api::apply_fps_cap`, вызов в начале render) перезаписывает `[0x1B206EC]`
**каждый кадр**: render идёт внутри `Present`, то есть после сеттера и до
ожидания пацера. Вместе с `POST /dt {"fixed":true}` (кадр = ровно 1/60 с
симуляции) снятый кап даёт **ускорение прогонов**: замер живьём 2026-09-13
(оконное 800×600, i7-1355U/Iris Xe, кап менялся на ходу) — `period=0` → **238
кадров движка в секунду и символьное время 3.96× реального**, `period=50`
(кап игры, 60 FPS) → 58.9 и 0.98×, `period=150` (`{"fps":20}`) → 19.8 и 0.33×;
отдельно `period=3000` (искусственно 1 с) → движок намерил кадр 1001 мс. Темп
кадров надо считать по дельте `dt.frames` (или `sim_ticks`), а не по полю `fps`:
оно было средним за сессию и при снятом капе показывало 40 вместо 238 —
исправлено на темп по кадрам буфера за последние 500 мс (`FPS_WINDOW_MS`);
проверено живьём на debug-сборке: при `period=0` `fps` = 176 при
`dt.frames/с` = 185 (release на том же месте давал 238).

### Camera

Static pointer: `base + 0x17EA1D0` (cCameraGame::Instance).

| Offset | Type | Field |
|--------|------|-------|
| `0x200` | `[f32; 16]` | View-projection matrix |

### Enemies (debug-панель, `read_enemies`)

Сущности сцены — из `EntitySystem` (SDK `ref/mgr-plugin-sdk` + disasm):

- `EntitySystem::ms_Instance` = `base + 0x17E9A98`; список сущностей `m_EntityList`
  (`Hw::cFixedList<Entity*>`) на `+0x38` (size `+0x0C`, первый узел `+0x14`,
  обход по `node->m_next` `+0x08`).
- `Entity`: имя `+0x04`, Behavior* (m_pSceneModel) `+0x3C` (disasm `Entity::getTransPos`
  0x67C8B0: `mov eax,[ecx+0x3C]`), m_pInstance `+0x48` (disasm `getEntityInstance` 0x67C8A0).
- У Behavior: позиция `+0x50` (cParts::m_vecTransPos), HP `+0x870`, r_anim `+0x618`.
- Фильтр врагов: имена `Em*`/`Ba*`/`Pl001*` (без игрока по кэшу объекта игрока в `Player`),
  позиция не (0,0,0), HP 1..1 000 000. Части моделей (`Pl0010_Hair` и т.п.) и фон
  (`byBgManager`, пустые имена) отсекаются позицией/HP.
- Анимация врага — подтверждено рантаймом (диагностика полей): `+0x618` — текущая
  анимация (ID меняются при смене: 65536/65540/14/23/19 — свои у EmSetCorps, не как
  у игрока 5–297), `+0x8B4` — номер кадра анимации (растёт 0..127, сбрасывается).
  Vtable-геттера анимации у врага НЕТ (в отличие от игрока: vtable 241 `mov eax,[ecx+0x618]`);
  слот анимации `Behavior+0x770` у всех сущностей NULL (SDK-смещение не сходится).
- Анимация 19 EmSetCorps («прыжок на игрока») — выпад по земле: высота (pos_y)
  стабильна ~0.0 м во всех полях (+0x54, +0x7F4, +0x8E4), движется только X/Z;
  цикл атаки ~34 кадра (0x8B4: 0→33). «Прыжок» — визуальный эффект анимации.
- Иерархия частей врага: `Em0010_Blade.owner (+0x518) → Em0160Body (+0x360) →
  EmSetCorps` (главный враг); Blade/Assault/Magazine — дочерние сущности
  (`BehaviorPartsModel`), их `+0x50` — ЛОКАЛЬНАЯ позиция, мировая — из матрицы
  cParts (+0x10 → m[3] = +0x40/+0x44/+0x48). Высота клинка (bladeY в панели):
  пик «прыжка» (anim 65545) ~2.97 м, выпад (anim 19) ~1.13, idle ~1.03 —
  анимация поднимает части, корень врага остаётся на земле (Y≈0).

- **Запись анимации врага (2026-09-12, найдено `POST /watch`):** поле `Behavior+0x618` пишет генерический сеттер `0x68CAF0` (4 аргумента `ret 0x10`, прежние значения кладёт в `+0x628…+0x634`); его зовёт **стейт-машина действия** `0x739F00` (id анимации — аргумент; переходы гейтятся текущей анимацией `0x2000E/0x3000C/0x1F/0x20`, полем `+0x1ADC` и флагами `+0xDD4`/`+0xDD8`; зовётся **844 прямыми `call`** из всего модуля ИИ врага `0x739000…0x75F000`), а её — **селектор** `0x745C60`, который выбирает id по полям ИИ врага: `+0x1744` (1→`0x60000`, 2→`0x50000`), `+0xDDC & 0x8000` и `+0xF8C`/`+0xFA4` (→`0x70000`/`0xE`), `+0x13DC` (1/7→`0xE`), и **диспетчер действий** `0x740880` (switch по коду действия: `0x28`→`0xA0022`, `0x29`→`0xA0022` c арг 6, `0x24`→`0xA000E`, `0x25`→`0xB0008`/`0xB0005` по глобалу `[0x1BEA094] & 0x20000`). Оба — виртуальные (прямых `call` нет), код действия/поля приходят аргументами от цикла обновления ИИ: единой «функции решения об атаке» нет, решение распределено по модулю `0x739000…0x75F000`. **Источник случайности — RNG:** подпрограммы ИИ выбирают действие броском глобального LCG (MSVC `rand`: `state = state*214013 + 2531011`, берётся `state>>16`), состояние — dword в объекте `base + 0x19D0814`; функции `0x9DE2A0` (`randRange(lo,hi)`, `ret 8`), `0x9DE2D0` (знаковый), `0x9DE300` (float [0,1)), `0x9DE290` (`state = 0`). Пример: ветка `0x754E8D` при `rand()%3` даёт `playAnim(0x12/0x10/0x11)`, ниже — `playAnim(25 + rand())`. **Сайты ИИ, бросающие RNG** (найдены watch'ем на самом состоянии `base+0x19D0814` — писатели `0x9DE2B5`/`0x9DE2E5` внутри `randRange`): unsigned `0x74E5C8`, `0x754E9B`, `0x756570`; signed `0x729FFC`, `0x72A02D`, `0x72A29C`, `0x737729`, `0x737752`, `0x7430AA`, `0x74326A`, `0x752A18`, `0x7550D2`, `0x75527C`, `0x755343`, `0x756701`. Поэтому тайминг/выбор атаки случаен и не коррелирует ни с HP, ни с позицией игрока (замер: HP 120–128, старт прыжка 73–81, корреляции нет).

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

SQLite at `%LOCALAPPDATA%\drmod\runs.db`.

**Tables:**
```sql
CREATE TABLE runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at TEXT NOT NULL
);

CREATE TABLE segments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mission_id INTEGER NOT NULL,
    mission_name TEXT NOT NULL,
    started_at TEXT NOT NULL,
    duration_ms INTEGER NOT NULL
);

CREATE TABLE segment_positions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    segment_id INTEGER NOT NULL,
    pos_x REAL NOT NULL,
    pos_y REAL NOT NULL,
    pos_z REAL NOT NULL,
    duration_ms INTEGER NOT NULL,
    FOREIGN KEY (segment_id) REFERENCES segments(id) ON DELETE CASCADE
);
```

- On flush: only the best (minimum `duration_ms`) segment per `mission_id` is kept
- WAL mode + NORMAL synchronous for fast bulk inserts
- Ghost replay reads best segment positions via `load_best_ghost()`

**Replay tables (Record/Replay):**

```sql
CREATE TABLE replay_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,                  -- 'record' | 'playback'
    mission_id INTEGER NOT NULL,
    mission_name TEXT NOT NULL,
    started_at TEXT NOT NULL,
    frame_count INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    source_replay_id INTEGER             -- для playback — id исходной записи
);

CREATE TABLE replay_record_frames (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    replay_id INTEGER NOT NULL,
    frame_index INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,        -- frame_index * 1000 / 60 (фиктивный, не используется)
    input_unit BLOB NOT NULL,            -- InputUnit (48 байт, repr(C))
    state BLOB NOT NULL,                 -- PlayerState (88 байт)
    camera BLOB NOT NULL,                -- CameraState (92 байт)
    blade_down INTEGER NOT NULL DEFAULT 0,
    ripper_pressed INTEGER NOT NULL DEFAULT 0,
    raw_down BLOB,                       -- m_aKeysDown [u32; 6]
    raw_pressed BLOB,                    -- m_aKeysPressed [u32; 6]
    enemy BLOB,                          -- EnemyState (32 байта) — ближайший враг
    FOREIGN KEY (replay_id) REFERENCES replay_runs(id) ON DELETE CASCADE
);
-- replay_playback_frames — та же форма
```

- BLOB'ы — сырые байты структур из `replay-types/` (layout версионируется размером: camera 76 байт = legacy до 2026-08-18, не читается dbdump; enemy 32 байта = `EnemyState`, колонка добавлена 2026-08-24)
- Миграция старых БД (`ensure_replay_frame_columns`): `ALTER TABLE` добавляет blade/ripper/raw/enemy-колонки
- Мод БД не читает (воспроизведение идёт из памяти сессии) — таблицы только для истории/аналитики, экспорт: `tools/dbdump`

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

- **Перед каждым коммитом** проверять актуальность `QWEN.md`: если изменения затрагивают архитектуру, зависимости, новые модули, референсы, или любую информацию из этого файла — обновить соответствующие секции.

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

- **Thread safety**: `HelloHud` has `unsafe impl Send/Sync` because hudhook requires it for the render loop. This is safe since addresses are computed once in `new()` and never mutated.
- All static addresses are calculated once at init time, not per-frame.
- **Debug-only features** (`#[cfg(debug_assertions)]`): `DrmodDebug` window (record/playback status, segment timer, mission/menu status, compact player state), `Actions` window (numpad hotkey reference), Numpad keys, saved position display. Release builds keep only Multiplayer and Settings windows.
- **Меню Settings как пульт TAS-настроек** (`src/ui.rs` → `render_tas_controls`, обе сборки): прямо в окне настроек — фиксация шага времени движка (`POST /dt`: чекбокс «Фиксированный dt» + слайдер `dt, мс` + «Синтетические часы (m_fTicks)»), пин RNG решений ИИ (`POST /rng`: комбо `off/lo/mid/hi/seed/freeze` + поле сида) и кап кадров (`POST /fps`: комбо «как в игре/снят/свой лимит» + поле FPS), а также headless-прогон (`POST /render`: три чекбокса «без overlay / без Present / без геометрии игры»; при включённом overlay-скипе окно Settings тоже скрывается — возврат только извне, `POST /render {"reset": true}`). Контролы пишут тот же runtime-стейт, что и HTTP-ручки, через общие `api::set_fixed_dt`/`set_fixed_dt_ms`/`set_rng_pin`/`set_fps_cap` (и `render_hooks::set_skip` — источник истины у `render_hooks`) — значения не рассинхронизируются с API; `base_addr` для адресов `cSlowRateManager`/пацера берётся из `HelloHud`.
- **Headless-прогон** (`src/render_hooks.rs`, ручка `POST /render`, галочки в Settings; design: `docs/HEADLESS.md`): три независимых выключателя отрисовки, все по умолчанию выключены — `skip_overlay` (overlay мода не строится и не рисуется: 3D-маркеры, экранные метки, окна imgui), `skip_present` (настоящий `Present` не вызывается, возвращаем `D3D_OK`), `skip_draw` (MinHook-заглушки `DrawPrimitive`/`DrawIndexedPrimitive`/`DrawPrimitiveUP`/`DrawIndexedPrimitiveUP` на слотах vtable живого устройства `base + 0x1B206D4` → `D3D_OK`; индексы слотов считаются `offset_of!` по биндингам windows-rs). Зачем именно так: главный цикл игры делает **одну итерацию = один тик симуляции** (`updateFrameTime` `0xA03970` → пацер-сеттер `0xB98AD0` → тик `0xA4F560`, внутри `0x61E8A0` → `updateInputUnit` ×4 → `EndScene` `0xB9BE20` (`[ecx+0xA8]`) → кадровый рендер `0x651080` → очередь кадра `0xB9AD00` → пацер `0xB98070` → `Present` `0xB97F90` (`[ecx+0x44]`) → `0xAA0A50`), а логика мода живёт внутри хука `Present` — поэтому «headless» тут не отдельный процесс, а снятие дорогих частей кадра с сохранением ветвления движка: заглушки устройства не меняют поток управления игры (обход сцены, состояния и очередь кадра отрабатывают как обычно), а кадр скрипта идёт по тикам (`api::feed_tick`), так что прогон ускоряется, а не меняется. Собственный путь «не рисовать» у игры есть — флаг `[0x1F206F4]` («устройство потеряно») пропускает блок отрисовки, — но по той же проверке пропускается и тик, поэтому для ускорения он не годится. Хуки ставятся лениво из render-цикла (из HTTP-потока патчить пролог исполняемой функции нельзя) и потом не снимаются — при `skip_draw: false` остаются пробросом в оригинал (см. `draw_hooked` в `/state`). ⚠️ При `skip_overlay` скрывается и окно Settings — вернуть отрисовку только `POST /render {"reset": true}` (подсказка есть в самом окне). ⚠️ При `skip_present`/`skip_draw` окно замирает на последнем показанном кадре — для замеров это и нужно. Меру ускорения считать по времени прогона и `dt.frames`/`sim_ticks` в `/state`, а не по полю `fps`. Статус: оффлайн — build debug+release, `cargo clippy --all-targets` (без новых замечаний), `cargo test --lib` (6 тестов, включая `render_hooks::tests::slots_match_disassembly` — сверка индексов vtable с дизассемблером: `Present` `+0x44`, `EndScene` `+0xA8` — и `skip_flags_round_trip`). ⚠️ **Живьём 2026-09-15:** демо-TAS с `--headless` (debug-сборка, `P310_RESTART`) — прогон 1 совпал с эталоном бит-в-бит (`pos=(-51.34, 9.11, -85.93)`, 1302 кадра), то есть логика от снятой отрисовки не меняется; но прогон 2 упал в `d3d9.dll` (`0xC0000005`, запись по `esi = 0x3FF` в методе-фабрике, который зовёт игра через vtable; наши заглушки стоят по другим адресам). Поэтому `skip_present` и `skip_draw` — **экспериментальные** (виновник не изолирован), `skip_overlay` безопасен по построению; изоляция — `py -3 tools\demo\demo_r03_tas.py --runs 2 --skip overlay|present|draw`, разбор падения и план — `docs/HEADLESS.md` §5. Скорость прогона (а не простоя) мерить настенным временем прогона: при всех выключателях 1302 кадра = 9.4 с (≈2.3× символьного времени), базовая цифра ещё не снята.
- **Скип катсцены «как на консоли»** (`src/game/cutscene_skip.rs`; включён по умолчанию, выключается галочкой в окне Settings): пока идёт сцена `P370_RESTART`/`P370_IN`, мод держит в `staFlags` (`base + 0x17EA060`) флаги `STA_SOFT_EVENT` (код 4) + `STA_SOFT_EVENT_SKIP_OK` (код 37) — от них Esc игрока открывает не обычную паузу, а консольное катсценное меню (`GameMenuStatus` = 6, `cEventPauseMenu`). Собственный пункт Skip на PC инертен (шаг 5 обработчика непроходим), поэтому мод сам читает решение по объекту меню (`base + 0x17EA140`: `+0x04 == -2` — решение принято, `+0x3C` — индекс пункта), убирает меню штатным путём движка (`GameMenuStatus` = 6 + шаг `base + 0x17EA118` = 6 → движок сам ведёт `6 → 12 → 1` и уничтожает объект), снимает `STA_PAUSE` (код 19) и по подтверждённому SKIP заказывает `P370_EVENT` (`order_subphase` с `clear_event`). Всё это кадровый автомат в render-цикле (движок не потокобезопасен), этап виден в `GET /state` → `cutscene_skip` (`off`/`armed`/`closing`/`skipped`) и в окне Settings. ⚠️ Заказ подфазы грузит сцену **только без паузы** — отсюда снятие `STA_PAUSE` перед заказом. ⚠️ Пока флаги держатся, обычное меню паузы в этой сцене не откроется (плата за консольное поведение). Проверено живьём 2026-09-12 (сначала внешним инструментом с той же логикой, затем тем же автоматом в DLL): `P370_IN` → `P370_EVENT`.
- **HTTP API** (`src/api.rs`, debug + release): собственный минимальный HTTP-сервер (raw `TcpListener`, без tiny_http) на `127.0.0.1:5223` — однопоточный, non-blocking accept, таймауты 1 с, `shutdown()` завершается за ограниченное время (eject не виснет). Ручки: `POST /script/run` (скрипты ввода в кадрах; `trigger` по позиции или тикам, `restart`), `GET /state`, `GET /logs`, `GET /health`, `POST /eject`, `POST /order` (заказ подфазы — рабочий скип катсцены), `POST /phase` (тупик — см. `docs/PITFALLS.md`), `POST /dt` (фиксированный шаг времени), `POST /fps` (кап кадров), `POST /rng` (пин RNG решений ИИ), `POST /render` (headless-прогон — снятие отрисовки), `POST/GET /watch` (debug — точка останова на запись). Ручки `/dt`, `/rng`, `/fps`, `/render` продублированы в меню Settings. Ring buffer 3600 кадров (60 с); скрипты сами стопаются на loading и перед eject. **Кадр скрипта = тик симуляции** (подача из детура `updateInputUnit`, `api::feed_tick` + счётчик `SIM_TICKS`; в меню/загрузке — 1 кадр за кадр отрисовки). Полная спецификация запросов/ответов, механика и адреса — `docs/API.md` (механика ввода меню — §10.5); пацер — QWEN.md «Frame pacer». **Фронты `pressed` для направлений**: команды `forward/backward/left/right` ставят не только `buttons_down`, но и `buttons_pressed` — в момент появления бита (сверка с предыдущим кадром), как считает игра; без этого скриптом не воспроизводился приём «вперёд, вперёд, хэви» (`anim 110`, lightning strike) — `docs/LIGHTNING_STRIKE.md`.
- Error handling uses Windows `MessageBoxW` for user-facing errors.
- Library is compiled as both `cdylib` (for injection) and `rlib` (for the binary to link against).
- **dbdump** (`tools/dbdump/`): экспорт кадров Record/Replay в CSV/Parquet — 90 плоских колонок (мета прогона + frame + InputUnit/PlayerState/CameraState + производные `cam_yaw`/`cam_pitch` + blade/ripper/raw + ближайший враг `enemy_pos_x/y/z`/`enemy_blade_y`/`enemy_anim`/`enemy_frame`/`enemy_hp`). По id record-прогона дампит и его playback'и (`source_replay_id`). Режим `--script`: конвертация записи в JSON-скрипт для `POST /script/run` (взведён триггером на позиции первого кадра — старт миссии; биты InputUnit → семантические поля через `replay-types::input_bits`, RLE-слияние одинаковых кадров, ripper — 1-кадровые команды; лимит 3600 кадров). Сборка — x64 (`cd tools/dbdump && cargo build --release`, свой `.cargo/config.toml` как у server; arrow-rs только 64-bit; корневой `cargo build` тул не собирает). Запуск: `dbdump <run_id> [--script [--pretty]] [--out DIR] [--db PATH]`. Тесты: `cargo test` из `tools/dbdump`. Детали: `tools/dbdump/README.md`.
- **script_tuning** (`tools/script_tuning/`, python): подбор таймингов core-скрипта (разгон → прыжок → тяжёлая атака у земли → риппер) для надёжного перелёта барьера 20 м в `P310_RESTART`, ускорение прогонов (`/dt`+`/fps`), автоматика рестарта/меню/fail-recovery. Актуальные инструменты, параметры и эталонный рецепт — `tools/script_tuning/README.md`; хроника экспериментов и «что не сработало» — `docs/SCRIPT_TUNING.md`. Грабли (креши DLL, фокус окна, UTF-8, «две копии игры») — `docs/PITFALLS.md`.
- **Несколько копий игры на одной машине — нельзя** (2026-09-13, разведано и откачено): вторая копия MGR:R умирает с кодом 0 через ~100 мс после загрузки `steam_api.dll` (независимо от пути exe/`appid`/env; форс успеха `SteamAPI_Init` не помогает). Сейвы — Steam Cloud, один файл на пару (steamid, appid). Разбор и варианты изоляции Steam-контекста/сейвов — `docs/PITFALLS.md`.
- **Десинк Record→Playback** (`docs/DESYNC_ANALYSIS.md`): главный источник — **лаг подачи ввода на 1 кадр** (override из render(K) применяется тиком K+1, `play[fi]==rec[fi-1]` на 100%) + фазовая неопределённость Present↔тик. Решение: подача кадров из детура `updateInputUnit` (`replay::PLAYBACK_FEED` + `feed_playback`, вариант B) + компенсация курса (`rsx_correction`, вариант E) → **record 110 → 111/112/113/114: 4/4 успех, |Δpos| 0.7–1.0 м, |Δyaw| медиана 0.12–0.26°**. Критическая точка — вход в ninja run (r_anim 5→13→14→71, fi≈233–248). История вариантов A/B/D/E и анализ — `docs/DESYNC_ANALYSIS.md`; инструменты — `tools/desync_analysis/` (pandas).
- **Мод `mods/cutscene_skip/`** (самостоятельный крейт: собственный `[workspace]` и свой `target/`, в корневой workspace не входит; вынос в отдельный репозиторий = перенос папки): только скип in-engine катсцены — порт `src/game/cutscene_skip.rs` без imgui/hudhook-dx9/HTTP API/сети. Состоит из лаунчера `cutscene_skip.exe` (ищет игру: рядом с собой → известный Steam-путь → `--exe`/`CUTSCENE_SKIP_GAME_EXE`; если игра запущена — инжектит в неё, иначе стартует и ждёт окно; флаги `--kill-first`, `--no-launch`, `--follow` (печать лога), `--timeout`) и встроенной в него DLL `cutscene_skip_lib.dll` (распаковывается в `%LOCALAPPDATA%\cutscene_skip\`, лог — там же). Пер-кадровая точка — MinHook на `updateFrameTime` (`0xA03970`, тот же движковый обновитель, что у `/dt`), автомат скипа крутится в детуре (поток игры). `WATCH = {P370_RESTART, P370_IN} → P370_EVENT` хардкод, UI-галочки нет (скип всегда активен в этих сценах). MinHook завендорен внутрь крейта (`vendor/minhook`, BSD-2-Clause) ради самодостаточности. Сборка: `cd mods/cutscene_skip && cargo build --release` (i686, свой `.cargo/config.toml`), тесты `cargo test`. Статус: проверено живьём 2026-09-12 (инжект лаунчером в уже запущенную игру, `--no-launch`): **физический Esc** открывает консольное меню без досылки фронта `pause` (хук `updateInputUnit` не нужен — мод читает решение из объекта меню), детур `updateFrameTime` работает и в паузе (`GameMenuStatus` = 6 — пер-кадровая точка годится), скип `P370_IN` → `P370_EVENT` отработал (сцена сменилась), падений нет; оффлайн — сборка debug+release, clippy, 4 теста. Детали: `mods/cutscene_skip/README.md`.

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
