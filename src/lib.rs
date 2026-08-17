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
mod segment;
mod settings;
mod skeleton;
mod tas;
mod ui;

use d3d_render::{CylinderRenderer, SphereRenderer};
use skeleton::BonePos;
use tas::addresses;
use tas::db;
use tas::hooks;
use tas::replay;
use tas::types;

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

    if db::create_replay_tables(&conn).is_err() {
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
    /// Статический адрес `base + 0x17EA100`, хранящий указатель на
    /// PlayerManagerImplement. Сам указатель перечитывается каждый кадр —
    /// при рестарте PlayerManagerImplement пересоздаётся, кэшировать его нельзя.
    pub(crate) player_manager_ptr_addr: Option<NonNull<u8>>,
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
    pub(crate) ghost_positions: Vec<(segment::Vec3, i64)>,
    pub(crate) ghost_label: String,
    pub(crate) settings: settings::Settings,
    // 3D test dummy
    dummy: CylinderRenderer,
    remote_sphere: SphereRenderer,
    pub(crate) cached_player_obj_ptr: *mut u8,
    // Хуки ввода (MinHook) + адреса сырого ввода — живут в tas::hooks.
    #[allow(dead_code)] // keep-alive: поле не читается, но Drop снимает хуки
    input_hooks: tas::hooks::InputHooks,
    // Полное логирование состояния при записи/воспроизведении (NumPad5/6).
    // Буферы копятся в памяти, флашатся в БД по завершении (bulk insert).
    #[cfg(debug_assertions)]
    pub(crate) record_armed: bool,
    #[cfg(debug_assertions)]
    pub(crate) record_active: bool,
    #[cfg(debug_assertions)]
    pub(crate) record_frames: Vec<types::ReplayFrame>,
    #[cfg(debug_assertions)]
    pub(crate) record_start: Option<Instant>,
    #[cfg(debug_assertions)]
    pub(crate) record_started_at: String,
    #[cfg(debug_assertions)]
    pub(crate) record_mission_id: i32,
    #[cfg(debug_assertions)]
    pub(crate) record_mission_name: String,
    #[cfg(debug_assertions)]
    pub(crate) last_record_id: Option<i64>,
    #[cfg(debug_assertions)]
    pub(crate) playback_armed: bool,
    #[cfg(debug_assertions)]
    pub(crate) playback_active: bool,
    #[cfg(debug_assertions)]
    pub(crate) playback_frames: Vec<types::ReplayFrame>,
    #[cfg(debug_assertions)]
    pub(crate) playback_frame_idx: usize,
    #[cfg(debug_assertions)]
    pub(crate) playback_log: Vec<types::ReplayFrame>,
    #[cfg(debug_assertions)]
    pub(crate) playback_start: Option<Instant>,
    #[cfg(debug_assertions)]
    pub(crate) playback_started_at: String,
    #[cfg(debug_assertions)]
    pub(crate) last_playback_id: Option<i64>,
    // Предыдущее состояние «игрок читаем» — для детекта перехода в loading.
    #[cfg(debug_assertions)]
    prev_player_readable: bool,
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

/// Re-entrancy guard для VEH-обработчика. Если исключение случается внутри
/// самого обработчика (например, в `log_line`/`format!` при рестарте), повторный
/// вход не логирует — иначе рекурсия диспетчера исключений → stack overflow.
#[cfg(debug_assertions)]
static IN_VEH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// VEH-обработчик исключений — логирует код и адрес креша в debug.log.
/// Вызывается первым при любом SEH-исключении (access violation и т.п.),
/// возвращает EXCEPTION_CONTINUE_SEARCH, чтобы игра обработала его дальше.
#[cfg(debug_assertions)]
unsafe extern "system" fn veh_handler(
    info: *mut windows::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS,
) -> i32 {
    let record = unsafe { &*(*info).ExceptionRecord };
    let code = record.ExceptionCode.0 as u32;
    // Пропускаем benign-исключения, которые массово летят при загрузке/создании
    // потоков: 0x406D1388 (MSVC SetThreadName) и 0x40010006 (DBG_PRINTEXCEPTION_C).
    // Тяжёлое логирование (chrono + файловый I/O) на свежесозданном потоке, где
    // TLS/CRT ещё не инициализирован, может само упасть и зациклить обработку
    // (рекурсия диспетчера исключений → переполнение стека).
    if code == 0x406D1388 || code == 0x40010006 {
        return windows::Win32::System::Diagnostics::Debug::EXCEPTION_CONTINUE_SEARCH;
    }
    // Повторный вход (исключение внутри log_line/format! при рестарте) — не
    // логируем, чтобы диспетчер исключений не зациклился.
    if IN_VEH.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return windows::Win32::System::Diagnostics::Debug::EXCEPTION_CONTINUE_SEARCH;
    }
    let fault = record.ExceptionAddress as usize;
    let access = if record.NumberParameters >= 2 {
        record.ExceptionInformation[1]
    } else {
        usize::MAX
    };
    replay::log_line(&format!(
        "EXCEPTION: code=0x{:08X} fault=0x{:08X} access=0x{:08X}",
        code, fault, access
    ));
    IN_VEH.store(false, std::sync::atomic::Ordering::SeqCst);
    windows::Win32::System::Diagnostics::Debug::EXCEPTION_CONTINUE_SEARCH
}

/// Проверяет, что адрес указывает на committed и читаемую память.
/// Защита от dangling-указателя объекта игрока при быстром рестарте:
/// `static_ptr` (`base+0x177B4A4`) может указывать на память, освобождённую
/// через `VirtualFree` (не `null`), не проходя через loading-состояние. В этом
/// случае `VirtualQuery` вернёт `Protect = 0` (MEM_FREE/MEM_RESERVE), и чтение
/// по такому адресу даёт ACCESS_VIOLATION.
fn is_readable_ptr(addr: usize) -> bool {
    use windows::Win32::System::Memory::{
        VirtualQuery, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
        PAGE_GUARD, PAGE_PROTECTION_FLAGS, PAGE_READONLY, PAGE_READWRITE,
    };

    const PAGE_READABLE: PAGE_PROTECTION_FLAGS = PAGE_PROTECTION_FLAGS(
        PAGE_READONLY.0 | PAGE_READWRITE.0 | PAGE_EXECUTE_READ.0 | PAGE_EXECUTE_READWRITE.0,
    );

    let mut mbi = MEMORY_BASIC_INFORMATION::default();
    let ok = unsafe {
        VirtualQuery(
            Some(addr as *const core::ffi::c_void),
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    // PAGE_GUARD (0x100) — комбинированный флаг: чтение guard-страницы даёт
    // STATUS_GUARD_PAGE_VIOLATION, хотя бит «readable» в protect может стоять.
    // Исключаем (см. docs/REPLAY_FINDINGS.md №3).
    ok != 0
        && (mbi.Protect & PAGE_READABLE).0 != 0
        && (mbi.Protect & PAGE_GUARD).0 == 0
}

impl HelloHud {
    fn new() -> Self {
        // Логируем SEH-исключения (креши) в debug.log — диагностика.
        #[cfg(debug_assertions)]
        unsafe {
            windows::Win32::System::Diagnostics::Debug::AddVectoredExceptionHandler(
                1,
                Some(veh_handler),
            );
        }

        let (current_run_start, prev_run_start, db_conn) = init_db();

        let base_addr = unsafe {
            windows::Win32::System::LibraryLoader::GetModuleHandleA(windows::core::PCSTR::null())
        }
        .map(|h| h.0 as usize)
        .unwrap_or(0);

        // Детур updateInputUnit использует базовый адрес для прямой записи
        // в сырые структуры ввода (эмуляция клавиши R / ripper).
        let _ = replay::set_base_addr(base_addr);

        let static_ptr_addr = if base_addr == 0 {
            None
        } else {
            NonNull::new(unsafe { (base_addr as *mut u8).add(0x177B4A4) })
        };

        let player_manager_ptr_addr = if base_addr == 0 {
            None
        } else {
            // base + 0x17EA100 — статический адрес, хранящий указатель на
            // PlayerManagerImplement. Указатель читается каждый кадр в
            // read_game_state (PlayerManagerImplement пересоздаётся при рестарте).
            NonNull::new(unsafe { (base_addr as *mut u8).add(0x17EA100) })
        };

        let camera_ptr_addr = if base_addr == 0 {
            None
        } else {
            // base + 0x17EA1D0 — статический адрес cCameraGame::Instance (SDK)
            NonNull::new(unsafe { (base_addr as *mut u8).add(0x17EA1D0) })
        };

        // Хуки ввода (updateInputUnit / isKeybindPressed / isKeybindDown)
        // и адреса сырого ввода — устанавливаются и логируются в tas::hooks.
        let input_hooks = tas::hooks::InputHooks::new(base_addr);

        // Отдельный лог состояния (velocity/rotation/heading/ripper/...),
        // перезатирается при старте мода — см. `replay::init_state_log`.
        replay::init_state_log();

        Self {
            current_run_start,
            prev_run_start,
            db_conn,
            base_addr,
            static_ptr_addr,
            player_manager_ptr_addr,
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
            ghost_positions: Vec::new(),
            ghost_label: String::new(),
            settings: settings::Settings::default(),
            dummy: CylinderRenderer::new(24, 0xFFFFFFFF), // white → colour via TFACTOR
            remote_sphere: SphereRenderer::new(16, 8, 0xFFFFFFFF),
            cached_player_obj_ptr: std::ptr::null_mut(),
            input_hooks,
            #[cfg(debug_assertions)]
            record_armed: false,
            #[cfg(debug_assertions)]
            record_active: false,
            #[cfg(debug_assertions)]
            record_frames: Vec::new(),
            #[cfg(debug_assertions)]
            record_start: None,
            #[cfg(debug_assertions)]
            record_started_at: String::new(),
            #[cfg(debug_assertions)]
            record_mission_id: 0,
            #[cfg(debug_assertions)]
            record_mission_name: String::new(),
            #[cfg(debug_assertions)]
            last_record_id: None,
            #[cfg(debug_assertions)]
            playback_armed: false,
            #[cfg(debug_assertions)]
            playback_active: false,
            #[cfg(debug_assertions)]
            playback_frames: Vec::new(),
            #[cfg(debug_assertions)]
            playback_frame_idx: 0,
            #[cfg(debug_assertions)]
            playback_log: Vec::new(),
            #[cfg(debug_assertions)]
            playback_start: None,
            #[cfg(debug_assertions)]
            playback_started_at: String::new(),
            #[cfg(debug_assertions)]
            last_playback_id: None,
            #[cfg(debug_assertions)]
            prev_player_readable: false,
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

            // Диагностика перехода в/из loading + остановка активной записи/
            // воспроизведения при входе в loading (креш при рестарте).
            #[cfg(debug_assertions)]
            {
                if player_readable != self.prev_player_readable {
                    replay::log_line(&format!(
                        "menu: player_readable {} -> {} (raw={})",
                        self.prev_player_readable, player_readable, state.menu_status_raw
                    ));
                    if !player_readable {
                        self.stop_on_loading();
                    }
                    self.prev_player_readable = player_readable;
                }
            }

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
        }

        // --- Pl0000 / Player ---
        // Читаем объект игрока только когда он «читаем» (не loading): в loading
        // статический указатель может указывать на освобождённую память
        // (dangling, не null) — разыменование даёт access violation при рестарте.
        if let Some(static_ptr) = self.static_ptr_addr {
            state.static_ptr_value = static_ptr.as_ptr() as usize;
            self.cached_player_obj_ptr = if player_readable {
                unsafe { *(static_ptr.as_ptr() as *const *mut u8) }
            } else {
                std::ptr::null_mut()
            };
            // Защита от dangling: при быстром рестарте static_ptr может указывать
            // на освобождённую память (не null), не проходя через loading-состояние.
            // Обнуляем кэш, если страница игрока больше не committed/читаема.
            if !self.cached_player_obj_ptr.is_null()
                && !is_readable_ptr(self.cached_player_obj_ptr as usize)
            {
                self.cached_player_obj_ptr = std::ptr::null_mut();
            }
            if !self.cached_player_obj_ptr.is_null() {
                state.player_found = true;
                // rAnim лежит в самом объекте Pl0000 по смещению 0x618
                // (подтверждено disasm vtable 241: `mov eax,[ecx+0x618]`).
                // Читаем из cached_player_obj_ptr, а не из отдельной
                // кэшированной цепочки указателей — при рестарте цепочка
                // становится dangling и даёт access violation.
                state.r_anim = unsafe { *(self.cached_player_obj_ptr.add(0x618) as *const i32) };
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
        if player_readable
            && let Some(pm_addr) = self.player_manager_ptr_addr
        {
            // Указатель на PlayerManagerImplement перечитываем каждый кадр —
            // он пересоздаётся при рестарте (кэшировать нельзя).
            let pm_ptr = unsafe { *(pm_addr.as_ptr() as *const *mut u8) };
            // При быстром рестарте указатель может стать dangling (не null) —
            // гейтим через is_readable_ptr, как и объект игрока.
            if !pm_ptr.is_null() && is_readable_ptr(pm_ptr as usize) {
                state.main_weapon = unsafe { *(pm_ptr.add(0xE0) as *const i32) };
                state.custom_weapon = unsafe { *(pm_ptr.add(0xE4) as *const i32) };
                state.sub_weapon = unsafe { *(pm_ptr.add(0xE8) as *const i32) };
            }
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

    /// Читает нормализованный ввод игрока (Pl0000::m_CurrentInput).
    pub(crate) fn read_current_input(&self) -> types::InputUnit {
        if self.cached_player_obj_ptr.is_null() {
            return types::InputUnit::default();
        }
        unsafe {
            self.cached_player_obj_ptr
                .add(addresses::CURRENT_INPUT_OFFSET)
                .cast::<types::InputUnit>()
                .read()
        }
    }

    /// Читает глобальный InputUnit[0] (base+0x177B850) — реальный источник
    /// входа игрока (Pl0000::updateInput копирует его в m_CurrentInput).
    pub(crate) fn read_global_input_unit(&self) -> types::InputUnit {
        if self.base_addr == 0 {
            return types::InputUnit::default();
        }
        unsafe {
            ((self.base_addr + addresses::GLOBAL_INPUT_UNIT0) as *const types::InputUnit).read()
        }
    }

    /// Читает полный снимок нормализованного ввода игрока (Pl0000) — InputUnit
    /// по 0xCF8 + m_fInputDirection и m_nButton* по подтверждённым SDK-смещениям.
    pub(crate) fn read_pl_input(&self) -> types::PlInputSnapshot {
        if self.cached_player_obj_ptr.is_null() {
            return types::PlInputSnapshot::default();
        }
        let p = self.cached_player_obj_ptr;
        unsafe {
            types::PlInputSnapshot {
                input: p.add(addresses::CURRENT_INPUT_OFFSET).cast::<types::InputUnit>().read(),
                input_mag_sq: *(p.add(addresses::PL_INPUT_MAG_SQ) as *const f32),
                input_direction: *(p.add(addresses::PL_INPUT_DIR) as *const f32),
                button_jump: *(p.add(addresses::PL_BUTTON_JUMP) as *const i32),
                button_light_attack: *(p.add(addresses::PL_BUTTON_LIGHT_ATTACK) as *const i32),
                button_heavy_attack: *(p.add(addresses::PL_BUTTON_HEAVY_ATTACK) as *const i32),
                button_action: *(p.add(addresses::PL_BUTTON_ACTION) as *const i32),
                button_ninjarun: *(p.add(addresses::PL_BUTTON_NINJARUN) as *const i32),
                button_blademode: *(p.add(addresses::PL_BUTTON_BLADEMODE) as *const i32),
                button_use_item: *(p.add(addresses::PL_BUTTON_USEITEM) as *const i32),
            }
        }
    }

    /// Читает полное состояние персонажа (позиция/поворот/скорость/HP/состояния)
    /// из `cached_player_obj_ptr`. Смещения из SDK — см. `types::PlayerState`.
    /// Новые смещения 0x90/0x890/0x3184/0x40C8 требуют рантайм-верификации.
    pub(crate) fn read_player_state(&self) -> Option<types::PlayerState> {
        if self.cached_player_obj_ptr.is_null() {
            return None;
        }
        let p = self.cached_player_obj_ptr;
        Some(unsafe {
            types::PlayerState {
                pos: [
                    *(p.add(0x50) as *const f32),
                    *(p.add(0x54) as *const f32),
                    *(p.add(0x58) as *const f32),
                ],
                rotation: [
                    *(p.add(0x90) as *const f32),
                    *(p.add(0x94) as *const f32),
                    *(p.add(0x98) as *const f32),
                ],
                velocity: [
                    *(p.add(0x890) as *const f32),
                    *(p.add(0x894) as *const f32),
                    *(p.add(0x898) as *const f32),
                ],
                hp: *(p.add(0x870) as *const i32),
                r_anim: *(p.add(0x618) as *const i32),
                sword_state: *(p.add(0x13FC) as *const i32),
                sword_hidden: *(p.add(0xB74) as *const i32),
                input_direction: *(p.add(0xD2C) as *const f32),
                desired_heading: *(p.add(0xD30) as *const f32),
                button_jump: *(p.add(0xE18) as *const i32),
                button_light_attack: *(p.add(0xE20) as *const i32),
                button_heavy_attack: *(p.add(0xE24) as *const i32),
                button_ninjarun: *(p.add(0xE48) as *const i32),
                button_blademode: *(p.add(0xE50) as *const i32),
                ripper_enabled: *(p.add(0x3184) as *const i32),
                blade_mode_type: *(p.add(0x40C8) as *const i32),
            }
        })
    }

    /// Читает состояние камеры: позиция (+0x1B0) и view-proj матрица (+0x200).
    pub(crate) fn read_camera_state(&self) -> Option<types::CameraState> {
        let addr = self.camera_ptr_addr?.as_ptr();
        Some(unsafe {
            types::CameraState {
                pos: [
                    *(addr.add(0x1B0) as *const f32),
                    *(addr.add(0x1B4) as *const f32),
                    *(addr.add(0x1B8) as *const f32),
                ],
                view_proj: *(addr.add(0x200) as *const [f32; 16]),
            }
        })
    }

    /// Этап 1 (debug): инжекция ввода через override хука updateInputUnit.
    /// Все действия пишутся в глобальный InputUnit[0] — реальный источник
    /// входа игрока (прямая запись в поля Pl0000 в Present не работает:
    /// поздно — после handleActions). Биты — см. `addresses::input_bits`.
    #[cfg(debug_assertions)]
    pub(crate) fn update_input_injection(&mut self) {
        // Во время воспроизведения override управляется исключительно playback —
        // debug-инъекция не должна затирать применяемый кадр.
        if self.playback_active {
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

        let mut unit = types::InputUnit {
            valid_input: 1,
            ..Default::default()
        };

        if script_active {
            let t = self.script_frames;
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
            self.script_frames += 1;
            if self.script_frames > SCRIPT_TOTAL {
                self.script_frames = 0;
            }
        }

        if self.inject_w {
            unit.buttons_down |= addresses::input_bits::FORWARD;
            unit.left_stick = [0.0, -1000.0];
        }
        if jump_active {
            unit.buttons_down |= addresses::input_bits::JUMP;
            unit.buttons_pressed |= addresses::input_bits::JUMP;
            self.inject_jump_frames -= 1;
        }
        if light_active {
            unit.buttons_down |= addresses::input_bits::LIGHT_ATTACK;
            unit.buttons_pressed |= addresses::input_bits::LIGHT_ATTACK;
            self.inject_light_frames -= 1;
        }
        if heavy_active {
            unit.buttons_down |= addresses::input_bits::HEAVY_ATTACK;
            unit.buttons_pressed |= addresses::input_bits::HEAVY_ATTACK;
            self.inject_heavy_frames -= 1;
        }
        if self.inject_camera {
            unit.right_stick = [500.0, 0.0];
        }
        replay::set_input_override(types::InputOverride {
            active,
            input: unit,
        });
    }

    /// Переключает запись по NumPad5 (отложенный старт).
    /// `active → стоп + flush`, `armed → отмена`, `idle → arm` (старт по триггеру).
    #[cfg(debug_assertions)]
    pub(crate) fn toggle_record(&mut self) {
        if self.record_active {
            self.stop_record();
        } else if self.record_armed {
            self.record_armed = false;
        } else {
            self.stop_playback();
            self.playback_armed = false;
            self.record_armed = true;
        }
    }

    /// Переключает воспроизведение по NumPad6 (отложенный старт).
    /// `active → стоп + flush`, `armed → отмена`, `idle → arm` (требует кадры).
    #[cfg(debug_assertions)]
    pub(crate) fn toggle_playback(&mut self) {
        if self.playback_active {
            self.stop_playback();
        } else if self.playback_armed {
            self.playback_armed = false;
        } else if !self.playback_frames.is_empty() {
            self.stop_record();
            self.record_armed = false;
            replay::set_input_override(types::InputOverride::default());
            // Снять остатки ручной keybind-эмуляции (NumPad7/8) до старта.
            replay::clear_keybind_emulation();
            self.playback_armed = true;
        }
    }

    /// Останавливает запись и флашит буфер в БД (kind=record). Кадры
    /// сохраняются в `playback_frames`, чтобы запись можно было воспроизвести.
    #[cfg(debug_assertions)]
    fn stop_record(&mut self) {
        if !self.record_active {
            return;
        }
        self.record_active = false;
        let frames = std::mem::take(&mut self.record_frames);
        let duration_ms = self
            .record_start
            .map(|i| i.elapsed().as_millis() as i64)
            .unwrap_or(0);
        self.record_start = None;
        let meta = types::ReplayRunMeta {
            kind: "record",
            mission_id: self.record_mission_id,
            mission_name: self.record_mission_name.clone(),
            started_at: self.record_started_at.clone(),
            duration_ms,
            source_replay_id: None,
        };
        self.last_record_id = self
            .db_conn
            .as_ref()
            .and_then(|conn| db::flush_replay(conn, &meta, &frames));
        self.playback_frames = frames;
        replay::log_line(&format!(
            "record: stop frames={} id={:?}",
            self.playback_frames.len(),
            self.last_record_id
        ));
    }

    /// Останавливает воспроизведение, снимает override и флашит лог результата
    /// в БД (kind=playback, source_replay_id=id исходной записи).
    #[cfg(debug_assertions)]
    fn stop_playback(&mut self) {
        replay::set_input_override(types::InputOverride::default());
        // Снять keybind-эмуляцию (ripper/blade): иначе удержание blade
        // останется активным и будет подмешиваться в реальный ввод.
        replay::clear_keybind_emulation();
        let was_active = self.playback_active;
        self.playback_active = false;
        self.playback_frame_idx = 0;
        if was_active && !self.playback_log.is_empty() {
            let log = std::mem::take(&mut self.playback_log);
            let duration_ms = self
                .playback_start
                .map(|i| i.elapsed().as_millis() as i64)
                .unwrap_or(0);
            let meta = types::ReplayRunMeta {
                kind: "playback",
                mission_id: self.record_mission_id,
                mission_name: self.record_mission_name.clone(),
                started_at: self.playback_started_at.clone(),
                duration_ms,
                source_replay_id: self.last_record_id,
            };
            self.last_playback_id = self
                .db_conn
                .as_ref()
                .and_then(|conn| db::flush_replay(conn, &meta, &log));
            replay::log_line(&format!(
                "playback: stop frames={} id={:?}",
                log.len(),
                self.last_playback_id
            ));
        }
        self.playback_start = None;
    }

    /// Отложенный старт: если arm и игрок в триггере — запускает запись или
    /// воспроизведение. Вызывается каждый кадр из `render()`.
    #[cfg(debug_assertions)]
    pub(crate) fn update_deferred_start(
        &mut self,
        pos: Option<segment::Vec3>,
        mission_id: i32,
        mission_name: &str,
    ) {
        if !in_bare_trigger(pos) {
            return;
        }
        if self.record_armed {
            self.record_armed = false;
            replay::log_line("deferred: trigger -> start recording");
            // Запись ловит только реальный ввод — сбрасываем keybind-эмуляцию,
            // чтобы остатки ручного NumPad7/8 не подмешались в кадры.
            replay::clear_keybind_emulation();
            self.record_active = true;
            self.record_frames.clear();
            self.record_start = Some(Instant::now());
            self.record_started_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            self.record_mission_id = mission_id;
            self.record_mission_name = mission_name.to_string();
        }
        if self.playback_armed {
            self.playback_armed = false;
            replay::log_line("deferred: trigger -> start playback");
            replay::set_input_override(types::InputOverride::default());
            replay::clear_keybind_emulation();
            self.playback_active = true;
            self.playback_frame_idx = 0;
            self.playback_log.clear();
            self.playback_start = Some(Instant::now());
            self.playback_started_at = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        }
    }

    /// Останавливает активную запись/воспроизведение при входе в loading.
    /// Arm НЕ снимается — он должен пережить loading и сработать на спавне.
    #[cfg(debug_assertions)]
    fn stop_on_loading(&mut self) {
        if self.record_active {
            replay::log_line("loading: stop active recording");
            self.stop_record();
        }
        if self.playback_active {
            replay::log_line("loading: stop active playback");
            self.stop_playback();
        }
        // Сбрасываем keybind-эмуляцию (ripper/blade): иначе при рестарте
        // детуры isKeybindPressed/isKeybindDown возвращают 1 на пересоздающемся
        // игроке → handleActions падает (access violation).
        replay::clear_keybind_emulation();
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
                        ((self.base_addr + addresses::GLOBAL_INPUT_UNIT0) as *const types::InputUnit)
                            .read()
                    };
                    (u.buttons_down, u.buttons_pressed, u.left_stick, u.valid_input)
                } else {
                    (0, 0, [0.0, 0.0], 0)
                };
                // Семантические кнопки Pl0000 + сырые клавиши/мышь —
                // для сопоставления «физическая клавиша → бит в cur_in».
                let pl = self.read_pl_input();
                let mouse_btns = hooks::read_mouse().map(|m| m.buttons).unwrap_or(0);
                let keys_down = hooks::read_keys().map(|(d, _)| d).unwrap_or([0; 6]);
                let space = keys_down[1] & 0x8000_0000 != 0;
                let w_down = keys_down[2] & 0x100 != 0;
                replay::log_line(&format!(
                    "frame: player=0x{:08X} status_raw={} cur_in down={:08X} pressed={:08X} L=({:.2},{:.2}) R=({:.2},{:.2}) dir={:.2} jump={} mouse={:X} space={} w={} pos=({:.2},{:.2},{:.2}) ov_active={} g_unit0: down={:08X} pressed={:08X} L=({:.2},{:.2}) valid={}",
                    self.cached_player_obj_ptr as usize,
                    ui_state.menu_status_raw,
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

        // --- STATE LOG (отдельный файл, перезатирается при старте) ---
        // Логирует полное состояние каждый кадр — для сопоставления с действиями
        // игрока при верификации смещений (velocity/rotation/ripper/blade/...).
        // Только когда игрок читаем (не в loading/меню) — иначе рискуем читать
        // освобождённую память при рестарте.
        #[cfg(debug_assertions)]
        if ui_state.player_found {
            let ps = self.read_player_state();
            let cs = self.read_camera_state();
            let (pos, vel, rot, heading, dir, ripper, blade, ninja, jump) = match ps {
                Some(p) => (
                    p.pos,
                    p.velocity,
                    p.rotation,
                    p.desired_heading,
                    p.input_direction,
                    p.ripper_enabled,
                    p.blade_mode_type,
                    p.button_ninjarun,
                    p.button_jump,
                ),
                None => ([0.0; 3], [0.0; 3], [0.0; 3], 0.0, 0.0, 0, 0, 0, 0),
            };
            let cam = cs.map(|c| c.pos).unwrap_or([0.0; 3]);
            // field_900 — предыдущая позиция (лаг ~1 кадр): дельта pos-prev
            // даёт скорость перемещения за кадр (горизонтальное движение
            // кинематическое — отдельного поля горизонтальной скорости нет).
            let prev = if self.cached_player_obj_ptr.is_null() {
                [0.0; 3]
            } else {
                let p = self.cached_player_obj_ptr;
                unsafe {
                    [
                        *(p.add(0x900) as *const f32),
                        *(p.add(0x904) as *const f32),
                        *(p.add(0x908) as *const f32),
                    ]
                }
            };
            replay::log_state_line(&format!(
                "f={} pos=({:.3},{:.3},{:.3}) vel=({:.3},{:.3},{:.3}) prev=({:.3},{:.3},{:.3}) rot=({:.3},{:.3},{:.3}) heading={:.3} dir={:.3} ripper={} blade={} ninja={} jump={} cam=({:.1},{:.1},{:.1})",
                self.d3d_frame_count,
                pos[0], pos[1], pos[2],
                vel[0], vel[1], vel[2],
                prev[0], prev[1], prev[2],
                rot[0], rot[1], rot[2],
                heading, dir, ripper, blade, ninja, jump,
                cam[0], cam[1], cam[2]
            ));
        }

        // --- ОТЛОЖЕННЫЙ СТАРТ (arm → триггер позиции) ---
        // Запускается запись/воспроизведение, когда игрок попал в триггерную
        // зону спавна. Перед блоком захвата/применения — чтобы кадр 0 записывался
        // в том же render, где сработал триггер.
        #[cfg(debug_assertions)]
        self.update_deferred_start(ui_state.position, ui_state.mission_id, &ui_state.mission_name);

        // --- RECORD CAPTURE: полное состояние на кадр ---
        #[cfg(debug_assertions)]
        if self.record_active {
            self.record_frames.push(types::ReplayFrame {
                frame_index: self.record_frames.len() as u32,
                input: self.read_current_input(),
                state: self.read_player_state().unwrap_or_default(),
                camera: self.read_camera_state().unwrap_or_default(),
            });
        }

        // --- PLAYBACK: подача кадра по индексу + захват результата ---
        // Кадры подаются строго по индексу (1 кадр на вызов render), а не по dt —
        // dt-сопоставление теряло однокадровые фронты pressed/released.
        // NOTE: override, выставленный здесь, применяется игрой на СЛЕДУЮЩЕМ тике,
        // поэтому playback_log[N] = результат кадра frame[N-1] (сдвиг на 1 кадр
        // относительно record[N]); playback_log[0] — состояние спавна до подачи.
        #[cfg(debug_assertions)]
        if self.playback_active {
            if self.playback_frame_idx < self.playback_frames.len() {
                let frame = self.playback_frames[self.playback_frame_idx];

                // Ripper/blade не идут через InputUnit — handleActions читает их
                // из DirectInput напрямую через isKeybindPressed(11)/isKeybindDown(8).
                // Подаём их через keybind-эмуляцию, выводя фронт/удержание из
                // записанного состояния: ripper — перепад ripper_enabled,
                // blade — blade_mode_type != 0 (hold). Задание в render(K)
                // применяется на тике K+1, т.е. синхронно с override InputUnit.
                let prev_ripper = if self.playback_frame_idx > 0 {
                    self.playback_frames[self.playback_frame_idx - 1].state.ripper_enabled
                } else {
                    0
                };
                if frame.state.ripper_enabled != prev_ripper {
                    replay::set_ripper_frames(1);
                    replay::log_line(&format!(
                        "playback: ripper edge {} -> {} at frame {}",
                        prev_ripper,
                        frame.state.ripper_enabled,
                        self.playback_frame_idx
                    ));
                }
                let blade_on = frame.state.blade_mode_type != 0;
                if blade_on != replay::blade_hold() {
                    replay::set_blade_hold(blade_on);
                    replay::log_line(&format!(
                        "playback: blade hold {} at frame {}",
                        if blade_on { "ON" } else { "OFF" },
                        self.playback_frame_idx
                    ));
                }

                replay::set_input_override(types::InputOverride {
                    active: true,
                    input: frame.input,
                });
                self.playback_frame_idx += 1;
                self.playback_log.push(types::ReplayFrame {
                    frame_index: self.playback_log.len() as u32,
                    input: self.read_current_input(),
                    state: self.read_player_state().unwrap_or_default(),
                    camera: self.read_camera_state().unwrap_or_default(),
                });
            } else {
                // Конец записи — снять override и флашнуть лог воспроизведения.
                self.stop_playback();
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

        // NumPad5/6 — запись/воспроизведение с полным логированием состояния (debug).
        #[cfg(debug_assertions)]
        {
            if ui.is_key_pressed_no_repeat(Key::Keypad5) {
                self.toggle_record();
            }
            if ui.is_key_pressed_no_repeat(Key::Keypad6) {
                self.toggle_playback();
            }
            // NumPad7 — эмуляция клавиши R (ripper) через хук isKeybindPressed:
            // handleActions видит "R нажата" и запускает штатную активацию.
            if ui.is_key_pressed_no_repeat(Key::Keypad7) {
                replay::set_ripper_frames(1);
                replay::log_line("NumPad7: emulate R (ripper) 1 frame via isKeybindPressed");
            }
            // NumPad8 — toggle удержания blade mode через isKeybindDown (hold).
            if ui.is_key_pressed_no_repeat(Key::Keypad8) {
                let on = !replay::blade_hold();
                replay::set_blade_hold(on);
                replay::log_line(&format!(
                    "NumPad8: blade_hold {}",
                    if on { "ON" } else { "OFF" }
                ));
            }
            // NumPad9 / NumPad0 — прямой вызов enableRipperMode()/disableRipperMode()
            // (обход ввода: игра читает DirectInput GetDeviceState напрямую).
            if !self.cached_player_obj_ptr.is_null() {
                if ui.is_key_pressed_no_repeat(Key::Keypad9) {
                    replay::enable_ripper(self.cached_player_obj_ptr);
                    replay::log_line("NumPad9: enableRipperMode()");
                }
                if ui.is_key_pressed_no_repeat(Key::Keypad0) {
                    replay::disable_ripper(self.cached_player_obj_ptr);
                    replay::log_line("NumPad0: disableRipperMode()");
                }
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
