---
name: separate-read-from-ui
description: Separate game memory reading from imgui-rs rendering in a hudhook overlay — collect all per-frame data into a UiState struct via a single read_game_state() method, then pass immutable state to pure rendering functions
source: auto-skill
extracted_at: '2026-07-20T13:06:51.124Z'
---

# Separate Memory Reads from UI Rendering

In a hudhook + imgui-rs overlay, the render loop often mixes `unsafe` memory reads with `ui.text()` / `ui.window()` calls inside the same closure. This creates tangled code where data collection and presentation are interleaved.

**Solution:** collect all per-frame game state into a single `UiState` struct via one `read_game_state(&mut self)` method, then pass an immutable `&UiState` to pure rendering functions. Mutations (segment tracking, key handlers) happen in `render()` between the read and the draw.

## When to use

- The overlay reads 5+ memory addresses per frame (mission, status, weapons, position, HP, etc.)
- Data collection and UI code are mixed inside `ui.window(…).build(|| { … })`
- You want to reorder the frame: read → mutate → draw
- You want rendering functions to be testable without game memory access

## Core pattern

### 1. Define UiState in the UI module

```rust
// src/ui.rs

pub struct UiState {
    pub mission_id: i32,
    pub mission_id_raw: i32,
    pub mission_name: String,
    pub menu_status_raw: i32,
    pub menu_status_valid: bool,
    pub menu_status: game::GameMenuStatus,
    pub main_weapon: i32,
    pub custom_weapon: i32,
    pub sub_weapon: i32,
    pub static_ptr_value: usize,
    pub position: Option<segment::Vec3>,
    pub hp: i32,
    pub player_found: bool,
    pub segment_action: segment::SegmentAction,
    // … любые другие поля, нужные для отрисовки
}
```

**Rules for UiState fields:**
- Only **read-only display data** — no raw pointers, no DB connections, no mutable state
- `String` fields are fine (allocated once per frame in `read_game_state`)
- Include computed values like `segment_action` to avoid recomputing in UI

### 2. Add read_game_state method on the overlay struct

```rust
// src/lib.rs — impl HelloHud

fn read_game_state(&mut self) -> ui::UiState {
    let mut state = ui::UiState::default(); // или ручная инициализация

    // --- читаем память (все unsafe-чтения здесь) ---
    if self.base_addr != 0 {
        let mission_id_addr = self.base_addr + 0x1764670;
        state.mission_id_raw = unsafe { *(mission_id_addr as *const i32) };
        // …
    }

    // --- читаем оружие ---
    if let Some(pm) = self.player_manager_addr {
        let ptr = pm.as_ptr();
        state.main_weapon = unsafe { *(ptr.add(0xE0) as *const i32) };
        // …
    }

    // --- читаем позицию игрока ---
    if let Some(sp) = self.static_ptr_addr {
        let obj = unsafe { *(sp.as_ptr() as *const *mut u8) };
        if !obj.is_null() {
            state.player_found = true;
            state.position = Some(segment::Vec3 {
                x: unsafe { *(obj.add(0x50) as *const f32) },
                // …
            });
            state.hp = unsafe { *(obj.add(0x870) as *const i32) };
        }
    }

    // --- вычисляем и применяем SegmentAction (мутация!) ---
    state.segment_action = segment::segment_action(…);
    match state.segment_action {
        SegmentAction::Start => { /* мутирует self.active_segment */ }
        SegmentAction::End   => { /* сохраняет в БД, чистит буфер */ }
        SegmentAction::Reset => { /* сбрасывает без сохранения */ }
        SegmentAction::None  => {}
    }

    // --- пушим позицию в буфер (если сегмент активен) ---
    if state.player_found {
        if let (Some(seg), Some(pos)) = (…, state.position) {
            self.position_buffer.push((pos, …));
        }
    }

    state
}
```

**Key design decisions:**
- `read_game_state` takes `&mut self` — it mutates overlay state (segment lifecycle, position buffer)
- It **returns** `UiState` by value — the caller passes it to render functions
- All `unsafe` memory reads live here; nowhere else
- SegmentAction is computed AND applied here (not deferred to UI)

### 3. Render function takes immutable references

```rust
// src/ui.rs

pub fn render_main_window(ui: &Ui, hud: &HelloHud, state: &UiState) {
    ui.window("##hello")
        .size([320., 600.], Condition::Always)
        .build(|| {
            // --- только ui.text / ui.text_colored / ui.button ---
            // никаких unsafe, никаких чтений памяти

            ui.text(format!("Mission: {} (0x{:04X})", state.mission_name, state.mission_id));

            if state.menu_status_valid {
                ui.text_colored(…, format!("Status: {}", state.menu_status.name()));
            }

            ui.text(format!("Main weapon: {}", state.main_weapon));

            if let Some(pos) = state.position {
                ui.text(format!("X: {:.3} Y: {:.3} Z: {:.3}", pos.x, pos.y, pos.z));
            }

            // можно читать hud для persistent-полей (saved_position, camera_ptr_addr, active_segment)
            if let Some((sx, sy, sz)) = hud.saved_position {
                ui.text(format!("Saved: {:.1} {:.1} {:.1}", sx, sy, sz));
            }

            if ui.button("Выход") { hudhook::eject(); }
        });
}
```

Сигнатура: `&HelloHud` (не `&mut`) — рендер ничего не меняет.

### 4. render() orchestrates the frame

```rust
// src/lib.rs — impl ImguiRenderLoop::render

fn render(&mut self, ui: &mut Ui) {
    // 1. Сетевая логика (poll, send, recv) — если есть
    if let Some(ref mut nc) = self.net_client { … }

    // 2. Читаем всё игровое состояние
    let ui_state = self.read_game_state();

    // 3. Key handlers — мутируют игру через cached_player_obj_ptr
    if !self.cached_player_obj_ptr.is_null() {
        let p = self.cached_player_obj_ptr;
        if ui.is_key_pressed_no_repeat(Key::Keypad1) {
            unsafe { *(p.add(0x54) as *mut f32) += 10.0; }
        }
        // NumPad2, NumPad3…
    }

    // 4. Отрисовка (только чтение)
    ui::render_main_window(ui, self, &ui_state);

    // 5. 2D-маркеры поверх окна (draw_world_pos)
    if let (Some(pos), Some(cam)) = (self.saved_position, self.camera_ptr_addr) {
        overlay::draw_world_pos(ui, pos, cam.as_ptr(), …);
    }

    // 6. Другие окна
    ui::render_multiplayer_window(ui, self);
}
```

**Порядок важен:**
- Сеть до чтения (чтобы отправить позицию предыдущего кадра)
- `read_game_state()` после сети (читает свежие данные)
- Key handlers после чтения (используют `cached_player_obj_ptr`, прочитанный в `read_game_state`)
- UI после key handlers (показывает результат мутаций)
- 2D-маркеры после UI (draw_world_pos требует `ui.get_foreground_draw_list()` — доступен после build)

## What goes WHERE

| Данные | Где живут |
|--------|-----------|
| mission, weapons, position, hp, menu_status | `UiState` — обновляются каждый кадр |
| saved_position, active_segment, ghost_* | `HelloHud` — persistent между кадрами |
| camera_ptr_addr, d3d_frame_count | `HelloHud` — persistent, read-only для UI |
| player_obj_ptr (для key handlers) | `HelloHud.cached_player_obj_ptr` — сырой указатель |
| DB connection, net_client | `HelloHud` — внешние ресурсы |

## Visibility

Если UI-модуль читает persistent-поля из `HelloHud`, они должны быть `pub(crate)`:

```rust
pub(crate) d3d_frame_count: u32,
pub(crate) d3d_last_error: String,
pub(crate) cached_player_obj_ptr: *mut u8,
```

## Anti-patterns to avoid

- ❌ Чтение `unsafe` памяти внутри `ui.window(…).build(|| { … })`
- ❌ `&mut HelloHud` в render-функциях (если они только отрисовывают)
- ❌ SegmentAction match в UI (должен быть применён до отрисовки)
- ❌ `position_buffer.push()` в UI
- ❌ Повторное чтение одних и тех же адресов в разных местах render()

## Related skills

- `segment-tracking` — SegmentAction state machine
- `rusqlite-injected-dll` — SQLite persistence pattern
- `world-to-screen` — 2D marker rendering via draw_world_pos
- `game-data-module` — extracting enums/helpers into a dedicated module
