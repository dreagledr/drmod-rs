// Мини-утилита: дамп input_unit последней записи из replay_record_frames.
// Запуск: cargo run --manifest-path tools/disasm/Cargo.toml
use rusqlite::Connection;
use std::path::Path;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct InputUnit {
    buttons_down: u32,
    buttons_pressed: u32,
    buttons_released: u32,
    buttons_alternated: u32,
    left_stick: [f32; 2],
    right_stick: [f32; 2],
    left_trigger: f32,
    right_trigger: f32,
    valid_input: i32,
    repeat_count: i32,
}

fn main() {
    let db_path = Path::new(&std::env::var("LOCALAPPDATA").unwrap_or_default()).join("drmod/runs.db");
    let conn = Connection::open(&db_path).expect("open db");
    // id берём из аргумента (по умолчанию последний record)
    let want_id: Option<i64> = std::env::args().nth(1).and_then(|s| s.parse().ok());
    let run_id: i64 = match want_id {
        Some(id) => id,
        None => conn
            .query_row(
                "SELECT id FROM replay_runs WHERE kind='record' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .expect("no record run"),
    };

    let mut stmt = conn
        .prepare("SELECT frame_index, input_unit, state, camera FROM replay_record_frames WHERE replay_id=?1 ORDER BY frame_index")
        .unwrap();
    let rows = stmt
        .query_map([run_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, Vec<u8>>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })
        .unwrap();

    let mut last = (0u32, 0u32);
    for row in rows {
        let (fi, buf, state_buf, _camera) = row.unwrap();
        let mut u = InputUnit::default();
        unsafe {
            std::ptr::copy_nonoverlapping(
                buf.as_ptr() as *const u8,
                &mut u as *mut InputUnit as *mut u8,
                buf.len().min(std::mem::size_of::<InputUnit>()),
            );
        }
        // raw_down/raw_pressed лежат после ReplayFrame-заголовка?
        // ReplayFrame layout: frame_index(u32) + input(InputUnit=44?) + state + camera + blade(u8) + ripper(u8) + raw_down([u32;6]) + raw_pressed([u32;6])
        // В БД отдельные колонки: input_unit, state, camera. raw — не сохраняются в БД!
        // Выведем только input_unit для сверки.
        if u.buttons_down != 0 || u.buttons_pressed != 0 || u.valid_input != 0 {
            let changed = u.buttons_down != last.0 || u.buttons_pressed != last.1;
            if changed {
                println!(
                    "f={:4} down={:08X} pressed={:08X} L=({:8.1},{:8.1}) R=({:8.1},{:8.1}) valid={}",
                    fi,
                    u.buttons_down,
                    u.buttons_pressed,
                    u.left_stick[0],
                    u.left_stick[1],
                    u.right_stick[0],
                    u.right_stick[1],
                    u.valid_input
                );
                last = (u.buttons_down, u.buttons_pressed);
            }
        }
    }
}
