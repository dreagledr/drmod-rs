# drmod-rs

## Project Overview

A Rust-based mod injector and HUD overlay for **Metal Gear Rising: Revengeance**. The project consists of two components:

- **Binary (`drmod`)**: Injects a DLL into the running game process
- **Library (`drmod_rs_lib`)**: Hooks into DirectX 9 to render an ImGui overlay that reads game memory in real-time

The overlay currently displays:
- Current and previous run start times (from SQLite database)
- Player coordinates (X, Y, Z) read from memory offsets
- Player HP
- Game menu status (In Game, Pause Menu, etc.)
- Equipped weapons: Main, Custom (numeric ID + name), Sub (numeric ID from PlayerManagerImplement)
- Saved position with teleport and on-screen projection

Built with [hudhook](https://github.com/veeenu/hudhook) for DirectX hooking and [imgui-rs](https://github.com/imgui-rs/imgui-rs) for the UI.

## Architecture

```
src/
├── main.rs    # Injector binary — finds game process, injects DLL
└── lib.rs     # HUD library — hooks DX9, renders ImGui overlay, reads game memory
```

**Key memory offsets** (from `lib.rs`):
- Static pointer to player object: `base + 0x177B4A4`
- Position X/Y/Z: offsets `0x50`, `0x54`, `0x58` from player object pointer
- HP: offset `0x870` from player object pointer
- Game menu status: `base + 0x17E9F9C` (enum 0-18)
- PlayerManagerImplement pointer: `base + 0x17EA100`
  - Main weapon: offset `0xE0` from PlayerManagerImplement
  - Custom weapon: offset `0xE4` from PlayerManagerImplement
  - Sub weapon: offset `0xE8` from PlayerManagerImplement

See `game/SDK_ANALYSIS.md` for full MGR plugin SDK analysis.

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

### Language & Edition

- Rust 2024 edition
- UI messages are in Russian

### Dependencies

| Crate | Purpose |
|-------|---------|
| `hudhook` (0.9.0) | DirectX hooking and injection |
| `imgui` (0.12.0) | ImGui bindings for UI rendering |
| `windows` (0.62.2) | Windows API (UI windows, module loading) |
| `rusqlite` (0.40.1, bundled) | SQLite for persisting run data (startup times, future: config, stats) |
| `chrono` (0.4.45) | Time formatting for run timestamps |

### Notes

- **Thread safety**: `HelloHud` has `unsafe impl Send/Sync` because hudhook requires it for the render loop. This is safe since addresses are computed once in `new()` and never mutated.
- All static addresses (`0x177B4A4`, `0x17E9F9C`, `0x17EA100`) are calculated once at init time, not per-frame, for performance.
- The `mgr-plugin-sdk/` directory contains a C++ SDK with 529 reverse-engineered game headers. `game/SDK_ANALYSIS.md` has the analysis.
- Weapon type IDs are raw `int` values — the SDK has no enum mapping weapon names to IDs. IDs must be discovered through runtime experimentation.
- Custom weapon ID → name mapping (in `custom_weapon_name()`): `0` → `None`, `2` → `Polearm`, `3` → `Sai`, `4` → `Pincer`. Unknown IDs show as `Unknown`.
- The `.CT` file in the root (`METAL GEAR RISING REVENGEANCE (1).CT`) is a Cheat Engine table, used to discover memory offsets.
- Error handling uses Windows `MessageBoxW` for user-facing errors
- The `show_msgbox` function encodes text as UTF-16 for the Windows API
- Library is compiled as both `cdylib` (for injection) and `rlib` (for the binary to link against)

### Run Persistence (SQLite)

On DLL load, a SQLite database is created/opened at `%LOCALAPPDATA%\drmod\runs.db`. The directory is auto-created on first run.

**Schema:**
```sql
CREATE TABLE IF NOT EXISTS runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    started_at TEXT NOT NULL
);
```

**Behavior:**
- `init_db()` in `lib.rs` runs once in `HelloHud::new()` — reads the previous run's `started_at`, inserts the current `chrono::Local::now()` timestamp
- UI displays `Current run:` and `Previous run:` (or `N/A` if no prior run or DB unavailable)
- All errors are silently handled — if `LOCALAPPDATA` is unset, directory creation fails, or SQLite fails, the fields gracefully fall back to showing only the current time and `N/A` for previous

### Input Handling

The overlay supports keyboard input via hudhook's built-in WndProc hook — it intercepts `WM_KEYDOWN`/`WM_KEYUP` messages from the game window and feeds them to imgui-rs through `Io::add_key_event()`.

**Key detection** (in `HelloHud::render()`):
- `ui.is_key_down(Key::*)` — клавиша зажата
- `ui.is_key_pressed_no_repeat(Key::*)` — однократное нажатие
- `ui.is_key_pressed(Key::*)` — нажатие с автоповтором

All `imgui::Key` variants (including `Key::Keypad0`–`Key::Keypad9`) are available. See `Cargo registry imgui-0.12.0/src/input/keyboard.rs` for the full enum.

**Current bindings:**
| Key | Action |
|-----|--------|
| `NumPad0` | Toggle `test_flag` (debug/development use only) |
| `NumPad1` | +10m к Y-координате игрока (прямая запись в память) |
| `NumPad2` | Save current position |
| `NumPad3` | Teleport to saved position |

Memory writes use raw `*mut f32` pointers — since the DLL is injected, it has direct access to game memory.
