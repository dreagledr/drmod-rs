//! Сущность игрока (Pl0000). Инкапсулирует статический указатель
//! (`base + 0x177B4A4`), кэш объекта игрока и методы чтения его состояния.
//! Наружу отдаёт только `read_*` методы — сырой указатель наружу не светится.

use std::ptr::NonNull;

use crate::segment;
use crate::skeleton::BonePos;
use crate::tas::addresses;
use crate::tas::types;

use super::is_readable_ptr;

/// Враг (сущность Em*/Ba*/Pl001*) из EntitySystem для debug-панели: позиция,
/// HP, анимация, дистанция до игрока, высота клинка (мировая, из матрицы части).
#[cfg(debug_assertions)]
pub(crate) struct EnemyInfo {
    pub name: String,
    pub pos: [f32; 3],
    pub hp: i32,
    pub r_anim: i32,
    pub dist: Option<f32>,
    pub blade_y: Option<f32>,
}

pub(crate) struct Player {
    /// Базовый адрес приложения (GetModuleHandleA(null)). 0 — модуль не найден.
    /// Используется только в debug-функции `read_enemies` (EntitySystem) —
    /// в release поле не читается.
    #[allow(dead_code)]
    base_addr: usize,
    /// Статический указатель на объект игрока: `base + 0x177B4A4`.
    static_ptr_addr: Option<NonNull<u8>>,
    /// Кэш объекта игрока (разыменованный static_ptr). Обновляется в `refresh`.
    cached_player_obj_ptr: *mut u8,
}

impl Player {
    /// Вычисляет статический адрес объекта игрока относительно базового адреса
    /// приложения. `base_addr == 0` — модуль не найден, сущность неактивна.
    pub(crate) fn new(base_addr: usize) -> Self {
        let static_ptr_addr = if base_addr == 0 {
            None
        } else {
            NonNull::new(unsafe { (base_addr as *mut u8).add(0x177B4A4) })
        };
        Self {
            base_addr,
            static_ptr_addr,
            cached_player_obj_ptr: std::ptr::null_mut(),
        }
    }

    /// Обновляет кэш объекта игрока. Вызывается каждый кадр из `read_game_state`.
    /// В loading игра обнуляет static_ptr → кэш = null. Защита от dangling:
    /// при быстром рестарте static_ptr может указывать на освобождённую память
    /// (не null), не проходя через loading-состояние — обнуляем кэш, если
    /// страница игрока больше не committed/читаема.
    pub(crate) fn refresh(&mut self, player_readable: bool) {
        self.cached_player_obj_ptr = if let Some(static_ptr) = self.static_ptr_addr {
            if player_readable {
                unsafe { *(static_ptr.as_ptr() as *const *mut u8) }
            } else {
                std::ptr::null_mut()
            }
        } else {
            std::ptr::null_mut()
        };
        if !self.cached_player_obj_ptr.is_null()
            && !is_readable_ptr(self.cached_player_obj_ptr as usize)
        {
            self.cached_player_obj_ptr = std::ptr::null_mut();
        }
    }

    /// Есть ли валидный объект игрока (кэш не null).
    pub(crate) fn is_found(&self) -> bool {
        !self.cached_player_obj_ptr.is_null()
    }

    /// Адрес объекта игрока как число (для debug-лога). 0 — объект не найден.
    pub(crate) fn obj_ptr(&self) -> usize {
        self.cached_player_obj_ptr as usize
    }

    /// Позиция игрока (Pl0000 + 0x50/0x54/0x58), если объект найден.
    pub(crate) fn position(&self) -> Option<segment::Vec3> {
        if self.cached_player_obj_ptr.is_null() {
            return None;
        }
        let p = self.cached_player_obj_ptr;
        Some(unsafe {
            segment::Vec3 {
                x: *(p.add(0x50) as *const f32),
                y: *(p.add(0x54) as *const f32),
                z: *(p.add(0x58) as *const f32),
            }
        })
    }

    /// Текущая анимация игрока (Pl0000 + 0x618), если объект найден.
    pub(crate) fn r_anim(&self) -> i32 {
        if self.cached_player_obj_ptr.is_null() {
            return 0;
        }
        unsafe { *(self.cached_player_obj_ptr.add(0x618) as *const i32) }
    }

    /// Предыдущая позиция игрока (Pl0000 + 0x900, лаг ~1 кадр) — для расчёта
    /// скорости перемещения за кадр. `None`, если объект не найден.
    pub(crate) fn prev_position(&self) -> Option<[f32; 3]> {
        if self.cached_player_obj_ptr.is_null() {
            return None;
        }
        let p = self.cached_player_obj_ptr;
        Some(unsafe {
            [
                *(p.add(0x900) as *const f32),
                *(p.add(0x904) as *const f32),
                *(p.add(0x908) as *const f32),
            ]
        })
    }

    /// Сдвигает игрока по Y на `dy` (debug: NumPad1). No-op, если объект не найден.
    pub(crate) fn add_y(&self, dy: f32) {
        if self.cached_player_obj_ptr.is_null() {
            return;
        }
        unsafe {
            *(self.cached_player_obj_ptr.add(0x54) as *mut f32) += dy;
        }
    }

    /// Телепортирует игрока в позицию (debug: NumPad3). No-op, если объект не найден.
    pub(crate) fn set_position(&self, pos: (f32, f32, f32)) {
        if self.cached_player_obj_ptr.is_null() {
            return;
        }
        let p = self.cached_player_obj_ptr;
        unsafe {
            *(p.add(0x50) as *mut f32) = pos.0;
            *(p.add(0x54) as *mut f32) = pos.1;
            *(p.add(0x58) as *mut f32) = pos.2;
        }
    }

    /// Читает нормализованный ввод игрока (Pl0000::m_CurrentInput).
    pub(crate) fn read_current_input(&self) -> types::InputUnit {
        if self.cached_player_obj_ptr.is_null() {
            return types::InputUnit::default();
        }
        unsafe {
            self.cached_player_obj_ptr
                .add(addresses::CURRENT_INPUT_OFFSET)
                .cast::<types::InputUnit>()
                .read()
        }
    }

    /// Читает направление ввода и кнопку прыжка игрока (Pl0000) по
    /// подтверждённым SDK-смещениям.
    pub(crate) fn read_pl_input(&self) -> types::PlInputSnapshot {
        if self.cached_player_obj_ptr.is_null() {
            return types::PlInputSnapshot::default();
        }
        let p = self.cached_player_obj_ptr;
        unsafe {
            types::PlInputSnapshot {
                input_direction: *(p.add(addresses::PL_INPUT_DIR) as *const f32),
                button_jump: *(p.add(addresses::PL_BUTTON_JUMP) as *const i32),
            }
        }
    }

    /// Читает полное состояние персонажа (позиция/поворот/скорость/HP/состояния)
    /// из `cached_player_obj_ptr`. Смещения из SDK — см. `types::PlayerState`.
    /// Новые смещения 0x90/0x890/0x3184/0x40C8 требуют рантайм-верификации.
    pub(crate) fn read_player_state(&self) -> Option<types::PlayerState> {
        if self.cached_player_obj_ptr.is_null() {
            return None;
        }
        let p = self.cached_player_obj_ptr;
        Some(unsafe {
            types::PlayerState {
                pos: [
                    *(p.add(0x50) as *const f32),
                    *(p.add(0x54) as *const f32),
                    *(p.add(0x58) as *const f32),
                ],
                rotation: [
                    *(p.add(0x90) as *const f32),
                    *(p.add(0x94) as *const f32),
                    *(p.add(0x98) as *const f32),
                ],
                velocity: [
                    *(p.add(0x890) as *const f32),
                    *(p.add(0x894) as *const f32),
                    *(p.add(0x898) as *const f32),
                ],
                hp: *(p.add(0x870) as *const i32),
                r_anim: *(p.add(0x618) as *const i32),
                sword_state: *(p.add(0x13FC) as *const i32),
                sword_hidden: *(p.add(0xB74) as *const i32),
                input_direction: *(p.add(0xD2C) as *const f32),
                desired_heading: *(p.add(0xD30) as *const f32),
                button_jump: *(p.add(0xE18) as *const i32),
                button_light_attack: *(p.add(0xE20) as *const i32),
                button_heavy_attack: *(p.add(0xE24) as *const i32),
                button_ninjarun: *(p.add(0xE48) as *const i32),
                button_blademode: *(p.add(0xE50) as *const i32),
                ripper_enabled: *(p.add(0x3184) as *const i32),
                blade_mode_type: *(p.add(0x40C8) as *const i32),
            }
        })
    }

    /// Читает скелет игрока (для multiplayer-отправки). Возвращает пустой
    /// вектор, если объект игрока не найден.
    pub(crate) fn read_skeleton(&self) -> Vec<BonePos> {
        if self.cached_player_obj_ptr.is_null() {
            return Vec::new();
        }
        unsafe { crate::skeleton::read_full_skeleton(self.cached_player_obj_ptr) }
    }

    /// Читает врагов (сущности Em*/Ba*/Pl001*) из EntitySystem::m_EntityList.
    /// Адреса и смещения — из `ref/mgr-plugin-sdk` + дизассемблирование:
    /// `EntitySystem::ms_Instance` = base + 0x17E9A98, список `m_EntityList`
    /// (Hw::cFixedList<Entity*>) на +0x38 (size +0x0C, m_pFirst +0x14, узел:
    /// value/prev/next); Entity: имя +0x04, Behavior* (m_pSceneModel) +0x3C
    /// (подтверждено disasm `Entity::getTransPos` 0x67C8B0: `mov eax,[ecx+0x3C]`,
    /// `add eax,0x50`). У Behavior: позиция +0x50 (cParts::m_vecTransPos),
    /// HP +0x870, r_anim +0x618 — та же иерархия, что у игрока.
    /// Высота клинка врага: сущность Em0010_Blade → владелец (+0x518) Em0160Body →
    /// +0x360 → EmSetCorps; мировая Y клинка — из матрицы cParts (+0x10 → m[3].y = +0x44).
    /// Возвращает (всего сущностей, враги). Только для debug-панели.
    #[cfg(debug_assertions)]
    pub(crate) fn read_enemies(&self) -> (usize, Vec<EnemyInfo>) {
        let mut out = Vec::new();
        let mut enemy_beh: Vec<(*mut u8, usize)> = Vec::new();
        let mut blades: Vec<(*mut u8, f32)> = Vec::new();
        let base_addr = self.base_addr;
        if base_addr == 0 {
            return (0, out);
        }
        let player_pos = if self.cached_player_obj_ptr.is_null() {
            None
        } else {
            let p = self.cached_player_obj_ptr;
            Some(unsafe {
                [
                    *(p.add(0x50) as *const f32),
                    *(p.add(0x54) as *const f32),
                    *(p.add(0x58) as *const f32),
                ]
            })
        };
        let list = (base_addr + 0x17E9A98 + 0x38) as *const u8;
        if !is_readable_ptr(list as usize) {
            return (0, out);
        }
        let total = unsafe { *(list.add(0x0C) as *const usize) };
        let first = unsafe { *(list.add(0x14) as *const *mut u8) };
        let mut node = first;
        for _ in 0..total.min(256) {
            if node.is_null() || !is_readable_ptr(node as usize) {
                break;
            }
            let ent = unsafe { *(node as *const *mut u8) };
            if !ent.is_null() && is_readable_ptr(ent as usize) {
                let name = unsafe { std::ffi::CStr::from_ptr(ent.add(0x04) as *const i8) }
                    .to_string_lossy()
                    .into_owned();
                let behavior = unsafe { *(ent.add(0x3C) as *const *mut u8) };
                // Клинок врага (часть): мировая высота из матрицы, владелец
                // (Body) — для сопоставления с врагом после цикла.
                if name == "Em0010_Blade"
                    && !behavior.is_null()
                    && is_readable_ptr(behavior as usize)
                {
                    unsafe {
                        let owner = *(behavior.add(0x518) as *const *mut u8);
                        let world_y = *(behavior.add(0x44) as *const f32);
                        if !owner.is_null() && is_readable_ptr(owner as usize) {
                            blades.push((owner, world_y));
                        }
                    }
                }
                // Игрок (Pl0010/Pl0000 в зависимости от сцены) — не враг.
                let is_player = !behavior.is_null() && behavior == self.cached_player_obj_ptr;
                // Кандидат во враги: Em*/Ba*/Pl001* (включая Em0010_Assault —
                // подчёркивание в имени не всегда часть модели).
                let is_enemy_candidate = !is_player
                    && !name.is_empty()
                    && (name.starts_with("Em")
                        || name.starts_with("Ba")
                        || name.starts_with("Pl001"));
                if is_enemy_candidate && !behavior.is_null() && is_readable_ptr(behavior as usize) {
                    unsafe {
                        let pos = [
                            *(behavior.add(0x50) as *const f32),
                            *(behavior.add(0x54) as *const f32),
                            *(behavior.add(0x58) as *const f32),
                        ];
                        let hp = *(behavior.add(0x870) as *const i32);
                        // r_anim — текущая анимация (как у игрока); у врагов
                        // слот анимации (+0x770) не работает — значения из +0x618.
                        let r_anim = *(behavior.add(0x618) as *const i32);
                        // Настоящий враг: реальная позиция в сцене (не спавн
                        // (0,0,0)) и живое HP в разумных пределах (не мусор).
                        let pos_nonzero = pos[0] != 0.0 || pos[1] != 0.0 || pos[2] != 0.0;
                        if pos_nonzero && hp > 0 && hp < 1_000_000 {
                            let dist = player_pos.map(|pp| {
                                ((pos[0] - pp[0]).powi(2)
                                    + (pos[1] - pp[1]).powi(2)
                                    + (pos[2] - pp[2]).powi(2))
                                .sqrt()
                            });
                            let idx = out.len();
                            out.push(EnemyInfo {
                                name,
                                pos,
                                hp,
                                r_anim,
                                dist,
                                blade_y: None,
                            });
                            enemy_beh.push((behavior, idx));
                        }
                    }
                }
            }
            node = unsafe { *(node.add(0x08) as *const *mut u8) };
        }
        // Сопоставление клинков с врагами: Blade.owner (Em0160Body) +0x360 → враг.
        for (body_beh, y) in &blades {
            if is_readable_ptr(*body_beh as usize) {
                unsafe {
                    let enemy = *(body_beh.add(0x360) as *const *mut u8);
                    if let Some((_, idx)) = enemy_beh.iter().find(|(b, _)| *b == enemy) {
                        out[*idx].blade_y = Some(*y);
                    }
                }
            }
        }
        (total, out)
    }
}
