//! SQLite-персистентность Record/Replay: таблицы и bulk-insert кадров.

use rusqlite::Connection;

use super::types::{ReplayFrame, ReplayRunMeta};

/// Сериализация `#[repr(C)]`-структуры в байты (для BLOB в SQLite).
/// Структуры состоят из f32/i32/u32 — без padding, round-trip корректен.
fn as_bytes<T>(v: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }
}

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
        "record" => "INSERT INTO replay_record_frames (replay_id, frame_index, duration_ms, input_unit, state, camera) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        _ => "INSERT INTO replay_playback_frames (replay_id, frame_index, duration_ms, input_unit, state, camera) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
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
            as_bytes(&f.input),
            as_bytes(&f.state),
            as_bytes(&f.camera)
        ]);
    }
    let _ = conn.execute("COMMIT", []);
    Some(replay_id)
}