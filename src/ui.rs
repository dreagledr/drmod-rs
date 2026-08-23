use crate::game;
use crate::net;
#[cfg(debug_assertions)]
use crate::overlay;
use crate::segment;
use crate::HelloHud;
use imgui::*;

/// Данные для отрисовки основного окна, собранные до вызова UI.
pub struct UiState {
    pub mission_id: i32,
    pub mission_id_raw: i32,
    pub mission_name: String,
    pub menu_status_raw: i32,
    pub menu_status_valid: bool,
    pub menu_status: game::GameMenuStatus,
    pub position: Option<segment::Vec3>,
    pub player_found: bool,
}

/// Враг (сущность Em*/Ba*/Pl001*) из EntitySystem для debug-панели: позиция,
/// HP, анимация, дистанция до игрока, высота клинка (мировая, из матрицы части).
#[cfg(debug_assertions)]
pub(super) struct EnemyInfo {
    pub name: String,
    pub pos: [f32; 3],
    pub hp: i32,
    pub r_anim: i32,
    pub dist: Option<f32>,
    pub blade_y: Option<f32>,
}

#[cfg(debug_assertions)]
pub fn render_main_window(ui: &Ui, hud: &mut HelloHud, state: &UiState) {
    ui.window("DrmodDebug")
        .size([380., 720.], Condition::Always)
        .scroll_bar(true)
        .build(|| {
            // --- ЗАПИСЬ (Record/Replay) ---
            ui.text(format!(
                "record: armed={} active={} frames={} id={:?}",
                hud.replay.record.armed,
                hud.replay.record.active,
                hud.replay.record.frames.len(),
                hud.replay.record.last_id
            ));
            ui.text(format!(
                "playback: armed={} active={} frame={}/{} log={} id={:?}",
                hud.replay.playback.armed,
                hud.replay.playback.active,
                hud.replay.playback.frame_idx,
                hud.replay.playback.frames.len(),
                hud.replay.playback.log.len(),
                hud.replay.playback.last_id
            ));

            // --- СЕГМЕНТ-ТАЙМЕР ---
            ui.separator();
            if let Some(ref seg) = hud.active_segment {
                let elapsed_ms = seg.start_instant.elapsed().as_millis() as u64;
                ui.text(format!(
                    "Segment: {}",
                    overlay::format_duration_ms(elapsed_ms)
                ));
                if let Some(best_ms) = seg.fastest_ms {
                    ui.text(format!(
                        "Best:    {}",
                        overlay::format_duration_ms(best_ms as u64)
                    ));
                } else {
                    ui.text_colored([0.5, 0.5, 0.5, 1.0], "Best:    N/A");
                }
            } else {
                ui.text_colored([0.5, 0.5, 0.5, 1.0], "No active segment");
            }
            ui.text(format!("Current run:  {}", hud.current_run_start));
            if let Some(ref prev) = hud.prev_run_start {
                ui.text(format!("Previous run: {}", prev));
            } else {
                ui.text_colored([0.5, 0.5, 0.5, 1.0], "Previous run: N/A");
            }

            // --- МИССИЯ + СТАТУС МЕНЮ ---
            ui.separator();
            if state.mission_id != 0 || !state.mission_name.is_empty() {
                ui.text(format!(
                    "Mission: {} (0x{:04X}) [raw: 0x{:04X}]",
                    state.mission_name, state.mission_id, state.mission_id_raw
                ));
            }
            if state.menu_status_valid {
                let color = if state.menu_status.is_in_game() {
                    [0.0, 1.0, 0.0, 1.0]
                } else {
                    [1.0, 1.0, 0.0, 1.0]
                };
                ui.text_colored(color, format!("Status: {}", state.menu_status.name()));
            } else {
                ui.text_colored(
                    [1.0, 0.5, 0.0, 1.0],
                    format!("Status: Unknown ({})", state.menu_status_raw),
                );
            }

            if hud.base_addr == 0 {
                ui.text_colored([1.0, 0.0, 0.0, 1.0], "Module not found!");
                return;
            }

            // --- СОСТОЯНИЕ ИГРОКА (компактно) ---
            ui.separator();
            if let Some(ps) = hud.read_player_state() {
                ui.text(format!(
                    "Pos: ({:.2}, {:.2}, {:.2})",
                    ps.pos[0], ps.pos[1], ps.pos[2]
                ));
                ui.text(format!(
                    "Rot: ({:.2}, {:.2}, {:.2})  Heading: {:.2}  Dir: {:.2}",
                    ps.rotation[0],
                    ps.rotation[1],
                    ps.rotation[2],
                    ps.desired_heading,
                    ps.input_direction
                ));
                ui.text(format!(
                    "Vel: ({:.2}, {:.2}, {:.2})  rAnim: {}",
                    ps.velocity[0], ps.velocity[1], ps.velocity[2], ps.r_anim
                ));
            } else {
                ui.text_colored([0.5, 0.5, 0.5, 1.0], "player state: N/A");
            }

            if !state.player_found {
                ui.text_colored([1.0, 0.5, 0.0, 1.0], "Player object pointer is NULL");
                ui.text("Убедитесь, что вы в игре (не в меню).");
            } else {
                ui.text_colored([0.0, 1.0, 0.0, 1.0], "Player found!");
            }

            // --- ВРАГИ (позиции в бою) ---
            ui.separator();
            if state.menu_status_valid && !state.menu_status.is_loading() {
                let (total, enemies) = hud.read_enemies();
                ui.text(format!("Entities: {}  Enemies: {}", total, enemies.len()));
                for e in &enemies {
                    let d = e
                        .dist
                        .map(|d| format!("{:.1}m", d))
                        .unwrap_or_else(|| "?".into());
                    let blade = e
                        .blade_y
                        .map(|y| format!(" bladeY:{:.2}", y))
                        .unwrap_or_default();
                    ui.text(format!(
                        "{} ({:.1}, {:.1}, {:.1}) HP:{} rAnim:{} {}{}",
                        e.name, e.pos[0], e.pos[1], e.pos[2], e.hp, e.r_anim, d, blade
                    ));
                }
            } else {
                ui.text_colored([0.5, 0.5, 0.5, 1.0], "enemies: N/A (loading/menu)");
            }

            // --- ВЫХОД ---
            ui.separator();
            if ui.button("Выход / Выгрузить DLL") {
                hud.api.request_eject();
            }
        });
}

/// Справка по numpad-хоткеям (debug). Сами хоткеи обрабатываются в lib.rs.
#[cfg(debug_assertions)]
pub fn render_actions_window(ui: &Ui) {
    ui.window("Actions")
        .size([300.0, 260.0], Condition::FirstUseEver)
        .position([10.0, 300.0], Condition::FirstUseEver)
        .build(|| {
            ui.text("NumPad1: +10m Y");
            ui.text("NumPad2: Save position");
            ui.text("NumPad3: Teleport");
            ui.text("NumPad4: бег→прыжок→удар→поворот камеры");
            ui.text("NumPad5: запись (arm→триггер)");
            ui.text("NumPad6: воспроизведение (arm→триггер)");
            ui.text("NumPad7: эмуляция R (ripper), 1 кадр");
            ui.text("NumPad8: blade mode (hold) toggle");
        });
}

pub fn render_multiplayer_window(ui: &Ui, hud: &mut HelloHud) {
    ui.window("Multiplayer")
        .size([300.0, 250.0], Condition::FirstUseEver)
        .position([10.0, 30.0], Condition::FirstUseEver)
        .build(|| {
            ui.input_text("Server", &mut hud.server_addr)
                .hint("127.0.0.1:5222")
                .build();
            ui.input_text("Name", &mut hud.player_name).build();
            ui.input_text("Room", &mut hud.room_name).build();

            if hud.net_client.is_none() {
                if ui.button("Connect")
                    && let Ok(nc) = net::NetClient::new(
                        &hud.server_addr,
                        &hud.player_name,
                        &hud.room_name,
                    )
                {
                    hud.net_client = Some(nc);
                }
            } else {
                if ui.button("Disconnect") {
                    hud.net_client = None;
                    hud.last_sent_pos = None;
                }
            }

            if let Some(nc) = &hud.net_client {
                ui.separator();
                if nc.my_id != 0 {
                    ui.text(format!("Status: Connected (ID: {})", nc.my_id));
                } else {
                    ui.text_colored([1.0, 1.0, 0.0, 1.0], "Status: Waiting for ID...");
                }

                ui.separator();
                ui.text("Players:");
                let my_mission = hud
                    .active_segment
                    .as_ref()
                    .map(|s| s.mission_name.as_str())
                    .unwrap_or("-");
                ui.text(format!("{} (you) - {}", hud.player_name, my_mission));

                for rp in &nc.remote_players {
                    let mission = format!("0x{:04X}", rp.mission_id);
                    let active = if hud.active_segment.as_ref().is_some_and(|s| {
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

pub fn render_settings_window(ui: &Ui, hud: &mut HelloHud) {
    let settings = &mut hud.settings;
    ui.window("Settings")
        .size([250.0, 150.0], Condition::FirstUseEver)
        .position([320.0, 30.0], Condition::FirstUseEver)
        .build(|| {
            ui.checkbox("Show best ghost", &mut settings.show_best_ghost);
            ui.slider("Ghost opacity", 0.0f32, 1.0f32, &mut settings.ghost_opacity);

            ui.separator();
            if ui.button("Выход / Выгрузить DLL") {
                hud.api.request_eject();
            }
        });
}
