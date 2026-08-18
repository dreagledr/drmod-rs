# drmod-rs

## Project Overview

A Rust-based mod injector and HUD overlay for **Metal Gear Rising: Revengeance**. The project consists of:

- **Binary (`drmod`)**: Injects a DLL into the running game process
- **Library (`drmod_rs_lib`)**: Hooks into DirectX 9 to render an ImGui overlay that reads game memory in real-time
- **Server (`server/`)**: Multiplayer relay server (tokio, Docker, 64-bit)
- **Protocol (`protocol/`)**: Shared types for TCP (JSON) and UDP (binary) communication

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
│   ├── db.rs        #   Replay SQLite tables + bulk insert
│   ├── replay.rs    #   Record/playback logic, input override
│   ├── hooks.rs     #   MinHook input hooks (updateInputUnit/isKeybindPressed/isKeybindDown), ripper/blade emulation, raw input readers
│   └── types.rs     #   Input/state DTOs (InputUnit, ReplayFrame, ...)
server/              # Multiplayer server (tokio, 64-bit, Docker)
protocol/            # Shared protocol types (TCP JSON + UDP binary PositionPacket)
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
| `tiny_http` (0.12) | HTTP server for the automation API (127.0.0.1:5223) |
| `drmod-protocol` | Shared types for client-server communication |

### Notes

- **Thread safety**: `HelloHud` has `unsafe impl Send/Sync` because hudhook requires it for the render loop. This is safe since addresses are computed once in `new()` and never mutated.
- All static addresses are calculated once at init time, not per-frame.
- **Debug-only features** (`#[cfg(debug_assertions)]`): `DrmodDebug` window (record/playback status, segment timer, mission/menu status, compact player state), `Actions` window (numpad hotkey reference), Numpad keys, saved position display. Release builds keep only Multiplayer and Settings windows.
- **HTTP API** (`src/api.rs`, debug + release): tiny_http on `127.0.0.1:5223` — `POST /script/run` (JSON scripts in frames), `GET /state`, `GET /logs?from_ms&to_ms&script_id`, `GET /health`. Ring buffer of 3600 frames (60 s). Scripts stop automatically on loading and before eject. NumPad4 runs the builtin script through the same `ScriptRunner`. Design: `docs/API.md`.
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
- Server: tokio-based, relays UDP to all clients in the same room
- Room concept: lobbies, no mission filtering on server side
