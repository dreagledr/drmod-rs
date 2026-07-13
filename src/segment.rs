use rusqlite::Connection;
use std::collections::HashMap;
use std::time::Instant;

pub struct ActiveSegment {
    pub start_instant: Instant,
    pub started_at: String,
    pub mission_id: i32,
    pub mission_name: String,
    pub fastest_ms: Option<i64>,
}

pub fn segment_should_start(
    player_ptr: *mut u8,
    mission_id: i32,
    mission_name: &str,
    pos: (f32, f32, f32),
    start_conditions: &HashMap<i32, (f32, f32, f32)>,
    active_segment: Option<&ActiveSegment>,
) -> bool {
    if active_segment.is_some() {
        return false;
    }
    if player_ptr.is_null() || mission_id == 0 || mission_name.is_empty() {
        return false;
    }
    if let Some(&(sx, sy, sz)) = start_conditions.get(&mission_id) {
        let (px, py, pz) = pos;
        if (px - sx).abs() > 0.1 || (py - sy).abs() > 0.1 || (pz - sz).abs() > 0.1 {
            return false;
        }
    }
    true
}

pub fn segment_should_end(
    player_ptr: *mut u8,
    mission_id: i32,
    mission_name: &str,
    active_segment: Option<&ActiveSegment>,
) -> bool {
    active_segment.is_some() && (player_ptr.is_null() || mission_id == 0 || mission_name.is_empty())
}

pub fn create_segment_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS segments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            mission_id INTEGER NOT NULL,
            mission_name TEXT NOT NULL,
            started_at TEXT NOT NULL,
            duration_ms INTEGER NOT NULL
        )",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS segment_positions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            segment_id INTEGER NOT NULL,
            pos_x REAL NOT NULL,
            pos_y REAL NOT NULL,
            pos_z REAL NOT NULL,
            duration_ms INTEGER NOT NULL,
            FOREIGN KEY (segment_id) REFERENCES segments(id) ON DELETE CASCADE
        )",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS segment_start_conditions (
            mission_id INTEGER PRIMARY KEY,
            start_x REAL NOT NULL,
            start_y REAL NOT NULL,
            start_z REAL NOT NULL
        )",
        (),
    )?;
    Ok(())
}

pub fn load_start_conditions(conn: &Connection) -> HashMap<i32, (f32, f32, f32)> {
    let mut map = HashMap::new();
    let Ok(mut stmt) =
        conn.prepare("SELECT mission_id, start_x, start_y, start_z FROM segment_start_conditions")
    else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i32>(0)?,
            row.get::<_, f32>(1)?,
            row.get::<_, f32>(2)?,
            row.get::<_, f32>(3)?,
        ))
    }) else {
        return map;
    };
    for row in rows.flatten() {
        map.insert(row.0, (row.1, row.2, row.3));
    }
    map
}

pub fn finish_segment(
    conn: &Connection,
    seg: &ActiveSegment,
    positions: &[(f32, f32, f32, i64)],
) {
    let duration_ms = seg.start_instant.elapsed().as_millis() as i64;
    let _ = conn.execute(
        "INSERT INTO segments (mission_id, mission_name, started_at, duration_ms) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![seg.mission_id, seg.mission_name, seg.started_at, duration_ms],
    );
    if !positions.is_empty() {
        let segment_id = conn.last_insert_rowid();
        let _ = conn.execute("BEGIN", []);
        let Ok(mut stmt) = conn.prepare(
            "INSERT INTO segment_positions (segment_id, pos_x, pos_y, pos_z, duration_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
        ) else {
            return;
        };
        for &(x, y, z, dur) in positions {
            let _ = stmt.execute(rusqlite::params![segment_id, x, y, z, dur]);
        }
        let _ = conn.execute("COMMIT", []);
    }
}

pub fn load_best_ghost(
    conn: &Connection,
    mission_id: i32,
) -> (Option<i64>, Vec<(f32, f32, f32, i64)>) {
    let Ok((best_id, best_ms)) = conn.query_row(
        "SELECT id, duration_ms FROM segments WHERE mission_id = ?1 ORDER BY duration_ms ASC LIMIT 1",
        [mission_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    ) else {
        return (None, Vec::new());
    };

    let mut positions = Vec::new();
    let Ok(mut stmt) = conn.prepare(
        "SELECT pos_x, pos_y, pos_z, duration_ms FROM segment_positions WHERE segment_id = ?1 ORDER BY duration_ms",
    ) else {
        return (Some(best_ms), positions);
    };
    let Ok(rows) = stmt.query_map([best_id], |row| {
        Ok((
            row.get::<_, f32>(0)?,
            row.get::<_, f32>(1)?,
            row.get::<_, f32>(2)?,
            row.get::<_, i64>(3)?,
        ))
    }) else {
        return (Some(best_ms), positions);
    };
    for row in rows {
        if let Ok(pos) = row {
            positions.push(pos);
        }
    }
    (Some(best_ms), positions)
}
