---
name: ghost-render
description: Load a previous segment's position trace from SQLite and render a trailing ghost marker in an imgui-rs overlay — binary search per frame, red color, via draw_world_pos. Uses ActiveSegment struct and load_best_ghost from segment module.
source: auto-skill
extracted_at: '2026-07-14T11:20:12.000Z'
---

# Ghost Render — Best-Segment Position Trace Overlay

When the player replays a mission, load the position trace (every frame) from the fastest previous completion and render a trailing "ghost" marker at the position the previous run was at the same elapsed time. This gives the player a visual target to beat.

Relies on: `segment-tracking` skill (for segment_positions table), `world-to-screen` skill (for `draw_world_pos`).

## When to use

- You already record per-frame positions during segments (see `segment-tracking`)
- The player wants to see their previous best run's position as a reference during a new attempt
- The project already has `world_to_screen` / `draw_world_pos` for projecting 3D → 2D

## Procedure

### 1. Add ghost fields to the struct

```rust
struct HelloHud {
    // ActiveSegment replaces separate segment fields (see segment-tracking skill)
    active_segment: Option<segment::ActiveSegment>,
    ghost_positions: Vec<(f32, f32, f32, i64)>, // (x, y, z, duration_ms) sorted by duration
    ghost_label: String,                          // "Best 03:47.250"
}
```

Initialized empty in `new()`:
```rust
ghost_positions: Vec::new(),
ghost_label: String::new(),
```

### 2. Load ghost trace at segment start

In the `SEGMENT START` block, use `segment::load_best_ghost`:

```rust
// Query best segment for this mission + load its position trace
self.ghost_positions.clear();
self.ghost_label.clear();

if let Some(ref conn) = self.db_conn {
    let (fastest_ms, positions) = segment::load_best_ghost(conn, mission_id);
    if let Some(best_ms) = fastest_ms {
        self.ghost_label = format!("Best {}", format_duration_ms(best_ms as u64));
    }
    self.ghost_positions = positions;
    // fastest_ms is stored in ActiveSegment below
}

self.active_segment = Some(segment::ActiveSegment {
    start_instant: Instant::now(),
    started_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    mission_id,
    mission_name: mission_name_str.clone(),
    fastest_ms,
});
```

`segment::load_best_ghost` returns `(Option<i64>, Vec<(f32, f32, f32, i64)>)` — the fastest duration and positions sorted by `duration_ms ASC`, already enabling binary search.

### 3. Binary search per frame + render

After the saved position rendering block, add ghost rendering:

```rust
// --- GHOST RENDER ---
if self.active_segment.is_some() && !self.ghost_positions.is_empty() && !self.ghost_label.is_empty() {
    if let (Some(camera_addr), Some(ref seg)) =
        (self.camera_ptr_addr, self.active_segment.as_ref())
    {
        let current_ms = seg.start_instant.elapsed().as_millis() as i64;
        let idx = self.ghost_positions
            .partition_point(|&(_, _, _, dur)| dur <= current_ms);
        if idx > 0 {
            let (gx, gy, gz, _) = self.ghost_positions[idx - 1];
            draw_world_pos(
                ui, (gx, gy, gz), camera_addr.as_ptr(),
                0xFF_00_00_FF,  // red on-screen
                0xFF_00_40_C0,  // dim red off-screen circle
                0xFF_40_20_C0,  // dim red off-screen text
                &self.ghost_label,
            );
        }
    }
}
```

Note: `self.active_segment` is `Option<segment::ActiveSegment>` — the `if let Some(ref seg)` pattern extracts elapsed time via `seg.start_instant`.

**How `partition_point` works:** returns the index of the first element where the predicate is `false`. `dur <= current_ms` is `true` for all positions up to the current elapsed time, so `idx` is the count of valid positions. `idx - 1` is the last valid position (largest duration ≤ current).

If `idx == 0`, the current elapsed is less than the first recorded position — nothing to render.

### 4. Clear ghost data when segment ends

In the `SEGMENT END` block, alongside clearing segment fields:

```rust
self.ghost_positions.clear();
self.ghost_label.clear();
```

### 5. Colors reference

ABGR format (`0xAA_BB_GG_RR`):

| Purpose | Color | ABGR |
|---------|-------|------|
| Ghost on-screen circle | Pure red | `0xFF_00_00_FF` |
| Ghost off-screen circle | Dim red | `0xFF_00_40_C0` |
| Ghost off-screen text | Dim red | `0xFF_40_20_C0` |
| Saved on-screen | Green | `0xFF_00_FF_00` |
| Saved off-screen | Orange | `0xFF_00_80_FF` |

## Performance

- `partition_point` is O(log N) — negligible even for 10K+ positions
- The `draw_world_pos` call does ~50 bytes of unsafe reads per frame (camera matrix + position) — trivial
- Position trace loaded once at segment start — no per-frame DB queries

## Verification

1. Complete a mission → segment record + positions written to DB
2. Restart the same mission → ghost marker appears (red dot tracking your previous path)
3. The ghost should lag behind when you go faster, stay ahead when you go slower
4. If no previous segment exists for that mission → no ghost (graceful)

## Related skills

- `segment-tracking` — produces the segment_positions data this skill reads
- `world-to-screen` — provides `draw_world_pos` and projection math
- `rusqlite-injected-dll` — DB connection pattern
