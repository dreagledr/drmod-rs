use chrono::Local;
use hudhook::{IDirect3DDevice9, ImguiRenderLoop, RenderContext};
use imgui::*;
use rusqlite::Connection;
use std::ptr::NonNull;
use std::time::Instant;

mod d3d_render;
mod game;
mod net;
pub mod protocol;
mod segment;

use d3d_render::CylinderRenderer;

pub const DEFAULT_TITLE: &str = "METAL GEAR RISING REVENGEANCE.exe";

/// Проецирует мировую позицию на экран через view-projection матрицу (D3DXMATRIX, row-major).
/// Возвращает `(screen_pos, distance_to_camera)` или `None` если точка за камерой.
fn world_to_screen(
    world_pos: (f32, f32, f32),
    view_proj: &[f32; 16],
    screen_size: [f32; 2],
    camera_pos: (f32, f32, f32),
) -> Option<([f32; 2], f32)> {
    let (wx, wy, wz) = world_pos;
    let (cx, cy, cz) = camera_pos;

    // Умножение row-vector (x,y,z,1) на матрицу 4x4 (row-major layout)
    let clip_x = wx * view_proj[0] + wy * view_proj[4] + wz * view_proj[8] + view_proj[12];
    let clip_y = wx * view_proj[1] + wy * view_proj[5] + wz * view_proj[9] + view_proj[13];
    let clip_w = wx * view_proj[3] + wy * view_proj[7] + wz * view_proj[11] + view_proj[15];

    if clip_w <= 0.0 {
        return None;
    }

    let inv_w = 1.0 / clip_w;
    let ndc_x = clip_x * inv_w;
    let ndc_y = clip_y * inv_w;

    let screen_x = (ndc_x * 0.5 + 0.5) * screen_size[0];
    let screen_y = (1.0 - (ndc_y * 0.5 + 0.5)) * screen_size[1];

    let dx = wx - cx;
    let dy = wy - cy;
    let dz = wz - cz;
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();

    Some(([screen_x, screen_y], dist))
}

fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let millis = ms % 1000;
    format!("{:02}:{:02}.{:03}", mins, secs, millis)
}

fn draw_world_pos(
    ui: &Ui,
    world_pos: (f32, f32, f32),
    camera_ptr: *const u8,
    color: u32,
    label: &str,
) {
    let view_proj = unsafe { *(camera_ptr.add(0x200) as *const [f32; 16]) };
    let cam_x = unsafe { *(camera_ptr.add(0x1B0) as *const f32) };
    let cam_y = unsafe { *(camera_ptr.add(0x1B4) as *const f32) };
    let cam_z = unsafe { *(camera_ptr.add(0x1B8) as *const f32) };
    let [sw, sh] = ui.io().display_size;

    if let Some(([scr_x, scr_y], dist)) =
        world_to_screen(world_pos, &view_proj, [sw, sh], (cam_x, cam_y, cam_z))
    {
        let on_screen = scr_x >= 0.0 && scr_x <= sw && scr_y >= 0.0 && scr_y <= sh;

        // Screen-space radius for a 0.5m world-space offset
        let (wx, wy, wz) = world_pos;
        let radius = if let Some(([rx, _], _)) = world_to_screen(
            (wx + 0.5, wy, wz),
            &view_proj,
            [sw, sh],
            (cam_x, cam_y, cam_z),
        ) {
            (rx - scr_x).abs().clamp(2.0, 64.0)
        } else {
            8.0
        };

        let (draw_x, draw_y, text_offset_x) = if on_screen {
            (scr_x, scr_y, radius + 4.0)
        } else {
            (
                scr_x.clamp(24.0, sw - 24.0),
                scr_y.clamp(24.0, sh - 24.0),
                radius + 4.0,
            )
        };

        let draw_list = ui.get_foreground_draw_list();

        if on_screen {
            // Beam: vertical line ground → +2m
            if let Some(([head_x, head_y], _)) = world_to_screen(
                (wx, wy + 2.0, wz),
                &view_proj,
                [sw, sh],
                (cam_x, cam_y, cam_z),
            ) {
                draw_list
                    .add_line([scr_x, scr_y], [head_x, head_y], color)
                    .thickness(1.5)
                    .build();
            }

            draw_list
                .add_circle([draw_x, draw_y], radius, color)
                .thickness(2.0)
                .build();
            draw_list.add_text(
                [draw_x + text_offset_x, draw_y - 8.0],
                color,
                format!("{} ({:.1}m)", label, dist),
            );
        } else {
            draw_list
                .add_circle([draw_x, draw_y], 8.0, color)
                .thickness(2.5)
                .build();
            draw_list.add_text(
                [draw_x + 6.0, draw_y - 8.0],
                color,
                format!("\u{25c6} {} ({:.1}m)", label, dist),
            );
        }
    }
}

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
    current_run_start: String,
    prev_run_start: Option<String>,
    db_conn: Option<Connection>,
    base_addr: usize,
    static_ptr_addr: Option<NonNull<u8>>,
    player_manager_addr: Option<NonNull<u8>>,
    camera_ptr_addr: Option<NonNull<u8>>,
    saved_position: Option<(f32, f32, f32)>,
    // Segment tracking
    active_segment: Option<segment::ActiveSegment>,
    segment_was_active: bool,
    position_buffer: Vec<(segment::Vec3, i64)>,
    ghost_positions: Vec<(segment::Vec3, i64)>,
    ghost_label: String,
    // 3D test dummy
    dummy: CylinderRenderer,
    d3d_frame_count: u32,
    d3d_last_error: String,
    // Multiplayer
    net_client: Option<net::NetClient>,
    player_name: String,
    room_name: String,
    server_addr: String,
    last_sent_pos: Option<segment::Vec3>,
    last_sent_mission_id: i32,
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
            active_segment: None,
            segment_was_active: false,
            position_buffer: Vec::new(),
            ghost_positions: Vec::new(),
            ghost_label: String::new(),
            dummy: CylinderRenderer::new(24, 0xFFFFFFFF), // white → colour via TFACTOR
            d3d_frame_count: 0,
            d3d_last_error: String::new(),
            net_client: None,
            player_name: "Raiden".to_string(),
            room_name: "default".to_string(),
            server_addr: "127.0.0.1:5222".to_string(),
            last_sent_pos: None,
            last_sent_mission_id: 0,
        }
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

        // Read camera view*proj matrix (needed for all draws)
        let camera_ptr = match self.camera_ptr_addr {
            Some(addr) => addr.as_ptr(),
            None => return,
        };
        let view_proj = unsafe { *(camera_ptr.add(0x200) as *const [f32; 16]) };

        // ── Ghost cylinder (red, semi-transparent) ──────────────────
        if self.active_segment.is_some() && !self.ghost_positions.is_empty() {
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
                    0x800000FF, // red, 50% alpha
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
        if let (Some(nc), Some(seg)) =
            (&self.net_client, self.active_segment.as_ref())
        {
            for rp in &nc.remote_players {
                if rp.mission_id != seg.mission_id {
                    continue;
                }
                if rp.last_update.elapsed() > std::time::Duration::from_secs(5) {
                    continue;
                }
                self.dummy.render(
                    device,
                    (rp.pos.x, rp.pos.y, rp.pos.z),
                    0.4,
                    2.0,
                    0x8000FFFF, // blue, 50% alpha
                    &view_proj,
                );
            }
        }
    }

    fn render(&mut self, ui: &mut Ui) {
        ui.window("##hello")
            .size([320., 600.], Condition::Always)
            .build(|| {
                if let Some(ref seg) = self.active_segment {
                    let elapsed_ms = seg.start_instant.elapsed().as_millis() as u64;
                    ui.text(format!("Segment: {}", format_duration_ms(elapsed_ms)));
                    if let Some(best_ms) = seg.fastest_ms {
                        ui.text(format!("Best:    {}", format_duration_ms(best_ms as u64)));
                    } else {
                        ui.text_colored([0.5, 0.5, 0.5, 1.0], "Best:    N/A");
                    }
                } else {
                    ui.text_colored([0.5, 0.5, 0.5, 1.0], "No active segment");
                }
                ui.text(format!("Current run:  {}", self.current_run_start));
                if let Some(ref prev) = self.prev_run_start {
                    ui.text(format!("Previous run: {}", prev));
                } else {
                    ui.text_colored([0.5, 0.5, 0.5, 1.0], "Previous run: N/A");
                }

                // --- MISSION ---
                let mut mission_id: i32 = 0;
                let mission_id_raw: i32;
                let mut mission_name_str = String::new();
                if self.base_addr != 0 {
                    let mission_id_addr = self.base_addr + 0x1764670;
                    mission_id_raw = unsafe { *(mission_id_addr as *const i32) };
                    let (name_addr, eff_id) = if mission_id_raw != 0 {
                        (self.base_addr + 0x1764674, mission_id_raw)
                    } else {
                        (self.base_addr + 0x1766008, unsafe {
                            *((self.base_addr + 0x1766004) as *const i32)
                        })
                    };
                    mission_id = eff_id;
                    mission_name_str = unsafe { std::ffi::CStr::from_ptr(name_addr as *const i8) }
                        .to_string_lossy()
                        .into_owned();
                    ui.text(format!(
                        "Mission: {} (0x{:04X}) [raw: 0x{:04X}]",
                        mission_name_str, mission_id, mission_id_raw
                    ));
                }

                // --- GAME MENU STATUS ---
                let mut menu_status = game::GameMenuStatus::None;
                if self.base_addr != 0 {
                    let menu_status_addr = self.base_addr + 0x17E9F9C;
                    let raw_status = unsafe { *(menu_status_addr as *const i32) };
                    if (0..=18).contains(&raw_status) {
                        // SAFETY: GameMenuStatus is repr(i32) с значениями 0..=18
                        let status =
                            unsafe { std::mem::transmute::<i32, game::GameMenuStatus>(raw_status) };
                        let color = if status.is_in_game() {
                            [0.0, 1.0, 0.0, 1.0]
                        } else {
                            [1.0, 1.0, 0.0, 1.0]
                        };
                        ui.text_colored(color, format!("Status: {}", status.name()));
                        menu_status = status;
                    } else {
                        ui.text_colored(
                            [1.0, 0.5, 0.0, 1.0],
                            format!("Status: Unknown ({})", raw_status),
                        );
                    }
                }

                // --- ДЕБАГ ПОЛЕЙ Pl0000 ---
                if let Some(static_ptr) = self.static_ptr_addr {
                    unsafe {
                        let pl0000_ptr = *(static_ptr.as_ptr() as *const *mut u8);
                        if !pl0000_ptr.is_null() {
                            // m_SwordState (+0x13FC) — int
                            let sword_state = *(pl0000_ptr.add(0x13FC) as *const i32);
                            // m_bSwordHidden (+0xB74) — int (bool)
                            let sword_hidden = *(pl0000_ptr.add(0xB74) as *const i32);

                            ui.separator();
                            ui.text("Pl0000 fields:");
                            ui.text(format!("SwordState: {}", sword_state));
                            ui.text(format!("SwordHidden: {}", sword_hidden));
                        }
                    }
                }

                let Some(static_ptr_addr) = self.static_ptr_addr else {
                    ui.text_colored([1.0, 0.0, 0.0, 1.0], "Module not found!");
                    return;
                };

                // --- WEAPONS ---
                if let Some(pm_addr) = self.player_manager_addr {
                    ui.separator();
                    ui.text("Weapons:");

                    let pm_ptr = pm_addr.as_ptr();
                    let main_weapon = unsafe { *(pm_ptr.add(0xE0) as *const i32) };
                    let custom_weapon = unsafe { *(pm_ptr.add(0xE4) as *const i32) };
                    let sub_weapon = unsafe { *(pm_ptr.add(0xE8) as *const i32) };

                    ui.text(format!("Main: {}", main_weapon));
                    ui.text(format!(
                        "Custom: {} ({})",
                        custom_weapon,
                        game::custom_weapon_name(custom_weapon)
                    ));
                    ui.text(format!("Sub: {}", sub_weapon));
                }

                ui.text(format!(
                    "Static Ptr Addr: 0x{:08X}",
                    static_ptr_addr.as_ptr() as usize
                ));

                // --- ЧИТАЕМ КООРДИНАТЫ ИЗ ТАБЛИЦЫ CE ---

                // 1. Адрес статического указателя: Base + 0x177B4A4
                let static_ptr_addr = static_ptr_addr.as_ptr();

                // Безопасное чтение указателя на объект игрока
                let player_obj_ptr = unsafe {
                    // Приводим адрес к типу "указатель на указатель"
                    let ptr_to_player_ptr = static_ptr_addr as *const *mut u8;

                    // Проверяем, что сам адрес указателя валиден
                    if ptr_to_player_ptr.is_null() {
                        std::ptr::null_mut()
                    } else {
                        // Разыменовываем, чтобы получить указатель на объект игрока
                        *ptr_to_player_ptr
                    }
                };

                // Читаем позицию (или None, если игрока нет)
                let pos_opt = if !player_obj_ptr.is_null() {
                    let pos_x = unsafe { *(player_obj_ptr.add(0x50) as *const f32) };
                    let pos_y = unsafe { *(player_obj_ptr.add(0x54) as *const f32) };
                    let pos_z = unsafe { *(player_obj_ptr.add(0x58) as *const f32) };
                    Some(segment::Vec3 {
                        x: pos_x,
                        y: pos_y,
                        z: pos_z,
                    })
                } else {
                    None
                };

                // --- SEGMENT ACTION ---
                let action = segment::segment_action(
                    mission_id,
                    &mission_name_str,
                    pos_opt,
                    menu_status,
                    self.active_segment.as_ref(),
                );

                // --- SEGMENT DEBUG ---
                if self.active_segment.is_some() {
                    let action_name = match action {
                        segment::SegmentAction::None => "None",
                        segment::SegmentAction::Start => "Start",
                        segment::SegmentAction::End => "End",
                        segment::SegmentAction::Reset => "Reset",
                    };
                    ui.text_colored(
                        [0.5, 0.8, 1.0, 1.0],
                        format!(
                            "SEGDEBUG: seg.mission={} cur.mission={} status={} ({}) => {}",
                            self.active_segment.as_ref().unwrap().mission_id,
                            mission_id,
                            menu_status.name(),
                            menu_status as i32,
                            action_name,
                        ),
                    );
                }

                match action {
                    segment::SegmentAction::Reset => {
                        self.position_buffer.clear();
                        self.ghost_positions.clear();
                        self.ghost_label.clear();
                        self.active_segment = None;
                    }
                    segment::SegmentAction::End => {
                        if let (Some(ref seg), Some(ref conn)) =
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
                            let (ms, positions) = segment::load_best_ghost(conn, mission_id);
                            if let Some(best_ms) = ms {
                                self.ghost_label =
                                    format!("Best {}", format_duration_ms(best_ms as u64));
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
                            mission_name: mission_name_str.clone(),
                            fastest_ms,
                        });
                    }
                    segment::SegmentAction::None => {}
                }

                let mut hp: i32 = 0;

                if player_obj_ptr.is_null() {
                    ui.text_colored([1.0, 0.5, 0.0, 1.0], "Player object pointer is NULL");
                    ui.text("Убедитесь, что вы в игре (не в меню).");
                } else {
                    ui.text_colored([0.0, 1.0, 0.0, 1.0], "Player found!");

                    // NumPad1: +3m к высоте
                    if ui.is_key_pressed_no_repeat(Key::Keypad1) {
                        unsafe {
                            let y_ptr: *mut f32 = player_obj_ptr.add(0x54) as *mut f32;
                            *y_ptr += 10.0;
                        }
                    }

                    // NumPad2: сохранение позиции
                    if ui.is_key_pressed_no_repeat(Key::Keypad2) {
                        unsafe {
                            let x = *(player_obj_ptr.add(0x50) as *const f32);
                            let y = *(player_obj_ptr.add(0x54) as *const f32);
                            let z = *(player_obj_ptr.add(0x58) as *const f32);
                            self.saved_position = Some((x, y, z));
                        }
                    }

                    // NumPad3: телепорт на сохранённую позицию
                    if ui.is_key_pressed_no_repeat(Key::Keypad3) {
                        if let Some((sx, sy, sz)) = self.saved_position {
                            unsafe {
                                *(player_obj_ptr.add(0x50) as *mut f32) = sx;
                                *(player_obj_ptr.add(0x54) as *mut f32) = sy;
                                *(player_obj_ptr.add(0x58) as *mut f32) = sz;
                            }
                        }
                    }

                    // Position buffer push
                    if self.active_segment.is_some() {
                        if let (Some(ref seg), Some(pos)) = (self.active_segment.as_ref(), pos_opt)
                        {
                            let dur = seg.start_instant.elapsed().as_millis() as i64;
                            self.position_buffer.push((pos, dur));
                        }
                    }

                    // Дополнительно: HP (offset 0x870)
                    hp = unsafe { *(player_obj_ptr.add(0x870) as *const i32) };
                    ui.separator();
                    ui.text(format!("HP: {}", hp));
                    ui.text("NumPad1: +10m Y");
                    ui.text("NumPad2: Save position");
                    ui.text("NumPad3: Teleport");
                }

                // ── Multiplayer network ─────────────────────────────
                // Обработка TCP-событий
                if let Some(ref mut nc) = self.net_client {
                    nc.poll_tcp();

                    // Отправка своей позиции
                    if let (Some(pos), Some(ref seg)) = (pos_opt, self.active_segment.as_ref()) {
                        let changed = self.last_sent_pos != Some(pos)
                            || self.last_sent_mission_id != seg.mission_id;
                        if changed {
                            nc.send_position(pos, hp, seg.mission_id);
                            self.last_sent_pos = Some(pos);
                            self.last_sent_mission_id = seg.mission_id;
                        }
                    }

                    // Приём чужих позиций
                    nc.recv_positions();
                }
                // ─────────────────────────────────────────────────────

                ui.separator();
                ui.text("Position:");
                if let Some(pos) = pos_opt {
                    ui.text(format!("X: {:.3}", pos.x));
                    ui.text(format!("Y: {:.3}", pos.y));
                    ui.text(format!("Z: {:.3}", pos.z));
                } else {
                    ui.text("X: Null");
                    ui.text("Y: Null");
                    ui.text("Z: Null");
                }

                // Сохранённая позиция
                ui.separator();
                ui.text("Saved Position:");
                if let Some((sx, sy, sz)) = self.saved_position {
                    ui.text(format!("X: {:.3}", sx));
                    ui.text(format!("Y: {:.3}", sy));
                    ui.text(format!("Z: {:.3}", sz));

                    // --- ДЕБАГ: проекция на экран ---
                    ui.separator();
                    ui.text("Screen projection debug:");
                    match self.camera_ptr_addr {
                        None => {
                            ui.text_colored([1.0, 0.5, 0.0, 1.0], "camera_ptr_addr is None");
                        }
                        Some(cam_addr) => {
                            // cam_addr.as_ptr() УЖЕ указывает на объект cCameraGame
                            // (SDK: *(cCameraGame*)(base + 0x17EA1D0))
                            let cam_ptr = cam_addr.as_ptr();

                            let view_proj = unsafe { *(cam_ptr.add(0x200) as *const [f32; 16]) };
                            let cam_x = unsafe { *(cam_ptr.add(0x1B0) as *const f32) };
                            let cam_y = unsafe { *(cam_ptr.add(0x1B4) as *const f32) };
                            let cam_z = unsafe { *(cam_ptr.add(0x1B8) as *const f32) };
                            let screen_size = ui.io().display_size;

                            ui.text(format!("Camera ptr: 0x{:08X}", cam_ptr as usize));
                            ui.text(format!("Cam pos: {:.1} {:.1} {:.1}", cam_x, cam_y, cam_z));
                            ui.text(format!(
                                "Screen: {:.0}x{:.0}",
                                screen_size[0], screen_size[1]
                            ));
                            ui.text(format!(
                                "VP[0..4]: {:.3} {:.3} {:.3} {:.3}",
                                view_proj[0], view_proj[1], view_proj[2], view_proj[3]
                            ));
                            ui.text(format!(
                                "VP[4..8]: {:.3} {:.3} {:.3} {:.3}",
                                view_proj[4], view_proj[5], view_proj[6], view_proj[7]
                            ));

                            match world_to_screen(
                                (sx, sy, sz),
                                &view_proj,
                                screen_size,
                                (cam_x, cam_y, cam_z),
                            ) {
                                Some(([scr_x, scr_y], dist)) => {
                                    let on_scr = scr_x >= 0.0
                                        && scr_x <= screen_size[0]
                                        && scr_y >= 0.0
                                        && scr_y <= screen_size[1];
                                    let color = if on_scr {
                                        [0.0, 1.0, 0.0, 1.0]
                                    } else {
                                        [1.0, 0.65, 0.0, 1.0]
                                    };
                                    ui.text_colored(
                                        color,
                                        format!(
                                            "Screen: {:.0} {:.0}  Dist: {:.1}m  {}",
                                            scr_x,
                                            scr_y,
                                            dist,
                                            if on_scr { "ON" } else { "OFF" }
                                        ),
                                    );
                                }
                                None => {
                                    ui.text_colored([1.0, 0.3, 0.3, 1.0], "Behind camera (w <= 0)");
                                }
                            }
                        }
                    }
                } else {
                    ui.text_colored([0.5, 0.5, 0.5, 1.0], "не сохранена");
                }

                self.segment_was_active = self.active_segment.is_some();

                // --- D3D DEBUG ---
                ui.separator();
                ui.text_colored(
                    [0.5, 1.0, 0.5, 1.0],
                    format!("D3D frames: {}", self.d3d_frame_count),
                );
                if !self.d3d_last_error.is_empty() {
                    ui.text_colored(
                        [1.0, 0.5, 0.0, 1.0],
                        format!("D3D error: {}", self.d3d_last_error),
                    );
                }

                // --- ВЫХОД ---
                ui.separator();
                if ui.button("Выход / Выгрузить DLL") {
                    // hudhook::eject() корректно снимает хуки и выгружает DLL,
                    // не убивая окно игры
                    hudhook::eject();
                }
            });

        // --- ОТРИСОВКА СОХРАНЁННОЙ ПОЗИЦИИ НА ЭКРАНЕ ---
        if let (Some((sx, sy, sz)), Some(camera_addr)) = (self.saved_position, self.camera_ptr_addr)
        {
            draw_world_pos(
                ui,
                (sx, sy, sz),
                camera_addr.as_ptr(),
                0xFF_00_FF_00,
                "Saved",
            );
        }

        // --- ОТРИСОВКА ПРИЗРАКА ЛУЧШЕГО СЕГМЕНТА ---
        if self.active_segment.is_some()
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
                    draw_world_pos(
                        ui,
                        (gp.x, gp.y, gp.z),
                        camera_addr.as_ptr(),
                        0xFF_00_00_FF,
                        &self.ghost_label,
                    );
                }
            }
        }

        // ── Multiplayer UI ─────────────────────────────────────────
        ui.window("Multiplayer")
            .size([300.0, 250.0], Condition::FirstUseEver)
            .build(|| {
                ui.input_text("Server", &mut self.server_addr)
                    .hint("127.0.0.1:5222")
                    .build();
                ui.input_text("Name", &mut self.player_name).build();
                ui.input_text("Room", &mut self.room_name).build();

                if self.net_client.is_none() {
                    if ui.button("Connect") {
                        match net::NetClient::new(
                            &self.server_addr,
                            &self.player_name,
                            &self.room_name,
                        ) {
                            Ok(nc) => {
                                self.net_client = Some(nc);
                            }
                            Err(e) => {
                                // silently fail — user sees status unchanged
                                let _ = e;
                            }
                        }
                    }
                } else {
                    if ui.button("Disconnect") {
                        self.net_client = None;
                        self.last_sent_pos = None;
                    }
                }

                if let Some(nc) = &self.net_client {
                    ui.separator();
                    if nc.my_id != 0 {
                        ui.text(format!("Status: Connected (ID: {})", nc.my_id));
                    } else {
                        ui.text_colored([1.0, 1.0, 0.0, 1.0], "Status: Waiting for ID...");
                    }

                    ui.separator();
                    ui.text("Players:");
                    let my_mission = self
                        .active_segment
                        .as_ref()
                        .map(|s| s.mission_name.as_str())
                        .unwrap_or("-");
                    ui.text(format!("{} (you) - {}", self.player_name, my_mission));

                    for rp in &nc.remote_players {
                        let mission = format!("0x{:04X}", rp.mission_id);
                        let active =
                            if self.active_segment.as_ref().map_or(false, |s| {
                                s.mission_id == rp.mission_id
                            }) {
                                " [active]"
                            } else {
                                ""
                            };
                        let mock_tag = if rp.is_mock { " [mock]" } else { "" };
                        let age = rp.last_update.elapsed().as_secs();
                        let stale = if age > 5 { " (stale)" } else { "" };
                        ui.text(format!(
                            "{}{} - {} HP:{}{}{}",
                            rp.name, mock_tag, mission, rp.hp, active, stale
                        ));
                    }
                }
            });
    }
}

hudhook::hudhook!(hudhook::hooks::dx9::ImguiDx9Hooks, HelloHud::new());
