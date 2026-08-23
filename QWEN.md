# drmod-rs

## Project Overview

A Rust-based mod injector and HUD overlay for **Metal Gear Rising: Revengeance**. The project consists of:

- **Binary (`drmod`)**: Injects a DLL into the running game process
- **Library (`drmod_rs_lib`)**: Hooks into DirectX 9 to render an ImGui overlay that reads game memory in real-time
- **Server (`server/`)**: Multiplayer relay server (tokio, Docker, 64-bit)
- **Protocol (`protocol/`)**: Shared types for TCP (JSON) and UDP (binary) communication
- **Replay-types (`replay-types/`)**: Shared replay DTOs (`InputUnit`/`PlayerState`/`CameraState`, `#[repr(C)]`) + `to_bytes`/`from_bytes` — on-disk layout replay BLOB'ов
- **dbdump (`tools/dbdump/`)**: CLI-экспорт кадров Record/Replay из `runs.db` в CSV/Parquet (83 плоские колонки) для аналитики

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
├── game.rs          # GameMenuStatus enum, weapon name helpers
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
replay-types/        # Общие replay-DTO (InputUnit/PlayerState/CameraState) + to_bytes/from_bytes
tools/
├── dbdump/          # Экспорт replay-кадров в CSV/Parquet (x64, отдельный .cargo/config.toml)
├── desync_analysis/ # pandas-скрипты анализа десинка Record→Playback (CSV от dbdump)
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
- Фильтр врагов: имена `Em*`/`Ba*`/`Pl001*` (без игрока по `cached_player_obj_ptr`),
  позиция не (0,0,0), HP 1..1 000 000. Части моделей (`Pl0010_Hair` и т.п.) и фон
  (`byBgManager`, пустые имена) отсекаются позицией/HP.
- Анимация врага — подтверждено рантаймом (диагностика полей): `+0x618` — текущая
  анимация (ID меняются при смене: 65536/65540/14/23 — свои у EmSetCorps, не как
  у игрока 5–297), `+0x8B4` — номер кадра анимации (растёт 0..127, сбрасывается).
  Vtable-геттера анимации у врага НЕТ (в отличие от игрока: vtable 241 `mov eax,[ecx+0x618]`);
  слот анимации `Behavior+0x770` у всех сущностей NULL (SDK-смещение не сходится).

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
    FOREIGN KEY (replay_id) REFERENCES replay_runs(id) ON DELETE CASCADE
);
-- replay_playback_frames — та же форма
```

- BLOB'ы — сырые байты структур из `replay-types/` (layout версионируется размером: camera 76 байт = legacy до 2026-08-18, не читается dbdump)
- Миграция старых БД (`ensure_replay_frame_columns`): `ALTER TABLE` добавляет blade/ripper/raw-колонки
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
| `drmod-replay-types` | Shared replay DTOs (`InputUnit`/`PlayerState`/`CameraState`) + `to_bytes`/`from_bytes` |

Тул `tools/dbdump` дополнительно тянет (только для него, x64): `rusqlite`, `csv`, `arrow` + `parquet` (59.x) — экспорт в CSV/Parquet.

### Notes

- **Thread safety**: `HelloHud` has `unsafe impl Send/Sync` because hudhook requires it for the render loop. This is safe since addresses are computed once in `new()` and never mutated.
- All static addresses are calculated once at init time, not per-frame.
- **Debug-only features** (`#[cfg(debug_assertions)]`): `DrmodDebug` window (record/playback status, segment timer, mission/menu status, compact player state), `Actions` window (numpad hotkey reference), Numpad keys, saved position display. Release builds keep only Multiplayer and Settings windows.
- **HTTP API** (`src/api.rs`, debug + release): own minimal HTTP server (raw `TcpListener`, no tiny_http) on `127.0.0.1:5223` — `POST /script/run` (JSON scripts in frames), `GET /state`, `GET /logs?from_ms&to_ms&script_id`, `GET /health`, `POST /eject` (unloads the DLL: HTTP thread sets a flag, the render loop performs `shutdown()` + `hudhook::eject()`). Ring buffer of 3600 frames (60 s). Scripts stop automatically on loading and before eject. NumPad4 runs the builtin script through the same `ScriptRunner`. Design: `docs/API.md`. The server is a single thread with non-blocking accept (10 ms stop-flag poll) and 1 s read/write timeouts per connection — `shutdown()` joins it in bounded time, so DLL eject never hangs (tiny_http was replaced because it had no socket timeouts and spawned unjoinable internal threads).
- Error handling uses Windows `MessageBoxW` for user-facing errors.
- Library is compiled as both `cdylib` (for injection) and `rlib` (for the binary to link against).
- **dbdump** (`tools/dbdump/`): экспорт кадров Record/Replay в CSV/Parquet — 83 плоские колонки (мета прогона + frame + InputUnit/PlayerState/CameraState + производные `cam_yaw`/`cam_pitch` + blade/ripper/raw). По id record-прогона дампит и его playback'и (`source_replay_id`). Сборка — x64 (`cd tools/dbdump && cargo build --release`, свой `.cargo/config.toml` как у server; arrow-rs только 64-bit; корневой `cargo build` тул не собирает). Запуск: `dbdump <run_id> [--out DIR] [--db PATH]`. Тесты: `cargo test` из `tools/dbdump`. Детали: `tools/dbdump/README.md`.
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
