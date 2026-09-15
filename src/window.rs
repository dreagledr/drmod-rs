//! Окно игры: сделать маленьким и запомнить его прямоугольник.
//!
//! Зачем: в прогонах TAS окно игры мешает (занимает экран, крадёт фокус), а
//! каждый рестарт `launch_game.py` поднимает его в исходном месте. Поэтому мод
//! умеет (а) поставить окну нужные размер и позицию и (б) запомнить
//! прямоугольник, чтобы вернуть его при следующем запуске.
//!
//! Прямоугольник здесь — **внешний** (то, что видит ОС: `GetWindowRect` /
//! `SetWindowPos`, включая рамку и заголовок). Внутренняя область чуть меньше;
//! так проще: что запомнили — то и вернули, бит-в-бит.
//!
//! Запросы на изменение окна приходят из HTTP-потока, а выполняет их render-цикл
//! (поток игры): `SetWindowPos` синхронно заходит в `WndProc` игры, а движок не
//! потокобезопасен — то же правило, что у хуков отрисовки.
//!
//! HWND берём из памяти движка (`Hw::OSWindow`, `base + 0x19D504C` — SDK
//! `ref/mgr-plugin-sdk/game/Hw.h`) и проверяем `IsWindow`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetClassNameW, GetWindowRect, IsWindow, IsZoomed, SetWindowPos,
    SWP_NOACTIVATE, SWP_NOZORDER,
};

use crate::logger;

/// `Hw::OSWindow` — окно игры (SDK: `ref/mgr-plugin-sdk/game/Hw.h`).
const OS_WINDOW: usize = 0x19D504C;

/// Сколько держать прямоугольник неизменным, прежде чем запомнить (секунды):
/// во время перетаскивания окна `WM_MOVE` идёт потоком, писать файл на каждое
/// движение незачем.
const SAVE_SETTLE_SECS: f32 = 1.0;

/// Прямоугольник окна в экранных координатах (внешний: рамка + заголовок).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    /// Прямоугольник не вырожден и в разумных пределах — защита от мусора в
    /// памяти движка и от битого файла настроек.
    fn is_sane(self) -> bool {
        (64..=16384).contains(&self.w)
            && (48..=16384).contains(&self.h)
            && self.x.abs() <= 32768
            && self.y.abs() <= 32768
    }
}

/// Запрос из другого потока (HTTP/UI) — выполняет render-цикл.
#[derive(Clone, Copy, Debug)]
enum Request {
    Apply(Rect),
    Save,
    Forget,
}

/// Запомненный прямоугольник (то, что вернём при следующем запуске).
static SAVED: Mutex<Option<Rect>> = Mutex::new(None);
/// Последний прочитанный прямоугольник — чтобы `/state` не дёргал окно зря.
static CURRENT: Mutex<Option<Rect>> = Mutex::new(None);
/// Отложенный запрос на изменение окна.
static REQUEST: Mutex<Option<Request>> = Mutex::new(None);
/// Состояние автомата: вернули ли уже запомненное и когда окно последний раз
/// двигали.
static KEEPER: Mutex<Keeper> = Mutex::new(Keeper::new());

struct Keeper {
    /// Файл настроек прочитан (делаем это в render-цикле, а не в `DllMain`).
    loaded: bool,
    /// Диагностика окна (класс/корень/rect) уже записана.
    logged_hwnd: bool,
    /// Последнее сырое значение `Hw::OSWindow` — чтобы не спамить, когда окна нет.
    last_raw: usize,
    restored: bool,
    last: Option<Rect>,
    changed_at: Option<Instant>,
}

impl Keeper {
    const fn new() -> Self {
        Self {
            loaded: false,
            logged_hwnd: false,
            last_raw: usize::MAX,
            restored: false,
            last: None,
            changed_at: None,
        }
    }
}

/// HWND окна игры из памяти движка, проверенный `IsWindow`.
pub(crate) fn hwnd(base_addr: usize) -> Option<HWND> {
    if base_addr == 0 {
        return None;
    }
    let raw = unsafe { *((base_addr + OS_WINDOW) as *const *mut core::ffi::c_void) };
    if raw.is_null() {
        return None;
    }
    let hwnd = HWND(raw);
    if unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        Some(hwnd)
    } else {
        None
    }
}

/// Сырое значение указателя из `Hw::OSWindow` (до проверок) — для диагностики.
fn raw_hwnd(base_addr: usize) -> usize {
    if base_addr == 0 {
        return 0;
    }
    unsafe { *((base_addr + OS_WINDOW) as *const *mut core::ffi::c_void) as usize }
}

/// Класс окна: видно, main это окно игры, дочернее или чужое.
fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

/// Корень иерархии окна (`GA_ROOT`): если main-окно — не корень, значит мод
/// смотрит на дочернее окно, а не на окно игры.
fn root_of(hwnd: HWND) -> Option<HWND> {
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    (!root.0.is_null()).then_some(root)
}

/// Текущий внешний прямоугольник окна; `None` — окна нет или оно неразумного
/// размера.
fn rect_of(hwnd: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
    let rect = Rect {
        x: r.left,
        y: r.top,
        w: r.right - r.left,
        h: r.bottom - r.top,
    };
    rect.is_sane().then_some(rect)
}

/// Ставит окну внешний прямоугольник (без активации и смены z-порядка).
fn apply(hwnd: HWND, rect: Rect) -> Result<(), String> {
    if !rect.is_sane() {
        return Err(format!("прямоугольник вне разумных пределов: {rect:?}"));
    }
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .map_err(|e| format!("SetWindowPos: {e}"))
}

/// Запомнить прямоугольник (из HTTP/UI) — выполнит render-цикл.
pub(crate) fn request_save() {
    *REQUEST.lock().unwrap() = Some(Request::Save);
}

/// Забыть запомненный прямоугольник (и стереть файл).
pub(crate) fn request_forget() {
    *REQUEST.lock().unwrap() = Some(Request::Forget);
}

/// Применить прямоугольник к окну (из HTTP/UI) — выполнит render-цикл.
pub(crate) fn request_apply(rect: Rect) {
    *REQUEST.lock().unwrap() = Some(Request::Apply(rect));
}

/// Снимок для `/state` и ответа `POST /window`: `(текущий, запомненный)`.
pub(crate) fn state() -> (Option<Rect>, Option<Rect>) {
    (
        *CURRENT.lock().unwrap(),
        *SAVED.lock().unwrap(),
    )
}

/// Автоматика окна; зовётся каждый кадр из render-цикла (поток игры).
///
/// * при первом кадре возвращает запомненный прямоугольник (если он есть и
///   отличается от текущего);
/// * выполняет отложенный запрос (применить/запомнить/забыть);
/// * запоминает прямоугольник, когда окно перестали двигать
///   (`SAVE_SETTLE_SECS` секунд неизменности).
pub(crate) fn service(base_addr: usize) {
    // Файл настроек читаем на первом кадре (в `DllMain` — не стоит: там loader lock).
    let first_frame = {
        let mut k = KEEPER.lock().unwrap();
        let first = !k.loaded;
        k.loaded = true;
        first
    };
    if first_frame {
        load_saved();
    }

    let Some(hwnd) = hwnd(base_addr) else {
        // Окно ещё не создано или указатель не сошёлся — молчим, но раз в
        // кадр не спамим: пишем только когда состояние меняется (см. KEEPER).
        let raw = raw_hwnd(base_addr);
        let mut k = KEEPER.lock().unwrap();
        if k.last_raw != raw {
            k.last_raw = raw;
            drop(k);
            logger::log_line(&format!(
                "window: HWND из памяти движка = 0x{raw:08X} — окна нет (IsWindow=false)"
            ));
        }
        return;
    };
    let current = rect_of(hwnd);
    {
        let mut k = KEEPER.lock().unwrap();
        // Диагностика первого кадра с окном: сырой HWND, класс, корень и rect —
        // чтобы сразу видеть, то ли это окно (main игры / дочернее / чужое).
        if !k.logged_hwnd {
            k.logged_hwnd = true;
            let raw = raw_hwnd(base_addr);
            logger::log_line(&format!(
                "window: HWND=0x{:08X} (raw 0x{raw:08X}) class='{}' root=0x{:08X} rect={current:?}",
                hwnd.0 as usize,
                class_name(hwnd),
                root_of(hwnd).map_or(0, |r| r.0 as usize)
            ));
        }
    }
    *CURRENT.lock().unwrap() = current;
    let saved = *SAVED.lock().unwrap();

    // Отложенный запрос из HTTP-потока.
    let request = REQUEST.lock().unwrap().take();
    match request {
        Some(Request::Apply(rect)) => match apply(hwnd, rect) {
            Ok(()) => {
                logger::log_line(&format!("window: применён прямоугольник {rect:?}"));
                *SAVED.lock().unwrap() = Some(rect);
                save_to_disk(rect);
            }
            Err(e) => logger::log_line(&format!("window: применить не вышло: {e}")),
        },
        Some(Request::Save) => {
            if let Some(rect) = current {
                *SAVED.lock().unwrap() = Some(rect);
                save_to_disk(rect);
                logger::log_line(&format!("window: запомнен прямоугольник {rect:?}"));
            }
        }
        Some(Request::Forget) => {
            *SAVED.lock().unwrap() = None;
            forget_on_disk();
            logger::log_line("window: запомненный прямоугольник забыт");
        }
        None => {}
    }

    let mut keeper = KEEPER.lock().unwrap();
    // Возврат запомненного при первом кадре с окном.
    if !keeper.restored {
        keeper.restored = true;
        if let Some(rect) = saved
            && current != Some(rect)
        {
            match apply(hwnd, rect) {
                Ok(()) => logger::log_line(&format!(
                    "window: возвращён запомненный прямоугольник {rect:?} (было {current:?})"
                )),
                Err(e) => logger::log_line(&format!("window: вернуть не вышло: {e}")),
            }
        }
    }
    // Запоминание, когда окно успокоилось.
    if current != keeper.last {
        keeper.last = current;
        keeper.changed_at = Some(Instant::now());
        return;
    }
    let Some(changed_at) = keeper.changed_at else {
        return;
    };
    if changed_at.elapsed().as_secs_f32() < SAVE_SETTLE_SECS {
        return;
    }
    // Максимизированное окно не запоминаем: это не то положение, которое
    // захочется вернуть.
    if unsafe { IsZoomed(hwnd) }.as_bool() {
        return;
    }
    if let Some(rect) = current
        && Some(rect) != saved
    {
        *SAVED.lock().unwrap() = Some(rect);
        save_to_disk(rect);
        logger::log_line(&format!("window: запомнен прямоугольник {rect:?}"));
    }
    keeper.changed_at = None;
}

/// Путь файла с запомненным прямоугольником (`%LOCALAPPDATA%\drmod\window.json`).
fn saved_path() -> Option<PathBuf> {
    let dir = std::env::var("LOCALAPPDATA").ok()?;
    Some(PathBuf::from(format!("{dir}\\drmod\\window.json")))
}

fn save_to_disk(rect: Rect) {
    let Some(path) = saved_path() else {
        return;
    };
    if let Err(e) = save_to(&path, rect) {
        logger::log_line(&format!("window: не сохранить {}: {e}", path.display()));
    }
}

fn forget_on_disk() {
    let Some(path) = saved_path() else {
        return;
    };
    if let Err(e) = std::fs::remove_file(&path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        logger::log_line(&format!("window: не стереть {}: {e}", path.display()));
    }
}

/// Читает запомненный прямоугольник при старте мода.
pub(crate) fn load_saved() {
    let Some(path) = saved_path() else {
        return;
    };
    if let Some(rect) = load_from(&path) {
        logger::log_line(&format!("window: из файла прочитан {rect:?}"));
        *SAVED.lock().unwrap() = Some(rect);
    }
}

fn save_to(path: &Path, rect: Rect) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
    }
    let json = serde_json::to_string_pretty(&rect).map_err(|e| format!("to_string: {e}"))?;
    std::fs::write(path, json).map_err(|e| format!("write: {e}"))
}

fn load_from(path: &Path) -> Option<Rect> {
    let text = std::fs::read_to_string(path).ok()?;
    let rect: Rect = serde_json::from_str(&text).ok()?;
    rect.is_sane().then_some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("drmod_window_test_{name}.json"));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// Что запомнили — то и читаем: прямоугольник переживает «перезапуск».
    #[test]
    fn rect_round_trip() {
        let path = temp_path("round_trip");
        let rect = Rect {
            x: -1200,
            y: 40,
            w: 320,
            h: 200,
        };
        save_to(&path, rect).expect("запись");
        assert_eq!(load_from(&path), Some(rect));
        let _ = std::fs::remove_file(&path);
    }

    /// Битый или бессмысленный файл не применяем (иначе окно улетит за экран).
    #[test]
    fn broken_file_is_ignored() {
        let path = temp_path("broken");
        std::fs::write(&path, "{ это не json }").expect("запись");
        assert_eq!(load_from(&path), None);
        std::fs::write(&path, r#"{"x":0,"y":0,"w":1,"h":1}"#).expect("запись");
        assert_eq!(load_from(&path), None, "вырожденный прямоугольник");
        let _ = std::fs::remove_file(&path);
    }

    /// Без базы модуля окна не ищем (никаких чтений по нулевому адресу).
    #[test]
    fn hwnd_without_base_is_none() {
        assert!(hwnd(0).is_none());
    }
}
