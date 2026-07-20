---
name: debug-logging
description: Add a per-frame debug log table to a hudhook-injected DLL with buffered bulk inserts — capture game state (player ptr, position, mission, menu status, segment state, HP) every frame for offline analysis of segment start/end conditions
source: auto-skill
extracted_at: '2026-07-14T16:45:09.270Z'
---

# Debug Logging — Per-Frame Game State Trace for Segment Analysis

Record a complete trace of game state every frame into a SQLite debug table. Buffer entries in memory and bulk-insert every N seconds to avoid per-row SQLite overhead. Use the resulting data to analyze and refine segment start/end conditions offline.

## When to use

- You need to understand *why* segment detection fires (or doesn't fire) at specific moments
- You want to correlate player pointer validity, mission changes, menu transitions, and HP with segment boundaries
- The overlay already reads these values per frame for display — you just need to persist them
- The project uses rusqlite with `bundled` feature and WAL mode (see `rusqlite-injected-dll` skill)

## Schema

```sql
CREATE TABLE IF NOT EXISTS debug_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp_ms INTEGER NOT NULL,        -- ms since DLL load
    player_ptr INTEGER NOT NULL,          -- raw pointer value (0 if null)
    pos_x REAL, pos_y REAL, pos_z REAL,   -- NULL when player not present
    mission_id INTEGER NOT NULL,
    mission_name TEXT NOT NULL,
    menu_status INTEGER NOT NULL,         -- raw GameMenuStatus value
    segment_active INTEGER NOT NULL,      -- 0 or 1
    segment_elapsed_ms INTEGER,           -- NULL if no active segment
    hp INTEGER                            -- NULL if player not present
);
```

**Design decisions:**
- `player_ptr` as `INTEGER` (not boolean) — captures pointer *changes* (respawn = new pointer = segment boundary)
- `pos_x/y/z` as nullable `REAL` — `NULL` when player is null, real values otherwise. Simpler to query than sentinel `0.0`
- `menu_status` as raw `INTEGER` — avoids dependency on the enum mapping; analyzable with `WHERE menu_status = 1` (InGame)
- No foreign keys — debug table is write-heavy, standalone, and may be dropped/recreated freely

## Module structure: `src/debug.rs`

Keep debug logic in a dedicated module to avoid cluttering `lib.rs`:

```rust
// src/debug.rs
use rusqlite::Connection;

pub struct DebugEntry {
    pub timestamp_ms: i64,
    pub player_ptr: usize,
    pub pos_x: Option<f32>,
    pub pos_y: Option<f32>,
    pub pos_z: Option<f32>,
    pub mission_id: i32,
    pub mission_name: String,
    pub menu_status: i32,
    pub segment_active: bool,
    pub segment_elapsed_ms: Option<i64>,
    pub hp: Option<i32>,
}

pub fn create_debug_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute("CREATE TABLE IF NOT EXISTS debug_log (...)", ())?;
    Ok(())
}

pub fn flush_debug_buffer(conn: &Connection, buffer: &[DebugEntry]) {
    if buffer.is_empty() {
        return;
    }
    let _ = conn.execute("BEGIN", []);
    let Ok(mut stmt) = conn.prepare(
        "INSERT INTO debug_log (timestamp_ms, player_ptr, pos_x, pos_y, pos_z,
         mission_id, mission_name, menu_status, segment_active, segment_elapsed_ms, hp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    ) else {
        return;
    };
    for e in buffer {
        let _ = stmt.execute(rusqlite::params![
            e.timestamp_ms, e.player_ptr as i64,
            e.pos_x, e.pos_y, e.pos_z,
            e.mission_id, e.mission_name,
            e.menu_status, e.segment_active,
            e.segment_elapsed_ms, e.hp,
        ]);
    }
    let _ = conn.execute("COMMIT", []);
}
```

Use the same `BEGIN`/`COMMIT` bulk-insert pattern as `segment::finish_segment`.

## Integration into `lib.rs`

### 1. Add the module

```rust
mod debug;
```

### 2. Add fields to `HelloHud`

```rust
struct HelloHud {
    // ...existing fields...
    debug_buffer: Vec<debug::DebugEntry>,
    debug_last_flush: Instant,
    dll_load_instant: Instant,  // for timestamp_ms
}
```

Initialize in `new()`:
```rust
debug_buffer: Vec::new(),
debug_last_flush: Instant::now(),
dll_load_instant: Instant::now(),
```

### 3. Create the table at init

In `init_db()`, after `segment::create_segment_tables()`:

```rust
let _ = debug::create_debug_table(&conn);
```

Use `let _ =` — debug table creation failure is non-fatal.

### 4. Capture per-frame data

The debug entry needs values that may be scoped inside different `if` blocks in `render()`. Hoist them to the closure scope:

**`raw_status`** — currently declared inside `if self.base_addr != 0 { let raw_status = ... }`. Move the declaration up:

```rust
let mut raw_status: i32 = 0;
if self.base_addr != 0 {
    // ...
    raw_status = unsafe { ... };
}
```

**Position** — scoped inside `else { let pos_x = ... }`. Declare a separate `Option` before the if/else:

```rust
let mut debug_pos: Option<(f32, f32, f32)> = None;

if player_obj_ptr.is_null() {
    // ...
} else {
    let pos_x = unsafe { ... };
    let pos_y = unsafe { ... };
    let pos_z = unsafe { ... };
    debug_pos = Some((pos_x, pos_y, pos_z));
    // ... rest of existing logic uses pos_x/y/z directly
}
```

### 5. Push entry each frame

At the end of the `.build(|| { ... })` closure, after `self.segment_was_active = ...`:

```rust
// --- DEBUG LOG ---
let seg_elapsed = self.active_segment.as_ref()
    .map(|seg| seg.start_instant.elapsed().as_millis() as i64);
let hp_val = if player_obj_ptr.is_null() {
    None
} else {
    Some(unsafe { *(player_obj_ptr.add(0x870) as *const i32) })
};

let (px, py, pz) = debug_pos
    .map(|(x, y, z)| (Some(x), Some(y), Some(z)))
    .unwrap_or((None, None, None));

self.debug_buffer.push(debug::DebugEntry {
    timestamp_ms: self.dll_load_instant.elapsed().as_millis() as i64,
    player_ptr: player_obj_ptr as usize,
    pos_x: px, pos_y: py, pos_z: pz,
    mission_id,
    mission_name: mission_name_str.clone(),
    menu_status: raw_status,
    segment_active: self.active_segment.is_some(),
    segment_elapsed_ms: seg_elapsed,
    hp: hp_val,
});
```

### 6. Periodic flush (every 10 seconds)

Right after pushing the entry:

```rust
if self.debug_last_flush.elapsed().as_secs() >= 10 {
    if let Some(ref conn) = self.db_conn {
        debug::flush_debug_buffer(conn, &self.debug_buffer);
    }
    self.debug_buffer.clear();
    self.debug_last_flush = Instant::now();
}
```

### 7. Flush on segment end

When a segment ends, flush immediately to capture the boundary:

```rust
if should_end {
    // ... existing finish_segment, clear fields ...
    self.active_segment = None;

    if let Some(ref conn) = self.db_conn {
        debug::flush_debug_buffer(conn, &self.debug_buffer);
    }
    self.debug_buffer.clear();
    self.debug_last_flush = Instant::now();
}
```

### 8. Flush on DLL eject

Before `hudhook::eject()`, flush remaining buffer:

```rust
if ui.button("Выход / Выгрузить DLL") {
    if let Some(ref conn) = self.db_conn {
        debug::flush_debug_buffer(conn, &self.debug_buffer);
    }
    hudhook::eject();
}
```

## Performance considerations

- At 60 FPS, 10 seconds = ~600 rows per batch. `BEGIN`/`COMMIT` with prepared statement handles this efficiently
- `mission_name` string is cloned per frame — acceptable for debug (not production)
- Buffer is a plain `Vec` — push is O(1) amortized, clear resets length without deallocating capacity
- WAL mode (set at init) allows concurrent reads during writes — you can query the DB while the game is running

## Querying the data

After a session, analyze with SQLite:

```sql
-- Find all segment boundaries (active flag transitions)
SELECT timestamp_ms, segment_active, player_ptr, mission_id, menu_status
FROM debug_log
WHERE id IN (
    SELECT id FROM (
        SELECT id, segment_active,
               LAG(segment_active) OVER (ORDER BY id) AS prev_active
        FROM debug_log
    ) WHERE segment_active != prev_active
)
ORDER BY id;

-- See what happened right before a segment ended
SELECT * FROM debug_log
WHERE id > (SELECT MAX(id) FROM debug_log WHERE segment_active = 0 AND segment_active_prev = 1)
ORDER BY id LIMIT 50;
```

## When to disable

- Before shipping a release build — debug logging adds ~600 writes per 10 seconds
- To disable: either don't push entries (check a boolean flag) or drop the table creation
- The buffer flush is guarded by `if let Some(ref conn)` — if `db_conn` fails, debug logging silently stops
