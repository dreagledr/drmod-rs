//! Логика dbdump: схема колонок экспорта, чтение БД, запись CSV/Parquet.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use arrow::array::{ArrayRef, Float32Array, Int64Array, StringBuilder};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use drmod_replay_types::{from_bytes, CameraState, EnemyState, InputUnit, PlayerState};
use parquet::arrow::arrow_writer::ArrowWriter;
use rusqlite::{params, Connection};

/// Метаданные прогона из `replay_runs`.
#[derive(Clone)]
pub(crate) struct RunMeta {
    pub id: i64,
    pub kind: String,
    pub mission_id: i64,
    pub mission_name: String,
    pub started_at: String,
    pub frame_count: i64,
    pub duration_ms: i64,
    pub source_replay_id: Option<i64>,
}

/// Один кадр, прочитанный из БД (BLOB уже декодирован).
/// `raw_down`/`raw_pressed`/`enemy` — `None`, если колонки отсутствуют
/// (старая схема) или для строки значение NULL.
#[derive(Debug)]
pub(crate) struct Frame {
    pub frame_index: i64,
    pub duration_ms: i64,
    pub input: InputUnit,
    pub state: PlayerState,
    pub camera: CameraState,
    pub blade_down: i64,
    pub ripper_pressed: i64,
    pub raw_down: Option<[u32; 6]>,
    pub raw_pressed: Option<[u32; 6]>,
    pub enemy: Option<EnemyState>,
}

/// Тип колонки экспорта. Все колонки в parquet помечаются nullable:
/// старые строки без raw-данных дают NULL.
#[derive(Clone, Copy)]
enum ColType {
    I64,
    F32,
    Str,
}

/// Ячейка строки экспорта.
#[derive(Clone, Debug)]
pub(crate) enum Cell {
    Int(i64),
    Float(f32),
    Str(String),
    Null,
}

/// Единая схема колонок — её едят оба писателя (CSV и Parquet).
/// Порядок обязан совпадать с `build_row`.
const SCHEMA: &[(&str, ColType)] = &[
    // Мета прогона (повторяется в каждой строке — удобно для join'ов)
    ("replay_id", ColType::I64),
    ("kind", ColType::Str),
    ("mission_id", ColType::I64),
    ("mission_name", ColType::Str),
    ("started_at", ColType::Str),
    ("frame_count", ColType::I64),
    ("duration_ms", ColType::I64),
    ("source_replay_id", ColType::I64),
    // Кадр
    ("frame_index", ColType::I64),
    ("frame_duration_ms", ColType::I64),
    // InputUnit
    ("buttons_down", ColType::I64),
    ("buttons_pressed", ColType::I64),
    ("buttons_released", ColType::I64),
    ("buttons_alternated", ColType::I64),
    ("left_stick_x", ColType::F32),
    ("left_stick_y", ColType::F32),
    ("right_stick_x", ColType::F32),
    ("right_stick_y", ColType::F32),
    ("left_trigger", ColType::F32),
    ("right_trigger", ColType::F32),
    ("valid_input", ColType::I64),
    ("repeat_count", ColType::I64),
    // PlayerState
    ("pos_x", ColType::F32),
    ("pos_y", ColType::F32),
    ("pos_z", ColType::F32),
    ("rotation_x", ColType::F32),
    ("rotation_y", ColType::F32),
    ("rotation_z", ColType::F32),
    ("velocity_x", ColType::F32),
    ("velocity_y", ColType::F32),
    ("velocity_z", ColType::F32),
    ("hp", ColType::I64),
    ("r_anim", ColType::I64),
    ("sword_state", ColType::I64),
    ("sword_hidden", ColType::I64),
    ("input_direction", ColType::F32),
    ("desired_heading", ColType::F32),
    ("button_jump", ColType::I64),
    ("button_light_attack", ColType::I64),
    ("button_heavy_attack", ColType::I64),
    ("button_ninjarun", ColType::I64),
    ("button_blademode", ColType::I64),
    ("ripper_enabled", ColType::I64),
    ("blade_mode_type", ColType::I64),
    // CameraState
    ("cam_pos_x", ColType::F32),
    ("cam_pos_y", ColType::F32),
    ("cam_pos_z", ColType::F32),
    ("cam_look_at_x", ColType::F32),
    ("cam_look_at_y", ColType::F32),
    ("cam_look_at_z", ColType::F32),
    ("cam_roll", ColType::F32),
    ("vp_00", ColType::F32),
    ("vp_01", ColType::F32),
    ("vp_02", ColType::F32),
    ("vp_03", ColType::F32),
    ("vp_10", ColType::F32),
    ("vp_11", ColType::F32),
    ("vp_12", ColType::F32),
    ("vp_13", ColType::F32),
    ("vp_20", ColType::F32),
    ("vp_21", ColType::F32),
    ("vp_22", ColType::F32),
    ("vp_23", ColType::F32),
    ("vp_30", ColType::F32),
    ("vp_31", ColType::F32),
    ("vp_32", ColType::F32),
    ("vp_33", ColType::F32),
    // Производные из pos→lookAt (см. комментарий CameraState в replay-types)
    ("cam_yaw", ColType::F32),
    ("cam_pitch", ColType::F32),
    // Дополнительные поля кадра (новые колонки схемы)
    ("blade_down", ColType::I64),
    ("ripper_pressed", ColType::I64),
    ("raw_down_0", ColType::I64),
    ("raw_down_1", ColType::I64),
    ("raw_down_2", ColType::I64),
    ("raw_down_3", ColType::I64),
    ("raw_down_4", ColType::I64),
    ("raw_down_5", ColType::I64),
    ("raw_pressed_0", ColType::I64),
    ("raw_pressed_1", ColType::I64),
    ("raw_pressed_2", ColType::I64),
    ("raw_pressed_3", ColType::I64),
    ("raw_pressed_4", ColType::I64),
    ("raw_pressed_5", ColType::I64),
    // Ближайший враг (EnemyState; NULL — старая схема без колонки enemy)
    ("enemy_pos_x", ColType::F32),
    ("enemy_pos_y", ColType::F32),
    ("enemy_pos_z", ColType::F32),
    ("enemy_blade_y", ColType::F32),
    ("enemy_anim", ColType::I64),
    ("enemy_frame", ColType::I64),
    ("enemy_hp", ColType::I64),
];

/// Собирает строку экспорта для кадра.
fn build_row(meta: &RunMeta, f: &Frame) -> Vec<Cell> {
    let cam = &f.camera;
    let dx = cam.look_at[0] - cam.pos[0];
    let dy = cam.look_at[1] - cam.pos[1];
    let dz = cam.look_at[2] - cam.pos[2];
    let cam_yaw = dx.atan2(dz);
    let cam_pitch = dy.atan2((dx * dx + dz * dz).sqrt());

    let mut row = vec![
        Cell::Int(meta.id),
        Cell::Str(meta.kind.clone()),
        Cell::Int(meta.mission_id),
        Cell::Str(meta.mission_name.clone()),
        Cell::Str(meta.started_at.clone()),
        Cell::Int(meta.frame_count),
        Cell::Int(meta.duration_ms),
        match meta.source_replay_id {
            Some(v) => Cell::Int(v),
            None => Cell::Null,
        },
        Cell::Int(f.frame_index),
        Cell::Int(f.duration_ms),
        Cell::Int(f.input.buttons_down as i64),
        Cell::Int(f.input.buttons_pressed as i64),
        Cell::Int(f.input.buttons_released as i64),
        Cell::Int(f.input.buttons_alternated as i64),
        Cell::Float(f.input.left_stick[0]),
        Cell::Float(f.input.left_stick[1]),
        Cell::Float(f.input.right_stick[0]),
        Cell::Float(f.input.right_stick[1]),
        Cell::Float(f.input.left_trigger),
        Cell::Float(f.input.right_trigger),
        Cell::Int(f.input.valid_input as i64),
        Cell::Int(f.input.repeat_count as i64),
        Cell::Float(f.state.pos[0]),
        Cell::Float(f.state.pos[1]),
        Cell::Float(f.state.pos[2]),
        Cell::Float(f.state.rotation[0]),
        Cell::Float(f.state.rotation[1]),
        Cell::Float(f.state.rotation[2]),
        Cell::Float(f.state.velocity[0]),
        Cell::Float(f.state.velocity[1]),
        Cell::Float(f.state.velocity[2]),
        Cell::Int(f.state.hp as i64),
        Cell::Int(f.state.r_anim as i64),
        Cell::Int(f.state.sword_state as i64),
        Cell::Int(f.state.sword_hidden as i64),
        Cell::Float(f.state.input_direction),
        Cell::Float(f.state.desired_heading),
        Cell::Int(f.state.button_jump as i64),
        Cell::Int(f.state.button_light_attack as i64),
        Cell::Int(f.state.button_heavy_attack as i64),
        Cell::Int(f.state.button_ninjarun as i64),
        Cell::Int(f.state.button_blademode as i64),
        Cell::Int(f.state.ripper_enabled as i64),
        Cell::Int(f.state.blade_mode_type as i64),
        Cell::Float(cam.pos[0]),
        Cell::Float(cam.pos[1]),
        Cell::Float(cam.pos[2]),
        Cell::Float(cam.look_at[0]),
        Cell::Float(cam.look_at[1]),
        Cell::Float(cam.look_at[2]),
        Cell::Float(cam.roll),
    ];
    for m in 0..4 {
        for k in 0..4 {
            row.push(Cell::Float(cam.view_proj[m * 4 + k]));
        }
    }
    row.push(Cell::Float(cam_yaw));
    row.push(Cell::Float(cam_pitch));
    row.push(Cell::Int(f.blade_down));
    row.push(Cell::Int(f.ripper_pressed));
    match f.raw_down {
        Some(a) => row.extend(a.iter().map(|v| Cell::Int(*v as i64))),
        None => row.extend(std::iter::repeat_n(Cell::Null, 6)),
    }
    match f.raw_pressed {
        Some(a) => row.extend(a.iter().map(|v| Cell::Int(*v as i64))),
        None => row.extend(std::iter::repeat_n(Cell::Null, 6)),
    }
    match &f.enemy {
        Some(e) => row.extend([
            Cell::Float(e.pos[0]),
            Cell::Float(e.pos[1]),
            Cell::Float(e.pos[2]),
            Cell::Float(e.blade_y),
            Cell::Int(e.r_anim as i64),
            Cell::Int(e.frame as i64),
            Cell::Int(e.hp as i64),
        ]),
        None => row.extend(std::iter::repeat_n(Cell::Null, 7)),
    }
    debug_assert_eq!(row.len(), SCHEMA.len(), "схема и build_row разошлись");
    row
}

pub(crate) fn build_rows(meta: &RunMeta, frames: &[Frame]) -> Vec<Vec<Cell>> {
    frames.iter().map(|f| build_row(meta, f)).collect()
}

/// Загружает мету прогона. `Ok(None)` — прогон не найден.
pub(crate) fn load_run_meta(conn: &Connection, id: i64) -> Result<Option<RunMeta>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, mission_id, mission_name, started_at, frame_count, duration_ms, source_replay_id
             FROM replay_runs WHERE id = ?1",
        )
        .map_err(|e| format!("prepare: {e}"))?;
    let mut rows = stmt
        .query_map(params![id], |r| {
            Ok(RunMeta {
                id: r.get(0)?,
                kind: r.get(1)?,
                mission_id: r.get(2)?,
                mission_name: r.get(3)?,
                started_at: r.get(4)?,
                frame_count: r.get(5)?,
                duration_ms: r.get(6)?,
                source_replay_id: r.get(7)?,
            })
        })
        .map_err(|e| format!("query: {e}"))?;
    match rows.next() {
        Some(Ok(m)) => Ok(Some(m)),
        Some(Err(e)) => Err(format!("row: {e}")),
        None => Ok(None),
    }
}

/// id прогонов для дампа: сам прогон; для record — связанные playback
/// (`source_replay_id = id`); для playback — его исходная запись.
pub(crate) fn linked_run_ids(conn: &Connection, meta: &RunMeta) -> Result<Vec<i64>, String> {
    let mut ids = Vec::new();
    if meta.kind == "record" {
        ids.push(meta.id);
        let mut stmt = conn
            .prepare("SELECT id FROM replay_runs WHERE source_replay_id = ?1 ORDER BY id")
            .map_err(|e| format!("prepare: {e}"))?;
        let rows: Result<Vec<i64>, _> = stmt
            .query_map(params![meta.id], |r| r.get(0))
            .map_err(|e| format!("query: {e}"))?
            .collect();
        ids.extend(rows.map_err(|e| format!("row: {e}"))?);
    } else {
        if let Some(src) = meta.source_replay_id {
            ids.push(src);
        }
        ids.push(meta.id);
    }
    Ok(ids)
}

/// Печатает последние прогоны (помощь, если id не найден).
pub(crate) fn print_recent_runs(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, mission_id, started_at, frame_count
             FROM replay_runs ORDER BY id DESC LIMIT 10",
        )
        .map_err(|e| format!("prepare: {e}"))?;
    let rows: Result<Vec<(i64, String, i64, String, i64)>, _> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .map_err(|e| format!("query: {e}"))?
        .collect();
    let rows = rows.map_err(|e| format!("row: {e}"))?;
    if rows.is_empty() {
        println!("  (в БД нет прогонов replay)");
    } else {
        println!("  Последние прогоны:");
        for (id, kind, mission_id, started_at, count) in rows {
            println!("    id={id} kind={kind} mission_id={mission_id} frames={count} started={started_at}");
        }
    }
    Ok(())
}

/// Загружает кадры прогона (по `kind` — нужная таблица), декодируя BLOB.
/// Колонки читаются динамически: старые БД без blade/ripper/raw дают 0/NULL.
pub(crate) fn load_frames(conn: &Connection, meta: &RunMeta) -> Result<Vec<Frame>, String> {
    let table = match meta.kind.as_str() {
        "record" => "replay_record_frames",
        _ => "replay_playback_frames",
    };
    const ALL: [&str; 10] = [
        "frame_index",
        "duration_ms",
        "input_unit",
        "state",
        "camera",
        "blade_down",
        "ripper_pressed",
        "raw_down",
        "raw_pressed",
        "enemy",
    ];
    let have = table_columns(conn, table)?;
    let present: Vec<&str> = ALL
        .iter()
        .copied()
        .filter(|c| have.iter().any(|h| h == c))
        .collect();
    let sql = format!(
        "SELECT {} FROM {} WHERE replay_id = ?1 ORDER BY frame_index",
        present.join(", "),
        table
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| format!("prepare frames: {e}"))?;
    let idx = |name: &str| present.iter().position(|c| *c == name);
    let i_frame = idx("frame_index").expect("frame_index колонка");
    let i_dur = idx("duration_ms").expect("duration_ms колонка");
    let i_input = idx("input_unit").expect("input_unit колонка");
    let i_state = idx("state").expect("state колонка");
    let i_camera = idx("camera").expect("camera колонка");
    let i_blade = idx("blade_down");
    let i_ripper = idx("ripper_pressed");
    let i_raw_down = idx("raw_down");
    let i_raw_pressed = idx("raw_pressed");
    let i_enemy = idx("enemy");

    let mut raw = Vec::new();
    {
        let rows = stmt
            .query_map(params![meta.id], |r| {
                Ok((
                    r.get::<_, i64>(i_frame)?,
                    r.get::<_, i64>(i_dur)?,
                    r.get::<_, Vec<u8>>(i_input)?,
                    r.get::<_, Vec<u8>>(i_state)?,
                    r.get::<_, Vec<u8>>(i_camera)?,
                    match i_blade {
                        Some(i) => r.get::<_, i64>(i)?,
                        None => 0,
                    },
                    match i_ripper {
                        Some(i) => r.get::<_, i64>(i)?,
                        None => 0,
                    },
                    match i_raw_down {
                        Some(i) => r.get::<_, Option<Vec<u8>>>(i)?,
                        None => None,
                    },
                    match i_raw_pressed {
                        Some(i) => r.get::<_, Option<Vec<u8>>>(i)?,
                        None => None,
                    },
                    match i_enemy {
                        Some(i) => r.get::<_, Option<Vec<u8>>>(i)?,
                        None => None,
                    },
                ))
            })
            .map_err(|e| format!("query frames: {e}"))?;
        for row in rows {
            raw.push(row.map_err(|e| format!("frame row: {e}"))?);
        }
    }

    let mut frames = Vec::with_capacity(raw.len());
    for (fi, dur, input_b, state_b, cam_b, blade, ripper, raw_d, raw_p, enemy_b) in raw {
        let input = from_bytes::<InputUnit>(&input_b).ok_or_else(|| {
            format!(
                "Кадр {fi}: input_unit BLOB размер {} != {}",
                input_b.len(),
                std::mem::size_of::<InputUnit>()
            )
        })?;
        let state = from_bytes::<PlayerState>(&state_b).ok_or_else(|| {
            format!(
                "Кадр {fi}: state BLOB размер {} != {}",
                state_b.len(),
                std::mem::size_of::<PlayerState>()
            )
        })?;
        let camera = from_bytes::<CameraState>(&cam_b).ok_or_else(|| {
            format!(
                "Кадр {fi}: camera BLOB размер {} != {} (записи до 2026-08-18 имели старый формат \
                 камеры 76 байт — такие прогоны не поддерживаются)",
                cam_b.len(),
                std::mem::size_of::<CameraState>()
            )
        })?;
        let raw_down = match raw_d {
            Some(b) => Some(
                from_bytes::<[u32; 6]>(&b)
                    .ok_or_else(|| format!("Кадр {fi}: raw_down BLOB размер {} != 24", b.len()))?,
            ),
            None => None,
        };
        let raw_pressed = match raw_p {
            Some(b) => Some(
                from_bytes::<[u32; 6]>(&b)
                    .ok_or_else(|| format!("Кадр {fi}: raw_pressed BLOB размер {} != 24", b.len()))?,
            ),
            None => None,
        };
        let enemy = match enemy_b {
            Some(b) => Some(
                from_bytes::<EnemyState>(&b).ok_or_else(|| {
                    format!(
                        "Кадр {fi}: enemy BLOB размер {} != {}",
                        b.len(),
                        std::mem::size_of::<EnemyState>()
                    )
                })?,
            ),
            None => None,
        };
        frames.push(Frame {
            frame_index: fi,
            duration_ms: dur,
            input,
            state,
            camera,
            blade_down: blade,
            ripper_pressed: ripper,
            raw_down,
            raw_pressed,
            enemy,
        });
    }
    Ok(frames)
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| format!("prepare pragma: {e}"))?;
    let cols: Result<Vec<String>, _> = stmt
        .query_map([], |r| r.get(1))
        .map_err(|e| format!("pragma: {e}"))?
        .collect();
    cols.map_err(|e| format!("pragma row: {e}"))
}

fn cell_str(c: &Cell) -> String {
    match c {
        Cell::Int(v) => v.to_string(),
        Cell::Float(v) => v.to_string(),
        Cell::Str(s) => s.clone(),
        Cell::Null => String::new(),
    }
}

pub(crate) fn write_csv(path: &Path, rows: &[Vec<Cell>]) -> Result<(), String> {
    let mut wtr = csv::Writer::from_path(path).map_err(|e| format!("csv: {e}"))?;
    wtr.write_record(SCHEMA.iter().map(|(n, _)| *n))
        .map_err(|e| format!("csv header: {e}"))?;
    for row in rows {
        let rec: Vec<String> = row.iter().map(cell_str).collect();
        wtr.write_record(&rec).map_err(|e| format!("csv: {e}"))?;
    }
    wtr.flush().map_err(|e| format!("csv flush: {e}"))?;
    Ok(())
}

pub(crate) fn write_parquet(path: &Path, rows: &[Vec<Cell>]) -> Result<(), String> {
    let schema = Arc::new(Schema::new(
        SCHEMA
            .iter()
            .map(|(name, ty)| {
                let dt = match ty {
                    ColType::I64 => DataType::Int64,
                    ColType::F32 => DataType::Float32,
                    ColType::Str => DataType::Utf8,
                };
                Field::new(*name, dt, true)
            })
            .collect::<Vec<_>>(),
    ));

    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(SCHEMA.len());
    for (ci, (name, ty)) in SCHEMA.iter().enumerate() {
        match ty {
            ColType::I64 => {
                let mut b = Int64Array::builder(rows.len());
                for row in rows {
                    match &row[ci] {
                        Cell::Int(v) => b.append_value(*v),
                        Cell::Null => b.append_null(),
                        other => return Err(format!("колонка {name}: ожидался Int, получено {other:?}")),
                    }
                }
                arrays.push(Arc::new(b.finish()));
            }
            ColType::F32 => {
                let mut b = Float32Array::builder(rows.len());
                for row in rows {
                    match &row[ci] {
                        Cell::Float(v) => b.append_value(*v),
                        Cell::Null => b.append_null(),
                        other => return Err(format!("колонка {name}: ожидался Float, получено {other:?}")),
                    }
                }
                arrays.push(Arc::new(b.finish()));
            }
            ColType::Str => {
                let mut b = StringBuilder::new();
                for row in rows {
                    match &row[ci] {
                        Cell::Str(s) => b.append_value(s),
                        Cell::Null => b.append_null(),
                        other => return Err(format!("колонка {name}: ожидалась строка, получено {other:?}")),
                    }
                }
                arrays.push(Arc::new(b.finish()));
            }
        }
    }

    let batch = RecordBatch::try_new(schema.clone(), arrays).map_err(|e| format!("batch: {e}"))?;
    let file = File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let mut writer =
        ArrowWriter::try_new(file, schema, None).map_err(|e| format!("parquet: {e}"))?;
    writer.write(&batch).map_err(|e| format!("parquet write: {e}"))?;
    writer.close().map_err(|e| format!("parquet close: {e}"))?;
    Ok(())
}

/// Читает parquet обратно (для тестов).
#[cfg(test)]
fn read_parquet(path: &Path) -> Result<RecordBatch, String> {
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

    let file = File::open(path).map_err(|e| format!("open: {e}"))?;
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).map_err(|e| format!("reader: {e}"))?;
    let mut reader = builder.build().map_err(|e| format!("build: {e}"))?;
    reader
        .next()
        .transpose()
        .map_err(|e| format!("batch: {e}"))?
        .ok_or_else(|| "пустой parquet".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Array, StringArray};
    use drmod_replay_types::to_bytes;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dbdump_{}_{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn open_db(dir: &std::path::Path, name: &str) -> Connection {
        let path = dir.join(name);
        Connection::open(&path).unwrap()
    }

    /// Схема, совпадающая с `create_replay_tables` в src/tas/db.rs.
    fn create_schema(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE replay_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                mission_id INTEGER NOT NULL,
                mission_name TEXT NOT NULL,
                started_at TEXT NOT NULL,
                frame_count INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                source_replay_id INTEGER
            );
            CREATE TABLE replay_record_frames (
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
            );
            CREATE TABLE replay_playback_frames (
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
            );",
        )
        .unwrap();
    }

    fn insert_run(conn: &Connection, kind: &str, source: Option<i64>) -> i64 {
        conn.execute(
            "INSERT INTO replay_runs (kind, mission_id, mission_name, started_at, frame_count, duration_ms, source_replay_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![kind, 1i64, "R-00".to_string(), "2026-08-23 12:00:00".to_string(), 2i64, 1000i64, source],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn sample_frame(i: u32) -> (InputUnit, PlayerState, CameraState, [u32; 6], EnemyState) {
        let input = InputUnit {
            buttons_down: 1 + i,
            buttons_pressed: i,
            left_stick: [0.0, -1000.0],
            valid_input: 1,
            ..Default::default()
        };
        let state = PlayerState {
            pos: [10.0, 20.0, 30.0],
            rotation: [0.0, 1.5, 0.0],
            hp: 100,
            r_anim: 42,
            ..Default::default()
        };
        let camera = CameraState {
            pos: [1.0, 2.0, 3.0],
            look_at: [4.0, 5.0, 6.0],
            roll: 0.5,
            view_proj: [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.3, 1.4, 1.5, 1.6],
        };
        let raw = [1u32, 2, 3, 4, 5, 6];
        let enemy = EnemyState {
            pos: [100.0, 0.0, 200.0],
            blade_y: 1.03,
            r_anim: 19,
            frame: 7 + i as i32,
            hp: 500,
            found: 1,
        };
        (input, state, camera, raw, enemy)
    }

    fn insert_frames(conn: &Connection, table: &str, replay_id: i64) {
        let sql = format!(
            "INSERT INTO {table} (replay_id, frame_index, duration_ms, input_unit, state, camera, blade_down, ripper_pressed, raw_down, raw_pressed, enemy)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
        );
        for i in 0..2 {
            let (input, state, camera, raw, enemy) = sample_frame(i);
            conn.execute(
                &sql,
                params![
                    replay_id,
                    i as i64,
                    (i as i64) * 1000 / 60,
                    to_bytes(&input),
                    to_bytes(&state),
                    to_bytes(&camera),
                    1i64,
                    0i64,
                    to_bytes(&raw),
                    to_bytes(&raw),
                    to_bytes(&enemy),
                ],
            )
            .unwrap();
        }
    }

    fn col_idx(name: &str) -> usize {
        SCHEMA.iter().position(|(n, _)| *n == name).unwrap()
    }

    #[test]
    fn roundtrip_csv_and_parquet() {
        let dir = test_dir("roundtrip");
        let conn = open_db(&dir, "test.db");
        create_schema(&conn);
        let rid = insert_run(&conn, "record", None);
        insert_frames(&conn, "replay_record_frames", rid);

        let meta = load_run_meta(&conn, rid).unwrap().unwrap();
        assert_eq!(meta.kind, "record");
        assert_eq!(meta.frame_count, 2);
        let frames = load_frames(&conn, &meta).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].frame_index, 0);
        assert_eq!(frames[1].frame_index, 1);
        let rows = build_rows(&meta, &frames);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].len(), SCHEMA.len());

        // CSV
        let csv_path = dir.join("run_1_record.csv");
        write_csv(&csv_path, &rows).unwrap();
        let content = std::fs::read_to_string(&csv_path).unwrap();
        let mut lines = content.lines();
        let header = lines.next().unwrap();
        assert!(header.starts_with("replay_id,kind,mission_id,"), "header: {header}");
        let first: Vec<&str> = lines.next().unwrap().split(',').collect();
        assert_eq!(first.len(), SCHEMA.len());
        assert_eq!(first[col_idx("replay_id")], "1");
        assert_eq!(first[col_idx("kind")], "record");
        assert_eq!(first[col_idx("buttons_down")], "1");
        assert_eq!(first[col_idx("pos_x")], "10");
        assert_eq!(first[col_idx("hp")], "100");
        assert_eq!(first[col_idx("blade_down")], "1");
        assert_eq!(first[col_idx("raw_down_0")], "1");
        assert_eq!(first[col_idx("raw_pressed_5")], "6");
        assert_eq!(first[col_idx("cam_yaw")], "0.7853982");
        assert_eq!(first[col_idx("enemy_pos_x")], "100");
        assert_eq!(first[col_idx("enemy_blade_y")], "1.03");
        assert_eq!(first[col_idx("enemy_anim")], "19");
        assert_eq!(first[col_idx("enemy_frame")], "7");
        assert_eq!(first[col_idx("enemy_hp")], "500");

        // Parquet
        let parquet_path = dir.join("run_1_record.parquet");
        write_parquet(&parquet_path, &rows).unwrap();
        let batch = read_parquet(&parquet_path).unwrap();
        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.num_columns(), SCHEMA.len());
        let pos_x = batch
            .column(col_idx("pos_x"))
            .as_any()
            .downcast_ref::<Float32Array>()
            .unwrap();
        assert_eq!(pos_x.value(0), 10.0);
        let buttons = batch
            .column(col_idx("buttons_down"))
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert_eq!(buttons.value(0), 1);
        assert_eq!(buttons.value(1), 2);
        let kind = batch
            .column(col_idx("kind"))
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(kind.value(0), "record");
    }

    #[test]
    fn record_playback_linkage() {
        let dir = test_dir("linkage");
        let conn = open_db(&dir, "test.db");
        create_schema(&conn);
        let rid = insert_run(&conn, "record", None);
        insert_frames(&conn, "replay_record_frames", rid);
        let pid = insert_run(&conn, "playback", Some(rid));
        insert_frames(&conn, "replay_playback_frames", pid);

        // от record: record + его playback
        let meta = load_run_meta(&conn, rid).unwrap().unwrap();
        assert_eq!(linked_run_ids(&conn, &meta).unwrap(), vec![rid, pid]);
        // от playback: исходная запись + сам playback
        let pmeta = load_run_meta(&conn, pid).unwrap().unwrap();
        assert_eq!(linked_run_ids(&conn, &pmeta).unwrap(), vec![rid, pid]);
    }

    #[test]
    fn missing_run_returns_none() {
        let dir = test_dir("missing");
        let conn = open_db(&dir, "test.db");
        create_schema(&conn);
        assert!(load_run_meta(&conn, 999).unwrap().is_none());
    }

    #[test]
    fn old_schema_without_raw_columns() {
        let dir = test_dir("old_schema");
        let conn = open_db(&dir, "test.db");
        // старая схема: нет blade/ripper/raw-колонок
        conn.execute_batch(
            "CREATE TABLE replay_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                kind TEXT NOT NULL,
                mission_id INTEGER NOT NULL,
                mission_name TEXT NOT NULL,
                started_at TEXT NOT NULL,
                frame_count INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                source_replay_id INTEGER
            );
            CREATE TABLE replay_record_frames (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                replay_id INTEGER NOT NULL,
                frame_index INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                input_unit BLOB NOT NULL,
                state BLOB NOT NULL,
                camera BLOB NOT NULL,
                FOREIGN KEY (replay_id) REFERENCES replay_runs(id) ON DELETE CASCADE
            );",
        )
        .unwrap();
        let rid = insert_run(&conn, "record", None);
        let sql = "INSERT INTO replay_record_frames (replay_id, frame_index, duration_ms, input_unit, state, camera)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6)";
        let (input, state, camera, _, _) = sample_frame(0);
        conn.execute(
            sql,
            params![rid, 0i64, 0i64, to_bytes(&input), to_bytes(&state), to_bytes(&camera)],
        )
        .unwrap();

        let meta = load_run_meta(&conn, rid).unwrap().unwrap();
        let frames = load_frames(&conn, &meta).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].blade_down, 0);
        assert!(frames[0].raw_down.is_none());
        assert!(frames[0].enemy.is_none());
        let rows = build_rows(&meta, &frames);
        assert!(matches!(rows[0][col_idx("raw_down_0")], Cell::Null));
        assert!(matches!(rows[0][col_idx("blade_down")], Cell::Int(0)));
        assert!(matches!(rows[0][col_idx("source_replay_id")], Cell::Null));
        assert!(matches!(rows[0][col_idx("enemy_pos_x")], Cell::Null));
        assert!(matches!(rows[0][col_idx("enemy_hp")], Cell::Null));

        // CSV: NULL-ячейки — пустые
        let csv_path = dir.join("old.csv");
        write_csv(&csv_path, &rows).unwrap();
        let content = std::fs::read_to_string(&csv_path).unwrap();
        let first: Vec<&str> = content.lines().nth(1).unwrap().split(',').collect();
        assert_eq!(first[col_idx("raw_down_0")], "");
        assert_eq!(first[col_idx("source_replay_id")], "");
        assert_eq!(first[col_idx("enemy_pos_x")], "");

        // Parquet: NULL-ячейки — null
        let parquet_path = dir.join("old.parquet");
        write_parquet(&parquet_path, &rows).unwrap();
        let batch = read_parquet(&parquet_path).unwrap();
        let raw0 = batch
            .column(col_idx("raw_down_0"))
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();
        assert!(raw0.is_null(0));
    }

    #[test]
    fn truncated_blob_is_an_error() {
        let dir = test_dir("truncated");
        let conn = open_db(&dir, "test.db");
        create_schema(&conn);
        let rid = insert_run(&conn, "record", None);
        conn.execute(
            "INSERT INTO replay_record_frames (replay_id, frame_index, duration_ms, input_unit, state, camera)
             VALUES (?1, 0, 0, ?2, ?3, ?4)",
            params![rid, vec![1u8, 2, 3], vec![0u8; 10], vec![0u8; 10]],
        )
        .unwrap();
        let meta = load_run_meta(&conn, rid).unwrap().unwrap();
        let err = load_frames(&conn, &meta).unwrap_err();
        assert!(err.contains("input_unit BLOB"), "{err}");
    }
}
