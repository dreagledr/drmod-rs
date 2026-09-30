//! SQLite-персистентность Record/Replay: таблицы и bulk-insert кадров.

use rusqlite::Connection;

use drmod_replay_types::to_bytes;
use super::types::{ReplayFrame, ReplayRunMeta};

/// Создаёт таблицы Record/Replay (если их нет).
pub(crate) fn create_replay_tables(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS replay_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            mission_id INTEGER NOT NULL,
            mission_name TEXT NOT NULL,
            started_at TEXT NOT NULL,
            frame_count INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            source_replay_id INTEGER
        )",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS replay_record_frames (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            replay_id INTEGER NOT NULL,
            frame_index INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            input_unit BLOB NOT NULL,
            state BLOB NOT NULL,
            camera BLOB NOT NULL,
            blade_down INTEGER NOT NULL DEFAULT 0,
            ripper_pressed INTEGER NOT NULL DEFAULT 0,
            raw_down BLOB,
            raw_pressed BLOB,
            enemy BLOB,
            FOREIGN KEY (replay_id) REFERENCES replay_runs(id) ON DELETE CASCADE
        )",
        (),
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS replay_playback_frames (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            replay_id INTEGER NOT NULL,
            frame_index INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            input_unit BLOB NOT NULL,
            state BLOB NOT NULL,
            camera BLOB NOT NULL,
            blade_down INTEGER NOT NULL DEFAULT 0,
            ripper_pressed INTEGER NOT NULL DEFAULT 0,
            raw_down BLOB,
            raw_pressed BLOB,
            enemy BLOB,
            FOREIGN KEY (replay_id) REFERENCES replay_runs(id) ON DELETE CASCADE
        )",
        (),
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_replay_record_frames ON replay_record_frames(replay_id, frame_index)",
        (),
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_replay_playback_frames ON replay_playback_frames(replay_id, frame_index)",
        (),
    )?;
    ensure_replay_frame_columns(conn, "replay_record_frames")?;
    ensure_replay_frame_columns(conn, "replay_playback_frames")?;
    Ok(())
}

/// Миграция старых БД: добавляет колонки blade/ripper/raw, если их нет
/// (таблицы могли быть созданы до расширения схемы). Для свежих таблиц
/// CREATE TABLE уже содержит колонки — ALTER не сработает (дубликат).
fn ensure_replay_frame_columns(conn: &Connection, table: &str) -> Result<(), rusqlite::Error> {
    let existing: Vec<String> = conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| row.get(1))?
        .collect::<Result<_, _>>()?;
    let add = |name: &str, ddl: &str| -> Result<(), rusqlite::Error> {
        if !existing.iter().any(|c| c == name) {
            conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {ddl}"), ())?;
        }
        Ok(())
    };
    add("blade_down", "blade_down INTEGER NOT NULL DEFAULT 0")?;
    add("ripper_pressed", "ripper_pressed INTEGER NOT NULL DEFAULT 0")?;
    add("raw_down", "raw_down BLOB")?;
    add("raw_pressed", "raw_pressed BLOB")?;
    add("enemy", "enemy BLOB")?;
    Ok(())
}

/// Флашит накопленные кадры в БД одним bulk-insert: строка в `replay_runs`
/// и кадры в `replay_record_frames`/`replay_playback_frames` (по `kind`).
/// Паттерн — как `segment::finish_segment`. Возвращает id прогона.
pub(crate) fn flush_replay(
    conn: &Connection,
    meta: &ReplayRunMeta,
    frames: &[ReplayFrame],
) -> Option<i64> {
    let frame_count = frames.len() as i64;
    let _ = conn.execute(
        "INSERT INTO replay_runs (kind, mission_id, mission_name, started_at, frame_count, duration_ms, source_replay_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            meta.kind,
            meta.mission_id,
            meta.mission_name,
            meta.started_at,
            frame_count,
            meta.duration_ms,
            meta.source_replay_id
        ],
    );
    let replay_id = conn.last_insert_rowid();
    if frames.is_empty() {
        return Some(replay_id);
    }

    let sql = match meta.kind {
        "record" => "INSERT INTO replay_record_frames (replay_id, frame_index, duration_ms, input_unit, state, camera, blade_down, ripper_pressed, raw_down, raw_pressed, enemy) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        _ => "INSERT INTO replay_playback_frames (replay_id, frame_index, duration_ms, input_unit, state, camera, blade_down, ripper_pressed, raw_down, raw_pressed, enemy) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    };

    let _ = conn.execute("BEGIN", []);
    let Ok(mut stmt) = conn.prepare(sql) else {
        let _ = conn.execute("ROLLBACK", []);
        return Some(replay_id);
    };
    for f in frames {
        let dur = (f.frame_index as i64) * 1000 / 60;
        let _ = stmt.execute(rusqlite::params![
            replay_id,
            f.frame_index as i64,
            dur,
            to_bytes(&f.input),
            to_bytes(&f.state),
            to_bytes(&f.camera),
            f.blade_down as i64,
            f.ripper_pressed as i64,
            to_bytes(&f.raw_down),
            to_bytes(&f.raw_pressed),
            to_bytes(&f.enemy)
        ]);
    }
    let _ = conn.execute("COMMIT", []);
    Some(replay_id)
}