//! Чтение полного скелета из `cModelBase::BoneSet` и визуализация.

use imgui::Ui;

use crate::overlay::world_to_screen;

// ── cModelBase offsets (от начала cParts / player_obj_ptr) ─────────
const BONESET_PBONES_OFFSET: usize = 0x350; // BoneSet::m_pBones (cParts*)
const BONESET_AMOUNT_OFFSET: usize = 0x358; // BoneSet::m_nBoneAmount (short)

// ── cParts offsets ─────────────────────────────────────────────────
const BONE_SIZE: usize = 0xB0; // sizeof(cParts)
const BONE_POS_X: usize = 0x40; // m_PositionMatrix._41
const BONE_POS_Y: usize = 0x44; // m_PositionMatrix._42
const BONE_POS_Z: usize = 0x48; // m_PositionMatrix._43
const BONE_INDEX: usize = 0xA0; // m_nBoneIndex (i16)
const BONE_PARENT: usize = 0xA8; // m_pParentBone (cParts*)

/// Мировая позиция одной кости.
#[derive(Clone, Debug)]
pub struct BonePos {
    /// Адрес cParts в памяти — для сопоставления parent-child.
    pub ptr: usize,
    pub index: u16,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Читает **все** кости из `cModelBase::BoneSet`.
///
/// `player_obj_ptr` — указатель на Pl0000 (он же cModelBase, он же cParts).
pub unsafe fn read_full_skeleton(player_obj_ptr: *const u8) -> Vec<BonePos> {
    if player_obj_ptr.is_null() {
        return Vec::new();
    }

    let p_bones = unsafe { *((player_obj_ptr.add(BONESET_PBONES_OFFSET)) as *const *const u8) };
    if p_bones.is_null() {
        return Vec::new();
    }

    let bone_count = unsafe { *(player_obj_ptr.add(BONESET_AMOUNT_OFFSET) as *const i16) };
    if bone_count <= 0 || bone_count > 512 {
        return Vec::new();
    }

    let mut bones = Vec::with_capacity(bone_count as usize);

    for i in 0..bone_count as usize {
        let bone = unsafe { p_bones.add(i * BONE_SIZE) };
        let x = unsafe { *(bone.add(BONE_POS_X) as *const f32) };
        let y = unsafe { *(bone.add(BONE_POS_Y) as *const f32) };
        let z = unsafe { *(bone.add(BONE_POS_Z) as *const f32) };
        let idx = unsafe { *(bone.add(BONE_INDEX) as *const i16) as u16 };

        bones.push(BonePos {
            ptr: bone as usize,
            index: idx,
            x,
            y,
            z,
        });
    }

    bones
}

/// Индекс кости головы (стабилен для модели игрока).
const HEAD_BONE_INDEX: u16 = 5;

/// Находит индекс кости-головы в массиве `bones` по `m_nBoneIndex == 5`.
pub fn find_head_bone(bones: &[BonePos]) -> Option<usize> {
    bones.iter().position(|b| b.index == HEAD_BONE_INDEX)
}

/// Строит список рёбер (пар индексов в `bones`) на основе `m_pParentBone`.
pub fn build_bone_edges(bones: &[BonePos]) -> Vec<(usize, usize)> {
    let mut edges = Vec::new();

    for (i, bone) in bones.iter().enumerate() {
        let parent_ptr = unsafe { *((bone.ptr as *const u8).add(BONE_PARENT) as *const usize) };
        if parent_ptr == 0 {
            continue;
        }
        if let Some(parent_idx) = bones.iter().position(|b| b.ptr == parent_ptr) {
            edges.push((parent_idx, i));
        }
    }

    edges
}

/// Рисует скелет как 2D-оверлей: линии между связанными костями + точки + номера.
pub fn draw_skeleton_overlay(ui: &Ui, bones: &[BonePos], camera_ptr: *const u8, color: u32) {
    if bones.is_empty() || camera_ptr.is_null() {
        return;
    }

    let view_proj = unsafe { *(camera_ptr.add(0x200) as *const [f32; 16]) };
    let cam_x = unsafe { *(camera_ptr.add(0x1B0) as *const f32) };
    let cam_y = unsafe { *(camera_ptr.add(0x1B4) as *const f32) };
    let cam_z = unsafe { *(camera_ptr.add(0x1B8) as *const f32) };
    let [sw, sh] = ui.io().display_size;

    let projections: Vec<Option<([f32; 2], f32)>> = bones
        .iter()
        .map(|b| world_to_screen((b.x, b.y, b.z), &view_proj, [0.0, 0.0, sw, sh], (cam_x, cam_y, cam_z)))
        .collect();

    let draw_list = ui.get_foreground_draw_list();

    let edges = build_bone_edges(bones);
    for (parent_idx, child_idx) in &edges {
        let p = &projections[*parent_idx];
        let c = &projections[*child_idx];
        if let (Some(([px, py], _)), Some(([cx, cy], _))) = (p, c) {
            let on_screen =
                |x: f32, y: f32| x >= -50.0 && x <= sw + 50.0 && y >= -50.0 && y <= sh + 50.0;
            if on_screen(*px, *py) || on_screen(*cx, *cy) {
                let clamp_x = |v: f32| v.clamp(-50.0, sw + 50.0);
                let clamp_y = |v: f32| v.clamp(-50.0, sh + 50.0);
                draw_list
                    .add_line(
                        [clamp_x(*px), clamp_y(*py)],
                        [clamp_x(*cx), clamp_y(*cy)],
                        color,
                    )
                    .thickness(1.5)
                    .build();
            }
        }
    }

    for (i, proj_opt) in projections.iter().enumerate() {
        if bones[i].index % 5 != 0 {
            continue;
        }
        if let Some(([sx, sy], _dist)) = proj_opt {
            let on_screen = *sx >= 0.0 && *sx <= sw && *sy >= 0.0 && *sy <= sh;
            let (dx, dy) = if on_screen {
                (*sx, *sy)
            } else {
                (sx.clamp(16.0, sw - 16.0), sy.clamp(16.0, sh - 16.0))
            };

            let r = if on_screen { 3.0 } else { 4.0 };
            draw_list
                .add_circle([dx, dy], r, color)
                .thickness(1.0)
                .build();

            draw_list.add_text([dx + 5.0, dy - 5.0], color, format!("#{}", bones[i].index));
        }
    }
}
