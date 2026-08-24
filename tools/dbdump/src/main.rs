//! dbdump — экспорт кадров Record/Replay из `runs.db` в CSV и Parquet.
//!
//! По id прогона раскладывает BLOB-колонки (`input_unit`/`state`/`camera` +
//! blade/ripper/raw) на плоские колонки для аналитики. Для record-прогона
//! дополнительно дампит связанные playback-прогоны (`source_replay_id`),
//! для playback — его исходную запись.

mod dump;
mod script;

use std::path::PathBuf;
use std::process;

use rusqlite::Connection;

fn usage() -> String {
    "Использование: dbdump <run_id> [--script [--pretty]] [--out DIR] [--db PATH]".to_string()
}

/// `%LOCALAPPDATA%\drmod\runs.db` — как в `init_db` мода.
fn default_db_path() -> Result<PathBuf, String> {
    match std::env::var("LOCALAPPDATA") {
        Ok(v) => Ok(PathBuf::from(format!("{v}\\drmod\\runs.db"))),
        Err(_) => Err("Переменная LOCALAPPDATA не найдена — укажите --db".to_string()),
    }
}

fn parse_args() -> Result<(i64, PathBuf, PathBuf, bool, bool), String> {
    let mut args = std::env::args().skip(1);
    let mut run_id: Option<i64> = None;
    let mut out = PathBuf::from(".");
    let mut db = default_db_path()?;
    let mut script = false;
    let mut pretty = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = PathBuf::from(args.next().ok_or("--out: нужен путь")?),
            "--db" => db = PathBuf::from(args.next().ok_or("--db: нужен путь")?),
            "--script" => script = true,
            "--pretty" => pretty = true,
            other => {
                if run_id.is_none() {
                    run_id = Some(
                        other
                            .parse()
                            .map_err(|_| format!("Некорректный run_id: {other}"))?,
                    );
                } else {
                    return Err(format!("Неожиданный аргумент: {other}\n{}", usage()));
                }
            }
        }
    }
    Ok((
        run_id.ok_or_else(usage)?,
        out,
        db,
        script,
        pretty,
    ))
}

fn run() -> Result<(), String> {
    let (run_id, out, db, script, pretty) = parse_args()?;
    let conn = Connection::open(&db).map_err(|e| format!("Не удалось открыть БД {}: {e}", db.display()))?;

    let meta = match dump::load_run_meta(&conn, run_id)? {
        Some(m) => m,
        None => {
            eprintln!("Прогон с id={run_id} не найден в {}", db.display());
            dump::print_recent_runs(&conn)?;
            return Err(format!("Прогон с id={run_id} не найден"));
        }
    };
    println!(
        "Прогон id={} kind={} mission={} ({} кадров) из {}",
        meta.id,
        meta.kind,
        meta.mission_name,
        meta.frame_count,
        db.display()
    );

    if script {
        // JSON-скрипт для HTTP API (POST /script/run) — только указанный прогон.
        let frames = dump::load_frames(&conn, &meta)?;
        let json = script::build_script(&meta, &frames, pretty)?;
        std::fs::create_dir_all(&out).map_err(|e| format!("Создать {}: {e}", out.display()))?;
        let path = out.join(format!("run_{}_script.json", meta.id));
        std::fs::write(&path, json).map_err(|e| format!("Записать {}: {e}", path.display()))?;
        println!(
            "  run_{}_script: {} кадров -> {}",
            meta.id,
            frames.len(),
            path.display()
        );
        return Ok(());
    }

    let ids = dump::linked_run_ids(&conn, &meta)?;
    std::fs::create_dir_all(&out).map_err(|e| format!("Создать {}: {e}", out.display()))?;
    for id in ids {
        let m = if id == run_id {
            meta.clone()
        } else {
            match dump::load_run_meta(&conn, id)? {
                Some(m) => m,
                None => return Err(format!("Связанный прогон id={id} не найден")),
            }
        };
        let frames = dump::load_frames(&conn, &m)?;
        let rows = dump::build_rows(&m, &frames);
        let stem = format!("run_{}_{}", m.id, m.kind);
        let csv_path = out.join(format!("{stem}.csv"));
        let parquet_path = out.join(format!("{stem}.parquet"));
        dump::write_csv(&csv_path, &rows)?;
        dump::write_parquet(&parquet_path, &rows)?;
        println!(
            "  {stem}: {} кадров -> {} / {}",
            rows.len(),
            csv_path.display(),
            parquet_path.display()
        );
    }
    Ok(())
}

fn main() {
    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!("{}", usage());
        return;
    }
    if let Err(e) = run() {
        eprintln!("Ошибка: {e}");
        process::exit(1);
    }
}
