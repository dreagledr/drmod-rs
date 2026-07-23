Part of [#4](https://github.com/dreagledr/drmod-rs/issues/4).

В релизной сборке не должно быть:
- Окна `DrmodDebug` (вся функция `ui::render_main_window`)
- Всех numpad-клавиш (Keypad1/2/3: +10m Y, save position, teleport)
- Сохранённой позиции и её экранной проекции

**Где:**
- `src/ui.rs` — `render_main_window()` (строка 28)
- `src/lib.rs` — обработчики Keypad1/2/3 (строки ~527–560)
- `src/lib.rs` — вызов `ui::render_main_window()` (строка 623)
- `src/lib.rs` — отрисовка сохранённой позиции (строки ~563–577)

**Что сделать:**
- Обернуть всё в `#[cfg(debug_assertions)]` / `#[cfg(not(debug_assertions))]`
- В релизе оставить только `render_multiplayer_window` и `render_settings_window`
