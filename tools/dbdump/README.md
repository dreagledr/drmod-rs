# dbdump — экспорт Record/Replay в CSV и Parquet

Раскладывает BLOB-кадры прогонов (`replay_runs` / `replay_record_frames` /
`replay_playback_frames` в `%LOCALAPPDATA%\drmod\runs.db`) на плоские колонки
для офлайн-аналитики. Layout BLOB'ов — общий с модом через крейт
`replay-types/` (`drmod-replay-types`): структуры `InputUnit`/`PlayerState`/
`CameraState` + `to_bytes`/`from_bytes` живут в одном месте, поэтому формат
у писателя и читателя всегда совпадает.

## Сборка

Тул — workspace-член корневого проекта, но собирается под **x64** (arrow-rs/
parquet — только 64-bit; свой `.cargo/config.toml` как у `server/`):

```bash
cd tools/dbdump
cargo build --release
# бинарь: ../../target/x86_64-pc-windows-msvc/release/dbdump.exe
```

Корневой `cargo build --release` (i686) тул **не** собирает — только из этой
директории. Тесты: `cargo test` (тоже из этой директории).

## Использование

```
dbdump <run_id> [--script [--pretty]] [--out DIR] [--db PATH]
```

- `run_id` — id прогона из `replay_runs` (последние прогоны выводятся, если id не найден).
- `--script` — вместо CSV/Parquet сгенерировать JSON-скрипт для HTTP API
  (`POST /script/run`, формат `docs/API.md` §4) из кадров указанного прогона.
  По умолчанию — компактный JSON (тело запроса ограничено 64 КБ в api.rs);
  `--pretty` — многострочный, для чтения человеком.
- `--out DIR` — куда писать файлы (по умолчанию текущая директория).
- `--db PATH` — путь к БД (по умолчанию `%LOCALAPPDATA%\drmod\runs.db`).

По id **record**-прогона дампится сам прогон **и** все связанные playback-прогоны
(`source_replay_id = id`); по id **playback** — его исходная запись и сам прогон.
Файлы: `run_<id>_<kind>.csv` / `run_<id>_<kind>.parquet` (kind = `record` | `playback`).

### Пример

```bash
dbdump 73 --out ./dump
# Прогон id=73 kind=record mission=P118_BEACH (1021 кадров) ...
#   run_73_record: 1021 кадров -> dump/run_73_record.csv / dump/run_73_record.parquet
#   run_74_playback: 1003 кадров -> dump/run_74_playback.csv / ...
```

## Режим `--script` (запись → скрипт API)

Конвертирует запись в JSON-скрипт для `POST /script/run` — воспроизведение
записанного ввода через HTTP API (без debug-панели playback). Файл:
`run_<id>_script.json`.

```bash
dbdump 117 --script --out ./out
# Прогон id=117 kind=record mission=P310_RESTART (618 кадров) ...
#   run_117_script: 618 кадров -> out/run_117_script.json
```

Скрипт **взведён триггером** на позиции первого кадра записи (старт миссии /
контрольная точка): `trigger.pos` — допуск как у отложенного старта
record/playback (±0.1 м X/Z, ±1.0 м Y). Удаление поля `trigger` из JSON —
немедленный запуск.

Конвертация (`src/script.rs`):
- биты `InputUnit.buttons_down` → семантические поля скрипта (биты — общие
  с модом, `drmod-replay-types::input_bits`); `ripper_pressed` → `ripper`;
  `right_stick` → `camera`; `left_stick` эмитится явно, если отличается от
  подразумеваемого направлениями (в записи 117 при FORWARD|RIGHT стик
  (0,-1000), а не (1000,-1000));
- одинаковые подряд идущие входы сливаются в команды `{t, duration}`
  (фронты `pressed` `script_tick` воспроизводит на старте команды);
  ripper — отдельная 1-кадровая команда; пустые кадры пропускаются;
- `t` = `frame_index` записи (таймлайн совпадает с записью).

Ограничения: `buttons_released`/`alternated` не воспроизводятся (формат
скрипта их не поддерживает); биты 0x1/0x8/0x10 трактуются как геймплейные
(weapon_select/ar_mode/jump), а не меню-навигация; `pause` (0x20) маппится,
но `script_tick` его пока не применяет. Записи длиннее 3600 кадров (60 сек)
отклоняются — лимит API.

## Схема колонок (83)

Единая для CSV и Parquet (порядок и типы заданы один раз в `src/dump.rs`,
`SCHEMA`): мета прогона (`replay_id`, `kind`, `mission_id`, `mission_name`,
`started_at`, `frame_count`, `duration_ms`, `source_replay_id`), `frame_index`,
`frame_duration_ms`, все поля `InputUnit` (4 битмаски + стики/триггеры),
`PlayerState` (позиция/поворот/скорость, HP, r_anim, оружие, кнопки, ripper/blade),
`CameraState` (позиция, look-at, roll, `vp_00`..`vp_33`), производные `cam_yaw`/
`cam_pitch` (из pos→lookAt), `blade_down`, `ripper_pressed`, `raw_down_0..5`,
`raw_pressed_0..5`.

- Старые строки без raw-данных (до 2026-08-23) — пустая ячейка CSV / `null` в parquet.
- Parquet-колонки все nullable.
- `cam_yaw`/`cam_pitch` — радианы, из `atan2(look_at - pos)`.

## Ограничения

- **Legacy camera-формат** (записи до 2026-08-18, camera BLOB 76 байт вместо 92)
  не поддерживается — понятная ошибка с подсказкой. Актуальные прогоны — от id 29.
- Мод БД не читает (воспроизведение идёт из памяти сессии); если колонки
  `blade_down`/`ripper_pressed`/`raw_*` ещё не добавлены миграцией (мод не
  запускался после 2026-08-23), дамп читает их как 0/NULL — без ошибки.
