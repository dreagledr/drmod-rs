Part of [#4](https://github.com/dreagledr/drmod-rs/issues/4).

После flush сегмента в БД (`segment::finish_segment`) нужно оставлять только лучший (минимальный `duration_ms`) сегмент для каждого `mission_id`, удаляя остальные.

**Где:** `src/segment.rs` — функция `finish_segment()` (строка ~262).

**Что сделать:**
- После `INSERT INTO segments` выполнять `DELETE FROM segments WHERE mission_id = ?1 AND id != (SELECT id FROM segments WHERE mission_id = ?1 ORDER BY duration_ms ASC LIMIT 1)`
- Каскадно удалятся и `segment_positions` (уже настроен `ON DELETE CASCADE`).
