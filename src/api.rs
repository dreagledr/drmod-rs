//! HTTP API автоматизации: скрипты ввода, состояние игры, кольцевой буфер логов.
//!
//! Сервер слушает `127.0.0.1:5223` (собственный минимальный HTTP-сервер на
//! `TcpListener`) в отдельном потоке. Render-цикл продвигает активный скрипт
//! (подача ввода через `replay::set_input_override` и keybind-эмуляцию
//! ripper/blade), пишет кадры в кольцевой буфер и обновляет снимок `/state`.
//! HTTP-поток только читает `SharedState` (Arc<Mutex>).
//!
//! Собственный сервер вместо tiny_http: tiny_http не выставляет таймауты на
//! сокетах (клиент может заблокировать поток навсегда в `respond`/чтении тела)
//! и порождает неуправляемые внутренние потоки (accept + TaskPool + на каждое
//! соединение), которые остаются живыми на выгруженном коде DLL при eject.
//! Здесь один поток, неблокирующий accept с опросом stop-флага и таймауты
//! read/write на каждом соединении — `shutdown()` гарантированно завершает
//! поток за ограниченное время.
//!
//! Дизайн — `docs/API.md`.

use crate::logger;
use crate::tas::addresses;
use crate::tas::hooks;
use crate::tas::replay;
use crate::tas::types::{InputOverride, InputUnit, PlayerState};
use crate::ui::UiState;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Адрес и порт HTTP API.
const BIND_ADDR: &str = "127.0.0.1:5223";
/// Максимальная длительность скрипта в кадрах (60 сек при 60 FPS).
const MAX_SCRIPT_FRAMES: u32 = 3600;
/// Ёмкость кольцевого буфера логов (60 FPS × 60 сек).
const RING_CAPACITY: usize = 3600;
/// Лимит тела запроса (защита от гигантских скриптов).
const MAX_BODY_BYTES: u64 = 64 * 1024;
/// Максимум кадров в ответе /logs.
const MAX_LOG_LIMIT: usize = 5000;
/// Таймаут чтения/записи на клиентском сокете: клиент, который не читает
/// ответ или не досылает тело, не может заблокировать HTTP-поток навсегда
/// (иначе join в shutdown() зависнет на выгрузке DLL).
const IO_TIMEOUT: Duration = Duration::from_secs(1);
/// Пауза accept-цикла при отсутствии соединений (неблокирующий listener) —
/// shutdown() будит поток максимум за это время.
const ACCEPT_POLL_MS: u64 = 10;
/// Лимит заголовков запроса (защита от гигантских заголовков).
const MAX_HEADER_BYTES: usize = 16 * 1024;

/// Встроенный скрипт NumPad4: бег ~1 сек → прыжок на бегу → лёгкий удар →
/// поворот камеры. Ровно текущее поведение `update_input_injection`.
/// В release используется только через HTTP (POST /script/run) — отсюда allow.
#[allow(dead_code)]
const BUILTIN_SCRIPT: &str = r#"{
  "name": "run-jump-attack",
  "commands": [
    { "t": 0,  "duration": 60, "input": { "forward": true } },
    { "t": 45, "duration": 2,  "input": { "jump": true } },
    { "t": 85, "duration": 2,  "input": { "light_attack": true } },
    { "t": 95, "duration": 35, "input": { "camera": [300, 0] } }
  ]
}"#;

/// Статус скрипта.
#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ScriptStatus {
    Running,
    Done,
    Stopped,
}

/// Вход одной команды скрипта (JSON-объект `input`). Все поля опциональны;
/// неизвестные ключи — ошибка (защита от опечаток LLM).
///
/// Семантика (см. docs/API.md §4.2):
/// - движение (`forward`/`backward`/`left`/`right`) — биты InputUnit + left_stick;
/// - hold-действия (`ninja_run`/`walk`/`dodge`/`blade`) — удержание keybind'а
///   на все кадры команды (isKeybindDown);
/// - pressed-действия (`ripper`/`lock_on`/`subweapon`/`item`/`ar_mode`/
///   `weapon_select`/`codec`/`pause`/`camera_reset`/`zandatsu`) — фронт keybind'а
///   на первом кадре команды (isKeybindPressed), `duration` игнорируется;
/// - меню-клавиши (`confirm`/`menu_up`/`menu_down`/`menu_left`/`menu_right`) —
///   сырые клавиши в кэш `ms_KeyInput` (меню читает их через isKeyDown/
///   isKeyPressed, а не через keybind'ы), фронт на первом кадре.
#[derive(Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScriptInput {
    #[serde(default)]
    forward: bool,
    #[serde(default)]
    backward: bool,
    #[serde(default)]
    left: bool,
    #[serde(default)]
    right: bool,
    #[serde(default)]
    jump: bool,
    #[serde(default)]
    light_attack: bool,
    #[serde(default)]
    heavy_attack: bool,
    #[serde(default)]
    camera: Option<[f32; 2]>,
    #[serde(default)]
    ripper: bool,
    #[serde(default)]
    blade: bool,
    #[serde(default)]
    ninja_run: bool,
    #[serde(default)]
    walk: bool,
    #[serde(default)]
    dodge: bool,
    #[serde(default)]
    lock_on: bool,
    #[serde(default)]
    subweapon: bool,
    #[serde(default)]
    item: bool,
    #[serde(default)]
    ar_mode: bool,
    #[serde(default)]
    weapon_select: bool,
    #[serde(default)]
    codec: bool,
    #[serde(default)]
    zandatsu: bool,
    #[serde(default)]
    camera_reset: bool,
    #[serde(default)]
    pause: bool,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    menu_up: bool,
    #[serde(default)]
    menu_down: bool,
    #[serde(default)]
    menu_left: bool,
    #[serde(default)]
    menu_right: bool,
    #[serde(default)]
    left_stick: Option<[f32; 2]>,
}

impl ScriptInput {
    /// Пустой ли вход (ни один ключ не задан).
    fn is_empty(&self) -> bool {
        !self.forward
            && !self.backward
            && !self.left
            && !self.right
            && !self.jump
            && !self.light_attack
            && !self.heavy_attack
            && !self.ripper
            && !self.blade
            && !self.ninja_run
            && !self.walk
            && !self.dodge
            && !self.lock_on
            && !self.subweapon
            && !self.item
            && !self.ar_mode
            && !self.weapon_select
            && !self.codec
            && !self.zandatsu
            && !self.camera_reset
            && !self.pause
            && !self.confirm
            && !self.menu_up
            && !self.menu_down
            && !self.menu_left
            && !self.menu_right
            && self.camera.is_none()
            && self.left_stick.is_none()
    }
}

/// Одна команда скрипта: входы активны с кадра `t` на `duration` кадров.
#[derive(Clone, Copy, Deserialize)]
struct ScriptCommand {
    t: u32,
    duration: u32,
    input: ScriptInput,
}

/// Тело `POST /script/run` (JSON).
#[derive(Deserialize)]
struct ScriptRequest {
    #[serde(default = "default_script_name")]
    name: String,
    commands: Vec<ScriptCommand>,
}

fn default_script_name() -> String {
    "script".to_string()
}

/// Активный (или последний) скрипт.
struct ScriptState {
    id: u32,
    name: String,
    commands: Vec<ScriptCommand>,
    frame: u32,
    status: ScriptStatus,
    total_frames: u32,
}

/// Один кадр кольцевого буфера (сырые данные, JSON-форма — `LogFrameJson`).
#[derive(Clone)]
struct LogFrame {
    t_ms: u64,
    frame: u32,
    script_id: Option<u32>,
    pos: [f32; 3],
    rot: [f32; 3],
    vel: [f32; 3],
    hp: i32,
    r_anim: i32,
    ripper: i32,
    blade: i32,
    input: InputUnit,
}

/// JSON-форма кадра лога: кнопки декодированы в имена (LLM-дружелюбно).
#[derive(Serialize)]
struct LogFrameJson {
    t_ms: u64,
    frame: u32,
    script_id: Option<u32>,
    pos: [f32; 3],
    rot: [f32; 3],
    vel: [f32; 3],
    hp: i32,
    r_anim: i32,
    ripper: i32,
    blade: i32,
    input: InputJson,
}

/// Декодированный ввод кадра лога.
#[derive(Serialize)]
struct InputJson {
    buttons: Vec<&'static str>,
    left_stick: [f32; 2],
    right_stick: [f32; 2],
}

/// Последний снимок состояния (обновляется в render, читается /state).
#[derive(Clone, Default)]
struct StateSnapshot {
    t_ms: u64,
    mission_id: i32,
    mission_name: String,
    menu_status: String,
    player_found: bool,
    pos: [f32; 3],
    rot: [f32; 3],
    vel: [f32; 3],
    hp: i32,
    r_anim: i32,
    ripper: i32,
    blade: i32,
}

/// `GET /health` — живость, версия, base_addr, uptime.
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    base_addr: String,
    uptime_ms: u64,
}

/// `GET /state` — снимок игрока.
#[derive(Serialize)]
struct PlayerSnapshot {
    found: bool,
    pos: [f32; 3],
    rot: [f32; 3],
    vel: [f32; 3],
    hp: i32,
    r_anim: i32,
    ripper: i32,
    blade: i32,
}

/// Статус скрипта (в `/state` и `/script/{id}`).
#[derive(Serialize)]
struct ScriptStatusJson {
    id: u32,
    name: String,
    status: ScriptStatus,
    frame: u32,
    total_frames: u32,
}

/// `GET /state` — текущий снимок игры + статус скрипта + fps.
#[derive(Serialize)]
struct StateResponse {
    t_ms: u64,
    mission_id: i32,
    mission_name: String,
    menu_status: String,
    player: PlayerSnapshot,
    script: Option<ScriptStatusJson>,
    fps: f32,
}

/// `POST /script/run` — ответ.
#[derive(Serialize)]
struct ScriptRunResponse {
    script_id: u32,
    name: String,
    total_frames: u32,
}

/// `POST /script/stop` — ответ.
#[derive(Serialize)]
struct ScriptStopResponse {
    stopped: bool,
    script_id: u32,
}

/// `POST /eject` — ответ. Сам eject выполняет render-цикл (см. `handle_eject`).
#[derive(Serialize)]
struct EjectResponse {
    ejecting: bool,
}

/// `GET /logs` — ответ.
#[derive(Serialize)]
struct LogsResponse {
    from_ms: u64,
    to_ms: u64,
    count: usize,
    frames: Vec<LogFrameJson>,
}

/// Ошибка: `{ "error": "..." }`.
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

/// Тело ответа — один из типизированных JSON-ответов (untagged: сериализуется
/// как содержимое варианта).
#[derive(Serialize)]
#[serde(untagged)]
enum Response {
    Health(HealthResponse),
    State(StateResponse),
    ScriptRun(ScriptRunResponse),
    ScriptStop(ScriptStopResponse),
    ScriptStatus(ScriptStatusJson),
    Logs(LogsResponse),
    Eject(EjectResponse),
    Error(ErrorResponse),
}

/// Кольцевой буфер кадров: при переполнении старые кадры вытесняются.
struct RingBuffer {
    frames: VecDeque<LogFrame>,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            frames: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, frame: LogFrame) {
        if self.frames.len() == self.capacity {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    fn query(
        &self,
        from_ms: u64,
        to_ms: u64,
        script_id: Option<u32>,
        limit: usize,
    ) -> Vec<LogFrame> {
        self.frames
            .iter()
            .filter(|f| f.t_ms >= from_ms && f.t_ms <= to_ms)
            .filter(|f| script_id.is_none_or(|id| f.script_id == Some(id)))
            .take(limit)
            .cloned()
            .collect()
    }
}

/// Общее состояние, разделяемое между render-потоком (пишет) и HTTP (читает).
struct SharedState {
    ring: RingBuffer,
    snapshot: StateSnapshot,
    script: Option<ScriptState>,
    start: Instant,
    frame_count: u32,
    fps: f32,
    base_addr: usize,
    next_script_id: u32,
    /// Запрос eject через `POST /eject`: HTTP-поток ставит флаг и успевает
    /// ответить, render-цикл проверяет его каждый кадр и выполняет
    /// `shutdown()` + `hudhook::eject()` (из HTTP-потока это невозможно —
    /// `shutdown()` джойнит сам себя).
    eject_requested: bool,
}

/// HTTP API-сервер. Владеет потоком; `Drop`/`shutdown` останавливает поток
/// (флаг + join) и снимает override ввода — обязателен перед eject DLL.
pub struct ApiServer {
    handle: Option<std::thread::JoinHandle<()>>,
    state: Arc<Mutex<SharedState>>,
    stop: Arc<AtomicBool>,
}

impl ApiServer {
    /// Запускает HTTP-сервер на `127.0.0.1:5223`. При неудачном bind сервер
    /// деградирует в no-op (логируется) — мод продолжает работать без API.
    pub fn new(base_addr: usize) -> Self {
        let state = Arc::new(Mutex::new(SharedState {
            ring: RingBuffer::new(RING_CAPACITY),
            snapshot: StateSnapshot::default(),
            script: None,
            start: Instant::now(),
            frame_count: 0,
            fps: 0.0,
            base_addr,
            next_script_id: 1,
            eject_requested: false,
        }));

        let stop = Arc::new(AtomicBool::new(false));
        let handle = match TcpListener::bind(BIND_ADDR) {
            Ok(listener) => {
                logger::log_line(&format!("api: http server on {}", BIND_ADDR));
                let state = Arc::clone(&state);
                let stop = Arc::clone(&stop);
                Some(
                    std::thread::Builder::new()
                        .name("drmod-api".into())
                        .spawn(move || api_thread(listener, state, stop))
                        .expect("api thread spawn"),
                )
            }
            Err(e) => {
                logger::log_line(&format!("api: http bind {} FAIL: {}", BIND_ADDR, e));
                None
            }
        };

        Self { handle, state, stop }
    }

    /// Покадровый апдейт из render: продвижение скрипта, запись кадра в буфер,
    /// обновление снимка. Вызывается каждый кадр, независимо от UI.
    pub fn frame_update(&self, ui_state: &UiState, input: InputUnit, state: PlayerState) {
        let mut guard = self.state.lock().unwrap();
        guard.frame_count += 1;
        let elapsed = guard.start.elapsed();
        let elapsed_ms = elapsed.as_millis() as u64;
        if elapsed.as_secs_f32() > 0.0 {
            guard.fps = guard.frame_count as f32 / elapsed.as_secs_f32();
        }

        // Авто-стоп при loading/меню: игрок не читаем — скрипт останавливается,
        // иначе залипший override сломает пересоздающегося игрока.
        if !ui_state.player_found
            && let Some(s) = guard.script.as_mut()
            && s.status == ScriptStatus::Running
        {
            logger::log_line(&format!(
                "api: script {} '{}' stopped (player not readable)",
                s.id, s.name
            ));
            stop_script(s);
        }

        // Продвижение активного скрипта: вычисляем InputUnit кадра и подаём
        // через override; по завершении снимаем override и keybind-эмуляцию.
        let script_id = if let Some(s) = guard.script.as_mut() {
            if s.status == ScriptStatus::Running {
                let ov = script_tick(s);
                replay::set_input_override(ov);
                if s.status == ScriptStatus::Done {
                    replay::set_input_override(InputOverride::default());
                    hooks::clear_keybind_emulation();
                    logger::log_line(&format!("api: script {} '{}' done", s.id, s.name));
                }
                Some(s.id)
            } else {
                None
            }
        } else {
            None
        };

        guard.snapshot = StateSnapshot {
            t_ms: elapsed_ms,
            mission_id: ui_state.mission_id,
            mission_name: ui_state.mission_name.clone(),
            menu_status: ui_state.menu_status.name().to_string(),
            player_found: ui_state.player_found,
            pos: state.pos,
            rot: state.rotation,
            vel: state.velocity,
            hp: state.hp,
            r_anim: state.r_anim,
            ripper: state.ripper_enabled,
            blade: state.blade_mode_type,
        };

        if ui_state.player_found {
            let frame_count = guard.frame_count;
            guard.ring.push(LogFrame {
                t_ms: elapsed_ms,
                frame: frame_count,
                script_id,
                pos: state.pos,
                rot: state.rotation,
                vel: state.velocity,
                hp: state.hp,
                r_anim: state.r_anim,
                ripper: state.ripper_enabled,
                blade: state.blade_mode_type,
                input,
            });
        }
    }

    /// Активен ли скрипт (используется, чтобы debug-инжекция/record/playback
    /// не вмешивались в ввод, которым управляет API-скрипт). В release
    /// вызывается только извне (HTTP) — отсюда allow.
    #[allow(dead_code)]
    pub fn is_script_running(&self) -> bool {
        self.state
            .lock()
            .unwrap()
            .script
            .as_ref()
            .is_some_and(|s| s.status == ScriptStatus::Running)
    }

    /// Запрошен ли eject через `POST /eject`. Проверяется в render-цикле
    /// каждый кадр; при `true` render выполняет `shutdown()` + `eject()`.
    pub fn eject_requested(&self) -> bool {
        self.state.lock().unwrap().eject_requested
    }

    /// Запрос eject из UI (кнопка «Выход / Выгрузить DLL»): ставит тот же
    /// флаг, что и `POST /eject` — render-цикл выполняет выгрузку в конце
    /// кадра. Единый путь для кнопки и HTTP.
    pub fn request_eject(&self) {
        self.state.lock().unwrap().eject_requested = true;
    }

    /// Запускает встроенный скрипт NumPad4 (run-jump-attack) — тот же механизм,
    /// что и `POST /script/run`. Игнорируется, если другой скрипт уже активен.
    /// В release вызывается только извне (HTTP) — отсюда allow.
    #[allow(dead_code)]
    pub fn start_builtin_script(&self) {
        let req = match parse_script(BUILTIN_SCRIPT) {
            Ok(v) => v,
            Err(e) => {
                logger::log_line(&format!("api: builtin script parse FAIL: {}", e));
                return;
            }
        };
        let mut guard = self.state.lock().unwrap();
        if guard
            .script
            .as_ref()
            .is_some_and(|s| s.status == ScriptStatus::Running)
        {
            logger::log_line("api: builtin script ignored (another script running)");
            return;
        }
        let id = guard.next_script_id;
        guard.next_script_id += 1;
        let total_frames = req.commands.iter().map(|c| c.t + c.duration).max().unwrap_or(0);
        hooks::clear_keybind_emulation();
        guard.script = Some(ScriptState {
            id,
            name: req.name.clone(),
            commands: req.commands,
            frame: 0,
            status: ScriptStatus::Running,
            total_frames,
        });
        logger::log_line(&format!(
            "api: builtin script {} '{}' started ({} frames)",
            id, req.name, total_frames
        ));
    }

    /// Останавливает HTTP-поток и снимает override ввода. Вызывается перед
    /// `hudhook::eject()` и в `Drop` — без этого поток останется висеть
    /// на выгруженном коде DLL, а игра — с залипшим вводом.
    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        replay::set_input_override(InputOverride::default());
        hooks::clear_keybind_emulation();
        if let Ok(mut guard) = self.state.lock()
            && let Some(s) = guard.script.as_mut()
            && s.status == ScriptStatus::Running
        {
            s.status = ScriptStatus::Stopped;
        }
        logger::log_line("api: http server stopped");
    }
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Цикл HTTP-потока: неблокирующий accept + опрос stop-флага. Поток выходит
/// максимум через ACCEPT_POLL_MS после shutdown (или после текущего
/// соединения, ограниченного IO_TIMEOUT) — join в shutdown() не виснет.
fn api_thread(listener: TcpListener, state: Arc<Mutex<SharedState>>, stop: Arc<AtomicBool>) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                // Таймауты на сокете: клиент, который не читает ответ или не
                // досылает тело, не может заблокировать поток навсегда.
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                handle_connection(stream, &state);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(ACCEPT_POLL_MS));
            }
            Err(_) => break,
        }
    }
}

/// Обслуживает одно соединение: читает заголовки и тело, маршрутизирует,
/// пишет ответ и закрывает соединение (Connection: close — без keep-alive,
/// висящих соединений не остаётся).
fn handle_connection(mut stream: TcpStream, state: &Arc<Mutex<SharedState>>) {
    // Читаем заголовки до \r\n\r\n (лимит MAX_HEADER_BYTES).
    let mut buf = [0u8; 2048];
    let mut head = Vec::new();
    let header_end = loop {
        if let Some(p) = head.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        if head.len() > MAX_HEADER_BYTES {
            respond(
                &mut stream,
                400,
                &Response::Error(ErrorResponse {
                    error: "headers too large".into(),
                }),
            );
            return;
        }
        match stream.read(&mut buf) {
            Ok(0) => return, // клиент закрыл соединение
            Ok(n) => head.extend_from_slice(&buf[..n]),
            Err(_) => return, // таймаут/ошибка — бросаем соединение
        }
    };

    // Первая строка: METHOD SP TARGET SP HTTP/x.y.
    let head_str = String::from_utf8_lossy(&head[..header_end]);
    let mut lines = head_str.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();

    // Content-Length и Transfer-Encoding из заголовков.
    let mut content_length = 0usize;
    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            let value = value.trim();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("transfer-encoding") {
                chunked = !value.eq_ignore_ascii_case("identity");
            }
        }
    }
    if chunked {
        respond(
            &mut stream,
            400,
            &Response::Error(ErrorResponse {
                error: "chunked transfer not supported".into(),
            }),
        );
        return;
    }
    if content_length > MAX_BODY_BYTES as usize {
        respond(
            &mut stream,
            413,
            &Response::Error(ErrorResponse {
                error: "body too large".into(),
            }),
        );
        return;
    }

    // Тело: остаток после заголовков (мог прийти в том же пакете) +
    // дозапись до Content-Length.
    let mut body_bytes = head[header_end..].to_vec();
    let mut remaining = content_length.saturating_sub(body_bytes.len());
    while remaining > 0 {
        match stream.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => {
                let take = n.min(remaining);
                body_bytes.extend_from_slice(&buf[..take]);
                remaining -= take;
            }
            Err(_) => return,
        }
    }
    let body = String::from_utf8_lossy(&body_bytes).into_owned();

    let (code, value) = route(&method, &target, &body, state);
    respond(&mut stream, code, &value);
}

/// Маршрутизация по (метод, путь). Возвращает (код, JSON-тело ответа).
fn route(
    method: &str,
    target: &str,
    body: &str,
    state: &Arc<Mutex<SharedState>>,
) -> (u16, Response) {
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    };
    match (method, path) {
        ("GET", "/health") => (200, Response::Health(health_json(state))),
        ("GET", "/state") => (200, Response::State(state_json(state))),
        ("POST", "/script/run") => handle_script_run(body, state),
        ("POST", "/script/stop") => handle_script_stop(state),
        ("POST", "/eject") => handle_eject(state),
        ("GET", path) if path.starts_with("/script/") => handle_script_get(path, state),
        ("GET", "/logs") => handle_logs(query, state),
        _ => (
            404,
            Response::Error(ErrorResponse {
                error: "not found".into(),
            }),
        ),
    }
}

/// `GET /health` — живость, версия, base_addr, uptime.
fn health_json(state: &Arc<Mutex<SharedState>>) -> HealthResponse {
    let guard = state.lock().unwrap();
    HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        base_addr: format!("0x{:08X}", guard.base_addr),
        uptime_ms: guard.start.elapsed().as_millis() as u64,
    }
}

/// `GET /state` — текущий снимок игры + статус скрипта + fps.
fn state_json(state: &Arc<Mutex<SharedState>>) -> StateResponse {
    let guard = state.lock().unwrap();
    let s = &guard.snapshot;
    let script = guard.script.as_ref().map(|sc| ScriptStatusJson {
        id: sc.id,
        name: sc.name.clone(),
        status: sc.status,
        frame: sc.frame,
        total_frames: sc.total_frames,
    });
    StateResponse {
        t_ms: s.t_ms,
        mission_id: s.mission_id,
        mission_name: s.mission_name.clone(),
        menu_status: s.menu_status.clone(),
        player: PlayerSnapshot {
            found: s.player_found,
            pos: s.pos,
            rot: s.rot,
            vel: s.vel,
            hp: s.hp,
            r_anim: s.r_anim,
            ripper: s.ripper,
            blade: s.blade,
        },
        script,
        fps: guard.fps,
    }
}

/// `POST /script/run` — запуск скрипта из JSON-тела.
fn handle_script_run(body: &str, state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    let req = match parse_script(body) {
        Ok(v) => v,
        Err(e) => return (400, Response::Error(ErrorResponse { error: e })),
    };
    let mut guard = state.lock().unwrap();
    if guard
        .script
        .as_ref()
        .is_some_and(|s| s.status == ScriptStatus::Running)
    {
        let s = guard.script.as_ref().unwrap();
        return (
            409,
            Response::Error(ErrorResponse {
                error: format!("script already running: id={} name={}", s.id, s.name),
            }),
        );
    }
    let id = guard.next_script_id;
    guard.next_script_id += 1;
    let total_frames = req.commands.iter().map(|c| c.t + c.duration).max().unwrap_or(0);
    // Сброс остатков keybind-эмуляции (ripper/blade) до старта.
    hooks::clear_keybind_emulation();
    guard.script = Some(ScriptState {
        id,
        name: req.name.clone(),
        commands: req.commands,
        frame: 0,
        status: ScriptStatus::Running,
        total_frames,
    });
    logger::log_line(&format!(
        "api: script {} '{}' started ({} frames)",
        id, req.name, total_frames
    ));
    (
        200,
        Response::ScriptRun(ScriptRunResponse {
            script_id: id,
            name: req.name,
            total_frames,
        }),
    )
}

/// `POST /script/stop` — остановка активного скрипта.
fn handle_script_stop(state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    let mut guard = state.lock().unwrap();
    match guard.script.as_mut() {
        Some(s) if s.status == ScriptStatus::Running => {
            let id = s.id;
            stop_script(s);
            logger::log_line(&format!("api: script {} stopped by request", id));
            (
                200,
                Response::ScriptStop(ScriptStopResponse {
                    stopped: true,
                    script_id: id,
                }),
            )
        }
        _ => (
            404,
            Response::Error(ErrorResponse {
                error: "no active script".into(),
            }),
        ),
    }
}

/// `POST /eject` — запрос выгрузки DLL. Только ставит флаг: сам eject
/// (`shutdown()` + `hudhook::eject()`) выполняет render-цикл, когда увидит
/// флаг в следующем кадре. Из HTTP-потока это невозможно — `shutdown()`
/// джойнит сам себя, а `hudhook::eject()` обрабатывается в render-цикле.
/// Идемпотентно: повторный запрос до обработки флага тоже возвращает 200.
fn handle_eject(state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    let mut guard = state.lock().unwrap();
    guard.eject_requested = true;
    logger::log_line("api: eject requested via POST /eject");
    (200, Response::Eject(EjectResponse { ejecting: true }))
}

/// `GET /script/{id}` — статус скрипта.
fn handle_script_get(path: &str, state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    let id_str = &path["/script/".len()..];
    let id: u32 = match id_str.parse() {
        Ok(v) => v,
        Err(_) => {
            return (
                400,
                Response::Error(ErrorResponse {
                    error: "invalid script id".into(),
                }),
            )
        }
    };
    let guard = state.lock().unwrap();
    match guard.script.as_ref().filter(|s| s.id == id) {
        Some(s) => (
            200,
            Response::ScriptStatus(ScriptStatusJson {
                id: s.id,
                name: s.name.clone(),
                status: s.status,
                frame: s.frame,
                total_frames: s.total_frames,
            }),
        ),
        None => (
            404,
            Response::Error(ErrorResponse {
                error: format!("script {} not found", id),
            }),
        ),
    }
}

/// `GET /logs` — кадры из кольцевого буфера по интервалу/скрипту.
fn handle_logs(query: &str, state: &Arc<Mutex<SharedState>>) -> (u16, Response) {
    let params = parse_query(query);
    let guard = state.lock().unwrap();
    let now = guard.start.elapsed().as_millis() as u64;
    let from_ms = params
        .get("from_ms")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let to_ms = params
        .get("to_ms")
        .and_then(|v| v.parse().ok())
        .unwrap_or(now);
    let script_id = params.get("script_id").and_then(|v| v.parse().ok());
    let limit = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000)
        .min(MAX_LOG_LIMIT);
    let frames: Vec<LogFrameJson> = guard
        .ring
        .query(from_ms, to_ms, script_id, limit)
        .iter()
        .map(|f| f.to_json())
        .collect();
    (
        200,
        Response::Logs(LogsResponse {
            from_ms,
            to_ms,
            count: frames.len(),
            frames,
        }),
    )
}

/// Пишет HTTP-ответ с JSON-телом и Connection: close (без keep-alive —
/// соединение живёт ровно один запрос, висящих соединений нет).
fn respond(stream: &mut TcpStream, code: u16, value: &Response) {
    let body = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        code, reason, body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

/// Разбор query-строки в map (без URL-декодирования — параметры числовые).
fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(k.to_string(), v.to_string());
        }
    }
    map
}

/// Парсинг скрипта из JSON-тела + пост-валидация лимитов. Serde покрывает
/// типы и неизвестные ключи (с path к полю), здесь — кросс-полевые проверки.
fn parse_script(body: &str) -> Result<ScriptRequest, String> {
    let req: ScriptRequest =
        serde_json::from_str(body).map_err(|e| format!("invalid script: {}", e))?;
    if req.name.len() > 64 {
        return Err("name too long (max 64)".into());
    }
    if req.commands.is_empty() {
        return Err("commands is empty".into());
    }
    for (i, cmd) in req.commands.iter().enumerate() {
        if cmd.duration == 0 {
            return Err(format!("commands[{}]: duration must be >= 1", i));
        }
        if cmd.t > MAX_SCRIPT_FRAMES || cmd.duration > MAX_SCRIPT_FRAMES {
            return Err(format!(
                "commands[{}]: t/duration exceeds max {}",
                i, MAX_SCRIPT_FRAMES
            ));
        }
        if cmd.t + cmd.duration > MAX_SCRIPT_FRAMES {
            return Err(format!(
                "commands[{}]: t+duration exceeds max {}",
                i, MAX_SCRIPT_FRAMES
            ));
        }
        if cmd.input.is_empty() {
            return Err(format!("commands[{}]: input is empty", i));
        }
    }
    Ok(req)
}

/// Вычисляет InputUnit кадра `script.frame` из активных команд и продвигает
/// скрипт. Keybind-действия подаются через эмуляцию `isKeybindPressed`
/// (фронт, 1 кадр) / `isKeybindDown` (удержание на время команды); меню-клавиши
/// (стрелки/Enter) — записью в кэш `ms_KeyInput` (меню читает их через
/// `isKeyDown`/`isKeyPressed`, а не через keybind'ы).
fn script_tick(script: &mut ScriptState) -> InputOverride {
    let k = script.frame;
    let mut unit = InputUnit {
        valid_input: 1,
        ..Default::default()
    };
    let mut active = false;
    let mut ripper_edge = false;
    let mut blade_on = false;
    let mut ninja_on = false;
    let mut walk_on = false;
    let mut dodge_on = false;
    let mut lock_on = false;
    let mut subweapon = false;
    let mut item = false;
    let mut camera_reset = false;
    let mut zandatsu = false;
    let mut raw_active = false;
    for cmd in &script.commands {
        if k < cmd.t || k >= cmd.t + cmd.duration {
            continue;
        }
        let inp = &cmd.input;
        // Движение: биты направлений + left_stick (если не задан явно).
        let mut stick = [0.0f32, 0.0];
        if inp.forward {
            unit.buttons_down |= addresses::input_bits::FORWARD;
            stick[1] -= 1000.0;
            active = true;
        }
        if inp.backward {
            unit.buttons_down |= addresses::input_bits::BACK;
            stick[1] += 1000.0;
            active = true;
        }
        if inp.left {
            unit.buttons_down |= addresses::input_bits::LEFT;
            stick[0] -= 1000.0;
            active = true;
        }
        if inp.right {
            unit.buttons_down |= addresses::input_bits::RIGHT;
            stick[0] += 1000.0;
            active = true;
        }
        if stick != [0.0, 0.0] && inp.left_stick.is_none() {
            unit.left_stick = stick;
        }
        if inp.jump {
            unit.buttons_down |= addresses::input_bits::JUMP;
            if k == cmd.t {
                unit.buttons_pressed |= addresses::input_bits::JUMP;
            }
            active = true;
        }
        if inp.light_attack {
            unit.buttons_down |= addresses::input_bits::LIGHT_ATTACK;
            if k == cmd.t {
                unit.buttons_pressed |= addresses::input_bits::LIGHT_ATTACK;
            }
            active = true;
        }
        if inp.heavy_attack {
            unit.buttons_down |= addresses::input_bits::HEAVY_ATTACK;
            if k == cmd.t {
                unit.buttons_pressed |= addresses::input_bits::HEAVY_ATTACK;
            }
            active = true;
        }
        if let Some([dx, dy]) = inp.camera {
            unit.right_stick = [dx, dy];
            active = true;
        }
        // Hold-действия (isKeybindDown): удержание на все кадры команды.
        if inp.ninja_run {
            ninja_on = true;
            active = true;
        }
        if inp.walk {
            walk_on = true;
            active = true;
        }
        if inp.dodge {
            dodge_on = true;
            active = true;
        }
        if inp.blade {
            blade_on = true;
        }
        // Toggle-действия: удержание keybind'а на все кадры команды
        // (isKeybindDown; игра сама детектирует фронт). Клавиши 1/2/3 и Esc
        // игра читает как сырые клавиши — они обрабатываются ниже.
        if inp.ripper && k == cmd.t {
            ripper_edge = true;
        }
        if inp.lock_on {
            lock_on = true;
            active = true;
        }
        if inp.subweapon {
            subweapon = true;
            active = true;
        }
        if inp.item {
            item = true;
            active = true;
        }
        if inp.camera_reset {
            camera_reset = true;
            active = true;
        }
        if inp.zandatsu {
            zandatsu = true;
            active = true;
        }
        // Меню-клавиши и цифры 1/2/3: сырые клавиши в кэш ms_KeyInput
        // (меню читает их через isKeyDown/isKeyPressed, а не через keybind'ы).
        // Удержание на все кадры команды + фронт pressed на первом кадре.
        let raw = [
            (inp.pause, addresses::KEY_ESC),
            (inp.ar_mode, addresses::KEY_DIGIT1),
            (inp.weapon_select, addresses::KEY_DIGIT2),
            (inp.codec, addresses::KEY_DIGIT3),
            (inp.confirm, addresses::KEY_ENTER),
            (inp.menu_up, addresses::KEY_UP),
            (inp.menu_down, addresses::KEY_DOWN),
            (inp.menu_left, addresses::KEY_LEFT),
            (inp.menu_right, addresses::KEY_RIGHT),
        ];
        for (on, code) in raw {
            if on {
                hooks::set_raw_key(code, false);
                if k == cmd.t {
                    hooks::set_raw_key(code, true);
                }
                raw_active = true;
            }
        }
        if let Some(ls) = inp.left_stick {
            unit.left_stick = ls;
        }
    }
    if ripper_edge {
        hooks::set_ripper_frames(1);
    }
    hooks::set_blade_hold(blade_on);
    hooks::set_keybind_hold(addresses::KEYBIND_NINJARUN, ninja_on);
    hooks::set_keybind_hold(addresses::KEYBIND_WALK, walk_on);
    hooks::set_keybind_hold(addresses::KEYBIND_DEFFENSIVE_OFFENSIVE, dodge_on);
    hooks::set_keybind_hold(addresses::KEYBIND_SWITCH_LOCK_ON, lock_on);
    hooks::set_keybind_hold(addresses::KEYBIND_USE_SUBWEAPON, subweapon);
    hooks::set_keybind_hold(addresses::KEYBIND_USE_ITEM, item);
    hooks::set_keybind_hold(addresses::KEYBIND_CAMERA_RESET, camera_reset);
    hooks::set_keybind_hold(addresses::KEYBIND_EXECUTION, zandatsu);
    // Raw-биты живут, пока активна команда с raw-входом: без таких команд
    // в этом кадре — сброс (иначе меню увидит «залипшую» клавишу).
    if !raw_active {
        hooks::clear_raw_keys();
    }
    script.frame += 1;
    if script.frame >= script.total_frames {
        script.status = ScriptStatus::Done;
    }
    InputOverride { active, input: unit }
}

/// Останавливает скрипт: снимает override и keybind-эмуляцию.
fn stop_script(script: &mut ScriptState) {
    script.status = ScriptStatus::Stopped;
    replay::set_input_override(InputOverride::default());
    hooks::clear_keybind_emulation();
}

impl LogFrame {
    fn to_json(&self) -> LogFrameJson {
        LogFrameJson {
            t_ms: self.t_ms,
            frame: self.frame,
            script_id: self.script_id,
            pos: self.pos,
            rot: self.rot,
            vel: self.vel,
            hp: self.hp,
            r_anim: self.r_anim,
            ripper: self.ripper,
            blade: self.blade,
            input: InputJson {
                buttons: decode_buttons(self.input.buttons_down),
                left_stick: self.input.left_stick,
                right_stick: self.input.right_stick,
            },
        }
    }
}

/// Декодирует битмаску `buttons_down` в имена действий (для логов).
fn decode_buttons(down: u32) -> Vec<&'static str> {
    let mut v = Vec::new();
    if down & addresses::input_bits::FORWARD != 0 {
        v.push("forward");
    }
    if down & addresses::input_bits::BACK != 0 {
        v.push("backward");
    }
    if down & addresses::input_bits::LEFT != 0 {
        v.push("left");
    }
    if down & addresses::input_bits::RIGHT != 0 {
        v.push("right");
    }
    if down & addresses::input_bits::JUMP != 0 {
        v.push("jump");
    }
    if down & addresses::input_bits::LIGHT_ATTACK != 0 {
        v.push("light_attack");
    }
    if down & addresses::input_bits::HEAVY_ATTACK != 0 {
        v.push("heavy_attack");
    }
    if down & addresses::input_bits::NINJA_RUN != 0 {
        v.push("ninja_run");
    }
    v
}