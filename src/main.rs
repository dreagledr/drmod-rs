use hudhook::inject::Process;
use std::env;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::*;

const DEFAULT_TITLE: &str = "METAL GEAR RISING REVENGEANCE.exe";

fn main() {
    let title = match parse_name_args(env::args()) {
        Ok(t) => t,
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
                &title, e
            ));
            return;
        }
    };

    let exe_path = env::current_exe().expect("Failed to get current exe path");
    let exe_dir = exe_path.parent().expect("Failed to get parent directory");
    let dll_path = exe_dir.join("drmod_rs_lib.dll");
    if !dll_path.exists() {
        show_msgbox(&format!(
            "Не смогли найти библиотеку с модом в {:?}",
            dll_path
        ));
        return;
    }

    if let Err(e) = process.inject(dll_path) {
        show_msgbox(&format!("Не смогли внедрить мод в MGR.\n{}", e));
    }
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

    return Ok(DEFAULT_TITLE.to_string());
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
