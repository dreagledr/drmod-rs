//! Рабочая область: список скриптов, чтение/запись, настройки, выбор папки.

use crate::model::{self, Script};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Запись списка скриптов: имя файла и метаданные для колонки.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// Скрипты рабочей области — `*.json` верхнего уровня, по алфавиту.
pub fn list_scripts(dir: &Path) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();

    for item in fs::read_dir(dir)? {
        let item = item?;
        let path = item.path();
        if !path.is_file() || !is_script(&path) {
            continue;
        }
        let metadata = item.metadata().ok();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        entries.push(Entry {
            name: name.to_string(),
            path,
            size: metadata.as_ref().map_or(0, |meta| meta.len()),
            modified: metadata.and_then(|meta| meta.modified().ok()),
        });
    }

    entries.sort_by_key(|entry| entry.name.to_lowercase());
    Ok(entries)
}

fn is_script(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

pub fn read_script(path: &Path) -> Result<Script, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("не удалось прочитать {}: {error}", path.display()))?;
    model::from_text(&text).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn write_script(path: &Path, script: &Script) -> Result<(), String> {
    fs::write(path, model::to_text(script))
        .map_err(|error| format!("не удалось записать {}: {error}", path.display()))
}

/// Имя файла для скрипта: имя из модели, очищенное от недопустимых символов.
pub fn file_name_for(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|symbol| {
            if symbol.is_ascii_alphanumeric() || matches!(symbol, '-' | '_' | ' ' | '.') {
                symbol
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "script".to_string()
    } else {
        cleaned
    }
}

pub fn create(dir: &Path, name: &str, script: &Script) -> Result<PathBuf, String> {
    let path = unique_path(dir, &file_name_for(name));
    write_script(&path, script)?;
    Ok(path)
}

pub fn duplicate(dir: &Path, source: &Path) -> Result<PathBuf, String> {
    let stem = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("script");
    let mut script = read_script(source)?;
    script.name = format!("{stem}-copy");
    let path = unique_path(dir, &format!("{stem}-copy"));
    write_script(&path, &script)?;
    Ok(path)
}

pub fn rename(source: &Path, new_name: &str) -> Result<PathBuf, String> {
    let dir = source.parent().unwrap_or_else(|| Path::new("."));
    let target = unique_path(dir, &file_name_for(new_name));
    fs::rename(source, &target).map_err(|error| {
        format!(
            "не удалось переименовать {} → {}: {error}",
            source.display(),
            target.display()
        )
    })?;
    Ok(target)
}

pub fn delete(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|error| format!("не удалось удалить {}: {error}", path.display()))
}

/// Свободный путь в каталоге: `name.json`, `name-2.json`, … — файлы не перезаписываем молча.
fn unique_path(dir: &Path, stem: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.json"));
    let mut counter = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}-{counter}.json"));
        counter += 1;
    }
    candidate
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
}

pub fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("tas-editor").join("settings.json"))
}

pub fn load_settings() -> Settings {
    let Some(path) = settings_path() else {
        return Settings::default();
    };
    read_settings(&path).unwrap_or_default()
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let Some(path) = settings_path() else {
        return Err("LOCALAPPDATA не задан".to_string());
    };
    write_settings(&path, settings)
}

fn read_settings(path: &Path) -> Option<Settings> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_settings(path: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("не удалось создать {}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("не удалось сериализовать настройки: {error}"))?;
    fs::write(path, text).map_err(|error| format!("не удалось записать {}: {error}", path.display()))
}

/// Системный выбор папки (`IFileOpenDialog` с `FOS_PICKFOLDERS`).
pub fn pick_folder() -> Option<PathBuf> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::Win32::UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
        SIGDN_FILESYSPATH,
    };

    // COM в UI-потоке уже инициализирован (STA) — WinUI 3 требует этого сам.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        dialog
            .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
            .ok()?;
        dialog.Show(None).ok()?;

        let item = dialog.GetResult().ok()?;
        let wide = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let text = wide.to_string().ok();
        CoTaskMemFree(Some(wide.0 as *const core::ffi::c_void));

        Some(PathBuf::from(text?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tas-editor-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("временный каталог создаётся");
        dir
    }

    #[test]
    fn lists_only_json_files_sorted() {
        let dir = temp_dir("list");
        fs::write(dir.join("b.json"), "{}").unwrap();
        fs::write(dir.join("a.json"), "{}").unwrap();
        fs::write(dir.join("notes.txt"), "x").unwrap();

        let names: Vec<String> = list_scripts(&dir)
            .expect("каталог читается")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, vec!["a.json", "b.json"]);
    }

    #[test]
    fn create_does_not_overwrite() {
        let dir = temp_dir("create");
        let script = Script::default();
        let first = create(&dir, "core", &script).expect("первый файл");
        let second = create(&dir, "core", &script).expect("второй файл");

        assert_eq!(first.file_name().unwrap(), "core.json");
        assert_eq!(second.file_name().unwrap(), "core-2.json");
    }

    #[test]
    fn file_name_is_sanitized() {
        assert_eq!(file_name_for("core117"), "core117");
        assert_eq!(file_name_for("R-03 / barrier"), "R-03 _ barrier");
        assert_eq!(file_name_for("  ..  "), "script");
    }

    #[test]
    fn script_round_trip() {
        let dir = temp_dir("roundtrip");
        let script = Script {
            name: "probe".to_string(),
            commands: vec![crate::model::Command {
                t: 5,
                duration: 3,
                input: crate::model::Input {
                    jump: true,
                    ..crate::model::Input::default()
                },
                when_enemy: None,
            }],
            ..Script::default()
        };

        let path = dir.join("probe.json");
        write_script(&path, &script).expect("запись");
        assert_eq!(read_script(&path).expect("чтение"), script);
    }

    #[test]
    fn settings_round_trip() {
        let dir = temp_dir("settings");
        let path = dir.join("nested").join("settings.json");
        let settings = Settings {
            workspace: Some("D:\\pet\\drmod-rs\\test_inputs".to_string()),
        };

        write_settings(&path, &settings).expect("запись настроек");
        assert_eq!(read_settings(&path), Some(settings));
        assert_eq!(read_settings(&dir.join("missing.json")), None);
    }
}
