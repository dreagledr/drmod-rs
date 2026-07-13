---
name: read-mission-level
description: Read the current mission/level ID and name from MGR:R game memory via static addresses discovered in RedTrainer and display them in an imgui-rs overlay
source: auto-skill
extracted_at: '2026-07-13T15:20:00.000Z'
---

# Read Current Mission / Level (MGR:R)

Read the current mission ID and human-readable name from static memory addresses used by the game engine. Display them in an imgui-rs overlay to give the player context (which level they are in) beyond raw coordinates.

## When to use

- The overlay shows player coordinates but the user wants to know *which level/mission* they're in
- You need static data that persists across frames and doesn't require pointer chasing
- The project is an imgui-rs + hudhook overlay injected into MGR:R (see `QWEN.md` in drmod-rs)

## Data source

From the **MGR-RedTrainer** project (`RTFunctions/functions.cpp`, `RedTrainer::setMission`):

| Address | Type | Content |
|---------|------|---------|
| `base + 0x1764670` | `i32` | Mission ID (story/VR) |
| `base + 0x1764674` | `[u8]` | Mission name (null-terminated ASCII string) |
| `base + 0x1766004` | `i32` | Alternate mission ID (used for the "other mode") |
| `base + 0x1766008` | `[u8]` | Alternate mission name |

The game appears to use a two-slot scheme: if `0x1764670` is non-zero, that slot is active; otherwise `0x1766004` holds the current mission. In practice, reading both and picking the non-zero one is reliable.

These are **static global variables** — no pointer chasing needed. Read them directly from `base + offset`.

## Procedure

### 1. Add fields to the struct

No new persistent state is needed; the values are read per-frame. Add optional fields only if you want to cache across frames:

```rust
struct HelloHud {
    // ...existing fields...
    /// Optional: cache the last-read mission string to avoid per-frame allocation
    current_mission: Option<String>,
}
```

### 2. Read mission ID and name

In `render()`, after the `base_addr` is confirmed non-zero:

```rust
if self.base_addr != 0 {
    // Read mission ID from both slots
    let mission_id_1 = unsafe { *((self.base_addr + 0x1764670) as *const i32) };
    let mission_id_2 = unsafe { *((self.base_addr + 0x1766004) as *const i32) };

    let (mission_id, name_offset) = if mission_id_1 != 0 {
        (mission_id_1, self.base_addr + 0x1764674)
    } else {
        (mission_id_2, self.base_addr + 0x1766008)
    };

    // Read null-terminated string (ASCII, max ~32 bytes expected)
    let mission_name = unsafe {
        let ptr = name_offset as *const u8;
        let mut len = 0usize;
        while len < 64 && *ptr.add(len) != 0 {
            len += 1;
        }
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len))
    };

    ui.separator();
    ui.text(format!("Mission: {} (ID: {})", mission_name, mission_id));
}
```

**Safety notes:**
- The string is expected to be ASCII (observed in PhaseInfo.xml: `PD30_MISSION1`, `PE39_CLEAR_01`, etc.). If non-ASCII appears, use `String::from_utf8_lossy` instead of `from_utf8_unchecked`.
- The 64-byte cap prevents runaway reads if the string is somehow not null-terminated.
- `mission_id` is `i32` but effectively a `short` (16-bit) based on RedTrainer's signature. Casting or masking to `u16` may be desirable: `(mission_id & 0xFFFF) as u16`.

### 3. Display in the UI window

Add a section inside the existing ImGui window (`.build(|| { ... })`):

```rust
ui.text(format!("Mission: {} (0x{:04X})", mission_name, mission_id));
```

If the mission name is empty or the ID is 0, the game may be in a menu/transition. Guard:

```rust
if mission_id != 0 {
    ui.text(format!("Mission: {} (0x{:04X})", mission_name, mission_id));
} else {
    ui.text_colored([0.5, 0.5, 0.5, 1.0], "Mission: N/A");
}
```

### 4. Optional: human-readable mapping

The raw mission names (`PD30_MISSION1`, `PE39_CLEAR_01`) are internal phase identifiers. A human-readable mapping can be built from `MGR-RedTrainer/readme/PhaseInfo.xml`:

```rust
fn mission_display_name(raw: &str) -> &str {
    match raw {
        "PD20_VR_01" => "VR Mission 01",
        "PD30_MISSION1" => "File R-01: Coup d'État",
        "PD30_MISSION2" => "File R-02: Research Facility",
        // ... add from PhaseInfo.xml
        _ => raw, // fallback: show raw name
    }
}
```

The full list of ~50 phases is in `MGR-RedTrainer/readme/PhaseInfo.xml`. Extract `<Name>` entries for the mapping.

## Integration with existing code (drmod-rs)

The `render()` method already reads `ui.io().display_size`, game menu status, player coords, and saved position. Add the mission read **after** the `base_addr != 0` check and **before** the player-object block (it doesn't depend on the player pointer).

Placement in the UI window:
```
Status: In Game           ← existing
Mission: PD30_MISSION1    ← new
--- separator ---
Position: ...             ← existing
Saved Position: ...       ← existing
Screen projection debug:  ← existing
HP: ...                   ← existing
```

## Verification

1. `cargo build --release` — no errors
2. Inject into a running game
3. While in a mission, check the overlay shows a non-zero mission ID and a readable name
4. Load different saves or progress to a new chapter — the name should change
5. In the main menu, the ID should be 0 / name empty — guard handles this gracefully

## Common pitfalls

| Symptom | Cause |
|---------|-------|
| Mission name is garbled/garbage | String not ASCII; use `from_utf8_lossy` |
| Mission always shows N/A | Wrong base address or the game hasn't loaded a mission yet |
| ID and name don't change between levels | You're reading from the alternate slot that's stale; try the other slot first |
| Read access violation | `base_addr` is 0 (DLL loaded without the game process) — guard with `if base_addr != 0` |
