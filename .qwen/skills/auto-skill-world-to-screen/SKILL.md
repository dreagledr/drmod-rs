---
name: world-to-screen
description: Project a 3D world position to 2D screen coordinates in an MGR:R hudhook/imgui-rs overlay using the game's camera matrices read from memory
source: auto-skill
extracted_at: '2026-07-13T15:00:00.000Z'
---

# World-to-Screen Projection (MGR:R + DirectX 9 + imgui-rs)

Project a 3D world position to 2D screen coordinates using the game's camera `view × projection` matrix, read directly from game memory. Draw screen-space markers via imgui foreground draw lists, with on-screen and off-screen (edge-clamped) styling.

## When to use

- You have a 3D world position (e.g. saved player coords, enemy positions) and want to draw a marker on screen at that location
- The project is an imgui-rs + hudhook overlay injected into a DirectX 9 game (see `QWEN.md` in drmod-rs)
- The game's camera class is known from an SDK or Cheat Engine table

## Prerequisites

- Known camera object address (static offset or pointer chain)
- Known offsets for `m_viewProjectionMatrix` (D3DXMATRIX, 16 × f32, row-major) and camera world position
- Screen dimensions from `ui.io().display_size`

## Camera data sources (MGR:R)

### Static singleton (preferred)

```rust
// SDK: static inline cCameraGame& Instance = *(cCameraGame*)(shared::base + 0x17EA1D0);
let camera_ptr_addr = NonNull::new(unsafe { (base_addr as *mut u8).add(0x17EA1D0) });
```

**Critical:** `base + 0x17EA1D0` IS the camera object, not a pointer to it. The C++ cast+ deref pattern `*(T*)(addr)` means "the object lives at this address". In Rust, use the address directly — do NOT dereference it again as a pointer-to-pointer.

### Via player object (alternative)

```rust
// player_obj + 0x2320 → cCameraGame* (from .CT table analysis)
let camera_ptr = unsafe { *(player_obj.add(0x2320) as *const *mut u8) };
```

Here the offset IS a pointer field, so one dereference is correct.

## Key offsets from `cCameraGame*`

| Offset | Type | Field |
|--------|------|-------|
| `+0x10` | D3DXMATRIX (64 B) | `m_projectionMatrix` (Hw::CameraProj) |
| `+0x90` | f32 | `m_fFOV` |
| `+0xB0` | D3DXMATRIX (64 B) | `m_viewMatrix` (Hw::cCameraBase) |
| `+0x1B0` | cVec4 (16 B) | `m_CameraMatrix.m_vecPosition` — camera world pos |
| `+0x200` | D3DXMATRIX (64 B) | `m_viewProjectionMatrix` — already combined view × proj |

D3DXMATRIX is 4×4 f32 in row-major layout: `_11 _12 _13 _14 _21 _22 _23 _24 _31 _32 _33 _34 _41 _42 _43 _44`.

## Procedure

### 1. Add camera address to the struct

```rust
struct HelloHud {
    // ...existing fields...
    camera_ptr_addr: Option<NonNull<u8>>,
}

impl HelloHud {
    fn new() -> Self {
        let camera_ptr_addr = if base_addr == 0 { None } else {
            NonNull::new(unsafe { (base_addr as *mut u8).add(0x17EA1D0) })
        };
        Self { /* ... */ camera_ptr_addr, /* ... */ }
    }
}
```

### 2. World-to-screen projection function

Pure function — no side effects, no unsafe (the caller handles memory reads):

```rust
/// Проецирует мировую позицию на экран через view-projection матрицу (D3DXMATRIX, row-major).
/// Возвращает `(screen_pos, distance_to_camera)` или `None` если точка за камерой.
fn world_to_screen(
    world_pos: (f32, f32, f32),
    view_proj: &[f32; 16],
    screen_size: [f32; 2],
    camera_pos: (f32, f32, f32),
) -> Option<([f32; 2], f32)> {
    let (wx, wy, wz) = world_pos;

    // Row-vector (x,y,z,1) × matrix (row-major layout)
    let clip_x = wx * view_proj[0] + wy * view_proj[4] + wz * view_proj[8] + view_proj[12];
    let clip_y = wx * view_proj[1] + wy * view_proj[5] + wz * view_proj[9] + view_proj[13];
    let clip_w = wx * view_proj[3] + wy * view_proj[7] + wz * view_proj[11] + view_proj[15];

    if clip_w <= 0.0 {
        return None; // behind camera
    }

    let inv_w = 1.0 / clip_w;
    let ndc_x = clip_x * inv_w;
    let ndc_y = clip_y * inv_w;

    // Clip-space [-1,1] → screen [0, W]  and [H, 0] (Y flipped)
    let screen_x = (ndc_x * 0.5 + 0.5) * screen_size[0];
    let screen_y = (1.0 - (ndc_y * 0.5 + 0.5)) * screen_size[1];

    let dist = {
        let dx = wx - camera_pos.0;
        let dy = wy - camera_pos.1;
        let dz = wz - camera_pos.2;
        (dx * dx + dy * dy + dz * dz).sqrt()
    };

    Some(([screen_x, screen_y], dist))
}
```

Only `clip_w` is needed for the "behind camera" check. `clip_z` is computed but unused unless you need depth-sorting.

### 3. Read camera data and project

Inside `render()`, after the ImGui window build (so `ui.get_foreground_draw_list()` is available):

```rust
if let (Some((sx, sy, sz)), Some(camera_addr)) = (self.saved_position, self.camera_ptr_addr) {
    let camera_ptr = camera_addr.as_ptr(); // the object, not a pointer-to-pointer!

    // m_viewProjectionMatrix at +0x200 (D3DXMATRIX row-major)
    let view_proj = unsafe { *(camera_ptr.add(0x200) as *const [f32; 16]) };
    // m_CameraMatrix.m_vecPosition at +0x1B0
    let cam_x = unsafe { *(camera_ptr.add(0x1B0) as *const f32) };
    let cam_y = unsafe { *(camera_ptr.add(0x1B4) as *const f32) };
    let cam_z = unsafe { *(camera_ptr.add(0x1B8) as *const f32) };

    let [sw, sh] = ui.io().display_size;

    if let Some(([scr_x, scr_y], dist)) = world_to_screen(
        (sx, sy, sz), &view_proj, [sw, sh], (cam_x, cam_y, cam_z),
    ) {
        // ... draw marker (see §4 below)
    }
}
```

### 4. Draw screen-space markers

Use `ui.get_foreground_draw_list()` — draws on top of all ImGui windows in screen coordinates.

**On-screen marker** (green circle + white text):
```rust
const EDGE_MARGIN: f32 = 24.0;
let on_screen = scr_x >= 0.0 && scr_x <= sw && scr_y >= 0.0 && scr_y <= sh;
let (draw_x, draw_y) = if on_screen {
    (scr_x, scr_y)
} else {
    (scr_x.clamp(EDGE_MARGIN, sw - EDGE_MARGIN),
     scr_y.clamp(EDGE_MARGIN, sh - EDGE_MARGIN))
};

let draw_list = ui.get_foreground_draw_list();
if on_screen {
    draw_list
        .add_circle([draw_x, draw_y], 8.0, 0xFF_00_FF_00)
        .thickness(2.0)
        .build();
    draw_list.add_text([draw_x + 12., draw_y - 8.], 0xFF_FF_FF_FF,
        format!("Saved ({:.1}m)", dist));
} else {
    draw_list
        .add_circle([draw_x, draw_y], 8.0, 0xFF_FF_80_00)  // orange
        .thickness(2.5)
        .build();
    draw_list.add_text([draw_x + 12., draw_y - 8.], 0xFF_FF_B0_40,
        format!("\u{25c6} Saved ({:.1}m)", dist));  // ◆ prefix to distinguish
}
```

### 5. Debug overlay (troubleshooting)

When the marker doesn't appear, add debug text inside the ImGui window:

```rust
ui.text(format!("Camera ptr: 0x{:08X}", camera_ptr as usize));
ui.text(format!("Cam pos: {:.1} {:.1} {:.1}", cam_x, cam_y, cam_z));
ui.text(format!("Screen: {:.0}x{:.0}", sw, sh));
// First 8 values of view-proj matrix (should be non-zero!)
ui.text(format!("VP[0..4]: {:.3} {:.3} {:.3} {:.3}",
    view_proj[0], view_proj[1], view_proj[2], view_proj[3]));
ui.text(format!("VP[4..8]: {:.3} {:.3} {:.3} {:.3}",
    view_proj[4], view_proj[5], view_proj[6], view_proj[7]));
```

Expected: matrix values are non-zero and change when moving the camera. If all zero, the camera pointer or offset is wrong.

## Reusable drawing function

When you need to render multiple world positions (saved marker, ghost trace, checkpoints), extract the projection + drawing into a single function to avoid code duplication:

```rust
fn draw_world_pos(
    ui: &Ui,
    world_pos: (f32, f32, f32),
    camera_ptr: *const u8,
    on_screen_color: u32,
    off_screen_color: u32,
    off_screen_text_color: u32,
    label: &str,
) {
    let view_proj = unsafe { *(camera_ptr.add(0x200) as *const [f32; 16]) };
    let cam_x = unsafe { *(camera_ptr.add(0x1B0) as *const f32) };
    let cam_y = unsafe { *(camera_ptr.add(0x1B4) as *const f32) };
    let cam_z = unsafe { *(camera_ptr.add(0x1B8) as *const f32) };

    let [sw, sh] = ui.io().display_size;
    const EDGE_MARGIN: f32 = 24.0;

    if let Some(([scr_x, scr_y], dist)) =
        world_to_screen(world_pos, &view_proj, [sw, sh], (cam_x, cam_y, cam_z))
    {
        let on_screen = scr_x >= 0.0 && scr_x <= sw && scr_y >= 0.0 && scr_y <= sh;
        let (draw_x, draw_y) = if on_screen {
            (scr_x, scr_y)
        } else {
            (scr_x.clamp(EDGE_MARGIN, sw - EDGE_MARGIN),
             scr_y.clamp(EDGE_MARGIN, sh - EDGE_MARGIN))
        };

        let draw_list = ui.get_foreground_draw_list();
        if on_screen {
            draw_list.add_circle([draw_x, draw_y], 8.0, on_screen_color).thickness(2.0).build();
            draw_list.add_text([draw_x + 12., draw_y - 8.], 0xFF_FF_FF_FF,
                format!("{} ({:.1}m)", label, dist));
        } else {
            draw_list.add_circle([draw_x, draw_y], 8.0, off_screen_color).thickness(2.5).build();
            draw_list.add_text([draw_x + 12., draw_y - 8.], off_screen_text_color,
                format!("\u{25c6} {} ({:.1}m)", label, dist));
        }
    }
}
```

Call it once per marker:

```rust
// Saved position (green)
draw_world_pos(ui, (sx, sy, sz), camera_addr.as_ptr(),
    0xFF_00_FF_00, 0xFF_00_80_FF, 0xFF_40_B0_FF, "Saved");

// Ghost trace (red)
draw_world_pos(ui, (gx, gy, gz), camera_addr.as_ptr(),
    0xFF_00_00_FF, 0xFF_00_40_C0, 0xFF_40_20_C0, &ghost_label);
```

The `camera_ptr` parameter is the raw `*const u8` of the camera object (not a pointer-to-pointer). Colors are ABGR (`0xAA_BB_GG_RR`).

## Common pitfalls

| Symptom | Likely cause |
|---------|-------------|
| Matrix all zeros, cam pos garbage | Double-dereferenced a static instance address (see §1) |
| Matrix all zeros, cam pos valid | Wrong offset for view-projection matrix |
| `Behind camera` when looking at the point | Y-axis sign wrong in projection; try flipping `ndc_y` or using `1.0 - ...` form |
| Marker doesn't move with camera | Reading from wrong address (static vs frame-specific) |
| Marker offset from expected position | D3DXMATRIX is row-major; use row-vector × matrix order, not matrix × column-vector |

### Rust format specifier note

`f32` uses `{:.precision}` NOT `{:.precisionf}`:
```rust
// CORRECT:
format!("{:.1}", some_f32)
// WRONG:
format!("{:.1f}", some_f32)  // compile error: unknown format trait `f`
```

## Extending

- **Multiple markers**: store `Vec<(f32, f32, f32)>` and loop the projection
- **Lines between markers**: use `draw_list.add_line(p1, p2, color)`
- **Dynamic camera**: if the camera pointer itself changes (cutscenes), read it from `player_obj + 0x2320` per-frame instead of the static instance
- **World axes / ESP**: project multiple points (e.g. enemy positions from entity list) with the same matrix
