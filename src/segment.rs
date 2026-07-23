use crate::game::GameMenuStatus;
use rusqlite::Connection;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

static START_CONDITIONS: &[(i32, Vec3)] = &[
    //R-00 start
    (
        0x0A10,
        Vec3 {
            x: 37.7,
            y: 10.1,
            z: -100.0,
        },
    ),
    //R-01 beach start
    (
        0x0118,
        Vec3 {
            x: -24.7,
            y: 12.14,
            z: 120.7,
        },
    ),
    //R-02 mexico start
    (
        0x0210,
        Vec3 {
            x: -88.0,
            y: -0.56,
            z: 106.0,
        },
    ),
    //R-03 start
    (
        0x0310,
        Vec3 {
            x: 2.86,
            y: 0.0,
            z: 70.91,
        },
    ),
    //R-04 start
    (
        0x0410,
        Vec3 {
            x: 0.0,
            y: 13.0,
            z: 48.0,
        },
    ),
    //R-05 start
    (
        0x0510,
        Vec3 {
            x: 534.7,
            y: -324.0,
            z: -814.0,
        },
    ),
    //R-06 start
    (
        0x0610,
        Vec3 {
            x: -32.63,
            y: 9.3,
            z: 0.03,
        },
    ),
    //R-07 start
    (
        0x0710,
        Vec3 {
            x: 74.24,
            y: 15.59,
            z: -66.14,
        },
    ),
];

pub enum SegmentAction {
    None,
    Start { mission_id: i32 },
    End,
    Reset,
}

pub struct ActiveSegment {
    pub start_instant: Instant,
    pub started_at: String,
    pub mission_id: i32,
    pub mission_name: String,
    pub fastest_ms: Option<i64>,
}

/// Определяет необходимое действие с сегментом на основе текущего состояния игры.
///
/// Старты (R-01–R-07): gStr4-переход `PF01 → Pxxx/EV60` (ASL start block).
/// Старт R-00: позиционный (gStr2 пока не читаем).
/// Финиши: ASL-сплиты через gStr/gStr2/rAnim (строгие, без fallback).
/// Сброс: `MainMenuLoad`.
pub fn segment_action(
    mission_id: i32,
    mission_name: &str,
    pos: Option<Vec3>,
    game_menu_status: GameMenuStatus,
    gstr: &str,
    prev_gstr: &str,
    gstr2: &str,
    prev_gstr2: &str,
    r_anim: i32,
    prev_r_anim: i32,
    active_segment: Option<&ActiveSegment>,
) -> SegmentAction {
    if let Some(seg) = active_segment {
        if game_menu_status == GameMenuStatus::MainMenuLoad {
            return SegmentAction::Reset;
        }

        // Финиши миссий (ASL-сплиты, строгие)
        match seg.mission_id {
            0x0A10 => {
                // R-00 finish: gStr2 "BEACH" ← "" && gStr ""
                if gstr2 == "BEACH" && prev_gstr2.is_empty() && gstr.is_empty() {
                    return SegmentAction::End;
                }
            }
            0x0118 => {
                // R-01 finish: gStr "MIST_RESU" ← "MISTRAL03"
                if gstr == "MIST_RESU" && prev_gstr == "MISTRAL03" {
                    return SegmentAction::End;
                }
            }
            0x0210 => {
                // R-02 finish: gStr "EVENT2" + rAnim transition to 43
                if gstr == "EVENT2" && r_anim != prev_r_anim && r_anim == 43 {
                    return SegmentAction::End;
                }
            }
            0x0310 => {
                // R-03 finish: gStr "MON_RESUL" ← "FINISH_QT"
                if gstr == "MON_RESUL" && prev_gstr == "FINISH_QT" {
                    return SegmentAction::End;
                }
            }
            0x0410 => {
                // R-04 finish: gStr "SUN_RESUL" ← "QTE"
                if gstr == "SUN_RESUL" && prev_gstr == "QTE" {
                    return SegmentAction::End;
                }
            }
            0x0510 => {
                // R-05 finish: gStr "" ← "STREET"
                if gstr.is_empty() && prev_gstr == "STREET" {
                    return SegmentAction::End;
                }
            }
            0x0610 => {
                // R-06 finish: gStr "BOSS_END" ← "BOSS"
                if gstr == "BOSS_END" && prev_gstr == "BOSS" {
                    return SegmentAction::End;
                }
            }
            0x0710 => {
                // R-07 finish: rAnim 70 → 297 (Armstrong QTE)
                if r_anim == 297 && prev_r_anim == 70 {
                    return SegmentAction::End;
                }
            }
            _ => {}
        }

        return SegmentAction::None;
    }

    // Нет активного сегмента — проверяем условия старта
    if mission_id == 0 || mission_name.is_empty() {
        return SegmentAction::None;
    }

    if let Some(pos) = pos {
        if let Some(&(_, start_pos)) = START_CONDITIONS.iter().find(|&&(id, _)| id == mission_id) {
            if (pos.x - start_pos.x).abs() <= 0.1
                && (pos.y - start_pos.y).abs() <= 1.0
                && (pos.z - start_pos.z).abs() <= 0.1
            {
                return SegmentAction::Start {
                    mission_id,
                };
            }
        }
    }

    SegmentAction::None
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
    Ok(())
}

#[allow(dead_code)]
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

pub fn finish_segment(conn: &Connection, seg: &ActiveSegment, positions: &[(Vec3, i64)]) {
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
        for &(pos, dur) in positions {
            let _ = stmt.execute(rusqlite::params![segment_id, pos.x, pos.y, pos.z, dur]);
        }
        let _ = conn.execute("COMMIT", []);
    }

    // Keep only the best (fastest) segment per mission_id
    let _ = conn.execute(
        "DELETE FROM segments WHERE mission_id = ?1 AND id != (
            SELECT id FROM segments WHERE mission_id = ?1 ORDER BY duration_ms ASC LIMIT 1
        )",
        [seg.mission_id],
    );
}

pub fn load_best_ghost(conn: &Connection, mission_id: i32) -> (Option<i64>, Vec<(Vec3, i64)>) {
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
            Vec3 {
                x: row.get::<_, f32>(0)?,
                y: row.get::<_, f32>(1)?,
                z: row.get::<_, f32>(2)?,
            },
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
