use chrono::Local;
use hudhook::{IDirect3DDevice9, ImguiRenderLoop, RenderContext};
use imgui::*;
use rusqlite::Connection;
use std::ptr::NonNull;
use std::time::Instant;

mod d3d_render;
mod game;
mod net;
mod overlay;
mod replay;
mod segment;
mod settings;
mod skeleton;
mod ui;

use d3d_render::{CylinderRenderer, SphereRenderer};
use skeleton::BonePos;

pub const DEFAULT_TITLE: &str = "METAL GEAR RISING REVENGEANCE.exe";

fn init_db() -> (String, Option<String>, Option<Connection>) {
    let now = Local::now();
    let current = now.format("%Y-%m-%d %H:%M:%S").to_string();

    let localappdata = match std::env::var("LOCALAPPDATA") {
        Ok(v) => v,
        Err(_) => return (current, None, None),
    };

    let db_dir = format!("{}\\drmod", localappdata);
    let db_path = format!("{}\\runs.db", db_dir);

    if std::fs::create_dir_all(&db_dir).is_err() {
        return (current, None, None);
    }

    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(_) => return (current, None, None),
    };

    // WAL mode + relaxed sync for fast bulk inserts
    let _ = conn.execute("PRAGMA journal_mode = WAL", []);
    let _ = conn.execute("PRAGMA synchronous = NORMAL", []);
    let _ = conn.execute("PRAGMA foreign_keys = ON", []);

    if conn
        .execute(
            "CREATE TABLE IF NOT EXISTS runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                started_at TEXT NOT NULL
            )",
            (),
        )
        .is_err()
    {
        return (current, None, Some(conn));
    }

    if segment::create_segment_tables(&conn).is_err() {
        return (current, None, Some(conn));
    }

    let prev = conn
        .query_row(
            "SELECT started_at FROM runs ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok();

    let _ = conn.execute("INSERT INTO runs (started_at) VALUES (?1)", [&current]);

    (current, prev, Some(conn))
}

struct HelloHud {
    pub(crate) current_run_start: String,
    pub(crate) prev_run_start: Option<String>,
    pub(crate) db_conn: Option<Connection>,
    pub(crate) base_addr: usize,
    pub(crate) static_ptr_addr: Option<NonNull<u8>>,
    pub(crate) player_manager_addr: Option<NonNull<u8>>,
    pub(crate) camera_ptr_addr: Option<NonNull<u8>>,
    pub(crate) saved_position: Option<(f32, f32, f32)>,
    pub(crate) saved_bones: Option<Vec<BonePos>>,
    // Segment tracking
    pub(crate) active_segment: Option<segment::ActiveSegment>,
    segment_was_active: bool,
    pub(crate) position_buffer: Vec<(segment::Vec3, i64)>,
    // Per-frame previous values for ASL transition detection
    prev_gstr: String,
    prev_gstr2: String,
    prev_gstr4: String,
    prev_r_anim: i32,
    r_anim_ptr: Option<NonNull<u8>>,
    pub(crate) ghost_positions: Vec<(segment::Vec3, i64)>,
    pub(crate) ghost_label: String,
    pub(crate) settings: settings::Settings,
    // 3D test dummy
    dummy: CylinderRenderer,
    remote_sphere: SphereRenderer,
    pub(crate) cached_player_obj_ptr: *mut u8,
    // Raw input (Record/Replay)
    pub(crate) key_input_addr: Option<NonNull<u8>>,
    pub(crate) mouse_input_addr: Option<NonNull<u8>>,
    // Хук cInput::updateInputUnit (подача ввода: подмена InputUnit[0])
    pub(crate) input_hook: Option<hudhook::mh::MhHook>,
    // Короткая запись/воспроизведение по нумпаду (smoke-тест InputUnit, Этап 1.5).
    #[cfg(debug_assertions)]
    pub(crate) bare_playback: bool,
    #[cfg(debug_assertions)]
    pub(crate) bare_playback_frames: Vec<replay::ReplayFrame>,
    #[cfg(debug_assertions)]
    pub(crate) bare_playback_start: Option<Instant>,
    // Инжекция ввода (debug-кнопки)
    #[cfg(debug_assertions)]
    pub(crate) inject_w: bool,
    #[cfg(debug_assertions)]
    pub(crate) inject_camera: bool,
    #[cfg(debug_assertions)]
    pub(crate) inject_jump_frames: u32,
    #[cfg(debug_assertions)]
    pub(crate) inject_light_frames: u32,
    #[cfg(debug_assertions)]
    pub(crate) inject_heavy_frames: u32,
    #[cfg(debug_assertions)]
    pub(crate) script_frames: u32,
    pub(crate) d3d_frame_count: u32,
    pub(crate) d3d_last_error: String,
    // Multiplayer
    pub(crate) net_client: Option<net::NetClient>,
    pub(crate) player_name: String,
    pub(crate) room_name: String,
    pub(crate) server_addr: String,
    pub(crate) last_sent_pos: Option<segment::Vec3>,
    last_sent_mission_id: i32,
    last_skeleton_send: Instant,
    pub(crate) viewport: [f32; 4], // [X, Y, Width, Height] from D3D GetViewport
}

impl HelloHud {
    fn new() -> Self {
        let (current_run_start, prev_run_start, db_conn) = init_db();

        let base_addr = unsafe {
            windows::Win32::System::LibraryLoader::GetModuleHandleA(windows::core::PCSTR::null())
        }
        .map(|h| h.0 as usize)
        .unwrap_or(0);

        let static_ptr_addr = if base_addr == 0 {
            None
        } else {
            NonNull::new(unsafe { (base_addr as *mut u8).add(0x177B4A4) })
        };

        let player_manager_addr = if base_addr == 0 {
            None
        } else {
            // base + 0x17EA100 содержит указатель на PlayerManagerImplement
            let pm_ptr = unsafe { *((base_addr + 0x17EA100) as *const usize) };
            NonNull::new(pm_ptr as *mut u8)
        };

        let camera_ptr_addr = if base_addr == 0 {
            None
        } else {
            // base + 0x17EA1D0 — статический адрес cCameraGame::Instance (SDK)
            NonNull::new(unsafe { (base_addr as *mut u8).add(0x17EA1D0) })
        };

        // Сырой ввод: cInput::ms_KeyInput / cInput::ms_MouseInput (SDK, Hw.h)
        let key_input_addr = if base_addr == 0 {
            None
        } else {
            NonNull::new(unsafe { (base_addr as *mut u8).add(replay::KEY_INPUT) })
        };
        let mouse_input_addr = if base_addr == 0 {
            None
        } else {
            NonNull::new(unsafe { (base_addr as *mut u8).add(replay::MOUSE_INPUT) })
        };

        // rAnim: 3-level pointer chain (ASL: 0x019C14C4 → +0x788 → +0x618)
        let r_anim_ptr = if base_addr == 0 {
            None
        } else {
            let l1 = unsafe { *((base_addr + 0x019C14C4) as *const usize) };
            if l1 == 0 {
                None
            } else {
                let l2 = unsafe { *((l1 + 0x788) as *const usize) };
                if l2 == 0 {
                    None
                } else {
                    NonNull::new((l2 + 0x618) as *mut u8)
                }
            }
        };

        // Хук cInput::updateInputUnit — подача ввода: после вызова оригинала
        // перезаписываем глобальный InputUnit[0] (реальный источник игрока).
        let input_hook = Self::create_input_hook(base_addr);

        replay::log_line(&format!(
            "=== drmod init === base=0x{:08X} input_hook={}",
            base_addr,
            if input_hook.is_some() { "OK" } else { "FAIL" }
        ));

        Self {
            current_run_start,
            prev_run_start,
            db_conn,
            base_addr,
            static_ptr_addr,
            player_manager_addr,
            camera_ptr_addr,
            saved_position: None,
            saved_bones: None,
            active_segment: None,
            segment_was_active: false,
            position_buffer: Vec::new(),
            prev_gstr: String::new(),
            prev_gstr2: String::new(),
            prev_gstr4: String::new(),
            prev_r_anim: 0,
            r_anim_ptr,
            ghost_positions: Vec::new(),
            ghost_label: String::new(),
            settings: settings::Settings::default(),
            dummy: CylinderRenderer::new(24, 0xFFFFFFFF), // white → colour via TFACTOR
            remote_sphere: SphereRenderer::new(16, 8, 0xFFFFFFFF),
            cached_player_obj_ptr: std::ptr::null_mut(),
            key_input_addr,
            mouse_input_addr,
            input_hook,
            #[cfg(debug_assertions)]
            bare_playback: false,
            #[cfg(debug_assertions)]
            bare_playback_frames: Vec::new(),
            #[cfg(debug_assertions)]
            bare_playback_start: None,
            #[cfg(debug_assertions)]
            inject_w: false,
            #[cfg(debug_assertions)]
            inject_camera: false,
            #[cfg(debug_assertions)]
            inject_jump_frames: 0,
            #[cfg(debug_assertions)]
            inject_light_frames: 0,
            #[cfg(debug_assertions)]
            inject_heavy_frames: 0,
            #[cfg(debug_assertions)]
            script_frames: 0,
            d3d_frame_count: 0,
            d3d_last_error: String::new(),
            net_client: None,
            player_name: "Raiden".to_string(),
            room_name: "default".to_string(),
            server_addr: "127.0.0.1:5222".to_string(),
            last_sent_pos: None,
            last_sent_mission_id: 0,
            last_skeleton_send: Instant::now(),
            viewport: [0.0; 4],
        }
    }

    /// Устанавливает MinHook на `cInput::updateInputUnit` (0x9DAFE0):
    /// после вызова оригинала детур перезаписывает InputUnit игрока.
    /// Возвращает хук для удержания (хранится в HelloHud).
    fn create_input_hook(base_addr: usize) -> Option<hudhook::mh::MhHook> {
        use core::ffi::c_void;
        use hudhook::mh::{MH_ApplyQueued, MhHook};

        if base_addr == 0 {
            replay::log_line("create_input_hook: base_addr=0");
            return None;
        }
        let target = (base_addr + replay::UPDATE_INPUT_UNIT) as *mut c_void;
        let detour = replay::update_input_unit_detour as *mut c_void;
        let hook = match unsafe { MhHook::new(target, detour) } {
            Ok(h) => h,
            Err(e) => {
                replay::log_line(&format!(
                    "create_input_hook: MH_CreateHook FAIL target=0x{:08X} err={:?}",
                    target as usize, e
                ));
                return None;
            }
        };
        let trampoline: unsafe extern "C" fn(*mut replay::InputUnit, i32) =
            unsafe { std::mem::transmute(hook.trampoline()) };
        let _ = replay::set_original_update_input_unit(trampoline);
        if let Err(e) = unsafe { hook.queue_enable() } {
            replay::log_line(&format!("create_input_hook: queue_enable FAIL err={:?}", e));
            return None;
        }
        let _ = unsafe { MH_ApplyQueued() };
        replay::log_line(&format!(
            "create_input_hook: OK target=0x{:08X} trampoline=0x{:08X}",
            target as usize,
            hook.trampoline() as usize
        ));
        Some(hook)
    }

    fn read_game_state(&mut self) -> ui::UiState {
        let mut state = ui::UiState {
            mission_id: 0,
            mission_id_raw: 0,
            mission_name: String::new(),
            menu_status_raw: 0,
            menu_status_valid: false,
            menu_status: game::GameMenuStatus::None,
            sword_state: 0,
            sword_hidden: 0,
            main_weapon: 0,
            custom_weapon: 0,
            sub_weapon: 0,
            static_ptr_value: 0,
            position: None,
            hp: 0,
            player_found: false,
            segment_action: segment::SegmentAction::None,
            gstr: String::new(),
            gstr2: String::new(),
            gstr4: String::new(),
            r_anim: 0,
        };

        // Строки миссии (gStr) и анимация (rAnim) в loading уничтожаются —
        // их нельзя читать, пока статус не станет валидным и не-загрузочным.
        let mut player_readable = false;

        if self.base_addr != 0 {
            // --- GAME MENU STATUS (первым: от него зависит, можно ли читать остальное) ---
            let menu_status_addr = self.base_addr + 0x17E9F9C;
            state.menu_status_raw = unsafe { *(menu_status_addr as *const i32) };
            if (0..=18).contains(&state.menu_status_raw) {
                state.menu_status = unsafe {
                    std::mem::transmute::<i32, game::GameMenuStatus>(state.menu_status_raw)
                };
                state.menu_status_valid = true;
            }
            player_readable = state.menu_status_valid && !state.menu_status.is_loading();

            // --- MISSION ---
            if player_readable {
                let mission_id_addr = self.base_addr + 0x1764670;
                state.mission_id_raw = unsafe { *(mission_id_addr as *const i32) };
                let (name_addr, eff_id) = if state.mission_id_raw != 0 {
                    (self.base_addr + 0x1764674, state.mission_id_raw)
                } else {
                    (self.base_addr + 0x1766008, unsafe {
                        *((self.base_addr + 0x1766004) as *const i32)
                    })
                };
                state.mission_id = eff_id;
                state.mission_name = unsafe { std::ffi::CStr::from_ptr(name_addr as *const i8) }
                    .to_string_lossy()
                    .into_owned();
            }

            // --- gStr / gStr2 / gStr4 (ASL location strings) ---
            if player_readable {
                state.gstr = unsafe {
                    std::ffi::CStr::from_ptr((self.base_addr + 0x14B9181) as *const i8)
                }
                .to_string_lossy()
                .into_owned();
                state.gstr2 = unsafe {
                    std::ffi::CStr::from_ptr((self.base_addr + 0x14B91AD) as *const i8)
                }
                .to_string_lossy()
                .into_owned();
                state.gstr4 = unsafe {
                    std::ffi::CStr::from_ptr((self.base_addr + 0x14B91A8) as *const i8)
                }
                .to_string_lossy()
                .into_owned();
            }

            // --- rAnim (Raiden animation, 3-level pointer chain) ---
            if player_readable
                && let Some(r_anim_ptr) = self.r_anim_ptr
            {
                state.r_anim = unsafe { *(r_anim_ptr.as_ptr() as *const i32) };
            }
        }

        // --- Pl0000 / Player ---
        if let Some(static_ptr) = self.static_ptr_addr {
            state.static_ptr_value = static_ptr.as_ptr() as usize;
            self.cached_player_obj_ptr = unsafe { *(static_ptr.as_ptr() as *const *mut u8) };
            if !self.cached_player_obj_ptr.is_null() {
                state.player_found = true;
                state.sword_state =
                    unsafe { *(self.cached_player_obj_ptr.add(0x13FC) as *const i32) };
                state.sword_hidden =
                    unsafe { *(self.cached_player_obj_ptr.add(0xB74) as *const i32) };
                state.position = Some(segment::Vec3 {
                    x: unsafe { *(self.cached_player_obj_ptr.add(0x50) as *const f32) },
                    y: unsafe { *(self.cached_player_obj_ptr.add(0x54) as *const f32) },
                    z: unsafe { *(self.cached_player_obj_ptr.add(0x58) as *const f32) },
                });
                state.hp = unsafe { *(self.cached_player_obj_ptr.add(0x870) as *const i32) };
            }
        }

        // --- WEAPONS ---
        if let Some(pm_addr) = self.player_manager_addr {
            let pm_ptr = pm_addr.as_ptr();
            state.main_weapon = unsafe { *(pm_ptr.add(0xE0) as *const i32) };
            state.custom_weapon = unsafe { *(pm_ptr.add(0xE4) as *const i32) };
            state.sub_weapon = unsafe { *(pm_ptr.add(0xE8) as *const i32) };
        }

        // --- SEGMENT ACTION ---
        if player_readable {
            state.segment_action = segment::segment_action(
                state.mission_id,
                &state.mission_name,
                state.position,
                state.menu_status,
                &state.gstr,
                &self.prev_gstr,
                &state.gstr2,
                &self.prev_gstr2,
                state.r_anim,
                self.prev_r_anim,
                self.active_segment.as_ref(),
            );

            // Update prev_ values for next frame
            self.prev_gstr = state.gstr.clone();
            self.prev_gstr2 = state.gstr2.clone();
            self.prev_gstr4 = state.gstr4.clone();
            self.prev_r_anim = state.r_anim;
        } else {
            // В загрузке gStr/rAnim заглушены — не двигаем prev_* и не даём
            // ложных finish-переходов. Выход в меню по-прежнему сбрасывает сегмент.
            state.segment_action =
                if state.menu_status == game::GameMenuStatus::MainMenuLoad
                    && self.active_segment.is_some()
                {
                    segment::SegmentAction::Reset
                } else {
                    segment::SegmentAction::None
                };
        }

        // --- APPLY SEGMENT ACTION ---
        match state.segment_action {
            segment::SegmentAction::Reset => {
                self.position_buffer.clear();
                self.ghost_positions.clear();
                self.ghost_label.clear();
                self.active_segment = None;
            }
            segment::SegmentAction::End => {
                if let (Some(seg), Some(conn)) =
                    (self.active_segment.as_ref(), self.db_conn.as_ref())
                {
                    segment::finish_segment(conn, seg, &self.position_buffer);
                }
                self.position_buffer.clear();
                self.ghost_positions.clear();
                self.ghost_label.clear();
                self.active_segment = None;
            }
            segment::SegmentAction::Start { mission_id } => {
                self.position_buffer.clear();
                self.ghost_positions.clear();
                self.ghost_label.clear();

                let fastest_ms = if let Some(ref conn) = self.db_conn {
                    let (ms, positions) = segment::load_best_ghost(conn, mission_id);
                    if let Some(best_ms) = ms {
                        self.ghost_label =
                            format!("Best {}", overlay::format_duration_ms(best_ms as u64));
                    }
                    self.ghost_positions = positions;
                    ms
                } else {
                    None
                };

                self.active_segment = Some(segment::ActiveSegment {
                    start_instant: Instant::now(),
                    started_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    mission_id,
                    mission_name: state.mission_name.clone(),
                    fastest_ms,
                });
            }
            segment::SegmentAction::None => {}
        }

        // --- POSITION BUFFER PUSH ---
        if state.player_found
            && let (Some(seg), Some(pos)) = (self.active_segment.as_ref(), state.position) {
                let dur = seg.start_instant.elapsed().as_millis() as i64;
                self.position_buffer.push((pos, dur));
            }

        self.segment_was_active = self.active_segment.is_some();

        state
    }

    /// Читает сырой ввод клавиатуры: (m_aKeysDown, m_aKeysPressed).
    pub(crate) fn read_keys(&self) -> ([u32; 6], [u32; 6]) {
        match self.key_input_addr {
            Some(addr) => {
                let k: replay::KeyInput = unsafe { addr.as_ptr().cast::<replay::KeyInput>().read() };
                (k.keys_down, k.keys_pressed)
            }
            None => Default::default(),
        }
    }

    /// Читает сырое состояние мыши (кнопки + позиция).
    pub(crate) fn read_mouse(&self) -> replay::MouseState {
        match self.mouse_input_addr {
            Some(addr) => {
                let base = addr.as_ptr();
                unsafe {
                    replay::MouseState {
                        buttons: *(base.cast::<i32>()),
                        buttons_pressed: *(base.add(0x04).cast::<i32>()),
                        position: *(base.add(0x10).cast::<[f32; 2]>()),
                        last_position: *(base.add(0x20).cast::<[f32; 2]>()),
                    }
                }
            }
            None => replay::MouseState::default(),
        }
    }

    /// Читает нормализованный ввод игрока (Pl0000::m_CurrentInput).
    pub(crate) fn read_current_input(&self) -> replay::InputUnit {
        if self.cached_player_obj_ptr.is_null() {
            return replay::InputUnit::default();
        }
        unsafe {
            self.cached_player_obj_ptr
                .add(replay::CURRENT_INPUT_OFFSET)
                .cast::<replay::InputUnit>()
                .read()
        }
    }

    /// Читает глобальный InputUnit[0] (base+0x177B850) — реальный источник
    /// входа игрока (Pl0000::updateInput копирует его в m_CurrentInput).
    pub(crate) fn read_global_input_unit(&self) -> replay::InputUnit {
        if self.base_addr == 0 {
            return replay::InputUnit::default();
        }
        unsafe {
            ((self.base_addr + replay::GLOBAL_INPUT_UNIT0) as *const replay::InputUnit).read()
        }
    }

    /// Читает полный снимок нормализованного ввода игрока (Pl0000) — InputUnit
    /// по 0xCF8 + m_fInputDirection и m_nButton* по подтверждённым SDK-смещениям.
    pub(crate) fn read_pl_input(&self) -> replay::PlInputSnapshot {
        if self.cached_player_obj_ptr.is_null() {
            return replay::PlInputSnapshot::default();
        }
        let p = self.cached_player_obj_ptr;
        unsafe {
            replay::PlInputSnapshot {
                input: p.add(replay::CURRENT_INPUT_OFFSET).cast::<replay::InputUnit>().read(),
                input_mag_sq: *(p.add(replay::PL_INPUT_MAG_SQ) as *const f32),
                input_direction: *(p.add(replay::PL_INPUT_DIR) as *const f32),
                button_jump: *(p.add(replay::PL_BUTTON_JUMP) as *const i32),
                button_light_attack: *(p.add(replay::PL_BUTTON_LIGHT_ATTACK) as *const i32),
                button_heavy_attack: *(p.add(replay::PL_BUTTON_HEAVY_ATTACK) as *const i32),
                button_action: *(p.add(replay::PL_BUTTON_ACTION) as *const i32),
                button_ninjarun: *(p.add(replay::PL_BUTTON_NINJARUN) as *const i32),
                button_blademode: *(p.add(replay::PL_BUTTON_BLADEMODE) as *const i32),
                button_use_item: *(p.add(replay::PL_BUTTON_USEITEM) as *const i32),
            }
        }
    }

    /// Этап 1 (debug): инжекция ввода через override хука updateInputUnit.
    /// Все действия пишутся в глобальный InputUnit[0] — реальный источник
    /// входа игрока (прямая запись в поля Pl0000 в Present не работает:
    /// поздно — после handleActions). Биты — см. `replay::input_bits`.
    #[cfg(debug_assertions)]
    pub(crate) fn update_input_injection(&mut self) {
        // Во время воспроизведения override управляется исключительно playback —
        // debug-инъекция не должна затирать применяемый кадр.
        if self.bare_playback {
            return;
        }

        // Скрипт-последовательность (NumPad4): бег ~1 сек → прыжок на бегу →
        // лёгкий удар → поворот камеры. Тайминги в кадрах (60 FPS).
        const SCRIPT_RUN_END: u32 = 60; // бег первые 60 кадров (~1 сек)
        const SCRIPT_JUMP_AT: u32 = 45; // прыжок на 45-м кадре (на бегу)
        const SCRIPT_ATTACK_AT: u32 = 85; // лёгкий удар на 85-м кадре (после)
        const SCRIPT_CAMERA_AT: u32 = 95; // поворот камеры с 95-го кадра
        const SCRIPT_TOTAL: u32 = 130; // конец скрипта

        let script_active = self.script_frames > 0;
        let jump_active = self.inject_jump_frames > 0;
        let light_active = self.inject_light_frames > 0;
        let heavy_active = self.inject_heavy_frames > 0;
        let active = script_active
            || self.inject_w
            || self.inject_camera
            || jump_active
            || light_active
            || heavy_active;

        let mut unit = replay::InputUnit {
            valid_input: 1,
            ..Default::default()
        };

        if script_active {
            let t = self.script_frames;
            if t <= SCRIPT_RUN_END {
                unit.buttons_down |= replay::input_bits::FORWARD;
                unit.left_stick = [0.0, -1000.0];
            }
            if (SCRIPT_JUMP_AT..SCRIPT_JUMP_AT + 2).contains(&t) {
                unit.buttons_down |= replay::input_bits::JUMP;
                unit.buttons_pressed |= replay::input_bits::JUMP;
            }
            if (SCRIPT_ATTACK_AT..SCRIPT_ATTACK_AT + 2).contains(&t) {
                unit.buttons_down |= replay::input_bits::LIGHT_ATTACK;
                unit.buttons_pressed |= replay::input_bits::LIGHT_ATTACK;
            }
            if (SCRIPT_CAMERA_AT..SCRIPT_TOTAL).contains(&t) {
                // Поворот камеры вправо (мышь = right_stick, дельта в пикселях)
                unit.right_stick = [300.0, 0.0];
            }
            self.script_frames += 1;
            if self.script_frames > SCRIPT_TOTAL {
                self.script_frames = 0;
            }
        }

        if self.inject_w {
            unit.buttons_down |= replay::input_bits::FORWARD;
            unit.left_stick = [0.0, -1000.0];
        }
        if jump_active {
            unit.buttons_down |= replay::input_bits::JUMP;
            unit.buttons_pressed |= replay::input_bits::JUMP;
            self.inject_jump_frames -= 1;
        }
        if light_active {
            unit.buttons_down |= replay::input_bits::LIGHT_ATTACK;
            unit.buttons_pressed |= replay::input_bits::LIGHT_ATTACK;
            self.inject_light_frames -= 1;
        }
        if heavy_active {
            unit.buttons_down |= replay::input_bits::HEAVY_ATTACK;
            unit.buttons_pressed |= replay::input_bits::HEAVY_ATTACK;
            self.inject_heavy_frames -= 1;
        }
        if self.inject_camera {
            unit.right_stick = [500.0, 0.0];
        }
        replay::set_input_override(replay::InputOverride {
            active,
            input: unit,
        });
    }

    /// Переключает короткую запись по NumPad5 (Этап 1.5). При стопе кладёт
    /// накопленные кадры в `bare_playback_frames`; при старте снимает активное
    /// воспроизведение, чтобы запись и воспроизведение не пересекались.
    #[cfg(debug_assertions)]
    pub(crate) fn toggle_bare_record(&mut self) {
        if replay::is_bare_recording() {
            let frames = replay::stop_bare_recording().unwrap_or_default();
            self.bare_playback_frames = frames;
        } else {
            self.stop_bare_playback();
            replay::start_bare_recording();
        }
    }

    /// Переключает воспроизведение короткой записи по NumPad6 (Этап 1.5).
    /// Стартует, только если есть кадры; при старте останавливает активную
    /// запись, чтобы не захватывать новый ввод поверх воспроизведения.
    #[cfg(debug_assertions)]
    pub(crate) fn toggle_bare_playback(&mut self) {
        if self.bare_playback {
            self.stop_bare_playback();
        } else if !self.bare_playback_frames.is_empty() {
            replay::stop_bare_recording();
            replay::set_input_override(replay::InputOverride::default());
            self.bare_playback = true;
            self.bare_playback_start = Some(Instant::now());
        }
    }

    /// Останавливает воспроизведение короткой записи: снимает override.
    /// Кадры сохраняются, чтобы запись можно было проиграть повторно.
    #[cfg(debug_assertions)]
    pub(crate) fn stop_bare_playback(&mut self) {
        if !self.bare_playback {
            return;
        }
        replay::set_input_override(replay::InputOverride::default());
        self.bare_playback = false;
        self.bare_playback_start = None;
    }
}

unsafe impl Send for HelloHud {}
unsafe impl Sync for HelloHud {}

impl ImguiRenderLoop for HelloHud {
    fn initialize<'a>(&'a mut self, ctx: &mut Context, _render_context: &'a mut dyn RenderContext) {
        let fonts = ctx.fonts();

        // Основной шрифт: Segoe UI Variable с поддержкой кириллицы
        fonts.add_font(&[FontSource::TtfData {
            data: include_bytes!("C:/Windows/Fonts/SegUIVar.ttf"),
            size_pixels: 16.0,
            config: Some(FontConfig {
                glyph_ranges: FontGlyphRanges::cyrillic(),
                ..Default::default()
            }),
        }]);
    }

    fn render_3d(&mut self, device: &IDirect3DDevice9) {
        self.d3d_frame_count = self.d3d_frame_count.wrapping_add(1);

        // Read D3D viewport — единственный надёжный источник размера области рендера
        {
            let mut vp = windows::Win32::Graphics::Direct3D9::D3DVIEWPORT9::default();
            unsafe {
                device.GetViewport(&mut vp).ok();
            }
            self.viewport = [vp.X as f32, vp.Y as f32, vp.Width as f32, vp.Height as f32];
        }

        // Read camera view*proj matrix (needed for all draws)
        let camera_ptr = match self.camera_ptr_addr {
            Some(addr) => addr.as_ptr(),
            None => return,
        };
        let view_proj = unsafe { *(camera_ptr.add(0x200) as *const [f32; 16]) };

        // ── Ghost cylinder (red, semi-transparent) ──────────────────
        if self.settings.show_best_ghost
            && let Some(seg) = self.active_segment.as_ref()
            && !self.ghost_positions.is_empty()
        {
            let current_ms = seg.start_instant.elapsed().as_millis() as i64;
            let idx = self
                .ghost_positions
                .partition_point(|&(_, dur)| dur <= current_ms);
            if idx > 0 {
                let (gp, _) = self.ghost_positions[idx - 1];
                self.dummy.render(
                    device,
                    (gp.x, gp.y, gp.z),
                    0.4,
                    2.0,
                    settings::apply_opacity(0x000000FF, self.settings.ghost_opacity),
                    &view_proj,
                );
            }
        }

        // ── Saved position cylinder (green, semi-transparent) ──────
        #[cfg(debug_assertions)]
        if let Some((sx, sy, sz)) = self.saved_position {
            self.dummy.render(
                device,
                (sx, sy, sz),
                0.4,
                2.0,
                settings::apply_opacity(0x0000FF00, self.settings.ghost_opacity),
                &view_proj,
            );
        }

        // ── Remote players (blue, semi-transparent) ────────────────
        if let (Some(nc), Some(seg)) = (&self.net_client, self.active_segment.as_ref()) {
            for rp in &nc.remote_players {
                if rp.mission_id != seg.mission_id {
                    continue;
                }
                if rp.last_update.elapsed() > std::time::Duration::from_secs(5) {
                    continue;
                }
                // Цилиндр — только если нет скелета
                if rp.skeleton.is_empty() {
                    self.dummy.render(
                        device,
                        (rp.pos.x, rp.pos.y, rp.pos.z),
                        0.4,
                        2.0,
                        settings::apply_opacity(0x0000FFFF, self.settings.ghost_opacity),
                        &view_proj,
                    );
                }
            }

            // 3D скелет: рёбра цилиндрами (батч) + сфера головы
            let mut all_capsules: Vec<(f32, f32, f32, f32, f32, f32)> = Vec::new();
            for rp in &nc.remote_players {
                if rp.skeleton.is_empty() || rp.mission_id != seg.mission_id {
                    continue;
                }
                if rp.last_update.elapsed() > std::time::Duration::from_secs(5) {
                    continue;
                }
                // Delta-компенсация по XZ + Y с учётом feet→pelvis offset (~1.0m)
                let (dx, dy, dz) = if let Some(root) = rp.skeleton.iter().find(|b| b.index == 0) {
                    (rp.pos.x - root.x, rp.pos.y - root.y + 1.0, rp.pos.z - root.z)
                } else {
                    (0.0, 0.0, 0.0)
                };
                // Собираем рёбра со смещением
                let edges = skeleton::build_edges_from_wire(&rp.skeleton);
                for &(p, c) in &edges {
                    let pb = &rp.skeleton[p];
                    let cb = &rp.skeleton[c];
                    all_capsules.push((
                        pb.x + dx, pb.y + dy, pb.z + dz,
                        cb.x + dx, cb.y + dy, cb.z + dz,
                    ));
                }
                // Сфера на голове (bone index 5)
                if let Some(head) = rp.skeleton.iter().find(|b| b.index == 5) {
                    self.remote_sphere.render(
                        device,
                        (head.x + dx, head.y + dy, head.z + dz),
                        0.12,
                        settings::apply_opacity(0x00FF0000, self.settings.ghost_opacity),
                        &view_proj,
                    );
                }
            }
            if !all_capsules.is_empty() {
                self.dummy.render_capsules_batched(
                    device,
                    &all_capsules,
                    0.04,
                    settings::apply_opacity(0x00FF8000, self.settings.ghost_opacity),
                    &view_proj,
                );
            }
        }
    }

    fn render(&mut self, ui: &mut Ui) {
        // Сначала собираем состояние игры: read_game_state обновляет
        // cached_player_obj_ptr (в loading игра обнуляет static_ptr → кэш = null),
        // иначе диагностика ниже читает stale-указатель освобождённого игрока.
        let ui_state = self.read_game_state();
        #[cfg(not(debug_assertions))]
        let _ = &ui_state; // suppress unused warning in release

        // ── Multiplayer network (выполняется каждый кадр, независимо от UI) ──
        if let Some(ref mut nc) = self.net_client {
            nc.poll_tcp();

            // Читаем позицию для отправки
            let pos_opt = self.static_ptr_addr.and_then(|addr| {
                let player_obj_ptr = unsafe { *(addr.as_ptr() as *const *mut u8) };
                if player_obj_ptr.is_null() {
                    None
                } else {
                    Some(segment::Vec3 {
                        x: unsafe { *(player_obj_ptr.add(0x50) as *const f32) },
                        y: unsafe { *(player_obj_ptr.add(0x54) as *const f32) },
                        z: unsafe { *(player_obj_ptr.add(0x58) as *const f32) },
                    })
                }
            });

            if let (Some(pos), Some(seg)) = (pos_opt, self.active_segment.as_ref()) {
                let changed =
                    self.last_sent_pos != Some(pos) || self.last_sent_mission_id != seg.mission_id;
                if changed {
                    let hp = self
                        .static_ptr_addr
                        .and_then(|addr| {
                            let player_obj_ptr = unsafe { *(addr.as_ptr() as *const *mut u8) };
                            if player_obj_ptr.is_null() {
                                None
                            } else {
                                Some(unsafe { *(player_obj_ptr.add(0x870) as *const i32) })
                            }
                        })
                        .unwrap_or(0);
                    nc.send_position(pos, hp, seg.mission_id);
                    self.last_sent_pos = Some(pos);
                    self.last_sent_mission_id = seg.mission_id;
                }
            }

            nc.recv_udp();

            // Отправляем скелет раз в ~100 мс
            if self.last_skeleton_send.elapsed() > std::time::Duration::from_millis(30) {
                self.last_skeleton_send = Instant::now();
                let player_obj_ptr = self.static_ptr_addr.and_then(|addr| {
                    let ptr = unsafe { *(addr.as_ptr() as *const *mut u8) };
                    if ptr.is_null() { None } else { Some(ptr) }
                });
                if let Some(ptr) = player_obj_ptr {
                    let bones = unsafe { skeleton::read_full_skeleton(ptr) };
                    if !bones.is_empty() {
                        let wire = skeleton::bones_to_wire(&bones);
                        nc.send_skeleton(&wire);
                    }
                }
            }
        }
        // ─────────────────────────────────────────────────────────────

        // Этап 1 (debug): инжекция ввода через override g_InputUnit0 —
        // выполняется каждый кадр, чтобы debug-панель показывала состояние
        #[cfg(debug_assertions)]
        self.update_input_injection();

        // Диагностика: дамп m_CurrentInput игрока, позиции и g_unit0.
        // Логируется при КАЖДОМ изменении кнопок (down/pressed) — чтобы
        // поймать однократные фронты (прыжок/атаки), плюс heartbeat каждые
        // 120 кадров (чтобы видеть движение и стики даже без кнопок).
        #[cfg(debug_assertions)]
        {
            let ci = self.read_current_input();
            let changed = replay::cur_in_changed(ci.buttons_down, ci.buttons_pressed);
            if changed || self.d3d_frame_count.is_multiple_of(120) {
                let (px, py, pz) = if self.cached_player_obj_ptr.is_null() {
                    (0.0, 0.0, 0.0)
                } else {
                    unsafe {
                        (
                            *(self.cached_player_obj_ptr.add(0x50) as *const f32),
                            *(self.cached_player_obj_ptr.add(0x54) as *const f32),
                            *(self.cached_player_obj_ptr.add(0x58) as *const f32),
                        )
                    }
                };
                let ov = replay::input_override();
                // Глобальный InputUnit[0] (0x01AEB850 = base+0x177B850) —
                // реальный источник входа игрока.
                let g = if self.base_addr != 0 {
                    let u = unsafe {
                        ((self.base_addr + replay::GLOBAL_INPUT_UNIT0) as *const replay::InputUnit)
                            .read()
                    };
                    (u.buttons_down, u.buttons_pressed, u.left_stick, u.valid_input)
                } else {
                    (0, 0, [0.0, 0.0], 0)
                };
                // Семантические кнопки Pl0000 + сырые клавиши/мышь —
                // для сопоставления «физическая клавиша → бит в cur_in».
                let pl = self.read_pl_input();
                let mouse_btns = self.read_mouse().buttons;
                let keys_down = self.read_keys().0;
                let space = keys_down[1] & 0x8000_0000 != 0;
                let w_down = keys_down[2] & 0x100 != 0;
                replay::log_line(&format!(
                    "frame: player=0x{:08X} cur_in down={:08X} pressed={:08X} L=({:.2},{:.2}) R=({:.2},{:.2}) dir={:.2} jump={} mouse={:X} space={} w={} pos=({:.2},{:.2},{:.2}) ov_active={} g_unit0: down={:08X} pressed={:08X} L=({:.2},{:.2}) valid={}",
                    self.cached_player_obj_ptr as usize,
                    ci.buttons_down,
                    ci.buttons_pressed,
                    ci.left_stick[0],
                    ci.left_stick[1],
                    ci.right_stick[0],
                    ci.right_stick[1],
                    pl.input_direction,
                    pl.button_jump,
                    mouse_btns,
                    space,
                    w_down,
                    px,
                    py,
                    pz,
                    ov.active,
                    g.0,
                    g.1,
                    g.2[0],
                    g.2[1],
                    g.3
                ));
            }
        }

        // --- BARE PLAYBACK (Этап 1.5): короткая запись по нумпаду, без сегмента ---
        #[cfg(debug_assertions)]
        if self.bare_playback
            && let Some(start) = self.bare_playback_start
        {
            let current_ms = start.elapsed().as_millis() as i64;
            let idx = self
                .bare_playback_frames
                .partition_point(|f| f.duration_ms <= current_ms);
            if idx > 0 {
                let input = self.bare_playback_frames[idx - 1].input;
                replay::set_input_override(replay::InputOverride {
                    active: true,
                    input,
                });
                if idx >= self.bare_playback_frames.len() {
                    // Конец записи — снять override, оставить кадры для повтора.
                    self.stop_bare_playback();
                }
            }
        }

        // Key handlers (NumPad1/2/3) — debug only
        #[cfg(debug_assertions)]
        if !self.cached_player_obj_ptr.is_null() {
            let p = self.cached_player_obj_ptr;
            if ui.is_key_pressed_no_repeat(Key::Keypad1) {
                unsafe {
                    *(p.add(0x54) as *mut f32) += 10.0;
                }
            }
            if ui.is_key_pressed_no_repeat(Key::Keypad2) {
                unsafe {
                    let x = *(p.add(0x50) as *const f32);
                    let y = *(p.add(0x54) as *const f32);
                    let z = *(p.add(0x58) as *const f32);
                    self.saved_position = Some((x, y, z));
                }
            }
            if ui.is_key_pressed_no_repeat(Key::Keypad3)
                && let Some((sx, sy, sz)) = self.saved_position
            {
                unsafe {
                    *(p.add(0x50) as *mut f32) = sx;
                    *(p.add(0x54) as *mut f32) = sy;
                    *(p.add(0x58) as *mut f32) = sz;
                }
            }
            // NumPad4 — скрипт: бег ~1 сек → прыжок на бегу → лёгкий удар
            if ui.is_key_pressed_no_repeat(Key::Keypad4) {
                self.script_frames = 1;
            }
        }

        // NumPad5/6 — короткая запись/воспроизведение ввода (Этап 1.5, debug).
        #[cfg(debug_assertions)]
        {
            if ui.is_key_pressed_no_repeat(Key::Keypad5) {
                self.toggle_bare_record();
            }
            if ui.is_key_pressed_no_repeat(Key::Keypad6) {
                self.toggle_bare_playback();
            }
        }

        // --- ОТРИСОВКА СОХРАНЁННОЙ ПОЗИЦИИ НА ЭКРАНЕ (debug) ---
        #[cfg(debug_assertions)]
        if let (Some((sx, sy, sz)), Some(camera_addr)) = (self.saved_position, self.camera_ptr_addr)
        {
            overlay::draw_world_pos(
                ui,
                (sx, sy, sz),
                camera_addr.as_ptr(),
                self.viewport,
                0xFF_00_FF_00,
                "Saved",
            );
        }

        // --- ОТРИСОВКА ПРИЗРАКА ЛУЧШЕГО СЕГМЕНТА ---
        if self.settings.show_best_ghost
            && self.active_segment.is_some()
            && !self.ghost_positions.is_empty()
            && !self.ghost_label.is_empty()
            && let (Some(camera_addr), Some(seg)) =
                (self.camera_ptr_addr, self.active_segment.as_ref())
            {
                let current_ms = seg.start_instant.elapsed().as_millis() as i64;
                let idx = self
                    .ghost_positions
                    .partition_point(|&(_, dur)| dur <= current_ms);
                if idx > 0 {
                    let (gp, _) = self.ghost_positions[idx - 1];
                    overlay::draw_world_pos(
                        ui,
                        (gp.x, gp.y, gp.z),
                        camera_addr.as_ptr(),
                        self.viewport,
                        0xFF0000FF,
                        &self.ghost_label,
                    );
                }
            }

        // --- ОТРИСОВКА ЧУЖИХ ИГРОКОВ (2D маркеры) ---
        if let (Some(nc), Some(camera_addr), Some(seg)) = (
            &self.net_client,
            self.camera_ptr_addr,
            self.active_segment.as_ref(),
        ) {
            for rp in &nc.remote_players {
                if rp.mission_id != seg.mission_id {
                    continue;
                }
                if rp.last_update.elapsed() > std::time::Duration::from_secs(5) {
                    continue;
                }
                let label = if rp.is_mock {
                    format!("{} [mock]", rp.name)
                } else {
                    format!("{} ({}HP)", rp.name, rp.hp)
                };
                overlay::draw_world_pos(
                    ui,
                    (rp.pos.x, rp.pos.y, rp.pos.z),
                    camera_addr.as_ptr(),
                    self.viewport,
                    0xFF8080FF,
                    &label,
                );
            }
        }

        #[cfg(debug_assertions)]
        ui::render_main_window(ui, self, &ui_state);

        ui::render_multiplayer_window(ui, self);

        ui::render_settings_window(ui, self);
    }
}

hudhook::hudhook!(hudhook::hooks::dx9::ImguiDx9Hooks, HelloHud::new());
