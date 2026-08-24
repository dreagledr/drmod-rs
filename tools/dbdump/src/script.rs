//! Конвертация записи (record) в JSON-скрипт для HTTP API (`POST /script/run`).
//!
//! Каждый кадр записи декодируется в семантический вход скрипта (поля
//! `ScriptInput` из `src/api.rs`), одинаковые подряд идущие входы сливаются
//! в команды `{t, duration}` (фронты `pressed` `script_tick` воспроизводит на
//! старте команды — в записи pressed бывает только на переходах down-состояния),
//! пустые кадры пропускаются. Скрипт взводится триггером на позиции первого
//! кадра записи (старт миссии / контрольная точка).
//!
//! Ограничения конвертации (формат скрипта покрывает не всё, что хранит запись):
//! - `buttons_released`/`buttons_alternated` не воспроизводятся (скрипт подаёт
//!   только down/pressed; отпускание — неявное снятие down на конце команды);
//! - биты 0x1/0x8/0x10 неоднозначны (weapon_select/ar_mode/jump в геймплее vs
//!   menu_left/menu_up/confirm в меню) — трактуются как геймплейные;
//! - `pause` (0x20) маппится в поле `pause`, но `script_tick` его пока не
//!   применяет (поле принимается без ошибки).

use drmod_replay_types::input_bits;
use serde::Serialize;

use super::dump::{Frame, RunMeta};

/// Лимит длительности скрипта в кадрах — как `MAX_SCRIPT_FRAMES` в src/api.rs.
const MAX_SCRIPT_FRAMES: u32 = 3600;

/// Вход одной команды скрипта — подмножество полей `ScriptInput` из
/// `src/api.rs`, восстанавливаемое из записи.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize)]
struct ScriptInput {
    #[serde(skip_serializing_if = "is_false")]
    forward: bool,
    #[serde(skip_serializing_if = "is_false")]
    backward: bool,
    #[serde(skip_serializing_if = "is_false")]
    left: bool,
    #[serde(skip_serializing_if = "is_false")]
    right: bool,
    #[serde(skip_serializing_if = "is_false")]
    jump: bool,
    #[serde(skip_serializing_if = "is_false")]
    light_attack: bool,
    #[serde(skip_serializing_if = "is_false")]
    heavy_attack: bool,
    #[serde(skip_serializing_if = "is_false")]
    ar_mode: bool,
    #[serde(skip_serializing_if = "is_false")]
    weapon_select: bool,
    #[serde(skip_serializing_if = "is_false")]
    ninja_run: bool,
    #[serde(skip_serializing_if = "is_false")]
    blade: bool,
    #[serde(skip_serializing_if = "is_false")]
    subweapon: bool,
    #[serde(skip_serializing_if = "is_false")]
    pause: bool,
    #[serde(skip_serializing_if = "is_false")]
    ripper: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    camera: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    left_stick: Option<[f32; 2]>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl ScriptInput {
    /// Пустой ли вход (ни одного действия/стика) — такие кадры пропускаются.
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Одна команда скрипта.
#[derive(Serialize)]
struct ScriptCommand {
    t: u32,
    duration: u32,
    input: ScriptInput,
}

/// Тело скрипта (`POST /script/run`).
#[derive(Serialize)]
struct ScriptJson {
    name: String,
    trigger: TriggerJson,
    commands: Vec<ScriptCommand>,
}

/// Триггер старта: позиция первого кадра записи (старт миссии).
#[derive(Serialize)]
struct TriggerJson {
    pos: [f32; 3],
}

/// Стик, который `script_tick` соберёт из направлений (см. api.rs):
/// forward→(0,-1000), backward→(0,1000), left→(-1000,0), right→(1000,0),
/// диагонали суммируются.
fn implied_stick(inp: &ScriptInput) -> [f32; 2] {
    let mut s = [0.0f32, 0.0];
    if inp.forward {
        s[1] -= 1000.0;
    }
    if inp.backward {
        s[1] += 1000.0;
    }
    if inp.left {
        s[0] -= 1000.0;
    }
    if inp.right {
        s[0] += 1000.0;
    }
    s
}

/// Декодирует кадр записи в семантический вход скрипта. `None` — кадр без
/// ввода (пропускается). `left_stick` эмитится явно, если отличается от
/// подразумеваемого направлениями: в записи 117 при FORWARD|RIGHT стик
/// (0,-1000), а не (1000,-1000) — без явного стика движение разошлось бы.
fn decode_input(f: &Frame) -> Option<ScriptInput> {
    let d = f.input.buttons_down;
    let mut inp = ScriptInput {
        forward: d & input_bits::FORWARD != 0,
        backward: d & input_bits::BACK != 0,
        left: d & input_bits::LEFT != 0,
        right: d & input_bits::RIGHT != 0,
        jump: d & input_bits::JUMP != 0,
        light_attack: d & input_bits::LIGHT_ATTACK != 0,
        heavy_attack: d & input_bits::HEAVY_ATTACK != 0,
        ar_mode: d & input_bits::AR_MODE != 0,
        weapon_select: d & input_bits::WEAPON_SELECT != 0,
        ninja_run: d & input_bits::NINJA_RUN != 0,
        blade: d & input_bits::BLADE != 0,
        subweapon: d & input_bits::SUBWEAPON != 0,
        pause: d & input_bits::CANCEL != 0,
        ripper: f.ripper_pressed != 0,
        ..Default::default()
    };
    if f.input.left_stick != implied_stick(&inp) {
        inp.left_stick = Some(f.input.left_stick);
    }
    if f.input.right_stick != [0.0, 0.0] {
        inp.camera = Some(f.input.right_stick);
    }
    if inp.is_empty() {
        None
    } else {
        Some(inp)
    }
}

/// Собирает команды скрипта: одинаковые подряд идущие входы сливаются в одну
/// команду (фронт `pressed` script_tick ставит на старте команды — совпадает
/// с записью, где pressed бывает только на переходах down-состояния).
/// Ripper — отдельная 1-кадровая команда (фронт keybind'а).
fn build_commands(frames: &[Frame]) -> Vec<ScriptCommand> {
    let mut cmds = Vec::new();
    let mut i = 0;
    while i < frames.len() {
        let Some(inp) = decode_input(&frames[i]) else {
            i += 1;
            continue;
        };
        if inp.ripper {
            cmds.push(ScriptCommand {
                t: frames[i].frame_index as u32,
                duration: 1,
                input: inp,
            });
            i += 1;
            continue;
        }
        let start = i;
        while i + 1 < frames.len() && decode_input(&frames[i + 1]) == Some(inp) {
            i += 1;
        }
        cmds.push(ScriptCommand {
            t: frames[start].frame_index as u32,
            duration: (frames[i].frame_index - frames[start].frame_index + 1) as u32,
            input: inp,
        });
        i += 1;
    }
    cmds
}

/// Собирает JSON-скрипт для записи: имя `replay-<id>`, триггер на позиции
/// первого кадра (старт миссии), команды из кадров. По умолчанию — компактный
/// JSON (тело POST /script/run ограничено 64 КБ в api.rs); `pretty` — для
/// чтения человеком.
pub(crate) fn build_script(meta: &RunMeta, frames: &[Frame], pretty: bool) -> Result<String, String> {
    let trigger_pos = frames
        .first()
        .map(|f| f.state.pos)
        .ok_or_else(|| "нет кадров для триггера".to_string())?;
    let commands = build_commands(frames);
    if let Some(last) = commands.last()
        && last.t + last.duration > MAX_SCRIPT_FRAMES
    {
        return Err(format!(
            "запись длиннее лимита скрипта ({} кадров > {}) — обрежьте или разбейте",
            last.t + last.duration,
            MAX_SCRIPT_FRAMES
        ));
    }
    let script = ScriptJson {
        name: format!("replay-{}", meta.id),
        trigger: TriggerJson { pos: trigger_pos },
        commands,
    };
    if pretty {
        serde_json::to_string_pretty(&script).map_err(|e| format!("json: {e}"))
    } else {
        serde_json::to_string(&script).map_err(|e| format!("json: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drmod_replay_types::{CameraState, InputUnit, PlayerState};

    fn frame(fi: i64, input: InputUnit, ripper: i64) -> Frame {
        Frame {
            frame_index: fi,
            duration_ms: fi * 1000 / 60,
            input,
            state: PlayerState {
                pos: [2.86, 0.0, 70.91],
                ..Default::default()
            },
            camera: CameraState::default(),
            blade_down: 0,
            ripper_pressed: ripper,
            raw_down: None,
            raw_pressed: None,
            enemy: None,
        }
    }

    fn unit(down: u32, stick: [f32; 2]) -> InputUnit {
        InputUnit {
            buttons_down: down,
            left_stick: stick,
            valid_input: 1,
            ..Default::default()
        }
    }

    #[test]
    fn decode_forward_omits_implied_stick() {
        let f = frame(0, unit(input_bits::FORWARD, [0.0, -1000.0]), 0);
        let inp = decode_input(&f).expect("forward");
        assert!(inp.forward && !inp.right);
        assert_eq!(inp.left_stick, None, "стик совпадает с подразумеваемым");
    }

    #[test]
    fn decode_forward_right_emits_explicit_stick() {
        // В записи 117 при FORWARD|RIGHT стик (0,-1000), а не (1000,-1000).
        let f = frame(0, unit(input_bits::FORWARD | input_bits::RIGHT, [0.0, -1000.0]), 0);
        let inp = decode_input(&f).expect("forward+right");
        assert!(inp.forward && inp.right);
        assert_eq!(inp.left_stick, Some([0.0, -1000.0]));
    }

    #[test]
    fn decode_jump_and_camera() {
        let f = frame(
            0,
            InputUnit {
                buttons_down: input_bits::JUMP,
                buttons_pressed: input_bits::JUMP,
                right_stick: [-200.0, 100.0],
                valid_input: 1,
                ..Default::default()
            },
            0,
        );
        let inp = decode_input(&f).expect("jump+camera");
        assert!(inp.jump);
        assert_eq!(inp.camera, Some([-200.0, 100.0]));
    }

    #[test]
    fn decode_ripper_and_subweapon() {
        let f = frame(
            0,
            unit(input_bits::SUBWEAPON, [0.0, 0.0]),
            1,
        );
        let inp = decode_input(&f).expect("ripper+subweapon");
        assert!(inp.ripper && inp.subweapon);
    }

    #[test]
    fn decode_empty_is_none() {
        let f = frame(0, InputUnit::default(), 0);
        assert_eq!(decode_input(&f), None);
    }

    #[test]
    fn rle_merges_identical_inputs() {
        let frames = vec![
            frame(0, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
            frame(1, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
            frame(
                2,
                unit(input_bits::FORWARD | input_bits::JUMP, [0.0, -1000.0]),
                0,
            ),
            frame(3, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
        ];
        let cmds = build_commands(&frames);
        assert_eq!(cmds.len(), 3);
        assert_eq!((cmds[0].t, cmds[0].duration), (0, 2));
        assert!(cmds[0].input.forward && !cmds[0].input.jump);
        assert_eq!((cmds[1].t, cmds[1].duration), (2, 1));
        assert!(cmds[1].input.jump);
        assert_eq!((cmds[2].t, cmds[2].duration), (3, 1));
    }

    #[test]
    fn ripper_is_single_frame_command() {
        let frames = vec![
            frame(0, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
            frame(1, unit(input_bits::FORWARD, [0.0, -1000.0]), 1),
            frame(2, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
        ];
        let cmds = build_commands(&frames);
        assert_eq!(cmds.len(), 3);
        assert_eq!((cmds[1].t, cmds[1].duration), (1, 1));
        assert!(cmds[1].input.ripper);
    }

    #[test]
    fn empty_frames_are_skipped() {
        let frames = vec![
            frame(0, InputUnit::default(), 0),
            frame(1, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
            frame(2, InputUnit::default(), 0),
        ];
        let cmds = build_commands(&frames);
        assert_eq!(cmds.len(), 1);
        assert_eq!((cmds[0].t, cmds[0].duration), (1, 1));
    }

    #[test]
    fn script_json_is_valid() {
        let meta = RunMeta {
            id: 117,
            kind: "record".into(),
            mission_id: 784,
            mission_name: "P310_RESTART".into(),
            started_at: "2026-08-23 21:50:00".into(),
            frame_count: 4,
            duration_ms: 1000,
            source_replay_id: None,
        };
        let frames = vec![
            frame(0, InputUnit::default(), 0),
            frame(1, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
            frame(2, unit(input_bits::FORWARD, [0.0, -1000.0]), 1),
            frame(3, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
        ];
        let json = build_script(&meta, &frames, false).expect("script");
        let v: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(v["name"], "replay-117");
        assert_eq!(v["trigger"]["pos"], serde_json::json!([2.86, 0.0, 70.91]));
        let cmds = v["commands"].as_array().expect("commands");
        assert!(!cmds.is_empty());
        for c in cmds {
            let t = c["t"].as_u64().unwrap();
            let d = c["duration"].as_u64().unwrap();
            assert!(d >= 1 && t + d <= MAX_SCRIPT_FRAMES as u64);
            assert!(!c["input"].as_object().unwrap().is_empty(), "input не пустой");
        }
        // Компактный JSON обязан влезать в лимит тела API (64 КБ, api.rs).
        assert!(json.len() <= 64 * 1024, "компактный JSON {} байт > 64 КБ", json.len());
    }

    #[test]
    fn pretty_json_is_larger_than_compact() {
        let meta = RunMeta {
            id: 1,
            kind: "record".into(),
            mission_id: 1,
            mission_name: "X".into(),
            started_at: "".into(),
            frame_count: 0,
            duration_ms: 0,
            source_replay_id: None,
        };
        let frames = vec![
            frame(0, InputUnit::default(), 0),
            frame(1, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
            frame(2, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
        ];
        let compact = build_script(&meta, &frames, false).unwrap();
        let pretty = build_script(&meta, &frames, true).unwrap();
        assert!(pretty.len() > compact.len());
    }

    #[test]
    fn script_rejects_too_long_recording() {
        let meta = RunMeta {
            id: 1,
            kind: "record".into(),
            mission_id: 1,
            mission_name: "X".into(),
            started_at: "".into(),
            frame_count: 0,
            duration_ms: 0,
            source_replay_id: None,
        };
        let frames = vec![
            frame(0, InputUnit::default(), 0),
            frame(4000, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
        ];
        assert!(build_script(&meta, &frames, false).is_err());
    }
}