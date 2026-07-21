---
name: world-to-screen
description: Project a 3D world position to 2D screen coordinates in an MGR:R hudhook/imgui-rs overlay using the game's camera matrices read from memory, with D3D viewport-based screen sizing and scissor fix for 4K fullscreen
source: auto-skill
extracted_at: '2026-07-13T15:00:00.000Z'
updated_at: '2026-07-20T16:00:00.000Z'
---

# World-to-Screen Projection (MGR:R + DirectX 9 + imgui-rs)

Project a 3D world position to 2D screen coordinates using the game's camera `view × projection` matrix, read directly from game memory. Draw screen-space markers via imgui foreground draw lists, with on-screen and off-screen (edge-clamped) styling.

**Critical:** Use the **D3D viewport** (`device.GetViewport`) as the screen size source — never `ui.io().display_size`. The latter comes from `GetClientRect` (window size) and diverges from the actual render area when Borderless Gaming stretches the window or when running 1080p fullscreen on a 4K monitor. See §"Viewport vs display_size" below.

**Location:** All functions live in `src/overlay.rs` — `world_to_screen`, `format_duration_ms`, `draw_world_pos`.

## When to use

- You have a 3D world position and want to draw a marker on screen at that location
- The project is an imgui-rs + hudhook overlay injected into a DirectX 9 game
- The game's camera class is known from an SDK or Cheat Engine table

## Prerequisites

- Known camera object address (static offset or pointer chain)
- Known offsets for `m_viewProjectionMatrix` and camera world position
- D3D viewport read from `device.GetViewport` in `render_3d`

## Camera data sources (MGR:R)

### Static singleton (preferred)

```rust
// SDK: static inline cCameraGame& Instance = *(cCameraGame*)(shared::base + 0x17EA1D0);
let camera_ptr_addr = NonNull::new(unsafe { (base_addr as *mut u8).add(0x17EA1D0) });
```

**Critical:** `base + 0x17EA1D0` IS the camera object, not a pointer to it. Do NOT dereference as pointer-to-pointer.

## Key offsets from `cCameraGame*`

| Offset | Type | Field |
|--------|------|-------|
| `+0x1B0` | cVec4 (16 B) | `m_CameraMatrix.m_vecPosition` — camera world pos |
| `+0x200` | D3DXMATRIX (64 B) | `m_viewProjectionMatrix` — combined view × proj |

## Viewport vs `ui.io().display_size`

**Never use `ui.io().display_size` for screen-space calculations.** It comes from `GetClientRect` (window size) and breaks in two scenarios:

| Scenario | Backbuffer | `display_size` | Result |
|----------|-----------|----------------|--------|
| Borderless Gaming | 1280×720 | 1920×1080 (window) | UI offset, markers off-screen, clicks broken |
| 1080p fullscreen → 4K | 3840×2160 | 3840×2160 (or window) | Viewport = 1920×1080 with offset; 3D clipped |

**Fix:** Read the real D3D viewport in `render_3d` and store in the overlay struct:

```rust
// In render_3d (has access to IDirect3DDevice9)
let mut vp = D3DVIEWPORT9::default();
unsafe { device.GetViewport(&mut vp).ok(); }
self.viewport = [vp.X as f32, vp.Y as f32, vp.Width as f32, vp.Height as f32];
```

Store in the struct:

```rust
pub(crate) viewport: [f32; 4],  // [X, Y, Width, Height]
```

Use `self.viewport` everywhere instead of `ui.io().display_size`.

## Procedure

### 1. Add viewport field + camera address to the struct

```rust
struct HelloHud {
    camera_ptr_addr: Option<NonNull<u8>>,
    viewport: [f32; 4],  // [X, Y, Width, Height] — updated per-frame in render_3d
}

impl HelloHud {
    fn new() -> Self {
        let camera_ptr_addr = if base_addr == 0 { None } else {
            NonNull::new(unsafe { (base_addr as *mut u8).add(0x17EA1D0) })
        };
        Self { camera_ptr_addr, viewport: [0.0; 4], /* … */ }
    }
}
```

### 2. Read viewport in render_3d

```rust
fn render_3d(&mut self, device: &IDirect3DDevice9) {
    // Read D3D viewport every frame
    {
        let mut vp = D3DVIEWPORT9::default();
        unsafe { device.GetViewport(&mut vp).ok(); }
        self.viewport = [vp.X as f32, vp.Y as f32, vp.Width as f32, vp.Height as f32];
    }
    // … rest of render_3d (camera matrix, cylinder draws)
}
```

### 3. World-to-screen with viewport

```rust
/// viewport: [X, Y, Width, Height] from D3D GetViewport — handles offset for
/// letterbox/4K where the game viewport is smaller than the backbuffer.
pub fn world_to_screen(
    world_pos: (f32, f32, f32),
    view_proj: &[f32; 16],
    viewport: [f32; 4],
    camera_pos: (f32, f32, f32),
) -> Option<([f32; 2], f32)> {
    let (wx, wy, wz) = world_pos;
    let [vp_x, vp_y, vp_w, vp_h] = viewport;

    // Row-vector × matrix (row-major)
    let clip_x = wx * view_proj[0] + wy * view_proj[4] + wz * view_proj[8] + view_proj[12];
    let clip_y = wx * view_proj[1] + wy * view_proj[5] + wz * view_proj[9] + view_proj[13];
    let clip_w = wx * view_proj[3] + wy * view_proj[7] + wz * view_proj[11] + view_proj[15];

    if clip_w <= 0.0 { return None; }

    let inv_w = 1.0 / clip_w;
    let ndc_x = clip_x * inv_w;
    let ndc_y = clip_y * inv_w;

    // NDC → screen, adding viewport offset
    let screen_x = (ndc_x * 0.5 + 0.5) * vp_w + vp_x;
    let screen_y = (1.0 - (ndc_y * 0.5 + 0.5)) * vp_h + vp_y;

    let dist = {
        let dx = wx - camera_pos.0;
        let dy = wy - camera_pos.1;
        let dz = wz - camera_pos.2;
        (dx * dx + dy * dy + dz * dz).sqrt()
    };

    Some(([screen_x, screen_y], dist))
}
```

Key difference from display_size version: `* vp_w + vp_x` and `* vp_h + vp_y` — this correctly positions markers when the viewport is offset (e.g., centered in letterbox).

### 4. draw_world_pos with viewport

```rust
pub fn draw_world_pos(
    ui: &Ui,
    world_pos: (f32, f32, f32),
    camera_ptr: *const u8,
    viewport: [f32; 4],
    color: u32,
    label: &str,
) {
    let view_proj = unsafe { *(camera_ptr.add(0x200) as *const [f32; 16]) };
    let cam_x = unsafe { *(camera_ptr.add(0x1B0) as *const f32) };
    let cam_y = unsafe { *(camera_ptr.add(0x1B4) as *const f32) };
    let cam_z = unsafe { *(camera_ptr.add(0x1B8) as *const f32) };
    let [vp_x, vp_y, vp_w, vp_h] = viewport;

    if let Some(([scr_x, scr_y], dist)) =
        world_to_screen(world_pos, &view_proj, viewport, (cam_x, cam_y, cam_z))
    {
        // on_screen check uses viewport bounds (not [0..display_size])
        let on_screen = scr_x >= vp_x && scr_x <= vp_x + vp_w
                     && scr_y >= vp_y && scr_y <= vp_y + vp_h;

        let (draw_x, draw_y) = if on_screen {
            (scr_x, scr_y)
        } else {
            (scr_x.clamp(vp_x + 24., vp_x + vp_w - 24.),
             scr_y.clamp(vp_y + 24., vp_y + vp_h - 24.))
        };

        let draw_list = ui.get_foreground_draw_list();
        if on_screen {
            draw_list.add_circle([draw_x, draw_y], 8.0, color).thickness(2.0).build();
            draw_list.add_text([draw_x + 12., draw_y - 8.], color,
                format!("{} ({:.1}m)", label, dist));
        } else {
            draw_list.add_circle([draw_x, draw_y], 8.0, color).thickness(2.5).build();
            draw_list.add_text([draw_x + 12., draw_y - 8.], color,
                format!("\u{25c6} {} ({:.1}m)", label, dist));
        }
    }
}
```

### 5. Call from render()

```rust
// Saved position marker
if let (Some((sx, sy, sz)), Some(cam)) = (self.saved_position, self.camera_ptr_addr) {
    overlay::draw_world_pos(ui, (sx, sy, sz), cam.as_ptr(), self.viewport,
        0xFF_00_FF_00, "Saved");
}
```

### 6. 3D cylinders: disable scissor test on 4K fullscreen

When running 1080p fullscreen on a 4K monitor, the D3D backbuffer is 3840×2160 but the game sets a scissor rect of 1920×1080. Our 3D cylinders (rendered via `CylinderRenderer` in `render_3d`) get clipped by this scissor rect.

**Fix:** In `render_3d`, disable `D3DRS_SCISSORTESTENABLE` before drawing cylinders, restore after:

```rust
use windows::Win32::Graphics::Direct3D9::D3DRS_SCISSORTESTENABLE;

fn render_3d(&mut self, device: &IDirect3DDevice9) {
    // … read viewport, camera matrix …

    // Disable scissor so cylinders render across the full backbuffer
    let mut saved_scissor: u32 = 0;
    unsafe {
        device.GetRenderState(D3DRS_SCISSORTESTENABLE, &mut saved_scissor).ok();
        device.SetRenderState(D3DRS_SCISSORTESTENABLE, 0).ok();
    }

    // … draw ghost, saved, remote cylinders …

    // Restore
    unsafe {
        device.SetRenderState(D3DRS_SCISSORTESTENABLE, saved_scissor).ok();
    }
}
```

**Do NOT change the viewport** — the viewport controls NDC→pixel mapping and must match the game's projection. Only the scissor rect causes clipping; viewport positioning is correct.

## Debug overlay (troubleshooting)

```rust
let [vp_x, vp_y, vp_w, vp_h] = hud.viewport;
ui.text(format!("Viewport: [{:.0},{:.0}] {:.0}x{:.0}", vp_x, vp_y, vp_w, vp_h));
ui.text(format!("Cam pos: {:.1} {:.1} {:.1}", cam_x, cam_y, cam_z));
ui.text(format!("VP[0..4]: {:.3} {:.3} {:.3} {:.3}",
    view_proj[0], view_proj[1], view_proj[2], view_proj[3]));
```

## Common pitfalls

| Symptom | Likely cause |
|---------|-------------|
| Markers offset / off-screen in Borderless Gaming | Using `ui.io().display_size` instead of viewport |
| 3D cylinders clipped to quadrant on 4K | Scissor test enabled with game's rect |
| All 3D cylinders disappear after viewport change | Expanded viewport breaks NDC→pixel mapping; only disable scissor |
| `Behind camera` when looking at the point | Y-axis sign wrong; try `1.0 - (ndc_y * …)` form |
| Matrix all zeros | Double-dereferenced static instance address |
