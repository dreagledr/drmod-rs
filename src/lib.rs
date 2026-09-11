use chrono::Local;
use hudhook::{IDirect3DDevice9, ImguiRenderLoop, RenderContext};
use imgui::*;
use rusqlite::Connection;
use std::time::Instant;

mod api;
mod d3d_render;
mod game;
mod logger;
mod net;
mod overlay;
mod segment;
mod settings;
mod skeleton;
mod tas;
mod ui;

use d3d_render::{CylinderRenderer, SphereRenderer};
use skeleton::BonePos;
#[cfg(debug_assertions)]
use tas::addresses;
use tas::db;
#[cfg(debug_assertions)]
use tas::hooks;
#[cfg(debug_assertions)]
use tas::replay;
#[cfg(debug_assertions)]
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
    pub(crate) player: game::Player,
    pub(crate) camera: game::Camera,
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
    // Хуки ввода (MinHook) + адреса сырого ввода — живут в tas::hooks.
    #[allow(dead_code)] // keep-alive: поле не читается, но Drop снимает хуки
    input_hooks: tas::hooks::InputHooks,
    // Состояние Record/Replay + ручной инжекции ввода (debug) — в tas::replay.
    #[cfg(debug_assertions)]
    pub(crate) replay: replay::ReplayState,
    // Предыдущее состояние «игрок читаем» — для детекта перехода в loading.
    #[cfg(debug_assertions)]
    prev_player_readable: bool,
    pub(crate) d3d_frame_count: u32,
    // Multiplayer
    pub(crate) net_client: Option<net::NetClient>,
    pub(crate) player_name: String,
    pub(crate) room_name: String,
    pub(crate) server_addr: String,
    pub(crate) last_sent_pos: Option<segment::Vec3>,
    last_sent_mission_id: i32,
    last_skeleton_send: Instant,
    pub(crate) viewport: [f32; 4], // [X, Y, Width, Height] from D3D GetViewport
    // HTTP API автоматизации (скрипты ввода, состояние, кольцевой буфер логов).
    pub(crate) api: api::ApiServer,
}

/// Re-entrancy guard для VEH-обработчика. Если исключение случается внутри
/// самого обработчика (например, в `logger::log_line`/`format!` при рестарте), повторный
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
    // Повторный вход (исключение внутри logger::log_line/format! при рестарте) — не
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
    logger::log_line(&format!(
        "EXCEPTION: code=0x{:08X} fault=0x{:08X} access=0x{:08X}",
        code, fault, access
    ));
    IN_VEH.store(false, std::sync::atomic::Ordering::SeqCst);
    windows::Win32::System::Diagnostics::Debug::EXCEPTION_CONTINUE_SEARCH
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

        // Сущности игры: игрок (Pl0000) и камера (cCameraGame) — инкапсулируют
        // свои статические адреса и кэш указателей, наружу отдают read_* методы.
        let player = game::Player::new(base_addr);
        let camera = game::Camera::new(base_addr);

        // Хуки ввода (updateInputUnit / isKeybindPressed / isKeybindDown)
        // и адреса сырого ввода — устанавливаются и логируются в tas::hooks.
        let input_hooks = tas::hooks::InputHooks::new(base_addr);
        tas::replay::set_base_addr(base_addr);

        // Отдельный лог состояния (velocity/rotation/heading/ripper/...),
        // перезатирается при старте мода — см. `logger::init_state_log`.
        logger::init_state_log();

        // HTTP API автоматизации — работает в debug и release.
        let api = api::ApiServer::new(base_addr);

        Self {
            current_run_start,
            prev_run_start,
            db_conn,
            base_addr,
            player,
            camera,
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
            input_hooks,
            #[cfg(debug_assertions)]
            replay: replay::ReplayState::default(),
            #[cfg(debug_assertions)]
            prev_player_readable: false,
            d3d_frame_count: 0,
            net_client: None,
            player_name: "Raiden".to_string(),
            room_name: "default".to_string(),
            server_addr: "127.0.0.1:5222".to_string(),
            last_sent_pos: None,
            last_sent_mission_id: 0,
            last_skeleton_send: Instant::now(),
            viewport: [0.0; 4],
            api,
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
            position: None,
            player_found: false,
        };

        // Строки миссии (gStr) и анимация (rAnim) в loading уничтожаются —
        // их нельзя читать, пока статус не станет валидным и не-загрузочным.
        let mut player_readable = false;
        let mut gstr = String::new();
        let mut gstr2 = String::new();
        let mut gstr4 = String::new();

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
                    logger::log_line(&format!(
                        "menu: player_readable {} -> {} (raw={})",
                        self.prev_player_readable, player_readable, state.menu_status_raw
                    ));
                    if !player_readable {
                        self.replay.stop_on_loading(self.db_conn.as_ref());
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
                gstr = unsafe {
                    std::ffi::CStr::from_ptr((self.base_addr + 0x14B9181) as *const i8)
                }
                .to_string_lossy()
                .into_owned();
                gstr2 = unsafe {
                    std::ffi::CStr::from_ptr((self.base_addr + 0x14B91AD) as *const i8)
                }
                .to_string_lossy()
                .into_owned();
                gstr4 = unsafe {
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
        // Кэш объекта обновляет и защищает от dangling сама сущность Player.
        self.player.refresh(player_readable);
        let r_anim = self.player.r_anim();
        if self.player.is_found() {
            state.player_found = true;
            state.position = self.player.position();
        }

        // --- SEGMENT ACTION ---
        let segment_action = if player_readable {
            let action = segment::segment_action(
                state.mission_id,
                &state.mission_name,
                state.position,
                state.menu_status,
                &gstr,
                &self.prev_gstr,
                &gstr2,
                &self.prev_gstr2,
                r_anim,
                self.prev_r_anim,
                self.active_segment.as_ref(),
            );

            // Update prev_ values for next frame
            self.prev_gstr = gstr.clone();
            self.prev_gstr2 = gstr2.clone();
            self.prev_gstr4 = gstr4.clone();
            self.prev_r_anim = r_anim;
            action
        } else {
            // В загрузке gStr/rAnim заглушены — не двигаем prev_* и не даём
            // ложных finish-переходов. Выход в меню по-прежнему сбрасывает сегмент.
            if state.menu_status == game::GameMenuStatus::MainMenuLoad
                && self.active_segment.is_some()
            {
                segment::SegmentAction::Reset
            } else {
                segment::SegmentAction::None
            }
        };

        // --- APPLY SEGMENT ACTION ---
        match segment_action {
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

    /// Выгрузка DLL: отключение сети, остановка HTTP-потока (снятие override
    /// ввода), затем флаг eject для hudhook (обрабатывается в render-цикле
    /// после Present). Единая точка для кнопки «Выход» и `POST /eject`.
    fn eject(&mut self) {
        self.net_client = None;
        self.api.shutdown();
        hudhook::eject();
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
        let view_proj = match self.camera.view_proj() {
            Some(vp) => vp,
            None => return,
        };

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
        // Сначала собираем состояние игры: read_game_state обновляет кэш игрока
        // (в loading игра обнуляет static_ptr → кэш = null), иначе диагностика
        // ниже читает stale-указатель освобождённого игрока.
        let ui_state = self.read_game_state();

        // ── Multiplayer network (выполняется каждый кадр, независимо от UI) ──
        if let Some(ref mut nc) = self.net_client {
            nc.poll_tcp();

            // Читаем позицию для отправки
            let pos_opt = self.player.position();

            if let (Some(pos), Some(seg)) = (pos_opt, self.active_segment.as_ref()) {
                let changed =
                    self.last_sent_pos != Some(pos) || self.last_sent_mission_id != seg.mission_id;
                if changed {
                    let hp = self.player.read_player_state().map(|s| s.hp).unwrap_or(0);
                    nc.send_position(pos, hp, seg.mission_id);
                    self.last_sent_pos = Some(pos);
                    self.last_sent_mission_id = seg.mission_id;
                }
            }

            nc.recv_udp();

            // Отправляем скелет раз в ~100 мс
            if self.last_skeleton_send.elapsed() > std::time::Duration::from_millis(30) {
                self.last_skeleton_send = Instant::now();
                let bones = self.player.read_skeleton();
                if !bones.is_empty() {
                    let wire = skeleton::bones_to_wire(&bones);
                    nc.send_skeleton(&wire);
                }
            }
        }
        // ─────────────────────────────────────────────────────────────

        // Диагностика: дамп m_CurrentInput игрока, позиции и g_unit0.
        // Логируется при КАЖДОМ изменении кнопок (down/pressed) — чтобы
        // поймать однократные фронты (прыжок/атаки), плюс heartbeat каждые
        // 120 кадров (чтобы видеть движение и стики даже без кнопок).
        #[cfg(debug_assertions)]
        {
            let ci = self.player.read_current_input();
            let changed = replay::cur_in_changed(ci.buttons_down, ci.buttons_pressed);
            if changed || self.d3d_frame_count.is_multiple_of(120) {
                let (px, py, pz) = self
                    .player
                    .position()
                    .map(|p| (p.x, p.y, p.z))
                    .unwrap_or((0.0, 0.0, 0.0));
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
                let pl = self.player.read_pl_input();
                let mouse_btns = hooks::read_mouse().map(|m| m.buttons).unwrap_or(0);
                let keys_down = hooks::read_keys().map(|(d, _)| d).unwrap_or([0; 6]);
                let space = keys_down[1] & 0x8000_0000 != 0;
                let w_down = keys_down[2] & 0x100 != 0;
                logger::log_line(&format!(
                    "frame: player=0x{:08X} status_raw={} cur_in down={:08X} pressed={:08X} L=({:.2},{:.2}) R=({:.2},{:.2}) dir={:.2} jump={} mouse={:X} space={} w={} pos=({:.2},{:.2},{:.2}) ov_active={} g_unit0: down={:08X} pressed={:08X} L=({:.2},{:.2}) valid={}",
                    self.player.obj_ptr(),
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
                // Диагностика меню: изменения сырого состояния клавиатуры.
                hooks::log_key_state_changes();
            }
        }

        // --- STATE LOG (отдельный файл, перезатирается при старте) ---
        // Логирует полное состояние каждый кадр — для сопоставления с действиями
        // игрока при верификации смещений (velocity/rotation/ripper/blade/...).
        // Только когда игрок читаем (не в loading/меню) — иначе рискуем читать
        // освобождённую память при рестарте.
        #[cfg(debug_assertions)]
        if ui_state.player_found {
            let ps = self.player.read_player_state();
            let cs = self.camera.read_camera_state();
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
            let prev = self.player.prev_position().unwrap_or([0.0; 3]);
            logger::log_state_line(&format!(
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

        // --- API: продвижение скрипта + кольцевой буфер + снимок (debug+release) ---
        // Чтение ввода/состояния — общий код: нужно и API, и record/replay.
        let input = self.player.read_current_input();
        let state = self.player.read_player_state().unwrap_or_default();
        let camera = self.camera.read_camera_state().unwrap_or_default();
        // Ближайший враг нужен и API-логам (подброс даёт атака врага), и
        // записи/воспроизведению. Обход EntitySystem недёшев и опасен, пока
        // сцена пересоздаётся, поэтому для скрипта читаем только в геймплее
        // (`In Game`): фазы рестарта/меню/loading сюда не попадают. У записи
        // гейт прежний — активная запись и не loading.
        #[cfg(debug_assertions)]
        let replay_wants = self.replay.is_active() && !ui_state.menu_status.is_loading();
        #[cfg(not(debug_assertions))]
        let replay_wants = false;
        let script_wants = self.api.is_script_running()
            && ui_state.menu_status_valid
            && ui_state.menu_status.is_in_game();
        let enemy = if (replay_wants || script_wants)
            && ui_state.menu_status_valid
            && !ui_state.menu_status.is_loading()
        {
            self.player.read_nearest_enemy()
        } else {
            types::EnemyState::default()
        };

        self.api
            .frame_update(&ui_state, input, state, camera, enemy);

        // --- RECORD/REPLAY: единый покадровый апдейт (debug) ---
        // Инжекция → отложенный старт (arm → триггер позиции) → захват кадра
        // записи → подача кадра воспроизведения. Кадр (input/state/camera)
        // читается один раз и используется и для записи, и для лога
        // воспроизведения. Не выполняется, пока активен API-скрипт — скрипт
        // эксклюзивно владеет override ввода.
        #[cfg(debug_assertions)]
        if !self.api.is_script_active() {
            self.replay.update(
                self.db_conn.as_ref(),
                ui_state.position,
                ui_state.mission_id,
                &ui_state.mission_name,
                input,
                state,
                camera,
                enemy,
            );
        }

        // Key handlers (NumPad1/2/3) — debug only
        #[cfg(debug_assertions)]
        if self.player.is_found() {
            if ui.is_key_pressed_no_repeat(Key::Keypad1) {
                self.player.add_y(10.0);
            }
            if ui.is_key_pressed_no_repeat(Key::Keypad2)
                && let Some(pos) = self.player.position()
            {
                self.saved_position = Some((pos.x, pos.y, pos.z));
            }
            if ui.is_key_pressed_no_repeat(Key::Keypad3)
                && let Some((sx, sy, sz)) = self.saved_position
            {
                self.player.set_position((sx, sy, sz));
            }
            // NumPad4 — встроенный скрипт (бег → прыжок → удар → камера) через
            // общий ScriptRunner API — тот же механизм, что POST /script/run.
            if ui.is_key_pressed_no_repeat(Key::Keypad4) {
                self.api.start_builtin_script();
            }
        }

        // NumPad5/6 — запись/воспроизведение с полным логированием состояния (debug).
        #[cfg(debug_assertions)]
        {
            if ui.is_key_pressed_no_repeat(Key::Keypad5) {
                self.replay.toggle_record(self.db_conn.as_ref());
            }
            if ui.is_key_pressed_no_repeat(Key::Keypad6) {
                self.replay.toggle_playback(self.db_conn.as_ref());
            }
            // NumPad7 — эмуляция клавиши R (ripper) через хук isKeybindPressed:
            // handleActions видит "R нажата" и запускает штатную активацию.
            if ui.is_key_pressed_no_repeat(Key::Keypad7) {
                hooks::set_ripper_frames(1);
                logger::log_line("NumPad7: emulate R (ripper) 1 frame via isKeybindPressed");
            }
            // NumPad8 — toggle удержания blade mode через isKeybindDown (hold).
            if ui.is_key_pressed_no_repeat(Key::Keypad8) {
                let on = !hooks::blade_hold();
                hooks::set_blade_hold(on);
                logger::log_line(&format!(
                    "NumPad8: blade_hold {}",
                    if on { "ON" } else { "OFF" }
                ));
            }
        }

        // --- ОТРИСОВКА СОХРАНЁННОЙ ПОЗИЦИИ НА ЭКРАНЕ (debug) ---
        #[cfg(debug_assertions)]
        if let (Some((sx, sy, sz)), Some(vp), Some(cam_pos)) =
            (self.saved_position, self.camera.view_proj(), self.camera.pos())
        {
            overlay::draw_world_pos(
                ui,
                (sx, sy, sz),
                &vp,
                (cam_pos[0], cam_pos[1], cam_pos[2]),
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
            && let (Some(vp), Some(cam_pos), Some(seg)) = (
                self.camera.view_proj(),
                self.camera.pos(),
                self.active_segment.as_ref(),
            )
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
                    &vp,
                    (cam_pos[0], cam_pos[1], cam_pos[2]),
                    self.viewport,
                    0xFF0000FF,
                    &self.ghost_label,
                );
            }
        }

        // --- ОТРИСОВКА ЧУЖИХ ИГРОКОВ (2D маркеры) ---
        if let (Some(nc), Some(vp), Some(cam_pos), Some(seg)) = (
            &self.net_client,
            self.camera.view_proj(),
            self.camera.pos(),
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
                    &vp,
                    (cam_pos[0], cam_pos[1], cam_pos[2]),
                    self.viewport,
                    0xFF8080FF,
                    &label,
                );
            }
        }

        #[cfg(debug_assertions)]
        ui::render_main_window(ui, self, &ui_state);

        #[cfg(debug_assertions)]
        ui::render_actions_window(ui);

        ui::render_multiplayer_window(ui, self);

        ui::render_settings_window(ui, self);

        // --- EJECT через API: POST /eject ставит флаг в SharedState, здесь
        // (в render-цикле, как и кнопка «Выход») выполняем саму выгрузку. ---
        if self.api.eject_requested() {
            self.eject();
        }
    }
}

hudhook::hudhook!(hudhook::hooks::dx9::ImguiDx9Hooks, HelloHud::new());
