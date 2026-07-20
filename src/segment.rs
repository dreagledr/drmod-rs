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
    Start,
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
/// - `Reset`: активный сегмент есть + `MainMenuLoad` — сброс без сохранения
/// - `End`: активный сегмент с mission_id 280 + `InMenu` + текущий mission_id 210
/// - `Start`: нет активного сегмента + mission_id в хардкод-условиях + позиция совпадает
/// - `None`: ничего не делать
pub fn segment_action(
    mission_id: i32,
    mission_name: &str,
    pos: Option<Vec3>,
    game_menu_status: GameMenuStatus,
    active_segment: Option<&ActiveSegment>,
) -> SegmentAction {
    if let Some(seg) = active_segment {
        // Сброс при выходе в главное меню
        if game_menu_status == GameMenuStatus::MainMenuLoad {
            return SegmentAction::Reset;
        }

        // Хардкод: R-00 → R-01 переход
        if seg.mission_id == 0x0A10
            && game_menu_status == GameMenuStatus::InMenu
            && mission_id == 0x0118
        {
            return SegmentAction::End;
        }

        // Хардкод: R-01 → R-02 переход (mission 280 → 210 через InMenu)
        if seg.mission_id == 0x0118
            && game_menu_status == GameMenuStatus::InMenu
            && mission_id == 0x0210
        {
            return SegmentAction::End;
        }

        // Хардкод: R-02 → R-03 переход
        if seg.mission_id == 0x0210
            && game_menu_status == GameMenuStatus::InMenu
            && mission_id == 0x0310
        {
            return SegmentAction::End;
        }

        // Хардкод: R-03 → R-04 переход
        if seg.mission_id == 0x0310
            && game_menu_status == GameMenuStatus::InMenu
            && mission_id == 0x0410
        {
            return SegmentAction::End;
        }

        // Хардкод: R-04 → R-05 переход
        if seg.mission_id == 0x0410
            && game_menu_status == GameMenuStatus::InMenu
            && mission_id == 0x0510
        {
            return SegmentAction::End;
        }

        // Хардкод: R-05 → R-06 переход
        if seg.mission_id == 0x0510
            && game_menu_status == GameMenuStatus::InMenu
            && mission_id == 0x0610
        {
            return SegmentAction::End;
        }

        // Хардкод: R-06 → R-07 переход
        if seg.mission_id == 0x0610
            && game_menu_status == GameMenuStatus::InMenu
            && mission_id == 0x0710
        {
            return SegmentAction::End;
        }

        // Хардкод: R-07 B qte
        if let Some(pos) = pos {
            if seg.mission_id == 0x0710
                && game_menu_status == GameMenuStatus::InGame
                && (pos.x - (-195.73)).abs() <= 0.1
                && (pos.y - (-7.1)).abs() <= 0.1
                && (pos.z - (-491.38)).abs() <= 0.1
            {
                return SegmentAction::End;
            }
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
                return SegmentAction::Start;
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
