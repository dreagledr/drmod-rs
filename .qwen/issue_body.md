- [ ] [#5](https://github.com/dreagledr/drmod-rs/issues/5) Очистка БД: оставлять только лучший сегмент после flush
- [ ] [#6](https://github.com/dreagledr/drmod-rs/issues/6) Скрыть дебаг-панель и numpad читы в release-билде
- [ ] [#7](https://github.com/dreagledr/drmod-rs/issues/7) Исправить окончание R-07 и проверить чекпойнты автосплиттера

---

### 1. Очистка базы от неактуальных ранов

После flush сегмента в БД (`segment::finish_segment`) нужно оставлять только лучший (минимальный `duration_ms`) сегмент для каждого `mission_id`, удаляя остальные.

**Где:** `src/segment.rs` — функция `finish_segment()` (строка ~262).

**Что сделать:**
- После `INSERT INTO segments` выполнять `DELETE FROM segments WHERE mission_id = ?1 AND id != (SELECT id FROM segments WHERE mission_id = ?1 ORDER BY duration_ms ASC LIMIT 1)`
- Каскадно удалятся и `segment_positions` (уже настроен `ON DELETE CASCADE`).

---

### 2. Убрать дебаг-панель и numpad читы из release-билда

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

---

### 3. Исправить окончание R-07 и проверить остальные чекпойнты автосплиттера

**Проблема:** сплит на конце миссии R-07 не срабатывает.

**Где:** `src/segment.rs` — функция `segment_action()`, строки 177–185:

```rust
if seg.mission_id == 0x0710
    && game_menu_status == GameMenuStatus::InGame
    && (pos.x - (-195.73)).abs() <= 0.1
    && (pos.y - (-7.1)).abs() <= 0.1
    && (pos.z - (-491.38)).abs() <= 0.1
{
    return SegmentAction::End;
}
```

Текущее условие: позиция `(-195.73, -7.1, -491.38)` с допуском ±0.1 и статус `InGame`.

**Что сделать:**
- Проверить корректность координат конца R-07 (возможно, позиция не совпадает с реальной в игре)
- Возможно, нужен другой статус (не `InGame`, а что-то другое — например `InMenu` как у остальных переходов)
- Проверить все остальные переходы между миссиями (R-00→R-01, R-01→R-02, …, R-06→R-07) на корректность координат и условий
- Возможно, добавить отладочный вывод текущей позиции при входе в условие конца R-07
