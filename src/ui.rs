use crate::game;
use crate::net;
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
    pub sword_state: i32,
    pub sword_hidden: i32,
    pub main_weapon: i32,
    pub custom_weapon: i32,
    pub sub_weapon: i32,
    pub static_ptr_value: usize,
    pub position: Option<segment::Vec3>,
    pub hp: i32,
    pub player_found: bool,
    pub segment_action: segment::SegmentAction,
}

pub fn render_main_window(ui: &Ui, hud: &mut HelloHud, state: &UiState) {
    ui.window("DrmodDebug")
        .size([320., 600.], Condition::Always)
        .build(|| {
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

            // --- MISSION ---
            if state.mission_id != 0 || !state.mission_name.is_empty() {
                ui.text(format!(
                    "Mission: {} (0x{:04X}) [raw: 0x{:04X}]",
                    state.mission_name, state.mission_id, state.mission_id_raw
                ));
            }

            // --- GAME MENU STATUS ---
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

            // --- ДЕБАГ ПОЛЕЙ Pl0000 ---
            if state.player_found {
                ui.separator();
                ui.text("Pl0000 fields:");
                ui.text(format!("SwordState: {}", state.sword_state));
                ui.text(format!("SwordHidden: {}", state.sword_hidden));
            }

            if state.static_ptr_value == 0 {
                ui.text_colored([1.0, 0.0, 0.0, 1.0], "Module not found!");
                return;
            }

            // --- WEAPONS ---
            ui.separator();
            ui.text("Weapons:");
            ui.text(format!("Main: {}", state.main_weapon));
            ui.text(format!(
                "Custom: {} ({})",
                state.custom_weapon,
                game::custom_weapon_name(state.custom_weapon)
            ));
            ui.text(format!("Sub: {}", state.sub_weapon));

            ui.text(format!("Static Ptr Addr: 0x{:08X}", state.static_ptr_value));

            // --- SEGMENT DEBUG ---
            if hud.active_segment.is_some() {
                let action_name = match state.segment_action {
                    segment::SegmentAction::None => "None",
                    segment::SegmentAction::Start => "Start",
                    segment::SegmentAction::End => "End",
                    segment::SegmentAction::Reset => "Reset",
                };
                ui.text_colored(
                    [0.5, 0.8, 1.0, 1.0],
                    format!(
                        "SEGDEBUG: seg.mission={} cur.mission={} status={} ({}) => {}",
                        hud.active_segment.as_ref().unwrap().mission_id,
                        state.mission_id,
                        state.menu_status.name(),
                        state.menu_status as i32,
                        action_name,
                    ),
                );
            }

            if !state.player_found {
                ui.text_colored([1.0, 0.5, 0.0, 1.0], "Player object pointer is NULL");
                ui.text("Убедитесь, что вы в игре (не в меню).");
            } else {
                ui.text_colored([0.0, 1.0, 0.0, 1.0], "Player found!");
                ui.separator();
                ui.text(format!("HP: {}", state.hp));
                ui.text("NumPad1: +10m Y");
                ui.text("NumPad2: Save position");
                ui.text("NumPad3: Teleport");
            }

            ui.separator();
            ui.text("Position:");
            if let Some(pos) = state.position {
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
            if let Some((sx, sy, sz)) = hud.saved_position {
                ui.text(format!("X: {:.3}", sx));
                ui.text(format!("Y: {:.3}", sy));
                ui.text(format!("Z: {:.3}", sz));

                // --- ДЕБАГ: проекция на экран ---
                ui.separator();
                ui.text("Screen projection debug:");
                match hud.camera_ptr_addr {
                    None => {
                        ui.text_colored([1.0, 0.5, 0.0, 1.0], "camera_ptr_addr is None");
                    }
                    Some(cam_addr) => {
                        let cam_ptr = cam_addr.as_ptr();

                        let view_proj =
                            unsafe { *(cam_ptr.add(0x200) as *const [f32; 16]) };
                        let cam_x = unsafe { *(cam_ptr.add(0x1B0) as *const f32) };
                        let cam_y = unsafe { *(cam_ptr.add(0x1B4) as *const f32) };
                        let cam_z = unsafe { *(cam_ptr.add(0x1B8) as *const f32) };
                        let [vp_x, vp_y, vp_w, vp_h] = hud.viewport;

                        ui.text(format!("Camera ptr: 0x{:08X}", cam_ptr as usize));
                        ui.text(format!(
                            "Cam pos: {:.1} {:.1} {:.1}",
                            cam_x, cam_y, cam_z
                        ));
                        ui.text(format!(
                            "Viewport: [{:.0},{:.0}] {:.0}x{:.0}",
                            vp_x, vp_y, vp_w, vp_h
                        ));
                        ui.text(format!(
                            "VP[0..4]: {:.3} {:.3} {:.3} {:.3}",
                            view_proj[0], view_proj[1], view_proj[2], view_proj[3]
                        ));
                        ui.text(format!(
                            "VP[4..8]: {:.3} {:.3} {:.3} {:.3}",
                            view_proj[4], view_proj[5], view_proj[6], view_proj[7]
                        ));

                        match overlay::world_to_screen(
                            (sx, sy, sz),
                            &view_proj,
                            hud.viewport,
                            (cam_x, cam_y, cam_z),
                        ) {
                            Some(([scr_x, scr_y], dist)) => {
                                let on_scr = scr_x >= vp_x
                                    && scr_x <= vp_x + vp_w
                                    && scr_y >= vp_y
                                    && scr_y <= vp_y + vp_h;
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
                                ui.text_colored(
                                    [1.0, 0.3, 0.3, 1.0],
                                    "Behind camera (w <= 0)",
                                );
                            }
                        }
                    }
                }
            } else {
                ui.text_colored([0.5, 0.5, 0.5, 1.0], "не сохранена");
            }

            // --- D3D DEBUG ---
            ui.separator();
            ui.text_colored(
                [0.5, 1.0, 0.5, 1.0],
                format!("D3D frames: {}", hud.d3d_frame_count),
            );
            if !hud.d3d_last_error.is_empty() {
                ui.text_colored(
                    [1.0, 0.5, 0.0, 1.0],
                    format!("D3D error: {}", hud.d3d_last_error),
                );
            }

            // --- ВЫХОД ---
            ui.separator();
            if ui.button("Выход / Выгрузить DLL") {
                hud.net_client = None;
                hudhook::eject();
            }
        });
}

pub fn render_multiplayer_window(ui: &Ui, hud: &mut HelloHud) {
    ui.window("Multiplayer")
        .size([300.0, 250.0], Condition::FirstUseEver)
        .build(|| {
            ui.input_text("Server", &mut hud.server_addr)
                .hint("127.0.0.1:5222")
                .build();
            ui.input_text("Name", &mut hud.player_name).build();
            ui.input_text("Room", &mut hud.room_name).build();

            if hud.net_client.is_none() {
                if ui.button("Connect") {
                    match net::NetClient::new(
                        &hud.server_addr,
                        &hud.player_name,
                        &hud.room_name,
                    ) {
                        Ok(nc) => {
                            hud.net_client = Some(nc);
                        }
                        Err(_) => {}
                    }
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
                    let active = if hud.active_segment.as_ref().map_or(false, |s| {
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
