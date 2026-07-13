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
}

pub fn custom_weapon_name(id: i32) -> &'static str {
    match id {
        0 => "None",
        2 => "Polearm",
        3 => "Sai",
        4 => "Pincer",
        _ => "Unknown",
    }
}
