//! Генератор JSON-фикстур скрипта (`POST /script/run`, docs/API.md §4) для
//! round-trip тестов редактора (`tas-editor-cs`).
//!
//! Формат гарантирован определением типа: фикстуры строятся
//! `drmod_replay_types::script` — теми же DTO, которыми мод десериализует
//! запрос, поэтому «примет ли мод этот JSON» проверяется не сверкой текста, а
//! общей структурой. Набор файлов и тесты покрытия — `src/fixtures.rs`.
//!
//! Пример:
//!     cargo run -p drmod-script-gen
//!     cargo run -p drmod-script-gen -- --out ..\..\tas-editor-cs\TasEditorCs.Tests\Fixtures

mod fixtures;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
Генерация JSON-фикстур скрипта (POST /script/run) для round-trip тестов tas-editor-cs.

Использование:
    script_gen [--out <каталог>]

    --out <каталог>  куда писать фикстуры
                     (по умолчанию tas-editor-cs/TasEditorCs.Tests/Fixtures)
    -h, --help       эта справка
";

fn main() -> ExitCode {
    let mut out: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => match args.next() {
                Some(dir) => out = Some(PathBuf::from(dir)),
                None => {
                    eprintln!("--out требует каталог\n\n{USAGE}");
                    return ExitCode::FAILURE;
                }
            },
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("неизвестный аргумент: {other}\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
        }
    }

    let out = out.unwrap_or_else(|| PathBuf::from(fixtures::DEFAULT_OUT));
    match write_all(&out) {
        Ok(written) => {
            println!("записано {} фикстур в {}:", written.len(), out.display());
            for path in written {
                println!("  {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ошибка: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Пишет все фикстуры в каталог (создаёт его при необходимости) и возвращает
/// пути записанных файлов. Файлы C# (`.expected.json`, `.tas`) не трогаются —
/// их пишет редактор, здесь только вход для конвертера.
fn write_all(out: &Path) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(out).map_err(|e| format!("создать каталог {}: {e}", out.display()))?;
    let mut written = Vec::new();
    for (name, fixture) in fixtures::all() {
        let path = out.join(name);
        let mut text = fixture.to_text().map_err(|e| format!("{name}: {e}"))?;
        text.push('\n');
        fs::write(&path, text).map_err(|e| format!("записать {}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}
