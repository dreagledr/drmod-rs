//! Конвертация записи (record) в JSON-скрипт для HTTP API (`POST /script/run`).
//!
//! Ядро конвертации — общее с модом и CLI (`drmod-script::record`): кадр записи
//! декодируется в семантический вход, одинаковые подряд идущие входы сливаются в
//! команды `{t, duration}`, пустые кадры пропускаются. Здесь остаётся только
//! обёртка базы: имя `replay-<id>` и триггер на позиции первого кадра.
//!
//! Запись не хранит статус меню, поэтому все кадры трактуются как геймплей
//! (`MENU_IN_GAME`) — в записи и не бывает меню-навигации: её пишут с включённым
//! вводом в бою. Ключевые ограничения формата (что не воспроизводится) описаны
//! в `drmod-script/src/record.rs`.

use drmod_script::record::{self, RecordFrame};

use super::dump::{Frame, RunMeta};

/// Потолок длительности скрипта — общая константа с модом, а не своя копия:
/// иначе выгрузка записи и приём её модом разъезжались бы по лимиту.
#[allow(unused_imports)]
use drmod_replay_types::script::MAX_SCRIPT_FRAMES;

/// Один кадр БД как кадр конвертера. Статус — геймплейный (см. модульную шапку).
fn record_of(frame: &Frame) -> RecordFrame {
    RecordFrame {
        frame_index: frame.frame_index as u32,
        input: frame.input,
        ripper: frame.ripper_pressed != 0,
        pos: frame.state.pos,
        menu_status_raw: record::MENU_IN_GAME,
    }
}

/// Собирает JSON-скрипт для записи: имя `replay-<id>`, триггер на позиции
/// первого кадра (старт миссии), команды из кадров. По умолчанию — компактный
/// JSON (тело POST /script/run ограничено 64 КБ в api.rs); `pretty` — для
/// чтения человеком.
pub(crate) fn build_script(meta: &RunMeta, frames: &[Frame], pretty: bool) -> Result<String, String> {
    let records: Vec<RecordFrame> = frames.iter().map(record_of).collect();
    let mut document = record::document(&records).map_err(|error| error.to_string())?;
    document.name = format!("replay-{}", meta.id);

    if pretty {
        serde_json::to_string_pretty(&document).map_err(|e| format!("json: {e}"))
    } else {
        serde_json::to_string(&document).map_err(|e| format!("json: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drmod_replay_types::{CameraState, InputUnit, PlayerState, input_bits};

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
        let inp = record::decode(&record_of(&f)).expect("forward");
        assert!(inp.forward && !inp.right);
        assert_eq!(inp.left_stick, None, "стик совпадает с подразумеваемым");
    }

    #[test]
    fn decode_forward_right_emits_explicit_stick() {
        // В записи 117 при FORWARD|RIGHT стик (0,-1000), а не (1000,-1000).
        let f = frame(0, unit(input_bits::FORWARD | input_bits::RIGHT, [0.0, -1000.0]), 0);
        let inp = record::decode(&record_of(&f)).expect("forward+right");
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
        let inp = record::decode(&record_of(&f)).expect("jump+camera");
        assert!(inp.jump);
        assert_eq!(inp.camera, Some([-200.0, 100.0]));
    }

    #[test]
    fn decode_ripper_and_subweapon() {
        let f = frame(0, unit(input_bits::SUBWEAPON, [0.0, 0.0]), 1);
        let inp = record::decode(&record_of(&f)).expect("ripper+subweapon");
        assert!(inp.ripper && inp.subweapon);
    }

    #[test]
    fn decode_empty_is_none() {
        let f = frame(0, InputUnit::default(), 0);
        assert_eq!(record::decode(&record_of(&f)), None);
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
        let records: Vec<RecordFrame> = frames.iter().map(record_of).collect();
        let cmds = record::commands(&records);
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
        let records: Vec<RecordFrame> = frames.iter().map(record_of).collect();
        let cmds = record::commands(&records);
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
        let records: Vec<RecordFrame> = frames.iter().map(record_of).collect();
        let cmds = record::commands(&records);
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
            frame(MAX_SCRIPT_FRAMES as i64 + 1, unit(input_bits::FORWARD, [0.0, -1000.0]), 0),
        ];
        assert!(build_script(&meta, &frames, false).is_err());
    }
}
