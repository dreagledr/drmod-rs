//! Сущности игры MGR:R — игрок (Pl0000), камера (cCameraGame) и статус меню.
//! Каждая сущность инкапсулирует свои статические адреса и кэш указателей,
//! наружу отдаёт только методы чтения состояния.

mod camera;
mod phase;
mod player;

pub(crate) use camera::Camera;
pub(crate) use phase::{change_phase, hash_name, order_subphase};
pub(crate) use player::Player;

/// Проверяет, что адрес указывает на committed и читаемую память.
/// Защита от dangling-указателя объекта игрока при быстром рестарте:
/// `static_ptr` (`base+0x177B4A4`) может указывать на память, освобождённую
/// через `VirtualFree` (не `null`), не проходя через loading-состояние. В этом
/// случае `VirtualQuery` вернёт `Protect = 0` (MEM_FREE/MEM_RESERVE), и чтение
/// по такому адресу даёт ACCESS_VIOLATION.
pub(crate) fn is_readable_ptr(addr: usize) -> bool {
    use windows::Win32::System::Memory::{
        VirtualQuery, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
        PAGE_GUARD, PAGE_PROTECTION_FLAGS, PAGE_READONLY, PAGE_READWRITE,
    };

    const PAGE_READABLE: PAGE_PROTECTION_FLAGS = PAGE_PROTECTION_FLAGS(
        PAGE_READONLY.0 | PAGE_READWRITE.0 | PAGE_EXECUTE_READ.0 | PAGE_EXECUTE_READWRITE.0,
    );

    let mut mbi = MEMORY_BASIC_INFORMATION::default();
    let ok = unsafe {
        VirtualQuery(
            Some(addr as *const core::ffi::c_void),
            &mut mbi,
            std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    // PAGE_GUARD (0x100) — комбинированный флаг: чтение guard-страницы даёт
    // STATUS_GUARD_PAGE_VIOLATION, хотя бит «readable» в protect может стоять.
    // Исключаем (см. docs/REPLAY_FINDINGS.md №3).
    ok != 0
        && (mbi.Protect & PAGE_READABLE).0 != 0
        && (mbi.Protect & PAGE_GUARD).0 == 0
}

#[allow(dead_code)]
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameMenuStatus {
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
    pub fn name(self) -> &'static str {
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

    pub fn is_in_game(self) -> bool {
        self == Self::InGame
    }

    /// Состояния загрузки/переходов, в которых строка миссии (`gStr`) и
    /// анимация (`rAnim`) уничтожаются/не инициализированы. Их чтение в этот
    /// момент — access violation при рестарте.
    pub fn is_loading(self) -> bool {
        matches!(
            self,
            Self::MainMenuLoad
                | Self::LoadingIntoBossMission
                | Self::VRMissionListLoading
                | Self::LoadingIntoMission
        )
    }
}
