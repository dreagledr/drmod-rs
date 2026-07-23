use drmod_rs_lib::DEFAULT_TITLE;
use hudhook::inject::Process;
use std::env;
use std::path::PathBuf;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{h, PCWSTR};

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

    if let Err(e) = process.inject(dll_path) {
        show_msgbox(&format!("Не смогли внедрить мод в MGR.\n{}", e));
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
