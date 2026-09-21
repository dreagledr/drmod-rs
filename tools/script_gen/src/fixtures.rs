//! Набор JSON-фикстур скрипта (`POST /script/run`, docs/API.md §4) для
//! round-trip тестов редактора (`tas-editor-cs`).
//!
//! Фикстуры строятся **теми же типами**, которыми мод десериализует запрос
//! (`drmod_replay_types::script`), поэтому их приёмка модом гарантирована
//! определением типа, а не сверкой формата вручную. Покрытие — весь §4.2:
//! 26 булевых входов, `left_stick`, `camera`, `raw_key`, `dik_key`,
//! `when_enemy`, `trigger`, `restart` и дефолты.
//!
//! Кадровый срез (то, что выражает DSL, docs/SCRIPT_DSL.md) и краевые входы
//! разведены по файлам: `all_inputs.json` — всё, что выражается в DSL,
//! `edge_inputs.json` — то, что не выражается (сырые коды клавиш, условие по
//! врагу), `rules_*.json` — строки правил, `minimal.json` — дефолты.

use drmod_replay_types::script::{
    EnemyCondition, RestartSpec, ScriptCommand, ScriptInput, ScriptRequest, ScriptTrigger,
};
use serde_json::Value;

/// Каталог фикстур редактора по умолчанию — относительно этого крейта, чтобы
/// `cargo run` из любого каталога писал туда же.
pub const DEFAULT_OUT: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../tas-editor-cs/TasEditorCs.Tests/Fixtures");

/// Все фикстуры в порядке записи: (имя файла, тело).
pub fn all() -> Vec<(&'static str, Fixture)> {
    vec![
        ("all_inputs.json", request(all_inputs())),
        ("edge_inputs.json", request(edge_inputs())),
        ("rules_restart.json", request(rules_restart())),
        ("rules_ticks.json", request(rules_ticks())),
        ("minimal.json", minimal()),
    ]
}

/// Типизированный скрипт как фикстура — обычный случай: тело пишется
/// сериализацией DTO, а не через `Value` (см. `Fixture`).
fn request(request: ScriptRequest) -> Fixture {
    Fixture::Request(Box::new(request))
}

/// Тело фикстуры: типизированный скрипт либо готовое JSON-тело (`minimal.json`
/// — единственная фикстура без `name`, проверяет дефолт мода).
///
/// Разделение не формальность: `serde_json::Value` хранит числа как `f64`, и
/// прогон `f32`-полей через него печатает `-24.700000762939453` вместо `-24.7`.
/// Типизированный скрипт пишется сериализацией самих DTO — тогда `f32`
/// печатается короткой записью, как в примерах `docs/API.md`.
#[derive(Clone, PartialEq, Debug)]
pub enum Fixture {
    Request(Box<ScriptRequest>),
    Json(Value),
}

impl Fixture {
    /// Текст фикстуры для записи в файл.
    pub fn to_text(&self) -> Result<String, String> {
        let result = match self {
            Self::Request(request) => serde_json::to_string_pretty(request.as_ref()),
            Self::Json(value) => serde_json::to_string_pretty(value),
        };
        result.map_err(|e| format!("сериализация фикстуры: {e}"))
    }
}

/// Все входы, выражаемые в DSL (`docs/SCRIPT_DSL.md`), плюс структура команд:
/// перекрытия, подряд идущие одинаковые входы (RLE в кадрах), разрывы,
/// `duration` по умолчанию, дробные и «угловые» (магнитуда > 1000) стики.
fn all_inputs() -> ScriptRequest {
    ScriptRequest {
        name: "all-inputs".to_string(),
        commands: vec![
            // Направления, атаки и прыжок; left_stick — угол-диагональ
            // [1000,-1000] (магнитуда 1414): сила стика больше единицы, и
            // клампом такое значение не выражается.
            command(
                0,
                1,
                ScriptInput {
                    forward: true,
                    backward: true,
                    left: true,
                    right: true,
                    jump: true,
                    light_attack: true,
                    heavy_attack: true,
                    left_stick: Some([1000.0, -1000.0]),
                    ..Default::default()
                },
            ),
            // Состояния персонажа (hold- и toggle-действия).
            command(
                1,
                2,
                ScriptInput {
                    ripper: true,
                    blade: true,
                    ninja_run: true,
                    walk: true,
                    dodge: true,
                    lock_on: true,
                    subweapon: true,
                    item: true,
                    ..Default::default()
                },
            ),
            // Режимы и меню-клавиши.
            command(
                3,
                3,
                ScriptInput {
                    ar_mode: true,
                    weapon_select: true,
                    codec: true,
                    zandatsu: true,
                    camera_reset: true,
                    pause: true,
                    confirm: true,
                    ..Default::default()
                },
            ),
            // Навигация в меню и камера (right_stick).
            command(
                6,
                4,
                ScriptInput {
                    menu_up: true,
                    menu_down: true,
                    menu_left: true,
                    menu_right: true,
                    camera: Some([300.0, 0.0]),
                    ..Default::default()
                },
            ),
            // Одиночный кадр.
            command(10, 1, ScriptInput { jump: true, ..Default::default() }),
            // Явный left_stick, совпадающий с подразумеваемым направлением.
            command(
                11,
                2,
                ScriptInput { forward: true, left_stick: Some([0.0, -1000.0]), ..Default::default() },
            ),
            // Дробный стик; команда перекрывается следующей (прыжок в беге).
            command(
                13,
                6,
                ScriptInput {
                    forward: true,
                    ninja_run: true,
                    left_stick: Some([-500.0, -866.0]),
                    ..Default::default()
                },
            ),
            command(15, 10, ScriptInput { heavy_attack: true, ..Default::default() }),
            // Подряд идущие одинаковые входы (склеиваются в кадрах) и разрыв.
            command(25, 10, ScriptInput { forward: true, ..Default::default() }),
            command(35, 10, ScriptInput { forward: true, ..Default::default() }),
            command(45, 5, ScriptInput { forward: true, ..Default::default() }),
        ],
        trigger: None,
        restart: None,
    }
}

/// Краевые входы §4.2, которые DSL не выражает: сырые коды клавиш и условие по
/// врагу (в том числе со всеми необязательными полями).
fn edge_inputs() -> ScriptRequest {
    ScriptRequest {
        name: "edge-inputs".to_string(),
        commands: vec![
            // Сырой игровой код клавиши (0x8B).
            command(0, 1, ScriptInput { raw_key: Some(139), ..Default::default() }),
            // Сырой DIK-код DirectInput (0xD0 — стрелка вниз).
            command(2, 2, ScriptInput { dik_key: Some(208), ..Default::default() }),
            // Ripper + явный стик в угол [1000,1000].
            command(
                5,
                1,
                ScriptInput { ripper: true, left_stick: Some([1000.0, 1000.0]), ..Default::default() },
            ),
            // Условие по врагу: минимальный набор (гейт только по высоте игрока).
            ScriptCommand {
                t: 9,
                duration: 24,
                input: ScriptInput { forward: true, heavy_attack: true, ..Default::default() },
                when_enemy: Some(EnemyCondition {
                    anim: vec![65545],
                    player_y_min: 0.3,
                    player_y_max: 0.8,
                    player_vy_max: 0.0,
                    ..Default::default()
                }),
            },
            // Условие по врагу со всеми полями и повтором.
            ScriptCommand {
                t: 40,
                duration: 30,
                input: ScriptInput { heavy_attack: true, ..Default::default() },
                when_enemy: Some(EnemyCondition {
                    anim: vec![19, 24],
                    frame_min: 5,
                    frame_max: 60,
                    dist_max: 2.5,
                    blade_dy_min: 0.3,
                    player_y_min: -1.0,
                    player_y_max: 3.0,
                    player_vy_max: 1.5,
                    repeat: true,
                }),
            },
        ],
        trigger: None,
        restart: None,
    }
}

/// Строка правил: имя, позиционный триггер и рестарт с не-дефолтными
/// параметрами (мод строит по ним фазу `restarting`).
fn rules_restart() -> ScriptRequest {
    ScriptRequest {
        name: "r01-beach-restart".to_string(),
        commands: vec![command(
            0,
            40,
            ScriptInput { forward: true, ..Default::default() },
        )],
        trigger: Some(ScriptTrigger { pos: Some([-24.70, 12.15, 120.70]), ticks: None }),
        restart: Some(RestartSpec {
            ups: 2,
            downs: 1,
            hold: 8,
            open_gap: 25,
            gap: 12,
            confirms: 3,
            confirm_gap: 30,
            tail: 20,
        }),
    }
}

/// Строка правил: тиковый триггер (старт ровно на первом тике геймплея) —
/// воспроизводимость фазы врага (docs/API.md §3.3).
fn rules_ticks() -> ScriptRequest {
    ScriptRequest {
        name: "r03-barrier-ticks".to_string(),
        commands: vec![
            command(0, 6, ScriptInput { forward: true, ..Default::default() }),
            command(6, 2, ScriptInput { forward: true, jump: true, ..Default::default() }),
            command(20, 24, ScriptInput { forward: true, heavy_attack: true, ..Default::default() }),
        ],
        trigger: Some(ScriptTrigger { pos: None, ticks: Some(0) }),
        restart: None,
    }
}

/// Дефолты: только `commands` — ни `name`, ни `trigger`, ни `restart`
/// (имя подставляет мод: `"script"`).
fn minimal() -> Fixture {
    let request = ScriptRequest {
        name: "script".to_string(),
        commands: vec![command(0, 1, ScriptInput { jump: true, ..Default::default() })],
        trigger: None,
        restart: None,
    };
    let mut value =
        serde_json::to_value(&request).expect("ScriptRequest сериализуется");
    let object = value.as_object_mut().expect("ScriptRequest — объект");
    let removed = object.remove("name");
    assert!(removed.is_some(), "у ScriptRequest есть поле name");
    Fixture::Json(value)
}

fn command(t: u32, duration: u32, input: ScriptInput) -> ScriptCommand {
    ScriptCommand { t, duration, input, when_enemy: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drmod_replay_types::script::MAX_SCRIPT_FRAMES;
    use serde_json::json;

    /// Проверки ходят по тем же байтам, что попадают в файл: `Value` собирается
    /// разбором текста фикстуры, а не из DTO напрямую (`Value` хранит числа как
    /// `f64` и печатал бы `f32`-поля длинно).
    fn value_of(fixture: &Fixture) -> Value {
        let text = fixture.to_text().expect("фикстура сериализуется");
        serde_json::from_str(&text).expect("фикстура — валидный JSON")
    }

    /// Все ключи объекта `input` из всех фикстур — то, что покрыто примерами.
    fn covered_input_keys() -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for (_, fixture) in all() {
            for command in value_of(&fixture)["commands"].as_array().expect("commands — массив") {
                for key in command["input"].as_object().expect("input — объект").keys() {
                    if !keys.contains(key) {
                        keys.push(key.clone());
                    }
                }
            }
        }
        keys.sort();
        keys
    }

    /// Полный список ключей `input` — берётся сериализацией заполненного
    /// `ScriptInput`, то есть из определения типа: переименование или новое поле
    /// в `replay-types` сразу меняет ожидание, а не только фикстуры.
    fn every_input_key() -> Vec<String> {
        let full = ScriptInput {
            forward: true,
            backward: true,
            left: true,
            right: true,
            jump: true,
            light_attack: true,
            heavy_attack: true,
            camera: Some([0.0, 0.0]),
            ripper: true,
            blade: true,
            ninja_run: true,
            walk: true,
            dodge: true,
            lock_on: true,
            subweapon: true,
            item: true,
            ar_mode: true,
            weapon_select: true,
            codec: true,
            zandatsu: true,
            camera_reset: true,
            pause: true,
            confirm: true,
            menu_up: true,
            menu_down: true,
            menu_left: true,
            menu_right: true,
            raw_key: Some(0),
            dik_key: Some(0),
            left_stick: Some([0.0, 0.0]),
        };
        let mut keys: Vec<String> = serde_json::to_value(full)
            .expect("ScriptInput сериализуется")
            .as_object()
            .expect("ScriptInput — объект")
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    }

    /// «Все кейсы» — это ровно ключи `input` из §4.2: 26 булевых плюс `camera`,
    /// `left_stick`, `raw_key`, `dik_key`.
    #[test]
    fn every_input_key_is_exactly_the_one_from_the_format() {
        assert_eq!(every_input_key().len(), 30);
    }

    #[test]
    fn fixtures_cover_every_input_key() {
        let covered = covered_input_keys();
        let keys = every_input_key();
        let missing: Vec<&String> = keys.iter().filter(|key| !covered.contains(key)).collect();
        assert!(missing.is_empty(), "в фикстурах нет ключей: {missing:?}");
    }

    /// Та же проверка, что делает мод в `parse_script`: serde покрывает типы и
    /// неизвестные ключи, кросс-полевые лимиты проверяются здесь.
    #[test]
    fn fixtures_deserialize_and_pass_the_mod_limits() {
        for (name, fixture) in all() {
            let text = fixture.to_text().expect("фикстура сериализуется");
            let request: ScriptRequest =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
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
    }

    /// Фикстуры пишутся в репозиторий — генерация обязана быть побитово
    /// повторяемой, иначе диффы шумят.
    #[test]
    fn fixtures_are_deterministic() {
        assert_eq!(all(), all());
    }

    /// `minimal.json` — единственная фикстура без `name`: она проверяет дефолт
    /// мода, а не форму записи.
    #[test]
    fn minimal_carries_nothing_but_commands() {
        let value = value_of(&minimal());
        let object = value.as_object().expect("объект");
        assert_eq!(object.keys().collect::<Vec<_>>(), vec!["commands"]);
    }

    /// Не-дефолтные параметры рестарта должны быть в фикстуре целиком: по ним
    /// мод строит фазу `restarting`, и дефолты здесь ничего не проверяют.
    #[test]
    fn restart_fixture_spells_out_every_parameter() {
        let value = value_of(&request(rules_restart()));
        let restart = value["restart"].as_object().expect("restart — объект");
        assert_eq!(restart.len(), 8);
    }

    /// Условие по врагу сериализуется только заполненными полями: «без
    /// ограничения» — это дефолты, и они не должны попадать в JSON
    /// (`f32::MAX` в файле читался бы как настоящее ограничение).
    #[test]
    fn enemy_condition_omits_unbounded_defaults() {
        let value = value_of(&request(edge_inputs()));
        let minimal = &value["commands"][3]["when_enemy"];
        assert_eq!(
            minimal.as_object().expect("объект").keys().collect::<Vec<_>>(),
            vec!["anim", "player_vy_max", "player_y_max", "player_y_min"]
        );

        let full = value["commands"][4]["when_enemy"].as_object().expect("объект");
        assert_eq!(full.len(), 9);
    }

    /// Наличие триггера проверяется отдельно: у `trigger` нужен хотя бы один из
    /// `pos`/`ticks` (docs/API.md §3.3).
    #[test]
    fn every_trigger_carries_pos_or_ticks() {
        for (name, fixture) in all() {
            let value = value_of(&fixture);
            let Some(trigger) = value.get("trigger") else {
                continue;
            };
            assert!(
                trigger.get("pos").is_some() || trigger.get("ticks").is_some(),
                "{name}: trigger без pos и ticks"
            );
        }
    }

    /// `serde_json` без `preserve_order` сортирует ключи — фикстуры должны
    /// оставаться читаемыми в этом же порядке.
    #[test]
    fn json_keys_are_sorted() {
        let object = json!({ "b": 1, "a": 2 });
        assert_eq!(object.as_object().expect("объект").keys().collect::<Vec<_>>(), vec!["a", "b"]);
    }
}
