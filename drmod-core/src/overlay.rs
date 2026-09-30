use imgui::*;

/// Проецирует мировую позицию на экран через view-projection матрицу (D3DXMATRIX, row-major).
/// Возвращает `(screen_pos, distance_to_camera)` или `None` если точка за камерой.
/// `viewport`: [X, Y, Width, Height] — из D3D GetViewport (с учётом смещения для 4K/letterbox).
pub fn world_to_screen(
    world_pos: (f32, f32, f32),
    view_proj: &[f32; 16],
    viewport: [f32; 4],
    camera_pos: (f32, f32, f32),
) -> Option<([f32; 2], f32)> {
    let (wx, wy, wz) = world_pos;
    let (cx, cy, cz) = camera_pos;
    let [vp_x, vp_y, vp_w, vp_h] = viewport;

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

    let screen_x = (ndc_x * 0.5 + 0.5) * vp_w + vp_x;
    let screen_y = (1.0 - (ndc_y * 0.5 + 0.5)) * vp_h + vp_y;

    let dx = wx - cx;
    let dy = wy - cy;
    let dz = wz - cz;
    let dist = (dx * dx + dy * dy + dz * dz).sqrt();

    Some(([screen_x, screen_y], dist))
}

pub fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let millis = ms % 1000;
    format!("{:02}:{:02}.{:03}", mins, secs, millis)
}

pub fn draw_world_pos(
    ui: &Ui,
    world_pos: (f32, f32, f32),
    view_proj: &[f32; 16],
    camera_pos: (f32, f32, f32),
    viewport: [f32; 4],
    color: u32,
    label: &str,
) {
    let [vp_x, vp_y, vp_w, vp_h] = viewport;

    if let Some(([scr_x, scr_y], dist)) = world_to_screen(world_pos, view_proj, viewport, camera_pos)
    {
        let on_screen = scr_x >= vp_x && scr_x <= vp_x + vp_w && scr_y >= vp_y && scr_y <= vp_y + vp_h;

        // Screen-space radius for a 0.5m world-space offset
        let (wx, wy, wz) = world_pos;
        let radius = if let Some(([rx, _], _)) =
            world_to_screen((wx + 0.5, wy, wz), view_proj, viewport, camera_pos)
        {
            (rx - scr_x).abs().clamp(2.0, 64.0)
        } else {
            8.0
        };

        let (draw_x, draw_y, text_offset_x) = if on_screen {
            (scr_x, scr_y, radius + 4.0)
        } else {
            (
                scr_x.clamp(vp_x + 24.0, vp_x + vp_w - 24.0),
                scr_y.clamp(vp_y + 24.0, vp_y + vp_h - 24.0),
                radius + 4.0,
            )
        };

        let draw_list = ui.get_foreground_draw_list();

        if on_screen {
            // Beam: vertical line ground → +2m
            if let Some(([head_x, head_y], _)) =
                world_to_screen((wx, wy + 2.0, wz), view_proj, viewport, camera_pos)
            {
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
