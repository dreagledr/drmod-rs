# drmod-rs

## Project Overview

A Rust-based mod injector and HUD overlay for **Metal Gear Rising: Revengeance**. The project consists of:

- **Binary (`drmod`)**: Injects a DLL into the running game process
- **Library (`drmod_rs_lib`)**: Hooks into DirectX 9 to render an ImGui overlay that reads game memory in real-time
- **Server (`server/`)**: Multiplayer relay server (tokio, Docker, 64-bit)
- **Protocol (`protocol/`)**: Shared types for TCP (JSON) and UDP (binary) communication
- **Replay-types** (`replay-types/`): Shared replay DTOs (`InputUnit`/`PlayerState`/`CameraState`, `#[repr(C)]`) + `to_bytes`/`from_bytes` — on-disk layout replay BLOB'ов; `input_bits` — биты действий в `InputUnit` (общие для мода и dbdump); `key_codes` — кодировка игровых кодов клавиш в словах `m_aKeysDown` (порядок бит обратный: `0x8000_0000 >> (code & 31)`)
- **dbdump (`tools/dbdump/`)**: CLI-экспорт кадров Record/Replay из `runs.db` в CSV/Parquet (90 плоских колонок) для аналитики + режим `--script` (запись → JSON-скрипт HTTP API)

Features:
- Segment-based autosplitter with SQLite persistence and ghost replay
- Multiplayer position sync (TCP + UDP)
- World-to-screen projection (camera matrix, D3D viewport)
- Debug panel with live game state (debug builds only)
- HTTP automation API (127.0.0.1:5223) — input scripts, game state, ring-buffer logs (design: `docs/API.md`)

Built with [hudhook](https://github.com/veeenu/hudhook) for DirectX hooking and [imgui-rs](https://github.com/imgui-rs/imgui-rs) for the UI.

## Architecture

```
src/
├── main.rs          # Injector binary — finds game process, injects DLL
├── lib.rs           # HUD library — DX9 hook, ImGui overlay, game memory, main loop
├── api.rs           # HTTP API (127.0.0.1:5223) — scripts, state, ring-buffer logs
├── segment.rs       # Segment tracking — start conditions, ASL-based finish triggers, DB cleanup
├── ui.rs            # ImGui windows — debug panel (debug only), multiplayer, settings
├── game/            # Сущности игры — игрок (Pl0000), камера (cCameraGame), статус меню
│   ├── mod.rs       #   GameMenuStatus enum, is_readable_ptr, re-export Player/Camera
│   ├── player.rs    #   Player — кэш объекта игрока, read_player_state/read_current_input/read_pl_input/read_enemies/read_skeleton
│   └── camera.rs    #   Camera — read_camera_state/view_proj/pos
├── net.rs           # TCP + UDP client for multiplayer
├── overlay.rs       # world_to_screen projection, draw_world_pos
├── settings.rs      # User settings (ghost opacity, show ghost toggle)
├── d3d_render.rs    # CylinderRenderer, SphereRenderer for 3D overlays
├── skeleton.rs      # Bone/skeleton data structures
├── logger.rs        # Logging to %LOCALAPPDATA%\drmod\ (debug.log + buffered state.log)
├── tas/             # TAS (tool-assisted speedrun) — input record/replay
│   ├── addresses.rs #   Input memory addresses/constants
│   ├── db.rs        #   Replay SQLite tables, миграция колонок + bulk insert
│   ├── replay.rs    #   Record/playback logic, input override
│   ├── hooks.rs     #   MinHook input hooks (updateInputUnit/isKeybindPressed/isKeybindDown), ripper/blade emulation, raw input readers
│   └── types.rs     #   Re-export DTO из replay-types + ReplayFrame/внутренние типы
server/              # Multiplayer server (tokio, 64-bit, Docker)
protocol/            # Shared protocol types (TCP JSON + UDP binary PositionPacket)
replay-types/        # Общие replay-DTO (InputUnit/PlayerState/CameraState) + to_bytes/from_bytes + input_bits
tools/
├── dbdump/          # Экспорт replay-кадров в CSV/Parquet + --script (JSON для HTTP API) (x64, отдельный .cargo/config.toml)
├── desync_analysis/ # pandas-скрипты анализа десинка Record→Playback (CSV от dbdump)
├── script_tuning/   # Перебор таймингов core-скрипта 117 + прогон сетки через HTTP API (python)
└── disasm/          # Скрипты дизассемблирования (отдельный workspace, вне корневого)
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
| `base + 0x17E9F9C` | `i32` | GameMenuStatus (enum 0–18) |
| `base + 0x1764670` | `i32` | Current mission ID |
| `base + 0x1764674` | `*const i8` | Current mission name string |
| `base + 0x14B9181` | `*const i8` | gStr — game location string |
| `base + 0x14B91AD` | `*const i8` | gStr2 — game location string 2 |
| `base + 0x14B91A8` | `*const i8` | gStr4 — mission identifier ("P118", "EV60", etc.) |

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
| `hudhook` (0.9.0) | DirectX hooking and injection |
| `imgui` (0.12.0) | ImGui bindings for UI rendering |
| `windows` (0.62.2) | Windows API (UI windows, module loading) |
| `rusqlite` (0.40.1, bundled) | SQLite for persisting run data |
| `chrono` (0.4.45) | Time formatting for run timestamps |
| `serde` / `serde_json` (1) | JSON serialization for multiplayer protocol and HTTP API |
| `drmod-protocol` | Shared types for client-server communication |
| `drmod-replay-types` | Shared replay DTOs (`InputUnit`/`PlayerState`/`CameraState`) + `to_bytes`/`from_bytes` + `input_bits` (биты InputUnit) |

Тул `tools/dbdump` дополнительно тянет (только для него, x64): `rusqlite`, `csv`, `arrow` + `parquet` (59.x) — экспорт в CSV/Parquet; `serde` + `serde_json` — режим `--script` (JSON для HTTP API).

### Notes

- **Thread safety**: `HelloHud` has `unsafe impl Send/Sync` because hudhook requires it for the render loop. This is safe since addresses are computed once in `new()` and never mutated.
- All static addresses are calculated once at init time, not per-frame.
- **Debug-only features** (`#[cfg(debug_assertions)]`): `DrmodDebug` window (record/playback status, segment timer, mission/menu status, compact player state), `Actions` window (numpad hotkey reference), Numpad keys, saved position display. Release builds keep only Multiplayer and Settings windows.
- **HTTP API** (`src/api.rs`, debug + release): own minimal HTTP server (raw `TcpListener`, no tiny_http) on `127.0.0.1:5223` — `POST /script/run` (JSON scripts in frames; optional `trigger: {pos: [x,y,z]}` arms the script — starts when the player enters the zone, like deferred record/playback; optional `restart: {...}` — мод сам рестартует миссию через меню паузы (`pause` → стрелка `dik_key` вверх → `confirm` ×2), ждёт loading и только затем взводится по `trigger`; статус фазы `restarting`), `GET /state`, `GET /logs?from_ms&to_ms&script_id`, `GET /health`, `POST /eject` (unloads the DLL: HTTP thread sets a flag, the render loop performs `shutdown()` + `hudhook::eject()`), `POST /dt` (`{"fixed": true|false}` — фиксированный шаг времени движка, по умолчанию выключен; опции `ms` (величина шага) и `ticks` (синтетические часы, по умолчанию вкл). Детур `cSlowRateManager::updateFrameTime` (`0xA03970`, зовётся раз за итерацию главного цикла `0xA52510`) после оригинала пишет `m_fTickDifference` = ms, `m_fTickRate` = 1.0, `unit[0].m_fDelta` = 1.0 и ведёт **линейные часы** `m_fTicks` (+0x80) = база + N·ms, `base + 0x17E93B0`; состояние — в `/state` → `dt` (в т.ч. `frames`, `synth_ticks_ms`, `ticks`)), `POST/GET /watch` (debug: аппаратная точка останова на **запись** — `DR0`/`DR7` на все потоки процесса, VEH ловит `STATUS_SINGLE_STEP` и пишет в `/watch` `Eip` писателя + адрес возврата вызывающего + кандидатов-возвратов со стека; `{"on":true,"enemy":true}` — на поле анимации ближайшего врага, `{"addr":"0x…"}` — на произвольный адрес, `{"on":false}` — снять)). Ring buffer of 3600 frames (60 s). Scripts stop automatically on loading and before eject. **Кадр скрипта = тик симуляции (2026-09-11):** ввод API-скрипта подаётся из детура `updateInputUnit` через очередь кадров (`api::feed_tick` + счётчик `SIM_TICKS`, по образцу `PLAYBACK_FEED` у replay) — render досчитывает кадры по числу прошедших тиков; в меню/загрузке (тика ввода нет) остаётся один кадр за кадр отрисовки, иначе встал бы скрипт меню/рестарта. До правки кадр скрипта шёл по кадрам отрисовки, и при 43–57 FPS один и тот же кадр (прыжок 45, удар 76) попадал в разные моменты физики: одна и та же связка давала 5/8 и 0/15. NumPad4 runs the builtin script through the same `ScriptRunner`. Design: `docs/API.md` (механика ввода меню — §10.5). The server is a single thread with non-blocking accept (10 ms stop-flag poll) and 1 s read/write timeouts per connection — `shutdown()` joins it in bounded time, so DLL eject never hangs (tiny_http was replaced because it had no socket timeouts and spawned unjoinable internal threads).
- Error handling uses Windows `MessageBoxW` for user-facing errors.
- Library is compiled as both `cdylib` (for injection) and `rlib` (for the binary to link against).
- **dbdump** (`tools/dbdump/`): экспорт кадров Record/Replay в CSV/Parquet — 90 плоских колонок (мета прогона + frame + InputUnit/PlayerState/CameraState + производные `cam_yaw`/`cam_pitch` + blade/ripper/raw + ближайший враг `enemy_pos_x/y/z`/`enemy_blade_y`/`enemy_anim`/`enemy_frame`/`enemy_hp`). По id record-прогона дампит и его playback'и (`source_replay_id`). Режим `--script`: конвертация записи в JSON-скрипт для `POST /script/run` (взведён триггером на позиции первого кадра — старт миссии; биты InputUnit → семантические поля через `replay-types::input_bits`, RLE-слияние одинаковых кадров, ripper — 1-кадровые команды; лимит 3600 кадров). Сборка — x64 (`cd tools/dbdump && cargo build --release`, свой `.cargo/config.toml` как у server; arrow-rs только 64-bit; корневой `cargo build` тул не собирает). Запуск: `dbdump <run_id> [--script [--pretty]] [--out DIR] [--db PATH]`. Тесты: `cargo test` из `tools/dbdump`. Детали: `tools/dbdump/README.md`.
- **script_tuning** (`tools/script_tuning/`, python): перебор таймингов core-скрипта записи 117 (разгон → прыжок → хэви-атака у земли → риппер) для надёжного перелёта барьера 20 м в P310_RESTART. `core117.py` собирает очищенный скрипт (из 360 команд записи остаётся 7: выброшены 175 однокадровых команд камеры fi 131-558 и шумовые backward/left; высоту даёт root motion анимации тяжёлой атаки, лаунч на fi 96 — за 8 кадров до риппера), разгон всегда 5 кадров. `sweep.py` кладёт сетку `t_jump × t_attack` в `out\tuning\*.json` и с `--run` гоняет её через HTTP API, считая `max_y`/`cleared`/`spawn_ok` в `sweep.csv`. **Рестарт миссии автоматизирован (2026-09-10):** поле `restart` в `POST /script/run` — мод сам отыгрывает меню паузы (`pause` → стрелка вверх → `confirm` ×2: пункт Restart + диалог «Restart from last checkpoint?»), **ждёт фактического loading** и только потом взводит скрипт по `trigger` спавна (одним скриптом, потому что активный скрипт в моде один → 409; взвод строго после loading, иначе триггер сработал бы по старой позиции). Общий клиент — `drmod_api.py` (HTTP на **блокирующем** сокете: сокет с таймаутом теряет ответ, `WinError 10053` в ~2/3 случаев; активация окна игры). Ещё инструменты: `restart.py` (рестарт CLI), `run_json.py` (прогон JSON из `test_inputs/`), `menu_probe.py` (живой пробник меню), `parry_geometry.py` (геометрия парирования/подброса: выравнивание по кадру удара — из `fed_down_bits` — и по кадру парирования, корреляции с `post_gain`, оконный CSV, `--analyse` по готовым CSV), `selftest.py` (оффлайн-проверки: допуски `in_zone` и метрики). **Геометрия подброса (2026-09-11):** подброс бывает только от парирования (8/40 в серии 40, у всех `post_gain` 6.2–21.6 м; без парирования — только прыжок 1.6–3.5 м); высота клинка врага в момент контакта постоянна (`blade_y ≈ 3.0` м → в «треугольнике» переменных две: высота игрока и дистанция); прыжок врага стартует **вместе с нашим ударом** (в кадре удара враг уже в `anim 65545` с кадром 0–7 и в 4–4.8 м), поэтому на управляемом моменте парирование не предсказывается, а контакт приходит на 18–22 кадра позже — в конце прыжка (кадр 28–36 анимации 65545). Сила подброса не воспроизводится: в серии 40 коррелирует с высотой игрока при контакте (r=+0.89 при n=8), но в серии 30 лучшие подбросы (25.1/22.9 м) были при `y` 2.39–2.54 — ниже посредственных в серии 40, а дистанция меняет знак (`r` −0.63 / +0.70) и на кадре контакта загрязнена сменой анимации врага. ⚠️ Вывод инструментов — UTF-8 (`api.setup_stdout()`): в cp1251-логе кракозябры, а `→`/`≥`/`✓` роняли печать уже после записи CSV (так потерялись две сводки). **Сдвиг удара по врагу (2026-09-11):** `frame_min` в `when_enemy` сдвигает команду только **раньше** штатного `t` — если условие не выполнилось ни разу, команда срабатывает на `t` как обычная (доказано: при `frame_min 8/14` удар в 16 прогонах из 16 прошёл ровно на `t=76` при `враг_кадр` 0–9 и `игрок_y ≈ 0`), поэтому для настоящего сдвига штатный `t` надо двигать вместе с `frame_min`. Пилот 3×8 прогонов: удар по условию (кадры 69–73) дал `max_y` 19.0/19.3, а штатный кадр записи 76 — 31.1/31.2 и 30.2 (n=2–3 на плечо, тонко). **Mission Fail автоматизирован:** `ensure_gameplay`/`recover_fail` на статус из `FAIL_STATUSES` жмёт `confirm` (в fail-меню выбран Retry) и ждёт `In Game` — проверено живьём (`Mission Fail → In Game`, ~1 с), инструмент `fail_recovery.py`. **Ninja run (2026-09-11):** в `core117` ninja run не подавался вообще (в игре шла обычная пробежка) — добавлен `ninja=True`/`--ninja` (бит 0x4000 + удержание keybind 8 на всех командах). Ручная техника: разгон с ninja run → прыжок с ним → **стоп в воздухе** (`release_tail`, зазор без ввода) → `forward + ninja_run + тяжёлая атака`. Пилот (8 прогонов на вариант): ninja + удар 76 + стоп 6 → **5/8 парирований и 4/8 перелётов ≥20 м** (`post_gain` 18.3–29.9), тогда как ninja без стопа — 4/8 парирований и 0/8 перелётов (3.1–10.6 м: проносит мимо врага), ninja + удар 70 (со стопом и без) — парирования есть, подброса нет вовсе (0.1–6.1 м). **Эталонный рецепт перелёта (2026-09-11):** `core117.build(jump=39, run_frames=6, dur_attack=24, ripper=104, release_tail=6, ninja=True, ninja_flight=False, attack_when_enemy={"player_y_min":0.01,"player_y_max":0.25,"player_vy_max":0.0})` — замер `ab_runs.py`: 15 прогонов → 7/15 парирований и 7/15 перелётов ≥20 м (`max_y` 21.6–32.7, `post_gain` 19.2–30.1), суммарно по 4 сериям (30 прогонов) 17/30 (57%) и 14/30 (47%), причём почти все парирования дают перелёт. Смысл частей: ninja run (без него другие анимации прыжка и удара в воздухе) → отпустить ninja после прыжка (`ninja_flight=False`, скорость меньше, анимация «как на бегу») → стоп 6 кадров (`release_tail`) → удар по триггеру высоты 0.01–0.25 на спуске. Триггер работает по **состоянию игры**, а не по кадру отрисовки, поэтому не зависит от FPS — именно это сняло невоспроизводимость (кадр 76 при 43–57 FPS попадал то в воздух, то на землю). Числа подвижны: прыжок 38–41, стоп 5–10, края окна — проверять только чередующимся прогоном и повторять серию. **После переноса тика в физику (2026-09-11)** высота игрока в кадре удара стала монотонной функцией тика удара (0.73 при 66 → 0.35 при 74 → ≈0 при 76, раньше разъезжалась от −0.01 до 0.57): чем позже удар, тем вертикальнее подброс (0.2 → 12 → 30.8 м), но на 76 он уже на земле и не даёт ничего. Оптимум — **плато, а не точка**: окна 0.01–0.25…0.15–0.40 и ручки (стоп 4/6/8, `dur_attack` 20/24/28) дают одинаковый результат в пределах шума; сводка по 60 прогонам после правки с триггером — парирований 43% (26/60), перелётов ≥20 м 35% (21/60). **Фиксированный шаг времени движка (2026-09-11; уточнено 2026-09-12):** причина невоспроизводимости серий — `cSlowRateManager` (`base + 0x17E93B0`): `m_fTickDifference` (`+0x8C`, мс) — измеренная длительность кадра (16.25–19.25 при номинале 16.667 = 52–61 FPS), `m_fTickRate` (`+0x7C`) = `diff/16.667`. Оба пишет обновитель кадра `updateFrameTime` (`0xA03970`, thiscall `(this, flag, rate)`, зовётся раз за итерацию главного цикла `0xA52510`) по реальным часам `0x9F8C10`, он же выставляет часы `m_fTicks` (`+0x80`) = t. `POST /dt {"fixed": true}` ставит MinHook на `updateFrameTime` и после оригинала перезаписывает поля: `diff` = номинал, `rate` = 1.0, `unit[0].m_fDelta` = 1.0 и **линейные часы** `m_fTicks` = база + N·ms (опция `ticks: false` оставляет дельту, но возвращает часы на реальный dt — A/B). Замер внешним чтением памяти: `ticks: true` → `m_fTicks` +16.666 мс за кадр ровно, `ticks: false` → +22.4 (реальный dt); темп символьного времени 0.82 от реального (игра **замедляется**), то есть окно замедления риппера = 1000/16.667 = **60 кадров при любом FPS**. Игрок при этом детерминирован: траектория побитово одинакова между прогонами до контакта с врагом (первые ~96 кадров), переходы `r_anim` игрока идентичны во всех прогонах (`[(0,0),(34,71),(40,5),(77,94)]`) и при `ticks: true`, и при `ticks: false` — часы на анимационный автомат игрока в этом окне не влияют. Остаточный разброс даёт **враг**: его HP при спавне случаен (120–128), старт прыжка (`anim 65545`) плавает по кадрам (73/74/74/77) → парирование/подброс не воспроизводится. ⚠️ Хук ставится и в release (HTTP API доступен в обеих сборках). ⚠️ Фаза рестарта требует фокуса окна игры: игра опрашивает клавиатуру (`DirectInput`) только в foreground, `sweep.py --run` активирует окно сам. ⚠️ На одном прогоне игра крашнулась сразу после выбора Restart (до загрузки) — не расследовано, после перехода на ожидание loading не повторялось. Детали: `tools/script_tuning/README.md`; механика ввода меню (DIK-канал, кодировка бит) — `docs/API.md` §10.5.
- **Десинк Record→Playback** (`docs/DESYNC_ANALYSIS.md`, анализ 2026-08-23, record 73 → playbacks 78/81/82): гипотеза «дроп FPS» закрыта (длительности кадров record==playback побайтово, 60 FPS стабильно); главный источник — **лаг подачи ввода 1 кадр** (override из render(K) применяется тиком K+1, `play[fi]==rec[fi-1]` на 100%) + фазовая неопределённость Present↔тик (playback'и с идентичным вводом расходятся между собой до 17 м). **Вариант A (2026-08-23):** `playback_tick` подаёт `frame[frame_idx + 1]` — тик N+1 применяет `frame[N+1]`, как в записи; лаг 0 = 100%, лучший прогон 88 — |Δpos| 2.7 м (было ~17 м); недетерминизм сохранился (89/90 до 25 м, приземления мимо зон по высоте). **Вариант B (2026-08-23):** подача кадров перенесена из render в детур `updateInputUnit` (`replay::PLAYBACK_FEED` + `feed_playback`) — каждый тик симуляции получает кадр синхронно, фаза Present не влияет; `playback_tick` только логирует и останавливает при `done`; `next_idx=1` (frames[0] — нулевой спавн). **Вариант D (2026-08-23, ОТКЛЮЧЁН `DUP_ENABLED=false`):** дубль кадра при отставании вдоль (hold-окна) — на прогоне 96 дал перелёт при прыжках, исход не изменил. **Вариант E (2026-08-23):** компенсация курса — render считает dYaw (текущая камера vs запись), ставит поправку к `right_stick_x` следующего кадра (`rsx_correction`, чувствительность ~0.00065 °/ед/кадр, gain 0.6, порог 0.5°, клэмп ±2000; эталон — последний поданный кадр, коррекция только |dYaw|<90°, знак «плюс» — положительный rsx поворачивает камеру влево). **Результат (record 110 → 111/112/113/114): 4/4 успех, |Δpos| 0.7–1.0 м, |Δyaw| медиана 0.12–0.26°.** Критическая точка десинка — вход в ninja run (r_anim 5→13→14→71, fi≈233–248). Анализ: `tools/desync_analysis/` (pandas, `py -3 tools/desync_analysis/analyze*.py [DIR] [REC_ID PLAY_ID...]`, `plot_cam.py [DIR] [OUT.png] [REC_ID PLAY_ID...]`). Вариант C (snap-коррекция) — в документе, не реализован.

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
- Server: tokio-based, relays UDP to all clients in the same room
- Room concept: lobbies, no mission filtering on server side
