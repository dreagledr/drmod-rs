//! HTTP API автоматизации: скрипты ввода, состояние игры, кольцевой буфер логов.
//!
//! Сервер слушает `127.0.0.1:5223` (tiny_http) в отдельном потоке. Render-цикл
//! продвигает активный скрипт (подача ввода через `replay::set_input_override`
//! и keybind-эмуляцию ripper/blade), пишет кадры в кольцевой буфер и обновляет
//! снимок `/state`. HTTP-поток только читает `SharedState` (Arc<Mutex>).
//!
//! Дизайн — `docs/API.md`.

use crate::logger;
use crate::tas::addresses;
use crate::tas::hooks;
use crate::tas::replay;
use crate::tas::types::{InputOverride, InputUnit, PlayerState};
use crate::ui::UiState;
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tiny_http::{Header, Method, Request, Response, Server};

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

/// Декодированный вход одной команды скрипта.
#[derive(Clone, Copy, Default)]
struct ScriptInput {
    forward: bool,
    jump: bool,
    light_attack: bool,
    heavy_attack: bool,
    camera: Option<[f32; 2]>,
    ripper: bool,
    blade: bool,
    left_stick: Option<[f32; 2]>,
}

/// Одна команда скрипта: входы активны с кадра `t` на `duration` кадров.
#[derive(Clone, Copy)]
struct ScriptCommand {
    t: u32,
    duration: u32,
    input: ScriptInput,
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
        }));

        let stop = Arc::new(AtomicBool::new(false));
        let handle = match Server::http(BIND_ADDR) {
            Ok(server) => {
                logger::log_line(&format!("api: http server on {}", BIND_ADDR));
                let state = Arc::clone(&state);
                let stop = Arc::clone(&stop);
                Some(
                    std::thread::Builder::new()
                        .name("drmod-api".into())
                        .spawn(move || api_thread(server, state, stop))
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

    /// Запускает встроенный скрипт NumPad4 (run-jump-attack) — тот же механизм,
    /// что и `POST /script/run`. Игнорируется, если другой скрипт уже активен.
    /// В release вызывается только извне (HTTP) — отсюда allow.
    #[allow(dead_code)]
    pub fn start_builtin_script(&self) {
        let (name, commands) = match parse_script(BUILTIN_SCRIPT) {
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
        let total_frames = commands.iter().map(|c| c.t + c.duration).max().unwrap_or(0);
        hooks::clear_keybind_emulation();
        guard.script = Some(ScriptState {
            id,
            name: name.clone(),
            commands,
            frame: 0,
            status: ScriptStatus::Running,
            total_frames,
        });
        logger::log_line(&format!(
            "api: builtin script {} '{}' started ({} frames)",
            id, name, total_frames
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

/// Цикл HTTP-потока: принимает запросы, пока не взведён `stop`.
/// `recv_timeout` вместо блокирующего `recv` — поток выходит максимум через
/// 100 мс после `shutdown` (Server не Clone, unblock из другого потока недоступен).
fn api_thread(server: Server, state: Arc<Mutex<SharedState>>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match server.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(Some(request)) => handle_request(request, &state),
            Ok(None) => continue,
            Err(_) => break,
        }
    }
}

/// Разбирает запрос и возвращает ответ.
fn handle_request(mut request: Request, state: &Arc<Mutex<SharedState>>) {
    let method = request.method().clone();
    let url = request.url().to_string();
    let response = route(&method, &url, &mut request, state);
    let _ = request.respond(response);
}

/// Маршрутизация по (метод, путь).
fn route(
    method: &Method,
    url: &str,
    request: &mut Request,
    state: &Arc<Mutex<SharedState>>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    };
    match (method, path) {
        (&Method::Get, "/health") => json_response(200, &health_json(state)),
        (&Method::Get, "/state") => json_response(200, &state_json(state)),
        (&Method::Post, "/script/run") => handle_script_run(request, state),
        (&Method::Post, "/script/stop") => handle_script_stop(state),
        (&Method::Get, path) if path.starts_with("/script/") => handle_script_get(path, state),
        (&Method::Get, "/logs") => handle_logs(query, state),
        _ => json_response(404, &json!({ "error": "not found" })),
    }
}

/// `GET /health` — живость, версия, base_addr, uptime.
fn health_json(state: &Arc<Mutex<SharedState>>) -> serde_json::Value {
    let guard = state.lock().unwrap();
    json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "base_addr": format!("0x{:08X}", guard.base_addr),
        "uptime_ms": guard.start.elapsed().as_millis() as u64,
    })
}

/// `GET /state` — текущий снимок игры + статус скрипта + fps.
fn state_json(state: &Arc<Mutex<SharedState>>) -> serde_json::Value {
    let guard = state.lock().unwrap();
    let s = &guard.snapshot;
    let script = guard.script.as_ref().map(|sc| {
        json!({
            "id": sc.id,
            "name": sc.name,
            "status": sc.status,
            "frame": sc.frame,
            "total_frames": sc.total_frames,
        })
    });
    json!({
        "t_ms": s.t_ms,
        "mission_id": s.mission_id,
        "mission_name": s.mission_name,
        "menu_status": s.menu_status,
        "player": {
            "found": s.player_found,
            "pos": s.pos,
            "rot": s.rot,
            "vel": s.vel,
            "hp": s.hp,
            "r_anim": s.r_anim,
            "ripper": s.ripper,
            "blade": s.blade,
        },
        "script": script,
        "fps": guard.fps,
    })
}

/// `POST /script/run` — запуск скрипта из JSON-тела.
fn handle_script_run(
    request: &mut Request,
    state: &Arc<Mutex<SharedState>>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut body = String::new();
    if request
        .as_reader()
        .take(MAX_BODY_BYTES)
        .read_to_string(&mut body)
        .is_err()
    {
        return json_response(400, &json!({ "error": "failed to read body" }));
    }
    let (name, commands) = match parse_script(&body) {
        Ok(v) => v,
        Err(e) => return json_response(400, &json!({ "error": e })),
    };
    let mut guard = state.lock().unwrap();
    if guard
        .script
        .as_ref()
        .is_some_and(|s| s.status == ScriptStatus::Running)
    {
        let s = guard.script.as_ref().unwrap();
        return json_response(
            409,
            &json!({ "error": format!("script already running: id={} name={}", s.id, s.name) }),
        );
    }
    let id = guard.next_script_id;
    guard.next_script_id += 1;
    let total_frames = commands.iter().map(|c| c.t + c.duration).max().unwrap_or(0);
    // Сброс остатков keybind-эмуляции (ripper/blade) до старта.
    hooks::clear_keybind_emulation();
    guard.script = Some(ScriptState {
        id,
        name: name.clone(),
        commands,
        frame: 0,
        status: ScriptStatus::Running,
        total_frames,
    });
    logger::log_line(&format!(
        "api: script {} '{}' started ({} frames)",
        id, name, total_frames
    ));
    json_response(
        200,
        &json!({ "script_id": id, "name": name, "total_frames": total_frames }),
    )
}

/// `POST /script/stop` — остановка активного скрипта.
fn handle_script_stop(state: &Arc<Mutex<SharedState>>) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut guard = state.lock().unwrap();
    match guard.script.as_mut() {
        Some(s) if s.status == ScriptStatus::Running => {
            let id = s.id;
            stop_script(s);
            logger::log_line(&format!("api: script {} stopped by request", id));
            json_response(200, &json!({ "stopped": true, "script_id": id }))
        }
        _ => json_response(404, &json!({ "error": "no active script" })),
    }
}

/// `GET /script/{id}` — статус скрипта.
fn handle_script_get(
    path: &str,
    state: &Arc<Mutex<SharedState>>,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let id_str = &path["/script/".len()..];
    let id: u32 = match id_str.parse() {
        Ok(v) => v,
        Err(_) => return json_response(400, &json!({ "error": "invalid script id" })),
    };
    let guard = state.lock().unwrap();
    match guard.script.as_ref().filter(|s| s.id == id) {
        Some(s) => json_response(
            200,
            &json!({
                "id": s.id,
                "name": s.name,
                "status": s.status,
                "frame": s.frame,
                "total_frames": s.total_frames,
            }),
        ),
        None => json_response(404, &json!({ "error": format!("script {} not found", id) })),
    }
}

/// `GET /logs` — кадры из кольцевого буфера по интервалу/скрипту.
fn handle_logs(
    query: &str,
    state: &Arc<Mutex<SharedState>>,
) -> Response<std::io::Cursor<Vec<u8>>> {
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
    json_response(
        200,
        &json!({
            "from_ms": from_ms,
            "to_ms": to_ms,
            "count": frames.len(),
            "frames": frames,
        }),
    )
}

/// JSON-ответ с кодом и Content-Type: application/json.
fn json_response<T: Serialize>(code: u16, data: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());
    Response::from_string(body)
        .with_status_code(code)
        .with_header(
            Header::from_bytes(&b"Content-Type"[..], &b"application/json; charset=utf-8"[..])
                .expect("static header"),
        )
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

/// Парсинг скрипта из JSON-тела. Возвращает (name, commands) или текст ошибки.
fn parse_script(body: &str) -> Result<(String, Vec<ScriptCommand>), String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON: {}", e))?;
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("script")
        .to_string();
    if name.len() > 64 {
        return Err("name too long (max 64)".into());
    }
    let commands = v
        .get("commands")
        .and_then(|c| c.as_array())
        .ok_or("missing 'commands' array")?;
    if commands.is_empty() {
        return Err("commands is empty".into());
    }
    let mut out = Vec::with_capacity(commands.len());
    for (i, cmd) in commands.iter().enumerate() {
        let t = cmd
            .get("t")
            .and_then(|x| x.as_u64())
            .ok_or(format!("commands[{}]: missing/invalid 't'", i))? as u32;
        let duration = cmd
            .get("duration")
            .and_then(|x| x.as_u64())
            .ok_or(format!("commands[{}]: missing/invalid 'duration'", i))? as u32;
        if duration == 0 {
            return Err(format!("commands[{}]: duration must be >= 1", i));
        }
        if t > MAX_SCRIPT_FRAMES || duration > MAX_SCRIPT_FRAMES {
            return Err(format!(
                "commands[{}]: t/duration exceeds max {}",
                i, MAX_SCRIPT_FRAMES
            ));
        }
        if t + duration > MAX_SCRIPT_FRAMES {
            return Err(format!(
                "commands[{}]: t+duration exceeds max {}",
                i, MAX_SCRIPT_FRAMES
            ));
        }
        let input = cmd
            .get("input")
            .ok_or(format!("commands[{}]: missing 'input'", i))?;
        let input = parse_input(input, i)?;
        out.push(ScriptCommand { t, duration, input });
    }
    Ok((name, out))
}

/// Парсинг `input` команды. Неизвестные ключи — ошибка (защита от опечаток LLM).
fn parse_input(v: &serde_json::Value, i: usize) -> Result<ScriptInput, String> {
    let obj = v
        .as_object()
        .ok_or(format!("commands[{}]: 'input' must be an object", i))?;
    let mut si = ScriptInput::default();
    let mut any = false;
    for (k, val) in obj {
        let bool_val = |val: &serde_json::Value| -> Result<bool, String> {
            val.as_bool()
                .ok_or_else(|| format!("commands[{}]: '{}' must be a bool", i, k))
        };
        match k.as_str() {
            "forward" => {
                si.forward = bool_val(val)?;
                any |= si.forward;
            }
            "jump" => {
                si.jump = bool_val(val)?;
                any |= si.jump;
            }
            "light_attack" => {
                si.light_attack = bool_val(val)?;
                any |= si.light_attack;
            }
            "heavy_attack" => {
                si.heavy_attack = bool_val(val)?;
                any |= si.heavy_attack;
            }
            "ripper" => {
                si.ripper = bool_val(val)?;
                any |= si.ripper;
            }
            "blade" => {
                si.blade = bool_val(val)?;
                any |= si.blade;
            }
            "camera" => {
                let arr = val
                    .as_array()
                    .ok_or(format!("commands[{}]: 'camera' must be an array", i))?;
                if arr.len() != 2 {
                    return Err(format!("commands[{}]: 'camera' must be [dx, dy]", i));
                }
                let dx = arr[0]
                    .as_f64()
                    .ok_or(format!("commands[{}]: 'camera' dx must be a number", i))? as f32;
                let dy = arr[1]
                    .as_f64()
                    .ok_or(format!("commands[{}]: 'camera' dy must be a number", i))? as f32;
                si.camera = Some([dx, dy]);
                any = true;
            }
            "left_stick" => {
                let arr = val
                    .as_array()
                    .ok_or(format!("commands[{}]: 'left_stick' must be an array", i))?;
                if arr.len() != 2 {
                    return Err(format!("commands[{}]: 'left_stick' must be [x, y]", i));
                }
                let x = arr[0]
                    .as_f64()
                    .ok_or(format!("commands[{}]: 'left_stick' x must be a number", i))? as f32;
                let y = arr[1]
                    .as_f64()
                    .ok_or(format!("commands[{}]: 'left_stick' y must be a number", i))? as f32;
                si.left_stick = Some([x, y]);
                any = true;
            }
            other => {
                return Err(format!("commands[{}]: unknown input key '{}'", i, other));
            }
        }
    }
    if !any {
        return Err(format!("commands[{}]: input is empty", i));
    }
    Ok(si)
}

/// Вычисляет InputUnit кадра `script.frame` из активных команд и продвигает
/// скрипт. Ripper подаётся фронтом `isKeybindPressed` (1 кадр), blade —
/// удержанием `isKeybindDown` на время команды.
fn script_tick(script: &mut ScriptState) -> InputOverride {
    let k = script.frame;
    let mut unit = InputUnit {
        valid_input: 1,
        ..Default::default()
    };
    let mut active = false;
    let mut ripper_edge = false;
    let mut blade_on = false;
    for cmd in &script.commands {
        if k < cmd.t || k >= cmd.t + cmd.duration {
            continue;
        }
        let inp = &cmd.input;
        if inp.forward {
            unit.buttons_down |= addresses::input_bits::FORWARD;
            if unit.left_stick == [0.0, 0.0] {
                unit.left_stick = [0.0, -1000.0];
            }
            active = true;
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
        if inp.ripper && k == cmd.t {
            ripper_edge = true;
        }
        if inp.blade {
            blade_on = true;
        }
        if let Some(ls) = inp.left_stick {
            unit.left_stick = ls;
        }
    }
    if ripper_edge {
        hooks::set_ripper_frames(1);
    }
    hooks::set_blade_hold(blade_on);
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
    if down & addresses::input_bits::JUMP != 0 {
        v.push("jump");
    }
    if down & addresses::input_bits::LIGHT_ATTACK != 0 {
        v.push("light_attack");
    }
    if down & addresses::input_bits::HEAVY_ATTACK != 0 {
        v.push("heavy_attack");
    }
    v
}