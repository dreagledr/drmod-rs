# drmod-rs

## Project Overview

A Rust-based mod injector and HUD overlay for **Metal Gear Rising: Revengeance**. The project consists of two components:

- **Binary (`drmod`)**: Injects a DLL into the running game process
- **Library (`drmod_rs_lib`)**: Hooks into DirectX 9 to render an ImGui overlay that reads game memory in real-time

The overlay currently displays:
- Player coordinates (X, Y, Z) read from memory offsets
- Player HP
- Static pointer address for debugging

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

- `target/i686-pc-windows-msvc/release/drmod.exe` — injector binary
- `target/i686-pc-windows-msvc/release/drmod_rs_lib.dll` — HUD library DLL

Both files must be in the same directory for the injector to find the DLL.

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

### Notes

- **Thread safety**: `HelloHud` has `unsafe impl Send/Sync` because hudhook requires it for the render loop. This is safe since the static pointer address is computed once in `new()` and never mutated.
- The static pointer address (`base + 0x177B4A4`) is calculated once at init time, not per-frame, for performance.
- The `.CT` file in the root (`METAL GEAR RISING REVENGEANCE (1).CT`) is a Cheat Engine table, likely used to discover the memory offsets
- Error handling uses Windows `MessageBoxW` for user-facing errors
- The `show_msgbox` function encodes text as UTF-16 for the Windows API
- Library is compiled as both `cdylib` (for injection) and `rlib` (for the binary to link against)
