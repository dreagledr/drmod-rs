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
    pub(crate) ghost_positions: Vec<(segment::Vec3, i64)>,
    pub(crate) ghost_label: String,
    pub(crate) settings: settings::Settings,
    // 3D test dummy
    dummy: CylinderRenderer,
    remote_sphere: SphereRenderer,
    pub(crate) cached_player_obj_ptr: *mut u8,
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
            ghost_positions: Vec::new(),
            ghost_label: String::new(),
            settings: settings::Settings::default(),
            dummy: CylinderRenderer::new(24, 0xFFFFFFFF), // white → colour via TFACTOR
            remote_sphere: SphereRenderer::new(16, 8, 0xFFFFFFFF),
            cached_player_obj_ptr: std::ptr::null_mut(),
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
        };

        if self.base_addr != 0 {
            // --- MISSION ---
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

            // --- GAME MENU STATUS ---
            let menu_status_addr = self.base_addr + 0x17E9F9C;
            state.menu_status_raw = unsafe { *(menu_status_addr as *const i32) };
            if (0..=18).contains(&state.menu_status_raw) {
                state.menu_status = unsafe {
                    std::mem::transmute::<i32, game::GameMenuStatus>(state.menu_status_raw)
                };
                state.menu_status_valid = true;
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
        state.segment_action = segment::segment_action(
            state.mission_id,
            &state.mission_name,
            state.position,
            state.menu_status,
            self.active_segment.as_ref(),
        );

        // --- APPLY SEGMENT ACTION ---
        match state.segment_action {
            segment::SegmentAction::Reset => {
                self.position_buffer.clear();
                self.ghost_positions.clear();
                self.ghost_label.clear();
                self.active_segment = None;
            }
            segment::SegmentAction::End => {
                if let (Some(seg), Some(ref conn)) =
                    (self.active_segment.as_ref(), self.db_conn.as_ref())
                {
                    segment::finish_segment(conn, seg, &self.position_buffer);
                }
                self.position_buffer.clear();
                self.ghost_positions.clear();
                self.ghost_label.clear();
                self.active_segment = None;
            }
            segment::SegmentAction::Start => {
                self.position_buffer.clear();
                self.ghost_positions.clear();
                self.ghost_label.clear();

                let fastest_ms = if let Some(ref conn) = self.db_conn {
                    let (ms, positions) = segment::load_best_ghost(conn, state.mission_id);
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
                    mission_id: state.mission_id,
                    mission_name: state.mission_name.clone(),
                    fastest_ms,
                });
            }
            segment::SegmentAction::None => {}
        }

        // --- POSITION BUFFER PUSH ---
        if state.player_found {
            if let (Some(ref seg), Some(pos)) = (self.active_segment.as_ref(), state.position) {
                let dur = seg.start_instant.elapsed().as_millis() as i64;
                self.position_buffer.push((pos, dur));
            }
        }

        self.segment_was_active = self.active_segment.is_some();

        state
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
            && self.active_segment.is_some()
            && !self.ghost_positions.is_empty()
        {
            let current_ms = self
                .active_segment
                .as_ref()
                .unwrap()
                .start_instant
                .elapsed()
                .as_millis() as i64;
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
        if let Some((sx, sy, sz)) = self.saved_position {
            self.dummy.render(
                device,
                (sx, sy, sz),
                0.4,
                2.0,
                0x8000FF00, // green, 50% alpha
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

            if let (Some(pos), Some(ref seg)) = (pos_opt, self.active_segment.as_ref()) {
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

        let ui_state = self.read_game_state();

        // Key handlers (NumPad1/2/3)
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
        }

        // --- ОТРИСОВКА СОХРАНЁННОЙ ПОЗИЦИИ НА ЭКРАНЕ ---
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
        {
            if let (Some(camera_addr), Some(ref seg)) =
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
                        settings::apply_opacity(0x000000FF, self.settings.ghost_opacity),
                        &self.ghost_label,
                    );
                }
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
                    settings::apply_opacity(0x008080FF, self.settings.ghost_opacity),
                    &label,
                );
            }
        }

        ui::render_main_window(ui, self, &ui_state);

        ui::render_multiplayer_window(ui, self);

        ui::render_settings_window(ui, &mut self.settings);
    }
}

hudhook::hudhook!(hudhook::hooks::dx9::ImguiDx9Hooks, HelloHud::new());
