//! Скип in-engine катсцены «как на консоли» — целиком внутри мода, без
//! внешних инструментов.
//!
//! Пока идёт сцена из [`WATCH`], держим в `Trigger::staFlags` пару флагов —
//! `STA_SOFT_EVENT` (код 4) и `STA_SOFT_EVENT_SKIP_OK` (код 37): от них Esc
//! игрока открывает не обычную паузу, а **консольное катсценное меню**
//! (`GameMenuStatus` = 6, `cEventPauseMenu`) с пунктами CONTINUE и SKIP.
//!
//! Собственный пункт Skip на PC инертен, а шаг 5 обработчика меню непроходим
//! (ждёт `[0x1DC203C] <= 0`, где лежит константа `1.0f`) — поэтому решение
//! выполняем мы, а меню даёт консольный UX:
//!
//! 1. подтверждение читаем по объекту меню (`base + 0x17EA140`): `+0x04 == -2`
//!    (машина меню приняла решение) и `+0x3C` — индекс пункта (0 = CONTINUE,
//!    1 = SKIP);
//! 2. убираем меню **штатным путём движка**: пишем `GameMenuStatus` = 6 и шаг
//!    `base + 0x17EA118` = 6 — движок сам уничтожает объект меню и ведёт статус
//!    `6 → 12 → 1`;
//! 3. снимаем `STA_PAUSE` (код 19): его ставит шаг 0 меню, и без снятия игра
//!    остаётся на паузе — вместе с ней стоит и машина загрузки подфазы;
//! 4. если подтверждён SKIP — заказываем [`NEXT`] штатной движковой
//!    `request_subphase` (`game::order_subphase` с `clear_event`), и движок
//!    реально выгружает текущую сцену и грузит заказанную.
//!
//! ⚠️ Заказ подфазы грузит сцену **только без паузы** — отсюда порядок 3 → 4.
//! ⚠️ Всё это кадровый автомат без блокирующих ожиданий: код исполняется внутри
//! кадра игры (render-цикл), поэтому «подождать» можно только следующими кадрами.
//! ⚠️ Пока мы держим `STA_SOFT_EVENT`, обычное меню паузы в этой сцене не
//! откроется — это плата за консольное поведение (как в консольной версии).
//!
//! Проверено живьём 2026-09-12 внешним инструментом с той же логикой:
//! `P370_IN` → `P370_EVENT` (кадр сменился на катсцену перед боем).

use std::sync::atomic::{AtomicU32, Ordering};

use crate::logger;

/// `Trigger::staFlags`: `word0` — коды 0..31, следующий dword — коды 32..63.
const STA_FLAGS: usize = 0x17EA060;
/// `GameMenuStatus` (1 = InGame, 6 = CutscenePause, 12 = Pause1).
const MENU_STATUS: usize = 0x17E9F9C;
/// Шаг жизненного цикла катсценного меню (0..6), живёт при статусе 6.
const MENU_STEP: usize = 0x17EA118;
/// Указатель на живой объект `cEventPauseMenu`.
const MENU_OBJ: usize = 0x17EA140;
/// Текущая подфаза: её хэш (объект состояния `+0x38`).
const SUB_HASH: usize = 0x14B9178;

/// Поля объекта `cEventPauseMenu` (`+0x00` — vtable, `+0x04` — состояние).
const OBJ_STATE: usize = 0x04;
const OBJ_CONFIRMED: usize = 0x3C;
/// Состояние машины меню, когда игрок подтвердил пункт.
const OBJ_STATE_DECIDED: i32 = -2;

/// Пункт «SKIP» в меню (0 — CONTINUE).
const ITEM_SKIP: i32 = 1;

const SOFT_EVENT: u32 = 0x0800_0000; // код 4 → пауза даёт катсценное меню
const SKIP_OK: u32 = 0x0400_0000; // код 37 (word1) → меню получает пункт Skip
const STA_PAUSE: u32 = 0x0000_1000; // код 19 → игра на паузе (стопорит загрузку)

const MENU_CUTSCENE: i32 = 6;
const MENU_PAUSE1: i32 = 12; // промежуточный статус закрытия меню
const MENU_STEP_CLOSE: u32 = 6;

/// Сколько кадров ждать, пока движок сам уничтожит меню (шаг 6), прежде чем
/// снять паузу принудительно (~5 с при 60 FPS), и карантин после скипа, чтобы
/// не поймать меню следующей сцены.
const CLOSE_TIMEOUT_FRAMES: u32 = 300;
const COOLDOWN_FRAMES: u32 = 120;

/// Сцена, в которой включаем консольное меню, и куда уводит скип.
/// Имена — как в сценарных данных; хэш считает [`sub_hash`].
const WATCH: [(&str, u32); 2] = [
    ("P370_RESTART", sub_hash("P370_RESTART")),
    ("P370_IN", sub_hash("P370_IN")),
];
const NEXT: &str = "P370_EVENT";

/// Этап для `GET /state` (сам автомат живёт в [`CutsceneSkip`]).
static STAGE: AtomicU32 = AtomicU32::new(STAGE_OFF);
const STAGE_OFF: u32 = 0;
const STAGE_ARMED: u32 = 1;
const STAGE_CLOSING: u32 = 2;
const STAGE_SKIPPED: u32 = 3;

/// Текущий этап скипа — короткая метка для `GET /state`.
pub(crate) fn status_name() -> &'static str {
    match STAGE.load(Ordering::Relaxed) {
        STAGE_ARMED => "armed",
        STAGE_CLOSING => "closing",
        STAGE_SKIPPED => "skipped",
        _ => "off",
    }
}

/// Хэш имени подфазы так, как его считает движок (`RVA 0xA03EA0`): CRC32 от
/// имени в нижнем регистре, старший бит отброшен.
const fn sub_hash(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0;
    while i < bytes.len() {
        crc ^= bytes[i].to_ascii_lowercase() as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        i += 1;
    }
    !crc & 0x7FFF_FFFF
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Off,
    /// Флаги выставлены, ждём подтверждения пункта в консольном меню.
    Armed,
    /// Меню убирается движком — ждём уничтожения объекта.
    Closing { item: i32, frames: u32 },
    /// Скип заказан — карантин, чтобы не поймать меню следующей сцены.
    Cooldown { frames: u32 },
}

/// Кадровый автомат скипа катсцены (см. документацию модуля).
pub(crate) struct CutsceneSkip {
    stage: Stage,
    /// Держим ли сейчас наши флаги в `staFlags`.
    flags_held: bool,
}

impl CutsceneSkip {
    pub(crate) fn new() -> Self {
        Self {
            stage: Stage::Off,
            flags_held: false,
        }
    }

    /// Текущее состояние человеческим языком — для окна настроек.
    pub(crate) fn status(&self) -> &'static str {
        match self.stage {
            Stage::Off => "",
            Stage::Armed => "сцена идёт: Esc → консольное меню, SKIP сработает",
            Stage::Closing { .. } => "меню подтверждено, убираю…",
            Stage::Cooldown { .. } => "скип заказан",
        }
    }

    /// Вызывается каждый кадр из render-цикла (поток игры: движок не
    /// потокобезопасен). `enabled` — галочка в настройках.
    pub(crate) fn update(&mut self, base_addr: usize, menu_status: i32, enabled: bool) {
        if base_addr == 0 {
            return;
        }
        if !enabled {
            if self.flags_held {
                Self::set_flags(base_addr, false);
                self.flags_held = false;
            }
            self.stage = Stage::Off;
            STAGE.store(STAGE_OFF, Ordering::Relaxed);
            return;
        }
        match self.stage {
            Stage::Off | Stage::Armed => self.idle(base_addr, menu_status),
            Stage::Closing { item, frames } => self.closing(base_addr, item, frames),
            Stage::Cooldown { frames } => {
                if frames > 0 {
                    self.stage = Stage::Cooldown { frames: frames - 1 };
                } else {
                    self.stage = Stage::Off;
                    STAGE.store(STAGE_OFF, Ordering::Relaxed);
                }
            }
        }
    }

    /// Мирная фаза: держим флаги, пока идёт сцена, и ловим решение в меню.
    fn idle(&mut self, base_addr: usize, menu_status: i32) {
        if !Self::in_scene(base_addr) {
            if self.flags_held {
                Self::set_flags(base_addr, false);
                self.flags_held = false;
                logger::log_line("cutscene_skip: сцена кончилась — флаги снял");
            }
            self.stage = Stage::Off;
            STAGE.store(STAGE_OFF, Ordering::Relaxed);
            return;
        }
        // Игра сама снимает SKIP_OK, когда пауза нажата без SOFT_EVENT, —
        // поэтому флаги не выставляем один раз, а поддерживаем.
        if !Self::flags_ready(base_addr) {
            Self::set_flags(base_addr, true);
            if !self.flags_held {
                logger::log_line(&format!(
                    "cutscene_skip: подфаза {} — консольное меню включено \
                     (SOFT_EVENT + SKIP_OK), Esc откроет PAUSE/SKIP",
                    Self::sub_name(base_addr)
                ));
            }
            self.flags_held = true;
        }
        self.stage = Stage::Armed;
        STAGE.store(STAGE_ARMED, Ordering::Relaxed);
        if menu_status != MENU_CUTSCENE {
            return;
        }
        let Some(item) = Self::decision(base_addr) else {
            return;
        };
        logger::log_line(&format!(
            "cutscene_skip: игрок подтвердил пункт {} — убираю меню движком",
            if item == ITEM_SKIP { "SKIP" } else { "CONTINUE" }
        ));
        // Статус 6 + шаг 6: движок сам уничтожает объект меню и ведёт статус
        // 6 → 12 → 1 (шаг 5 у него непроходим — см. документацию модуля).
        Self::write_u32(base_addr + MENU_STATUS, MENU_CUTSCENE as u32);
        Self::write_u32(base_addr + MENU_STEP, MENU_STEP_CLOSE);
        self.stage = Stage::Closing { item, frames: 1 };
        STAGE.store(STAGE_CLOSING, Ordering::Relaxed);
    }

    /// Ждём, пока движок уничтожит объект меню, затем снимаем паузу и (для
    /// SKIP) заказываем следующую подфазу.
    fn closing(&mut self, base_addr: usize, item: i32, frames: u32) {
        let closed = Self::menu_object(base_addr) == 0
            && !matches!(Self::menu_status_raw(base_addr), MENU_CUTSCENE | MENU_PAUSE1);
        if !closed && frames <= CLOSE_TIMEOUT_FRAMES {
            self.stage = Stage::Closing {
                item,
                frames: frames + 1,
            };
            return;
        }
        if closed {
            logger::log_line("cutscene_skip: меню убрано движком");
        } else {
            logger::log_line(&format!(
                "cutscene_skip: меню не убралось за {CLOSE_TIMEOUT_FRAMES} кадров \
                 — снимаю паузу как есть"
            ));
        }
        self.flags_held = false;
        Self::set_flags(base_addr, false);
        Self::clear_pause(base_addr);
        if item == ITEM_SKIP {
            let ok = crate::game::order_subphase(base_addr, NEXT, 1, true).is_some();
            logger::log_line(&format!(
                "cutscene_skip: заказ подфазы {NEXT} → {}",
                if ok { "вызвано" } else { "не удалось" }
            ));
            if ok {
                STAGE.store(STAGE_SKIPPED, Ordering::Relaxed);
            }
        } else {
            logger::log_line("cutscene_skip: CONTINUE — сцена возвращается, заказ не нужен");
        }
        self.stage = Stage::Cooldown {
            frames: COOLDOWN_FRAMES,
        };
    }

    /// Индекс подтверждённого пункта, если машина меню уже приняла решение.
    /// Обработчик ввода (`RVA 0x5A5930`) пишет состояние `-2` и индекс вместе,
    /// поэтому одного `+0x3C >= 0` мало — ждём и состояние.
    fn decision(base_addr: usize) -> Option<i32> {
        let obj = Self::menu_object(base_addr);
        if obj == 0 {
            return None;
        }
        let state = Self::read_i32(obj + OBJ_STATE);
        let confirmed = Self::read_i32(obj + OBJ_CONFIRMED);
        (state == OBJ_STATE_DECIDED && confirmed >= 0).then_some(confirmed)
    }

    /// Указатель на живой объект меню (0, если его нет или он не читаем).
    fn menu_object(base_addr: usize) -> usize {
        let ptr = Self::read_usize(base_addr + MENU_OBJ);
        if ptr != 0 && crate::game::is_readable_ptr(ptr) {
            ptr
        } else {
            0
        }
    }

    fn menu_status_raw(base_addr: usize) -> i32 {
        Self::read_i32(base_addr + MENU_STATUS)
    }

    fn in_scene(base_addr: usize) -> bool {
        let hash = Self::read_u32(base_addr + SUB_HASH);
        WATCH.iter().any(|(_, watched)| *watched == hash)
    }

    fn sub_name(base_addr: usize) -> &'static str {
        let hash = Self::read_u32(base_addr + SUB_HASH);
        WATCH
            .iter()
            .find(|(_, watched)| *watched == hash)
            .map(|(name, _)| *name)
            .unwrap_or("?")
    }

    fn flags_ready(base_addr: usize) -> bool {
        Self::read_u32(base_addr + STA_FLAGS) & SOFT_EVENT != 0
            && Self::read_u32(base_addr + STA_FLAGS + 4) & SKIP_OK != 0
    }

    /// Ставит/снимает наши флаги, не трогая остальные биты `staFlags`.
    fn set_flags(base_addr: usize, on: bool) {
        let (w0, w1) = (
            Self::read_u32(base_addr + STA_FLAGS),
            Self::read_u32(base_addr + STA_FLAGS + 4),
        );
        let (n0, n1) = if on {
            (w0 | SOFT_EVENT, w1 | SKIP_OK)
        } else {
            (w0 & !SOFT_EVENT, w1 & !SKIP_OK)
        };
        if n0 != w0 {
            Self::write_u32(base_addr + STA_FLAGS, n0);
        }
        if n1 != w1 {
            Self::write_u32(base_addr + STA_FLAGS + 4, n1);
        }
    }

    /// Снимает `STA_PAUSE` — иначе сцена и машина загрузки стоят.
    fn clear_pause(base_addr: usize) {
        let w0 = Self::read_u32(base_addr + STA_FLAGS);
        if w0 & STA_PAUSE != 0 {
            Self::write_u32(base_addr + STA_FLAGS, w0 & !STA_PAUSE);
            logger::log_line("cutscene_skip: снял STA_PAUSE — сцена снова идёт");
        }
    }

    fn read_u32(addr: usize) -> u32 {
        unsafe { *(addr as *const u32) }
    }

    fn read_usize(addr: usize) -> usize {
        unsafe { *(addr as *const usize) }
    }

    fn read_i32(addr: usize) -> i32 {
        unsafe { *(addr as *const i32) }
    }

    fn write_u32(addr: usize, value: u32) {
        unsafe { *(addr as *mut u32) = value };
    }
}

#[cfg(test)]
mod tests {
    use super::{sub_hash, CutsceneSkip, NEXT, WATCH};

    /// Хэши подфаз сверены с живыми чтениями объекта состояния игры.
    #[test]
    fn sub_hash_matches_game() {
        assert_eq!(sub_hash("P370_RESTART"), 0x3C9A_2F06);
        assert_eq!(sub_hash("P370_IN"), 0x6915_135D);
        assert_eq!(sub_hash("P370_EVENT"), 0x2540_A957);
        assert_eq!(sub_hash("P380_MONSOON"), 0x70AB_682C);
        // регистр не важен: движок хэширует имя в нижнем регистре
        assert_eq!(sub_hash("p370_in"), sub_hash("P370_IN"));
    }

    #[test]
    fn watch_holds_precomputed_hashes() {
        assert!(WATCH.iter().any(|(name, _)| *name == "P370_IN"));
        assert!(WATCH.iter().any(|(name, _)| *name == "P370_RESTART"));
        assert_eq!(NEXT, "P370_EVENT");
    }

    /// Без base_addr (мод ещё не нашёл модуль игры) автомат ничего не делает.
    #[test]
    fn idle_on_zero_base() {
        let mut skip = CutsceneSkip::new();
        skip.update(0, 6, true);
        assert_eq!(skip.status(), "");
    }
}
