---
name: game-data-module
description: Extract game-specific reverse-engineered enums and ID-to-name helpers from the main overlay file into a dedicated src/game.rs module — pub types, repr(i32) for transmute enums, and mod game wiring
source: auto-skill
extracted_at: '2026-07-14T15:16:00.000Z'
---

# Game Data Module — Reverse-Engineered Types in `src/game.rs`

As an injected overlay grows, lib.rs accumulates game-specific enums and helper functions discovered through reverse engineering. Extract them into `src/game.rs` to keep the overlay render loop clean and make types reusable.

## When to use

- lib.rs has `enum GameMenuStatus { ... }`, `fn custom_weapon_name(...)`, or similar reverse-engineered constants
- These types are used in the render loop via `transmute` from raw memory integers
- The enum has many variants (10+) and an associated `name()` / `is_in_game()` impl

## Procedure

### 1. Create `src/game.rs`

```rust
// src/game.rs

#[allow(dead_code)]         // variants only created via transmute, never constructed directly
#[repr(i32)]                 // MUST match the integer width read from game memory
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMenuStatus {
    InMenu = 0,
    InGame = 1,
    ProcessPause = 2,
    // ... all known variants with explicit discriminants
    ProcessOutOfPause = 18,
}

impl GameMenuStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::InMenu => "In Menu",
            Self::InGame => "In Game",
            // ...
        }
    }

    pub fn is_in_game(self) -> bool {
        self == Self::InGame
    }
}

pub fn custom_weapon_name(id: i32) -> &'static str {
    match id {
        0 => "None",
        2 => "Polearm",
        3 => "Sai",
        4 => "Pincer",
        _ => "Unknown",
    }
}
```

**Critical details:**

| Item | Why |
|------|-----|
| `#[repr(i32)]` | Without it, the compiler packs the enum into 1 byte (8 bits) because there are < 256 variants. `transmute::<i32, GameMenuStatus>` then fails with "cannot transmute between types of different sizes". The `repr(i32)` forces 4-byte layout matching the memory read width. |
| `#[allow(dead_code)]` | Variants are never constructed by name — only through `transmute`. Without this, every variant triggers a `dead_code` warning. |
| `pub` on enum, methods, function | They were previously `pub(crate)` implicitly in lib.rs. Now they're in a child module — `pub` is required for the parent to access them. |

### 2. Wire into `lib.rs`

```rust
// lib.rs
mod game;

// Replace bare type references:
// GameMenuStatus       → game::GameMenuStatus
// custom_weapon_name() → game::custom_weapon_name()
```

Example transmute update:

```rust
// Before:
Some(unsafe { std::mem::transmute::<i32, GameMenuStatus>(raw_status) })

// After:
Some(unsafe { std::mem::transmute::<i32, game::GameMenuStatus>(raw_status) })
```

### 3. Verify with a clean build

```bash
cargo build --release
```

Ensure no warnings or errors. If you see `transmute` size mismatch — check `#[repr(i32)]` is present.

## Future additions to `game.rs`

As more game data is reverse-engineered, add to this module:

- Weapon type enums
- Difficulty enums
- Mission ID → name tables (if not read from game memory strings)
- Known coordinate constants (spawn points, checkpoint locations)

All should follow the same pattern: `pub`, `#[repr(i32)]` if transmuted, `#[allow(dead_code)]` if variants are only constructed via transmute.

## Related skills

- `segment-tracking` — uses `mission_id` from memory, co-exists with game data types
- `read-mission-level` — reads mission ID/name from game memory, displays alongside game status
