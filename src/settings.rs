pub struct Settings {
    pub show_best_ghost: bool,
    pub ghost_opacity: f32,
    /// Скип in-engine катсцены «как на консоли»: в сцене `P370_*` держим флаги
    /// консольного меню, по подтверждённому SKIP убираем меню, снимаем паузу и
    /// заказываем следующую подфазу (см. `game::cutscene_skip`).
    pub cutscene_skip: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_best_ghost: true,
            ghost_opacity: 0.5,
            cutscene_skip: true,
        }
    }
}

/// Заменяет альфа-байт в D3DCOLOR (AABBGGRR), оставляя RGB нетронутым.
/// `base_rgb` — цвет с нулевой альфой (например 0x000000FF для красного).
pub fn apply_opacity(base_rgb: u32, opacity: f32) -> u32 {
    let alpha = ((opacity * 255.0) as u32).clamp(0, 255);
    (base_rgb & 0x00FFFFFF) | (alpha << 24)
}
