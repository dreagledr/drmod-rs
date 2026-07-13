---
name: save-load-game-memory
description: Add save/load memory slots (position, HP, etc.) to an imgui-rs game overlay with key bindings and UI display
source: auto-skill
extracted_at: '2026-07-13T14:00:25.083Z'
---

# Save/Load Game Memory Slots (imgui-rs + hudhook)

Add a save/load feature for any game memory value (position, stats, flags) to an imgui-rs overlay injected via hudhook. The pattern uses raw pointer reads/writes and imgui key events.

## When to use

- The project is an imgui-rs + hudhook game overlay (see `QWEN.md` in drmod-rs)
- You need to let the user save a snapshot of game memory values on one key and restore them on another
- You already know the memory offsets (discovered via Cheat Engine or SDK analysis)

## Procedure

### 1. Add a struct field for saved state

In the render loop struct (e.g. `HelloHud`), add an `Option` holding the values to snapshot:

```rust
struct HelloHud {
    // ...existing fields...
    saved_position: Option<(f32, f32, f32)>,   // e.g. (X, Y, Z)
}

impl HelloHud {
    fn new() -> Self {
        Self {
            // ...existing fields...
            saved_position: None,   // init as None
        }
    }
}
```

For a single value use `Option<f32>`, for many use `Option<MyStruct>`.

### 2. Add key handlers in `render()`

Inside the block where the target memory pointer is available (e.g. inside `if !player_obj_ptr.is_null()`):

**Save handler** — read from memory, store in field:
```rust
// NumPad2: save current position
if ui.is_key_pressed_no_repeat(Key::Keypad2) {
    unsafe {
        let x = *(player_obj_ptr.add(0x50) as *const f32);
        let y = *(player_obj_ptr.add(0x54) as *const f32);
        let z = *(player_obj_ptr.add(0x58) as *const f32);
        self.saved_position = Some((x, y, z));
    }
}
```

**Load/teleport handler** — write stored values back to memory:
```rust
// NumPad3: teleport to saved position
if ui.is_key_pressed_no_repeat(Key::Keypad3) {
    if let Some((sx, sy, sz)) = self.saved_position {
        unsafe {
            *(player_obj_ptr.add(0x50) as *mut f32) = sx;
            *(player_obj_ptr.add(0x54) as *mut f32) = sy;
            *(player_obj_ptr.add(0x58) as *mut f32) = sz;
        }
    }
}
```

Use `is_key_pressed_no_repeat` for one-shot actions (save/teleport), `is_key_down` for continuous actions.

### 3. Display saved state in the UI

After the live value display block, add a "Saved X:" section:

```rust
// Сохранённая позиция
ui.separator();
ui.text("Saved Position:");
if let Some((sx, sy, sz)) = self.saved_position {
    ui.text(format!("X: {:.3}", sx));
    ui.text(format!("Y: {:.3}", sy));
    ui.text(format!("Z: {:.3}", sz));
} else {
    ui.text_colored([0.5, 0.5, 0.5, 1.0], "не сохранена");
}
```

Append key binding labels to the existing hotkey hints:

```rust
ui.text("NumPad1: +10m Y");
ui.text("NumPad2: Save position");
ui.text("NumPad3: Teleport");
```

### 4. Update QWEN.md

In the project's `QWEN.md`, update the **Current bindings** table to reflect the new keys, remove any old bindings that were replaced:

```markdown
**Current bindings:**
| Key | Action |
|-----|--------|
| `NumPad1` | +10m к Y-координате игрока |
| `NumPad2` | Save current position |
| `NumPad3` | Teleport to saved position |
```

### 5. Build and verify

```bash
cargo build --release
```

Deploy the DLL, start the game, inject, and test:
- Press save key → saved coords appear in UI
- Move the character → press teleport → character returns to saved spot

## Key bindings checklist

| Key | Action |
|-----|--------|
| `NumPad0` | (reserved for debug/test) |
| `NumPad1` | Usually +10m Y boost |
| `NumPad2` | Save snapshot |
| `NumPad3` | Load/teleport snapshot |

Free keys: `NumPad4`–`NumPad9`. Use `is_key_pressed_no_repeat` for one-shot actions to avoid repeated triggers.

## Known pitfalls

- The pointer to the player object (`player_obj_ptr`) may be NULL in menus — guard writes behind a null check
- All memory addresses are 32-bit (the game is `i686-pc-windows-msvc`)
- `unsafe impl Send/Sync` on the struct is required by hudhook; it is safe as long as captured addresses are computed once in `new()` and never mutated
- imgui `Key::*` variants include `Keypad0`–`Keypad9` — see `imgui-0.12.0/src/input/keyboard.rs` for the full enum
