//! Мок-данные для отладки компоновки: скрипты живут в памяти, диска нет.
//!
//! Когда интерфейс устоится, сюда придёт рабочая область (`workspace.rs`):
//! те же `Script`, но прочитанные из `*.json`.

use crate::model::{Script, from_text};

pub(crate) struct MockScript {
    pub(crate) name: &'static str,
    pub(crate) script: Script,
}

/// Набор моков разной формы: короткий, средний, с условной командой, длинный.
pub(crate) fn scripts() -> Vec<MockScript> {
    let sources: &[(&str, &str)] = &[
        (
            "blade-run",
            r#"{
              "name": "blade-run",
              "commands": [
                { "t": 0, "duration": 60, "input": { "forward": true, "blade": true } }
              ]
            }"#,
        ),
        (
            "run-jump-attack",
            r#"{
              "name": "run-jump-attack",
              "commands": [
                { "t": 0,  "duration": 60, "input": { "forward": true, "left_stick": [0, -1000] } },
                { "t": 45, "duration": 2,  "input": { "jump": true } },
                { "t": 85, "duration": 2,  "input": { "light_attack": true } },
                { "t": 95, "duration": 35, "input": { "camera": [300, 0] } }
              ]
            }"#,
        ),
        (
            "r03-barrier",
            r#"{
              "name": "r03-barrier",
              "trigger": { "pos": [-51.3, 9.1, -85.93] },
              "commands": [
                { "t": 0,  "duration": 45, "input": { "forward": true } },
                { "t": 45, "duration": 12, "input": { "ninja_run": true } },
                { "t": 57, "duration": 30, "input": { "forward": true, "heavy_attack": true } },
                { "t": 74, "duration": 24, "input": { "forward": true, "heavy_attack": true },
                  "when_enemy": { "anim": [65545], "player_y_min": 0.3, "player_y_max": 0.8,
                                  "player_vy_max": 0.0 } },
                { "t": 90, "duration": 1,  "input": { "ripper": true } }
              ]
            }"#,
        ),
        (
            "core117-taps",
            r#"{
              "name": "core117-taps",
              "restart": {},
              "trigger": { "ticks": 30 },
              "commands": [
                { "t": 0,   "duration": 12, "input": { "forward": true } },
                { "t": 12,  "duration": 8,  "input": { "forward": true, "ninja_run": true } },
                { "t": 20,  "duration": 6,  "input": { "jump": true, "forward": true } },
                { "t": 30,  "duration": 20, "input": { "heavy_attack": true } },
                { "t": 52,  "duration": 4,  "input": { "blade": true } },
                { "t": 58,  "duration": 30, "input": { "forward": true, "camera": [1500, -200] } },
                { "t": 90,  "duration": 40, "input": { "walk": true, "forward": true } },
                { "t": 132, "duration": 50, "input": { "subweapon": true, "forward": true } }
              ]
            }"#,
        ),
    ];

    sources
        .iter()
        .map(|(name, source)| MockScript {
            name,
            script: from_text(source).unwrap_or_else(|error| panic!("мок {name} не разобран: {error}")),
        })
        .collect()
}
