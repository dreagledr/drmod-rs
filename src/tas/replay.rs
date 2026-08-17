//! Record/Replay — типы и адреса системы ввода (этапы 0–1: чтение и подача).
//!
//! Подача ввода работает через override глобального `InputUnit[0]`
//! (`base + 0x177B850`) в хуке `cInput::updateInputUnit` (см. `docs/REPLAY.md`).
//! Хук и детуры ввода живут в `hooks.rs`; здесь — состояние override,
//! состояние записи/воспроизведения и ручной инжекции (debug).
//! Логирование — в `crate::logger`. Прямая запись в сырые кэши и поля
//! `Pl0000` не работает — игрок читает ввод из `g_InputUnit0`, а не из этих мест.

use super::types::{InputOverride, InputUnit};
#[cfg(debug_assertions)]
use super::addresses;
#[cfg(debug_assertions)]
use super::types::{CameraState, PlayerState, ReplayFrame, ReplayRunMeta};
use crate::logger;
#[cfg(debug_assertions)]
use crate::segment;
#[cfg(debug_assertions)]
use chrono::Local;
#[cfg(debug_assertions)]
use rusqlite::Connection;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
#[cfg(debug_assertions)]
use std::time::Instant;

const INPUT_OVERRIDE_INIT: InputOverride = InputOverride {
    active: false,
    input: InputUnit {
        buttons_down: 0,
        buttons_pressed: 0,
        buttons_released: 0,
        buttons_alternated: 0,
        left_stick: [0.0; 2],
        right_stick: [0.0; 2],
        left_trigger: 0.0,
        right_trigger: 0.0,
        valid_input: 0,
        repeat_count: 0,
    },
};

static INPUT_OVERRIDE: Mutex<InputOverride> = Mutex::new(INPUT_OVERRIDE_INIT);

/// Последнее значение m_CurrentInput.buttons_down<<32 | buttons_pressed —
/// для ловли фронтов (pressed/down) при реальном вводе.
static LAST_CUR_IN: AtomicU64 = AtomicU64::new(0);

/// Возвращает true, если (down, pressed) изменились с прошлого вызова,
/// и запоминает новые значения. Используется в render для логирования
/// однократных нажатий (прыжок/атаки), которые иначе проскакивают
/// между периодическими frame-логами.
pub fn cur_in_changed(down: u32, pressed: u32) -> bool {
    let key = ((down as u64) << 32) | pressed as u64;
    LAST_CUR_IN.swap(key, Ordering::Relaxed) != key
}

/// Устанавливает override для хука ввода (вызывается из render).
/// Логирует изменения состояния в debug.log.
pub fn set_input_override(ov: InputOverride) {
    if let Ok(mut guard) = INPUT_OVERRIDE.lock() {
        let changed = guard.active != ov.active
            || guard.input.left_stick != ov.input.left_stick
            || guard.input.right_stick != ov.input.right_stick
            || guard.input.buttons_down != ov.input.buttons_down
            || guard.input.buttons_pressed != ov.input.buttons_pressed;
        if changed {
            logger::log_line(&format!(
                "set_override: active={} down={:08X} pressed={:08X} L=({:.2},{:.2}) R=({:.2},{:.2})",
                ov.active,
                ov.input.buttons_down,
                ov.input.buttons_pressed,
                ov.input.left_stick[0],
                ov.input.left_stick[1],
                ov.input.right_stick[0],
                ov.input.right_stick[1]
            ));
        }
        *guard = ov;
    }
}

/// Доступ к текущему override (используется в debug-панели).
pub fn input_override() -> MutexGuard<'static, InputOverride> {
    INPUT_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Применяет активный override к unit (вызывается из детура
/// `updateInputUnit` в `hooks.rs`).
pub(super) fn apply_override(unit: *mut InputUnit) {
    if let Ok(guard) = INPUT_OVERRIDE.lock()
        && guard.active
    {
        unsafe { *unit = guard.input };
    }
}

/// Триггер отложенного старта записи/воспроизведения (спавн R-01 beach).
/// Зеркалит `segment::START_CONDITIONS` для `mission_id == 0x0118`.
#[cfg(debug_assertions)]
const BARE_START_TRIGGER: segment::Vec3 = segment::Vec3 {
    x: -24.7,
    y: 12.14,
    z: 120.7,
};

/// Попадает ли позиция игрока в триггерную зону (допуск как в `segment_action`).
#[cfg(debug_assertions)]
fn in_bare_trigger(pos: Option<segment::Vec3>) -> bool {
    let Some(p) = pos else {
        return false;
    };
    (p.x - BARE_START_TRIGGER.x).abs() <= 0.1
        && (p.y - BARE_START_TRIGGER.y).abs() <= 1.0
        && (p.z - BARE_START_TRIGGER.z).abs() <= 0.1
}

/// Состояние активной записи (NumPad5, отложенный старт по триггеру).
#[cfg(debug_assertions)]
#[derive(Default)]
pub(crate) struct RecordState {
    pub(crate) armed: bool,
    pub(crate) active: bool,
    pub(crate) frames: Vec<ReplayFrame>,
    pub(crate) start: Option<Instant>,
    pub(crate) started_at: String,
    pub(crate) mission_id: i32,
    pub(crate) mission_name: String,
    pub(crate) last_id: Option<i64>,
}

/// Состояние воспроизведения (NumPad6, отложенный старт по триггеру).
#[cfg(debug_assertions)]
#[derive(Default)]
pub(crate) struct PlaybackState {
    pub(crate) armed: bool,
    pub(crate) active: bool,
    pub(crate) frames: Vec<ReplayFrame>,
    pub(crate) frame_idx: usize,
    pub(crate) log: Vec<ReplayFrame>,
    pub(crate) start: Option<Instant>,
    pub(crate) started_at: String,
    pub(crate) last_id: Option<i64>,
}

/// Состояние ручной инжекции ввода (debug-панель + NumPad4-скрипт).
#[cfg(debug_assertions)]
#[derive(Default)]
pub(crate) struct InjectionState {
    pub(crate) w: bool,
    pub(crate) camera: bool,
    pub(crate) jump_frames: u32,
    pub(crate) light_frames: u32,
    pub(crate) heavy_frames: u32,
    pub(crate) script_frames: u32,
}

/// Всё состояние Record/Replay (debug): запись + воспроизведение + инжекция.
#[cfg(debug_assertions)]
#[derive(Default)]
pub(crate) struct ReplayState {
    pub(crate) record: RecordState,
    pub(crate) playback: PlaybackState,
    pub(crate) inject: InjectionState,
}

#[cfg(debug_assertions)]
impl ReplayState {
    /// Переключает запись по NumPad5 (отложенный старт).
    /// `active → стоп + flush`, `armed → отмена`, `idle → arm` (старт по триггеру).
    pub(crate) fn toggle_record(&mut self, conn: Option<&Connection>) {
        if self.record.active {
            self.stop_record(conn);
        } else if self.record.armed {
            self.record.armed = false;
        } else {
            self.stop_playback(conn);
            self.playback.armed = false;
            self.record.armed = true;
        }
    }

    /// Переключает воспроизведение по NumPad6 (отложенный старт).
    /// `active → стоп + flush`, `armed → отмена`, `idle → arm` (требует кадры).
    pub(crate) fn toggle_playback(&mut self, conn: Option<&Connection>) {
        if self.playback.active {
            self.stop_playback(conn);
        } else if self.playback.armed {
            self.playback.armed = false;
        } else if !self.playback.frames.is_empty() {
            self.stop_record(conn);
            self.record.armed = false;
            set_input_override(InputOverride::default());
            // Снять остатки ручной keybind-эмуляции (NumPad7/8) до старта.
            super::hooks::clear_keybind_emulation();
            self.playback.armed = true;
        }
    }

    /// Останавливает запись и флашит буфер в БД (kind=record). Кадры
    /// сохраняются в `playback.frames`, чтобы запись можно было воспроизвести.
    pub(crate) fn stop_record(&mut self, conn: Option<&Connection>) {
        if !self.record.active {
            return;
        }
        self.record.active = false;
        let frames = std::mem::take(&mut self.record.frames);
        let duration_ms = self
            .record
            .start
            .map(|i| i.elapsed().as_millis() as i64)
            .unwrap_or(0);
        self.record.start = None;
        let meta = ReplayRunMeta {
            kind: "record",
            mission_id: self.record.mission_id,
            mission_name: self.record.mission_name.clone(),
            started_at: self.record.started_at.clone(),
            duration_ms,
            source_replay_id: None,
        };
        self.record.last_id = conn.and_then(|conn| super::db::flush_replay(conn, &meta, &frames));
        self.playback.frames = frames;
        logger::log_line(&format!(
            "record: stop frames={} id={:?}",
            self.playback.frames.len(),
            self.record.last_id
        ));
    }

    /// Останавливает воспроизведение, снимает override и флашит лог результата
    /// в БД (kind=playback, source_replay_id=id исходной записи).
    pub(crate) fn stop_playback(&mut self, conn: Option<&Connection>) {
        set_input_override(InputOverride::default());
        // Снять keybind-эмуляцию (ripper/blade): иначе удержание blade
        // останется активным и будет подмешиваться в реальный ввод.
        super::hooks::clear_keybind_emulation();
        let was_active = self.playback.active;
        self.playback.active = false;
        self.playback.frame_idx = 0;
        if was_active && !self.playback.log.is_empty() {
            let log = std::mem::take(&mut self.playback.log);
            let duration_ms = self
                .playback
                .start
                .map(|i| i.elapsed().as_millis() as i64)
                .unwrap_or(0);
            let meta = ReplayRunMeta {
                kind: "playback",
                mission_id: self.record.mission_id,
                mission_name: self.record.mission_name.clone(),
                started_at: self.playback.started_at.clone(),
                duration_ms,
                source_replay_id: self.record.last_id,
            };
            self.playback.last_id =
                conn.and_then(|conn| super::db::flush_replay(conn, &meta, &log));
            logger::log_line(&format!(
                "playback: stop frames={} id={:?}",
                log.len(),
                self.playback.last_id
            ));
        }
        self.playback.start = None;
    }

    /// Отложенный старт: если arm и игрок в триггере — запускает запись или
    /// воспроизведение. Вызывается каждый кадр из `render()`.
    pub(crate) fn update_deferred_start(
        &mut self,
        pos: Option<segment::Vec3>,
        mission_id: i32,
        mission_name: &str,
    ) {
        if !in_bare_trigger(pos) {
            return;
        }
        if self.record.armed {
            self.record.armed = false;
            logger::log_line("deferred: trigger -> start recording");
            // Запись ловит только реальный ввод — сбрасываем keybind-эмуляцию,
            // чтобы остатки ручного NumPad7/8 не подмешались в кадры.
            super::hooks::clear_keybind_emulation();
            self.record.active = true;
            self.record.frames.clear();
            self.record.start = Some(Instant::now());
            self.record.started_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            self.record.mission_id = mission_id;
            self.record.mission_name = mission_name.to_string();
        }
        if self.playback.armed {
            self.playback.armed = false;
            logger::log_line("deferred: trigger -> start playback");
            set_input_override(InputOverride::default());
            super::hooks::clear_keybind_emulation();
            self.playback.active = true;
            self.playback.frame_idx = 0;
            self.playback.log.clear();
            self.playback.start = Some(Instant::now());
            self.playback.started_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        }
    }

    /// Останавливает активную запись/воспроизведение при входе в loading.
    /// Arm НЕ снимается — он должен пережить loading и сработать на спавне.
    pub(crate) fn stop_on_loading(&mut self, conn: Option<&Connection>) {
        if self.record.active {
            logger::log_line("loading: stop active recording");
            self.stop_record(conn);
        }
        if self.playback.active {
            logger::log_line("loading: stop active playback");
            self.stop_playback(conn);
        }
        // Сбрасываем keybind-эмуляцию (ripper/blade): иначе при рестарте
        // детуры isKeybindPressed/isKeybindDown возвращают 1 на пересоздающемся
        // игроке → handleActions падает (access violation).
        super::hooks::clear_keybind_emulation();
    }

    /// Этап 1 (debug): инжекция ввода через override хука updateInputUnit.
    /// Все действия пишутся в глобальный InputUnit[0] — реальный источник
    /// входа игрока (прямая запись в поля Pl0000 в Present не работает:
    /// поздно — после handleActions). Биты — см. `addresses::input_bits`.
    pub(crate) fn update_input_injection(&mut self) {
        // Во время воспроизведения override управляется исключительно playback —
        // debug-инъекция не должна затирать применяемый кадр.
        if self.playback.active {
            return;
        }

        // Скрипт-последовательность (NumPad4): бег ~1 сек → прыжок на бегу →
        // лёгкий удар → поворот камеры. Тайминги в кадрах (60 FPS).
        const SCRIPT_RUN_END: u32 = 60; // бег первые 60 кадров (~1 сек)
        const SCRIPT_JUMP_AT: u32 = 45; // прыжок на 45-м кадре (на бегу)
        const SCRIPT_ATTACK_AT: u32 = 85; // лёгкий удар на 85-м кадре (после)
        const SCRIPT_CAMERA_AT: u32 = 95; // поворот камеры с 95-го кадра
        const SCRIPT_TOTAL: u32 = 130; // конец скрипта

        let script_active = self.inject.script_frames > 0;
        let jump_active = self.inject.jump_frames > 0;
        let light_active = self.inject.light_frames > 0;
        let heavy_active = self.inject.heavy_frames > 0;
        let active = script_active
            || self.inject.w
            || self.inject.camera
            || jump_active
            || light_active
            || heavy_active;

        let mut unit = InputUnit {
            valid_input: 1,
            ..Default::default()
        };

        if script_active {
            let t = self.inject.script_frames;
            if t <= SCRIPT_RUN_END {
                unit.buttons_down |= addresses::input_bits::FORWARD;
                unit.left_stick = [0.0, -1000.0];
            }
            if (SCRIPT_JUMP_AT..SCRIPT_JUMP_AT + 2).contains(&t) {
                unit.buttons_down |= addresses::input_bits::JUMP;
                unit.buttons_pressed |= addresses::input_bits::JUMP;
            }
            if (SCRIPT_ATTACK_AT..SCRIPT_ATTACK_AT + 2).contains(&t) {
                unit.buttons_down |= addresses::input_bits::LIGHT_ATTACK;
                unit.buttons_pressed |= addresses::input_bits::LIGHT_ATTACK;
            }
            if (SCRIPT_CAMERA_AT..SCRIPT_TOTAL).contains(&t) {
                // Поворот камеры вправо (мышь = right_stick, дельта в пикселях)
                unit.right_stick = [300.0, 0.0];
            }
            self.inject.script_frames += 1;
            if self.inject.script_frames > SCRIPT_TOTAL {
                self.inject.script_frames = 0;
            }
        }

        if self.inject.w {
            unit.buttons_down |= addresses::input_bits::FORWARD;
            unit.left_stick = [0.0, -1000.0];
        }
        if jump_active {
            unit.buttons_down |= addresses::input_bits::JUMP;
            unit.buttons_pressed |= addresses::input_bits::JUMP;
            self.inject.jump_frames -= 1;
        }
        if light_active {
            unit.buttons_down |= addresses::input_bits::LIGHT_ATTACK;
            unit.buttons_pressed |= addresses::input_bits::LIGHT_ATTACK;
            self.inject.light_frames -= 1;
        }
        if heavy_active {
            unit.buttons_down |= addresses::input_bits::HEAVY_ATTACK;
            unit.buttons_pressed |= addresses::input_bits::HEAVY_ATTACK;
            self.inject.heavy_frames -= 1;
        }
        if self.inject.camera {
            unit.right_stick = [500.0, 0.0];
        }
        set_input_override(InputOverride {
            active,
            input: unit,
        });
    }

    /// Захват кадра записи (вызывается из render каждый кадр).
    pub(crate) fn capture_frame(&mut self, input: InputUnit, state: PlayerState, camera: CameraState) {
        if self.record.active {
            self.record.frames.push(ReplayFrame {
                frame_index: self.record.frames.len() as u32,
                input,
                state,
                camera,
            });
        }
    }

    /// Подача кадра воспроизведения по индексу + захват результата.
    /// Кадры подаются строго по индексу (1 кадр на вызов render), а не по dt —
    /// dt-сопоставление теряло однокадровые фронты pressed/released.
    /// NOTE: override, выставленный здесь, применяется игрой на СЛЕДУЮЩЕМ тике,
    /// поэтому playback.log[N] = результат кадра frame[N-1] (сдвиг на 1 кадр
    /// относительно record[N]); playback.log[0] — состояние спавна до подачи.
    pub(crate) fn playback_tick(
        &mut self,
        conn: Option<&Connection>,
        cur_input: InputUnit,
        state: PlayerState,
        camera: CameraState,
    ) {
        if !self.playback.active {
            return;
        }
        if self.playback.frame_idx < self.playback.frames.len() {
            let frame = self.playback.frames[self.playback.frame_idx];

            // Ripper/blade не идут через InputUnit — handleActions читает их
            // из DirectInput напрямую через isKeybindPressed(11)/isKeybindDown(8).
            // Подаём их через keybind-эмуляцию, выводя фронт/удержание из
            // записанного состояния: ripper — перепад ripper_enabled,
            // blade — blade_mode_type != 0 (hold). Задание в render(K)
            // применяется на тике K+1, т.е. синхронно с override InputUnit.
            let prev_ripper = if self.playback.frame_idx > 0 {
                self.playback.frames[self.playback.frame_idx - 1].state.ripper_enabled
            } else {
                0
            };
            if frame.state.ripper_enabled != prev_ripper {
                super::hooks::set_ripper_frames(1);
                logger::log_line(&format!(
                    "playback: ripper edge {} -> {} at frame {}",
                    prev_ripper,
                    frame.state.ripper_enabled,
                    self.playback.frame_idx
                ));
            }
            let blade_on = frame.state.blade_mode_type != 0;
            if blade_on != super::hooks::blade_hold() {
                super::hooks::set_blade_hold(blade_on);
                logger::log_line(&format!(
                    "playback: blade hold {} at frame {}",
                    if blade_on { "ON" } else { "OFF" },
                    self.playback.frame_idx
                ));
            }

            set_input_override(InputOverride {
                active: true,
                input: frame.input,
            });
            self.playback.frame_idx += 1;
            self.playback.log.push(ReplayFrame {
                frame_index: self.playback.log.len() as u32,
                input: cur_input,
                state,
                camera,
            });
        } else {
            // Конец записи — снять override и флашнуть лог воспроизведения.
            self.stop_playback(conn);
        }
    }
}
