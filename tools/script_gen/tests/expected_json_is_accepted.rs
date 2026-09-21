//! Приёмка JSON, записанного C#-конвертером (`tas-editor-cs`).
//!
//! Редактор читает фикстуру, собранную этим крейтом, и пишет рядом
//! `<фикстура>.expected.json`. Здесь этот файл десериализуется **типами мода**:
//! он обязан приняться без правок и совпасть с исходной фикстурой — то есть
//! round-trip не теряет и не искажает данные. Пропуск файла — не «нет теста», а
//! провал: комплект золотых файлов должен быть полным.

use drmod_replay_types::script::{MAX_SCRIPT_FRAMES, ScriptRequest};
use std::fs;
use std::path::{Path, PathBuf};

/// Каталог фикстур редактора — тот же, куда пишет `script_gen`.
const FIXTURES: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../tas-editor-cs/TasEditorCs.Tests/Fixtures");

/// Суффикс JSON, записанного редактором (совпадает с `fixtures::EXPECTED_SUFFIX`).
const EXPECTED_SUFFIX: &str = ".expected.json";

#[test]
fn every_fixture_has_an_expected_file() {
    let dir = Path::new(FIXTURES);
    let mut fixtures = 0;
    for path in fixtures_in(dir) {
        let name = file_name(&path);
        let stem = name.strip_suffix(".json").expect(".json");
        let expected = dir.join(format!("{stem}{EXPECTED_SUFFIX}"));
        assert!(
            expected.is_file(),
            "нет {} — прогоните тесты редактора с TAS_REGEN_GOLDENS=1 и закоммитьте файл",
            file_name(&expected)
        );
        fixtures += 1;
    }
    assert!(fixtures > 0, "в {} нет фикстур — прогоните `cargo run -p drmod-script-gen`", dir.display());
}

#[test]
fn csharp_json_is_accepted_and_matches_the_fixture() {
    let dir = Path::new(FIXTURES);
    for path in fixtures_in(dir) {
        let name = file_name(&path);
        let stem = name.strip_suffix(".json").expect(".json");
        let expected_path = dir.join(format!("{stem}{EXPECTED_SUFFIX}"));
        let source = load(&path);
        let round_tripped = load(&expected_path);
        assert_eq!(
            round_tripped,
            source,
            "{} разошёлся с {}",
            file_name(&expected_path),
            name
        );
        check_mod_limits(&round_tripped, &file_name(&expected_path));
    }
}

/// JSON-фикстуры каталога (без золотых файлов редактора), по алфавиту.
fn fixtures_in(dir: &Path) -> Vec<PathBuf> {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("каталог фикстур {}: {e}", dir.display()));
    let mut fixtures: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            let name = file_name(path);
            name.ends_with(".json") && !name.ends_with(EXPECTED_SUFFIX)
        })
        .collect();
    fixtures.sort();
    fixtures
}

fn load(path: &Path) -> ScriptRequest {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Кросс-полевые лимиты из `parse_script` (src/api.rs): их serde не проверяет.
fn check_mod_limits(request: &ScriptRequest, name: &str) {
    assert!(!request.commands.is_empty(), "{name}: commands пуст");
    assert!(request.name.len() <= 64, "{name}: name длиннее 64");
    for (i, command) in request.commands.iter().enumerate() {
        assert!(command.duration >= 1, "{name}: commands[{i}] duration < 1");
        assert!(
            command.t + command.duration <= MAX_SCRIPT_FRAMES,
            "{name}: commands[{i}] выходит за {MAX_SCRIPT_FRAMES} кадров"
        );
        assert!(!command.input.is_empty(), "{name}: commands[{i}] input пуст");
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_string()
}
