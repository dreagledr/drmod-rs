use hudhook::ImguiRenderLoop;
use imgui::*;
use std::ptr::NonNull;
use std::time::Instant;

pub const DEFAULT_TITLE: &str = "METAL GEAR RISING REVENGEANCE.exe";

#[allow(dead_code)]
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameMenuStatus {
    InMenu = 0,
    InGame = 1,
    ProcessPause = 2,
    PauseMenu = 3,
    Codec = 4,
    StateDisallowPause = 5,
    CutscenePause = 6,
    VRFail = 7,
    MissionFail = 8,
    SelectWeaponMenu = 9,
    GraphicsSettings = 10,
    AttackHelpScreen = 11,
    Pause1 = 12,
    MainMenuLoad = 13,
    LoadingIntoBossMission = 14,
    VRMissionListLoading = 15,
    None = 16,
    LoadingIntoMission = 17,
    ProcessOutOfPause = 18,
}

impl GameMenuStatus {
    fn name(self) -> &'static str {
        match self {
            Self::InMenu => "In Menu",
            Self::InGame => "In Game",
            Self::ProcessPause => "Process Pause",
            Self::PauseMenu => "Pause Menu",
            Self::Codec => "Codec",
            Self::StateDisallowPause => "State Disallow Pause",
            Self::CutscenePause => "Cutscene Pause",
            Self::VRFail => "VR Fail",
            Self::MissionFail => "Mission Fail",
            Self::SelectWeaponMenu => "Select Weapon Menu",
            Self::GraphicsSettings => "Graphics Settings",
            Self::AttackHelpScreen => "Attack Help Screen",
            Self::Pause1 => "Pause 1",
            Self::MainMenuLoad => "Main Menu Load",
            Self::LoadingIntoBossMission => "Loading Into Boss Mission",
            Self::VRMissionListLoading => "VR Mission List Loading",
            Self::None => "NONE",
            Self::LoadingIntoMission => "Loading Into Mission",
            Self::ProcessOutOfPause => "Process Out of Pause",
        }
    }

    fn is_in_game(self) -> bool {
        self == Self::InGame
    }
}

struct HelloHud {
    start_time: Instant,
    base_addr: usize,
    static_ptr_addr: Option<NonNull<u8>>,
    player_manager_addr: Option<NonNull<u8>>,
}

impl HelloHud {
    fn new() -> Self {
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

        Self {
            start_time: Instant::now(),
            base_addr,
            static_ptr_addr,
            player_manager_addr,
        }
    }
}

unsafe impl Send for HelloHud {}
unsafe impl Sync for HelloHud {}

impl ImguiRenderLoop for HelloHud {
    fn render(&mut self, ui: &mut Ui) {
        ui.window("##hello")
            .size([320., 600.], Condition::Always)
            .build(|| {
                ui.text(format!("Elapsed: {:?}", self.start_time.elapsed()));

                // --- GAME MENU STATUS ---
                if self.base_addr != 0 {
                    let menu_status_addr = self.base_addr + 0x17E9F9C;
                    let raw_status = unsafe { *(menu_status_addr as *const i32) };
                    let menu_status = if (0..=18).contains(&raw_status) {
                        // SAFETY: GameMenuStatus is repr(i32) с значениями 0..=18
                        Some(unsafe { std::mem::transmute::<i32, GameMenuStatus>(raw_status) })
                    } else {
                        None
                    };

                    if let Some(status) = menu_status {
                        let color = if status.is_in_game() {
                            [0.0, 1.0, 0.0, 1.0]
                        } else {
                            [1.0, 1.0, 0.0, 1.0]
                        };
                        ui.text_colored(color, format!("Status: {}", status.name()));
                    } else {
                        ui.text_colored(
                            [1.0, 0.5, 0.0, 1.0],
                            format!("Status: Unknown ({})", raw_status),
                        );
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
                    ui.text(format!("Custom: {}", custom_weapon));
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

                if player_obj_ptr.is_null() {
                    ui.text_colored([1.0, 0.5, 0.0, 1.0], "Player object pointer is NULL");
                    ui.text("Убедитесь, что вы в игре (не в меню).");
                } else {
                    ui.text_colored([0.0, 1.0, 0.0, 1.0], "Player found!");

                    // 2. Читаем координаты из объекта игрока
                    // X: offset 0x50, Y: 0x54, Z: 0x58
                    let pos_x = unsafe { *(player_obj_ptr.add(0x50) as *const f32) };
                    let pos_y = unsafe { *(player_obj_ptr.add(0x54) as *const f32) };
                    let pos_z = unsafe { *(player_obj_ptr.add(0x58) as *const f32) };

                    ui.separator();
                    ui.text("Position:");
                    ui.text(format!("X: {:.3}", pos_x));
                    ui.text(format!("Y: {:.3}", pos_y));
                    ui.text(format!("Z: {:.3}", pos_z));

                    // Дополнительно: HP (offset 0x870)
                    let hp = unsafe { *(player_obj_ptr.add(0x870) as *const i32) };
                    ui.separator();
                    ui.text(format!("HP: {}", hp));
                }
            });
    }
}

hudhook::hudhook!(hudhook::hooks::dx9::ImguiDx9Hooks, HelloHud::new());
