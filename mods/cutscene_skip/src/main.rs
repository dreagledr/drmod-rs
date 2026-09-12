//! Лаунчер мода: находит игру (рядом с собой → известный Steam-путь →
//! `--exe`), запускает её при необходимости, инжектит встроенную DLL и
//! опционально показывает лог мода (`--follow`).
//!
//! Работает и из `cargo run`: exe игры ищется в conventional-путях, а если игра
//! уже запущена — инжект идёт прямо в неё.

use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use cutscene_skip_lib::{DEFAULT_TITLE, LIB_NAME, PROCESS_NAME, data_dir, log_path};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, INFINITE, OpenProcess, PROCESS_ACCESS_RIGHTS,
    PROCESS_CREATE_THREAD, PROCESS_TERMINATE, PROCESS_VM_OPERATION, PROCESS_VM_READ,
    PROCESS_VM_WRITE, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{PCWSTR, h, s, w};

/// Путь к exe внутри стандартной установки Steam (порт `GAME_EXE` из
/// `tools/script_tuning/launch_game.py`).
const STEAM_REL: &str = r"Steam\steamapps\common\METAL GEAR RISING REVENGEANCE\METAL GEAR RISING REVENGEANCE.exe";

// DLL встроена в лаунчер. Бинарный крейт компилируется после библиотечного, к
// этому моменту DLL уже лежит в target.
#[cfg(debug_assertions)]
const EMBEDDED_DLL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/i686-pc-windows-msvc/debug/cutscene_skip_lib.dll"
));
#[cfg(not(debug_assertions))]
const EMBEDDED_DLL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/i686-pc-windows-msvc/release/cutscene_skip_lib.dll"
));

struct Args {
    exe: Option<PathBuf>,
    kill_first: bool,
    no_launch: bool,
    follow: bool,
    timeout: Duration,
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            show_msgbox(&e);
            return;
        }
    };

    let dll = match extract_dll() {
        Ok(path) => path,
        Err(e) => {
            let msg = format!("Не удалось распаковать DLL мода.\n{e}");
            println!("{msg}");
            show_msgbox(&msg);
            return;
        }
    };

    let mut pid = running_pid();
    if let Some(p) = pid
        && args.kill_first
    {
        println!("завершаю запущенную игру (PID {p})");
        terminate(p);
        pid = None;
    }

    let pid = match pid {
        Some(p) => {
            println!("игра уже запущена: PID {p}");
            p
        }
        None => {
            if args.no_launch {
                println!("{PROCESS_NAME} не запущена, а --no-launch задан");
                return;
            }
            let Some(exe) = find_game_exe(args.exe.as_deref()) else {
                let msg = format!(
                    "Не нашёл exe игры ({PROCESS_NAME}).\n\
                     Положи лаунчер рядом с игрой или укажи --exe <путь>."
                );
                println!("{msg}");
                show_msgbox(&msg);
                return;
            };
            println!("запускаю игру: {}", exe.display());
            let child = match Command::new(&exe).current_dir(exe.parent().unwrap_or(Path::new("."))).spawn() {
                Ok(child) => child,
                Err(e) => {
                    let msg = format!("Не удалось запустить игру.\n{e}");
                    println!("{msg}");
                    show_msgbox(&msg);
                    return;
                }
            };
            let child_pid = child.id();
            if !wait_for_window(args.timeout) {
                println!(
                    "окно игры не появилось за {} с — инжекчу всё равно",
                    args.timeout.as_secs()
                );
            }
            running_pid().unwrap_or(child_pid)
        }
    };

    match inject(pid, &dll) {
        Ok(handle) if handle != 0 => {
            println!("инжект OK: LoadLibraryW вернул 0x{handle:08X} ({})", dll.display());
            if let Some(path) = log_path() {
                println!("лог мода: {}", path.display());
            }
        }
        Ok(_) => {
            let msg = format!(
                "LoadLibraryW в игре вернул 0 — DLL не загрузилась.\n{}",
                dll.display()
            );
            println!("{msg}");
            show_msgbox(&msg);
            return;
        }
        Err(e) => {
            let msg = format!("Не смогли внедрить мод в MGR.\n{e}");
            println!("{msg}");
            show_msgbox(&msg);
            return;
        }
    }

    if args.follow {
        follow_log();
    }
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        exe: None,
        kill_first: false,
        no_launch: false,
        follow: false,
        timeout: Duration::from_secs(120),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--exe" => {
                let val = it.next().ok_or("--exe требует путь")?;
                args.exe = Some(PathBuf::from(val));
            }
            "--kill-first" => args.kill_first = true,
            "--no-launch" => args.no_launch = true,
            "--follow" => args.follow = true,
            "--timeout" => {
                let val = it.next().ok_or("--timeout требует число секунд")?;
                let secs: u64 = val.parse().map_err(|_| format!("--timeout: {val}"))?;
                args.timeout = Duration::from_secs(secs);
            }
            "-h" | "--help" => {
                println!(
                    "cutscene_skip — лаунчер мода скипа катсцены\n\
                     \n\
                     --exe <путь>    путь к exe игры (иначе ищется рядом/в Steam)\n\
                     --kill-first    завершить запущенную игру и стартовать заново\n\
                     --no-launch     не запускать игру (только инжект в запущенную)\n\
                     --follow        после инжекта печатать лог мода (Ctrl+C — выход)\n\
                     --timeout <с>   сколько ждать окна игры (по умолчанию 120)"
                );
                std::process::exit(0);
            }
            other => return Err(format!("неизвестный аргумент: {other}")),
        }
    }
    Ok(args)
}

/// Ищет exe игры: явный путь → env `CUTSCENE_SKIP_GAME_EXE` → рядом с
/// лаунчером → известный Steam-путь.
fn find_game_exe(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return path.is_file().then(|| path.to_path_buf());
    }
    if let Ok(path) = std::env::var("CUTSCENE_SKIP_GAME_EXE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    candidate_exes().into_iter().find(|path| path.is_file())
}

/// Кандидаты путей к exe игры (рядом с лаунчером + стандартные установки Steam).
fn candidate_exes() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(me) = std::env::current_exe()
        && let Some(dir) = me.parent()
    {
        out.push(dir.join(PROCESS_NAME));
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if is_revengeance_exe(&path) {
                    out.push(path);
                }
            }
        }
    }
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(pf) = std::env::var(var) {
            out.push(Path::new(&pf).join(STEAM_REL));
        }
    }
    out
}

fn is_revengeance_exe(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.to_ascii_uppercase().contains("REVENGEANCE"))
}

/// PID уже запущенной игры (обход процессов через ToolHelp).
fn running_pid() -> Option<u32> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut pid = None;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                if wide_to_string(&entry.szExeFile).eq_ignore_ascii_case(PROCESS_NAME) {
                    pid = Some(entry.th32ProcessID);
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        pid
    }
}

fn wide_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Мягко завершает игру (для `--kill-first`) и ждёт её исчезновения.
fn terminate(pid: u32) {
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(handle, 1);
            let _ = CloseHandle(handle);
        }
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if running_pid().is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// Ждёт появления окна игры по заголовку.
fn wait_for_window(timeout: Duration) -> bool {
    let title = title_wide();
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let hwnd = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) };
        if hwnd.is_ok_and(|h| !h.is_invalid()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

fn title_wide() -> Vec<u16> {
    DEFAULT_TITLE.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Распаковывает встроенную DLL в `%LOCALAPPDATA%\cutscene_skip`.
fn extract_dll() -> Result<PathBuf, String> {
    let dir = data_dir().ok_or("Переменная LOCALAPPDATA не найдена")?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Не удалось создать {}: {e}", dir.display()))?;
    let path = dir.join(LIB_NAME);
    std::fs::write(&path, EMBEDDED_DLL)
        .map_err(|e| format!("Не удалось записать {}: {e}", path.display()))?;
    Ok(path)
}

fn inject(pid: u32, dll_path: &Path) -> Result<usize, String> {
    let access = PROCESS_ACCESS_RIGHTS(
        PROCESS_CREATE_THREAD.0 | PROCESS_VM_OPERATION.0 | PROCESS_VM_WRITE.0 | PROCESS_VM_READ.0,
    );
    let process = unsafe { OpenProcess(access, false, pid) }
        .map_err(|e| format!("OpenProcess({pid}): {e}"))?;
    let result = inject_and_check(process, dll_path);
    let _ = unsafe { CloseHandle(process) };
    result
}

/// Загружает DLL в процесс игры удалённым `LoadLibraryW` и возвращает код
/// выхода потока (HMODULE при успехе, 0 при неудаче). Порт `inject_and_check`
/// из основного мода.
fn inject_and_check(process: HANDLE, dll_path: &Path) -> Result<usize, String> {
    let wide: Vec<u16> = dll_path
        .canonicalize()
        .map_err(|e| format!("canonicalize {}: {e}", dll_path.display()))?
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let bytes = wide.len() * size_of::<u16>();
    unsafe {
        let kernel32 =
            GetModuleHandleW(w!("Kernel32")).map_err(|e| format!("GetModuleHandleW: {e}"))?;
        let load_library =
            GetProcAddress(kernel32, s!("LoadLibraryW")).ok_or("нет LoadLibraryW")?;
        let remote = VirtualAllocEx(process, None, bytes, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE);
        if remote.is_null() {
            return Err("VirtualAllocEx вернул NULL".into());
        }
        let mut written = 0usize;
        WriteProcessMemory(process, remote, wide.as_ptr().cast(), bytes, Some(&mut written))
            .map_err(|e| format!("WriteProcessMemory: {e}"))?;
        let thread = CreateRemoteThread(
            process,
            None,
            0,
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
            >(load_library)),
            Some(remote),
            0,
            None,
        )
        .map_err(|e| format!("CreateRemoteThread: {e}"))?;
        WaitForSingleObject(thread, INFINITE);
        let mut code = 0u32;
        GetExitCodeThread(thread, &mut code).map_err(|e| format!("GetExitCodeThread: {e}"))?;
        let _ = CloseHandle(thread);
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        Ok(code as usize)
    }
}

/// Печатает новые строки лога мода, пока не прервут (Ctrl+C).
fn follow_log() {
    let Some(path) = log_path() else {
        return;
    };
    println!("--- лог: {} (Ctrl+C — выход) ---", path.display());
    let mut pos = 0u64;
    let mut buf = Vec::new();
    loop {
        if let Ok(mut file) = std::fs::File::open(&path) {
            let len = file.metadata().map(|m| m.len()).unwrap_or(0);
            if len < pos {
                pos = 0;
            }
            if len > pos {
                buf.clear();
                if file.seek(SeekFrom::Start(pos)).is_ok() {
                    let _ = file.read_to_end(&mut buf);
                    print!("{}", String::from_utf8_lossy(&buf));
                    let _ = std::io::stdout().flush();
                    pos += buf.len() as u64;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn show_msgbox(text: &str) {
    let msg: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(msg.as_ptr()),
            h!("cutscene_skip"),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::is_revengeance_exe;
    use std::path::Path;

    #[test]
    fn recognizes_game_exe() {
        assert!(is_revengeance_exe(Path::new(
            r"C:\games\METAL GEAR RISING REVENGEANCE.exe"
        )));
        assert!(is_revengeance_exe(Path::new("Metal Gear Rising Revengeance.exe")));
        assert!(!is_revengeance_exe(Path::new("steam.exe")));
        assert!(!is_revengeance_exe(Path::new("REVENGEANCE.txt")));
    }
}
