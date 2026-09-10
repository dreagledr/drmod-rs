use drmod_rs_lib::DEFAULT_TITLE;
use hudhook::inject::Process;
use std::env;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, INFINITE, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{PCWSTR, h, s, w};

// DLL embedded at compile time. Binary crate compiles after the library,
// so the DLL already exists in the target directory.
#[cfg(debug_assertions)]
const EMBEDDED_DLL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/i686-pc-windows-msvc/debug/drmod_rs_lib.dll"
));
#[cfg(not(debug_assertions))]
const EMBEDDED_DLL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/i686-pc-windows-msvc/release/drmod_rs_lib.dll"
));

fn main() {
    let title = match parse_name_args(env::args()) {
        Ok(t) => t,
        Err(e) => {
            show_msgbox(&e);
            return;
        }
    };

    let dll_path = match extract_dll() {
        Ok(p) => p,
        Err(e) => {
            show_msgbox(&e);
            return;
        }
    };

    let process = match Process::by_name(&title) {
        Ok(p) => p,
        Err(e) => {
            show_msgbox(&format!(
                "Не смогли найти {}. Убедитесь что игра запущена.\n{}",
                title, e
            ));
            return;
        }
    };

    // Своя загрузка вместо `Process::inject`: hudhook не проверяет код выхода
    // удалённого `LoadLibraryW`, поэтому неудачная загрузка DLL выглядит как
    // «инжект прошёл, а мода нет». Здесь код выхода печатается (HMODULE или 0).
    match inject_and_check(process.handle(), &dll_path) {
        Ok(handle) if handle != 0 => {
            println!("инжект OK: LoadLibraryW вернул 0x{handle:08X} ({})", dll_path.display());
        }
        Ok(_) => {
            let msg = format!(
                "LoadLibraryW в игре вернул 0 — DLL не загрузилась.\n{}",
                dll_path.display()
            );
            println!("{msg}");
            show_msgbox(&msg);
        }
        Err(e) => {
            let msg = format!("Не смогли внедрить мод в MGR.\n{e}");
            println!("{msg}");
            show_msgbox(&msg);
        }
    }
}

/// Загружает DLL в процесс игры удалённым `LoadLibraryW` и возвращает код
/// выхода потока (HMODULE при успехе, 0 при неудаче).
fn inject_and_check(
    process: HANDLE,
    dll_path: &std::path::Path,
) -> std::result::Result<usize, String> {
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
        WriteProcessMemory(
            process,
            remote,
            wide.as_ptr().cast(),
            bytes,
            Some(&mut written),
        )
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

/// Extract embedded DLL to %LOCALAPPDATA%\drmod\ dir.
fn extract_dll() -> std::result::Result<PathBuf, String> {
    let localappdata =
        env::var("LOCALAPPDATA").map_err(|_| "Переменная LOCALAPPDATA не найдена".to_string())?;
    let drmod_dir = format!("{}\\drmod", localappdata);
    let dll_path = PathBuf::from(format!("{}\\drmod_rs_lib.dll", drmod_dir));

    std::fs::create_dir_all(&drmod_dir)
        .map_err(|e| format!("Не удалось создать {}: {}", drmod_dir, e))?;
    std::fs::write(&dll_path, EMBEDDED_DLL)
        .map_err(|e| format!("Не удалось записать DLL в {}: {}", dll_path.display(), e))?;

    Ok(dll_path)
}

fn parse_name_args(mut args: env::Args) -> std::result::Result<String, String> {
    args.next();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-n" | "--name" => {
                if let Some(val) = args.next() {
                    return Ok(val);
                } else {
                    return Err("После -n должно идти название окна.".into());
                }
            }
            _ => {}
        }
    }

    Ok(DEFAULT_TITLE.to_string())
}

fn show_msgbox(text: &str) {
    let msg: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(msg.as_ptr()),
            h!("Ошибка при старте drmod"),
            MB_OK | MB_ICONERROR,
        );
    }
}
