# QWEN.md — drmod-rs

## Project Overview

**drmod-rs** is a Rust-based mod injector for the PC game **Metal Gear Rising: Revengeance**. It works by:

1. Finding the game process by window title (default: `"METAL GEAR RISING: REVENGEANCE"`).
2. Injecting a compiled DLL (`drmod_rs_lib.dll`) into the process using `hudhook`.
3. Rendering an ImGui overlay inside the game via DirectX 9 (or DX11, commented out) hooks.

The project produces two artifacts:
- **`drmod`** (binary) — the injector CLI that locates the game process and performs DLL injection.
- **`drmod_rs_lib`** (cdylib) — the DLL that gets injected; contains the ImGui overlay logic.

### Key dependencies
- **hudhook** (`0.9.0`) — DirectX hooking / ImGui overlay infrastructure for games.
- **imgui** (`0.12.0`) — Dear ImGui Rust bindings.
- **windows** (`0.62.2`) — Windows API bindings (used in the injector for `MessageBoxW` and process interaction).

### Edition
The project uses **Rust edition 2024**.

## Building and Running

### Prerequisites
- Rust toolchain (stable or nightly, depending on edition 2024 support).
- A running instance of *Metal Gear Rising: Revengeance* (or any game with a custom window name).

### Commands
```bash
# Build both the injector binary and the library (DLL)
cargo build --release

# The DLL will be at: target/debug/drmod_rs_lib.dll (or target/release/)
# The injector binary will be at: target/debug/drmod.exe
```

### Usage
```bash
# Default: inject into "METAL GEAR RISING: REVENGEANCE"
cargo run --release --bin drmod

# Custom window name
cargo run --release --bin drmod -- -n "My Game Window"
cargo run --release --bin drmod -- --name "My Game Window"
```

## Project Structure

```
drmod-rs/
├── Cargo.toml          # Project manifest; defines binary + cdylib targets
├── src/
│   ├── lib.rs          # ImGui overlay logic (HelloHud — renders "Hello, world!" + elapsed time)
│   └── main.rs         # Injector CLI (process lookup + DLL injection)
└── target/             # Build output (gitignored)
```

## Development Notes

- **Hook selection**: DX9 hooks are active in `lib.rs`; DX11 hooks are commented out. Switch by toggling the `hudhook!(...)` macro invocation.
- **Error messages** in the injector are displayed in Russian (e.g., `"Ошибка при старте drmod"`, `"Не смогли найти..."`).
- The ImGui window is intentionally unnamed (`"##hello"`) to prevent user interaction — it's a debug overlay.
- No test infrastructure is currently in place.

## Conventions

- Single-source-file crates: `lib.rs` and `main.rs` each contain all logic for their respective targets.
- No linting or formatting config (e.g., `rustfmt.toml`, `.clippy.toml`) is present. Default `rustfmt` / `clippy` conventions apply.
