---
name: segment-tracking
description: Per-frame state-machine tracking of a game memory pointer + mission data to detect gameplay segments, with position-gated start via a DB-backed conditions table — using standalone detection functions, an ActiveSegment struct wrapped in Option, per-frame position buffer, bulk SQLite insert on end, and live elapsed + best time in an imgui-rs overlay
source: auto-skill
extracted_at: '2026-07-14T10:56:30.486Z'
---

# Segment Tracking — Gameplay Phase Detection via Memory Pointer State

Detect continuous gameplay segments by monitoring game memory (player pointer + mission ID/name + optionally player position). On each frame, call standalone detection functions to decide whether a segment should start or end. Store active segment state in an `Option<ActiveSegment>` struct on the overlay context, record per-frame positions in a buffer, and bulk-insert into SQLite on segment end.

## When to use

- The overlay already reads a game pointer that indicates "in-game" vs "not in-game" (player object, level pointer, game state enum)
- You want to track time spent per segment/mission/level automatically
- The project is an imgui-rs + hudhook overlay with rusqlite persistence (see `rusqlite-injected-dll` skill)
- You need best-time tracking per mission without manual start/stop
- You want to gate segment starts on player position (e.g. only start when near a known spawn point)

## Core pattern: ActiveSegment struct + detection functions

**Key insight:** instead of spreading active-segment state across 6 individual `Option<T>` fields, group them into a single struct and store as `Option<ActiveSegment>`. This eliminates N separate `None` checks and makes state transitions atomic (single assignment).

### The ActiveSegment struct

```rust
struct ActiveSegment {
    start_instant: Instant,
    started_at: String,
    mission_id: i32,
    mission_name: String,
    fastest_ms: Option<i64>,
}
```

On the overlay struct, replace individual fields:

```rust
struct HelloHud {
    // OLD (removed):
    // segment_active: bool,
    // segment_start_instant: Option<Instant>,
    // segment_started_at: Option<String>,
    // segment_mission_id: Option<i32>,
    // segment_mission_name: Option<String>,
    // segment_fastest_ms: Option<i64>,

    // NEW:
    active_segment: Option<ActiveSegment>,
    segment_was_active: bool,  // edge detector from previous frame
    start_conditions: HashMap<i32, (f32, f32, f32)>,  // mission_id → (x, y, z)
    position_buffer: Vec<(f32, f32, f32, i64)>,
    ghost_positions: Vec<(f32, f32, f32, i64)>,
    ghost_label: String,
}
```

All initialized as `None` / `false` / empty in `new()`.

### Standalone detection functions (not methods)

Use free functions — they take all state explicitly and return a pure `bool`. This is testable independently of the overlay struct.

```rust
fn segment_should_start(
    player_ptr: *mut u8,
    mission_id: i32,
    mission_name: &str,
    pos: (f32, f32, f32),
    start_conditions: &HashMap<i32, (f32, f32, f32)>,
    active_segment: Option<&ActiveSegment>,
) -> bool {
    // Never start if a segment is already active
    if active_segment.is_some() {
        return false;
    }
    // Player must be present and mission must be valid
    if player_ptr.is_null() || mission_id == 0 || mission_name.is_empty() {
        return false;
    }
    // If start conditions exist for this mission, check position delta
    if let Some(&(sx, sy, sz)) = start_conditions.get(&mission_id) {
        let (px, py, pz) = pos;
        if (px - sx).abs() > 0.1 || (py - sy).abs() > 0.1 || (pz - sz).abs() > 0.1 {
            return false;
        }
    }
    true
}

fn segment_should_end(
    player_ptr: *mut u8,
    mission_id: i32,
    mission_name: &str,
    active_segment: Option<&ActiveSegment>,
) -> bool {
    active_segment.is_some()
        && (player_ptr.is_null() || mission_id == 0 || mission_name.is_empty())
}
```

**Important:** `segment_should_end` does NOT require position — it can be called even when `player_ptr` is null. `segment_should_start` DOES require position (for the delta check), so call it only when `player_ptr` is valid.

### Frame ordering in render()

The detection functions must be called in the correct order relative to player null-check:

```rust
// segment_should_end is safe even with null player_ptr
let should_end = segment_should_end(
    player_obj_ptr, mission_id, &mission_name_str,
    self.active_segment.as_ref(),
);

// --- SEGMENT END ---
if should_end {
    // ... save to DB, clear position_buffer, ghost, active_segment = None
}

if player_obj_ptr.is_null() {
    // display "Player pointer is NULL" message
} else {
    // Read position (safe: player_ptr is not null)
    let pos_x = unsafe { *(player_obj_ptr.add(0x50) as *const f32) };
    let pos_y = unsafe { *(player_obj_ptr.add(0x54) as *const f32) };
    let pos_z = unsafe { *(player_obj_ptr.add(0x58) as *const f32) };

    let should_start = segment_should_start(
        player_obj_ptr, mission_id, &mission_name_str,
        (pos_x, pos_y, pos_z), &self.start_conditions,
        self.active_segment.as_ref(),
    );

    // --- SEGMENT START ---
    if should_start {
        self.active_segment = Some(ActiveSegment {
            start_instant: Instant::now(),
            started_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            mission_id,
            mission_name: mission_name_str.clone(),
            fastest_ms: None,  // populated below from DB
        });
        // ... query best time, load ghost positions ...
    }

    // Record position buffer
    if let Some(ref seg) = self.active_segment {
        let dur = seg.start_instant.elapsed().as_millis() as i64;
        self.position_buffer.push((pos_x, pos_y, pos_z, dur));
    }
}

// Update previous-frame state at END of frame
self.segment_was_active = self.active_segment.is_some();
```

**Why this ordering matters:**

- `segment_should_end` fires based on *current* `active_segment` state; it doesn't need position. Call it first so the end happens even when the player vanishes.
- `segment_should_start` needs position — call it only inside the `else` branch where `player_ptr` is guaranteed non-null.
- The position buffer recording uses `self.active_segment` directly (no separate `self.segment_active` flag needed).

### Display in UI

```rust
if let Some(ref seg) = self.active_segment {
    let elapsed_ms = seg.start_instant.elapsed().as_millis() as u64;
    ui.text(format!("Segment: {}", format_duration_ms(elapsed_ms)));
    if let Some(best_ms) = seg.fastest_ms {
        ui.text(format!("Best:    {}", format_duration_ms(best_ms as u64)));
    } else {
        ui.text_colored([0.5, 0.5, 0.5, 1.0], "Best:    N/A");
    }
} else {
    ui.text_colored([0.5, 0.5, 0.5, 1.0], "No active segment");
}
```

### Ghost rendering

Reference `self.active_segment` instead of separate fields:

```rust
if self.active_segment.is_some() && !self.ghost_positions.is_empty() && !self.ghost_label.is_empty() {
    if let (Some(camera_addr), Some(ref seg)) =
        (self.camera_ptr_addr, self.active_segment.as_ref())
    {
        let current_ms = seg.start_instant.elapsed().as_millis() as i64;
        // ... partition_point binary search, draw_world_pos ...
    }
}
```

## Position-gated start: segment_start_conditions table

To prevent segments from starting when the player loads mid-level (e.g. after a checkpoint reload), define per-mission spawn coordinates. A segment only starts when the player is within 0.1m of a known start position.

### DB table

```sql
CREATE TABLE IF NOT EXISTS segment_start_conditions (
    mission_id INTEGER PRIMARY KEY,
    start_x REAL NOT NULL,
    start_y REAL NOT NULL,
    start_z REAL NOT NULL
)
```

### Loading at init

In `init_db()`, after creating tables, load conditions into a `HashMap`:

```rust
let mut start_conditions = HashMap::new();
if let Ok(mut stmt) = conn.prepare(
    "SELECT mission_id, start_x, start_y, start_z FROM segment_start_conditions"
) {
    if let Ok(rows) = stmt.query_map([], |row| {
        Ok((row.get::<_, i32>(0)?, row.get::<_, f32>(1)?,
            row.get::<_, f32>(2)?, row.get::<_, f32>(3)?))
    }) {
        for row in rows.flatten() {
            start_conditions.insert(row.0, (row.1, row.2, row.3));
        }
    }
}
```

Return it alongside the connection so the caller can store it:

```rust
fn init_db() -> (String, Option<String>, Option<Connection>, HashMap<i32, (f32, f32, f32)>) {
    // ...
    (current, prev, Some(conn), start_conditions)
}
```

### Populating conditions

Insert rows manually via any SQLite tool (or later: in-game keybind to capture current position):

```sql
INSERT INTO segment_start_conditions (mission_id, start_x, start_y, start_z)
VALUES (257, -12.5, 0.0, 45.3);
```

If no row exists for a given `mission_id`, the position check is skipped — the segment starts immediately when player + mission are valid (backward compatible).

## DB schema: segments + positions

```sql
CREATE TABLE IF NOT EXISTS segments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    mission_id INTEGER NOT NULL,
    mission_name TEXT NOT NULL,
    started_at TEXT NOT NULL,
    duration_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS segment_positions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    segment_id INTEGER NOT NULL,
    pos_x REAL NOT NULL,
    pos_y REAL NOT NULL,
    pos_z REAL NOT NULL,
    duration_ms INTEGER NOT NULL,
    FOREIGN KEY (segment_id) REFERENCES segments(id) ON DELETE CASCADE
);
```

Enable foreign key enforcement:

```sql
PRAGMA foreign_keys = ON;
```

## Segment end: deferred DB insert

INSERT happens on segment **end** only — avoids unfinished rows if the DLL unloads mid-segment.

```rust
if should_end {
    if let Some(ref seg) = self.active_segment {
        if let Some(ref conn) = self.db_conn {
            let duration_ms = seg.start_instant.elapsed().as_millis() as i64;
            let _ = conn.execute(
                "INSERT INTO segments (mission_id, mission_name, started_at, duration_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![seg.mission_id, seg.mission_name, seg.started_at, duration_ms],
            );

            // Bulk-insert position buffer
            if !self.position_buffer.is_empty() {
                let segment_id = conn.last_insert_rowid();
                let _ = conn.execute("BEGIN", []);
                let mut stmt = conn.prepare(
                    "INSERT INTO segment_positions
                     (segment_id, pos_x, pos_y, pos_z, duration_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                ).unwrap();
                for &(x, y, z, dur) in &self.position_buffer {
                    let _ = stmt.execute(rusqlite::params![segment_id, x, y, z, dur]);
                }
                let _ = conn.execute("COMMIT", []);
            }
        }
    }
    self.position_buffer.clear();
    self.ghost_positions.clear();
    self.ghost_label.clear();
    self.active_segment = None;
}
```

## Code organization: extract to `src/segment.rs`

Once the segment logic stabilizes, extract it into a dedicated module. This keeps `lib.rs` focused on the overlay render loop and makes segment code testable independently.

### Module structure

```rust
// src/segment.rs

pub struct ActiveSegment { ... }

pub fn segment_should_start(...) -> bool { ... }
pub fn segment_should_end(...) -> bool { ... }

/// Creates segments, segment_positions, segment_start_conditions tables
pub fn create_segment_tables(conn: &Connection) -> Result<(), rusqlite::Error> { ... }

/// Loads mission_id → (x, y, z) from segment_start_conditions
pub fn load_start_conditions(conn: &Connection) -> HashMap<i32, (f32, f32, f32)> { ... }

/// Inserts segment row + bulk-inserts position buffer via BEGIN/COMMIT
pub fn finish_segment(conn: &Connection, seg: &ActiveSegment, positions: &[(f32, f32, f32, i64)]) { ... }

/// Returns (fastest_ms, ghost_positions) for a mission, or (None, empty) if no prior run
pub fn load_best_ghost(conn: &Connection, mission_id: i32) -> (Option<i64>, Vec<(f32, f32, f32, i64)>) { ... }
```

### Usage in `lib.rs`

```rust
mod segment;

// In init_db():
segment::create_segment_tables(&conn)?;
let start_conditions = segment::load_start_conditions(&conn);

// In render() — segment end:
if should_end {
    if let (Some(ref seg), Some(ref conn)) = (...)
        segment::finish_segment(conn, seg, &self.position_buffer);
    // ...
}

// In render() — segment start:
let (fastest_ms, positions) = segment::load_best_ghost(conn, mission_id);
self.ghost_positions = positions;
self.active_segment = Some(segment::ActiveSegment { ... });
```

The `format_duration_ms` utility stays in `lib.rs` — the caller creates the ghost label after `load_best_ghost` returns raw data.

### init_db return type

When `init_db` loads start_conditions, extend the return tuple:

```rust
fn init_db() -> (String, Option<String>, Option<Connection>, HashMap<i32, (f32, f32, f32)>) {
    // ...
    if segment::create_segment_tables(&conn).is_err() {
        return (current, None, Some(conn), HashMap::new());
    }
    let start_conditions = segment::load_start_conditions(&conn);
    // ...
    (current, prev, Some(conn), start_conditions)
}
```

All early-return paths must include `HashMap::new()` as the fourth element.

## Related skills

- `rusqlite-injected-dll` — base pattern for SQLite persistence in injected DLLs
- `read-mission-level` — reading mission ID/name from game memory
- `ghost-render` — loading a previous segment's position trace and rendering a trailing ghost marker
