//! Настройки мода: структура окна Settings и её персистентность в `runs.db`.
//!
//! Пользовательские галочки/слайдер окна Settings и параметры TAS-автоматизации
//! (`/dt`, `/rng`, `/fps`) сохраняются key/value-строками в таблице `settings`
//! той же БД, что и сегменты/реплеи (`%LOCALAPPDATA%\drmod\runs.db`). Формат
//! key/value, а не колонки, — чтобы новая настройка не требовала миграции.

use std::collections::HashMap;

use rusqlite::Connection;

pub struct Settings {
    pub show_best_ghost: bool,
    pub ghost_opacity: f32,
    /// Скип in-engine катсцены «как на консоли»: в сцене `P370_*` держим флаги
    /// консольного меню, по подтверждённому SKIP убираем меню, снимаем паузу и
    /// заказываем следующую подфазу (см. `game::cutscene_skip`).
    pub cutscene_skip: bool,
    /// Скип стартовой лого-секвенции при загрузке игры: ручной патч цикла
    /// лого-задачи (`game::intro_skip`). Включён по умолчанию.
    pub skip_intro: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_best_ghost: true,
            ghost_opacity: 0.5,
            cutscene_skip: true,
            skip_intro: true,
        }
    }
}

// --- Персистентность (таблица `settings`) -----------------------------------

/// Ключи настроек в таблице `settings` — как storage/API-имена, чтобы одна и
/// та же строка переживала переименование поля в UI.
pub(crate) const K_SHOW_BEST_GHOST: &str = "show_best_ghost";
pub(crate) const K_GHOST_OPACITY: &str = "ghost_opacity";
pub(crate) const K_CUTSCENE_SKIP: &str = "cutscene_skip";
pub(crate) const K_SKIP_INTRO: &str = "skip_intro";
pub(crate) const K_FIXED_DT: &str = "fixed_dt";
pub(crate) const K_FIXED_DT_MS: &str = "fixed_dt_ms";
pub(crate) const K_SYNTH_TICKS: &str = "synth_ticks";
pub(crate) const K_RNG_PIN: &str = "rng_pin";
pub(crate) const K_RNG_SEED: &str = "rng_seed";
pub(crate) const K_FPS_CAP_MODE: &str = "fps_cap_mode";
pub(crate) const K_FPS_CAP_VALUE: &str = "fps_cap_value";

/// Создаёт таблицу настроек (если её нет).
pub(crate) fn create_settings_table(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        (),
    )?;
    Ok(())
}

/// Читает все сохранённые настройки в map `key → value`. Ошибки чтения и
/// отсутствие таблицы не фатальны — вернётся пустая map (дефолты).
pub(crate) fn load_all(conn: &Connection) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(mut stmt) = conn.prepare("SELECT key, value FROM settings") else {
        return map;
    };
    let Ok(rows) = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
    else {
        return map;
    };
    for (k, v) in rows.flatten() {
        map.insert(k, v);
    }
    map
}

/// Записывает одну настройку (upsert).
pub(crate) fn save(conn: &Connection, key: &str, value: &str) {
    let _ = conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    );
}

/// Разбирает значение как bool: `"1"`/`"true"` → true, `"0"`/`"false"` → false,
/// иначе `default`.
pub(crate) fn get_bool(map: &HashMap<String, String>, key: &str, default: bool) -> bool {
    match map.get(key).map(String::as_str) {
        Some("1") | Some("true") => true,
        Some("0") | Some("false") => false,
        _ => default,
    }
}

/// Разбирает значение как `u32`, иначе `default`.
pub(crate) fn get_u32(map: &HashMap<String, String>, key: &str, default: u32) -> u32 {
    map.get(key)
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(default)
}

/// Разбирает значение как `f32`, иначе `default`.
pub(crate) fn get_f32(map: &HashMap<String, String>, key: &str, default: f32) -> f32 {
    map.get(key)
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(default)
}

/// Восстанавливает поля `Settings` из map (отсутствующие ключи = дефолт).
pub(crate) fn apply_to(settings: &mut Settings, map: &HashMap<String, String>) {
    let d = Settings::default();
    settings.show_best_ghost = get_bool(map, K_SHOW_BEST_GHOST, d.show_best_ghost);
    settings.ghost_opacity = get_f32(map, K_GHOST_OPACITY, d.ghost_opacity);
    settings.cutscene_skip = get_bool(map, K_CUTSCENE_SKIP, d.cutscene_skip);
    settings.skip_intro = get_bool(map, K_SKIP_INTRO, d.skip_intro);
}

/// Заменяет альфа-байт в D3DCOLOR (AABBGGRR), оставляя RGB нетронутым.
/// `base_rgb` — цвет с нулевой альфой (например 0x000000FF для красного).
pub fn apply_opacity(base_rgb: u32, opacity: f32) -> u32 {
    let alpha = ((opacity * 255.0) as u32).clamp(0, 255);
    (base_rgb & 0x00FFFFFF) | (alpha << 24)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        create_settings_table(&conn).unwrap();
        save(&conn, K_SHOW_BEST_GHOST, "0");
        save(&conn, K_GHOST_OPACITY, "0.25");
        save(&conn, K_CUTSCENE_SKIP, "false");
        // skip_intro не сохранён — должен остаться дефолтным (true).

        let map = load_all(&conn);
        let mut s = Settings::default();
        apply_to(&mut s, &map);
        assert!(!s.show_best_ghost);
        assert!((s.ghost_opacity - 0.25).abs() < f32::EPSILON);
        assert!(!s.cutscene_skip);
        assert!(s.skip_intro);
    }

    #[test]
    fn save_upsert_overwrites() {
        let conn = Connection::open_in_memory().unwrap();
        create_settings_table(&conn).unwrap();
        save(&conn, K_FPS_CAP_MODE, "1");
        save(&conn, K_FPS_CAP_MODE, "2");
        let map = load_all(&conn);
        assert_eq!(get_u32(&map, K_FPS_CAP_MODE, 0), 2);
    }

    #[test]
    fn getters_fall_back_to_default() {
        let map = HashMap::new();
        assert!(get_bool(&map, "missing", true));
        assert_eq!(get_u32(&map, "missing", 7), 7);
        assert!((get_f32(&map, "missing", 1.5) - 1.5).abs() < f32::EPSILON);
        let mut bad = HashMap::new();
        bad.insert("x".to_string(), "not-a-number".to_string());
        assert_eq!(get_u32(&bad, "x", 9), 9);
        assert!((get_f32(&bad, "x", 2.0) - 2.0).abs() < f32::EPSILON);
    }
}
