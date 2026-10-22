# Ревью по Google Rust Style Guide

Дата: 2026-10-22. Область: весь репозиторий (115 `.rs`, ~28 700 строк, 12 first-party
крейтов + вендоренный форк `drmod-hudhook`). Код мода при ревью не менялся; правки
форматирования — отдельный шаг, описан в конце (`§ Форматирование`).

Метод: чтение исходников, `git grep`/`rg`, `cargo fmt --check` (rustfmt 1.9.0-stable).
Claude/линтеры не запускались: CI их не запускает, а сборка i686 долгая.

Ссылки на правила даны по разделам гайда
(<https://google.github.io/styleguide/rustguide.html>). Правила с метками `[chrome]` /
`[Fuchsia]` к проекту не применимы; правила API-дизайна для application code сам гайд
называет «strong recommendation», поэтому приоритеты расставлены с учётом этого.

## Сводка

| Раздел гайда | Статус | Главное |
|---|---|---|
| Use the appropriate formatting tool | ✅ после правки | 95 файлов не проходили пофайловый `rustfmt --check` (74 — `cargo fmt --check`); конфига не было |
| Unsafe code | ✅ first-party / ⚠️ hudhook | 214 блоков закрыты `SAFETY:`; в вендоренном форке (81 блок) — по-прежнему нет |
| Handling poison | ❌ | `into_inner()` на poisoned mutex в 4 местах |
| Don't use glob imports | ⚠️ | `imgui::*`, `windows::…::*` в 5 производственных местах |
| A use statement imports one module | ⚠️ | 2 места с `mod::Type` внутри `{}` |
| Prefer locally-scoped trait imports | ⚠️ | `use std::fmt::Write as _` на уровне модуля ради одного `write!` |
| Reporting errors / expect over unwrap | ⚠️ | 132 `unwrap`, 121 `expect`; есть check-then-unwrap |
| Prefer enum over bool in APIs | ⚠️ | 20 функций с `bool`-параметрами, две — с двумя подряд |
| Accept flexible references | ⚠️ | `Option<&Vec<u8>>`, `&Arc<Mutex<T>>` по всей API-поверхности |
| `#[non_exhaustive]` / `#[must_use]` | ❌ | 0 / 1 (и тот в вендоренном коде) |
| Naming | ⚠️ | 16 `get_*`, устаревшее имя либы `drmod_rs_lib` |
| Documentation | ✅/⚠️ | Модульные доки отличные, точечные пробелы у публичных item'ов |
| Macros / catch_unwind / unstable | ✅ | `catch_unwind` 0, `#![feature]` 0, proc-macro нет |
| Iterator rules | ✅ | 1 цепочка в заголовке `for`, `for_each` только в вендоренном коде |

## P1 — нарушения, которые гайд формулирует жёстко

### 1. `unsafe` без `SAFETY:` — закрыто в first-party

Гайд: *«Add a safety comment starting with `SAFETY:`… Add a technical justification for
unsafe code»*.

Было: 295 `unsafe { }`, 33 `unsafe fn`, 27 `transmute`, 7 `static mut`, при этом
`SAFETY:` — 5.

Сделано: у всех **214** блоков first-party крейтов появился построчный `// SAFETY:` с
конкретным инвариантом (что проверено до блока, почему чтение/запись корректны):
`drmod-core` — 195, `drmod-protocol` — 2, `drmod-replay-types` — 2,
`drmod-tas-editor` — 8, `drmod-cli` — 4, `drmod-injector` — 2, `tools/disasm` — 1.
У публичных `unsafe fn` внутри крейта (`skeleton::read_full_skeleton`,
`d3d_render::draw_lines_3d`) добавлены секции `# Safety`.

Самыми рискованными были (теперь с обоснованием):

- `drmod-core/src/lib.rs:184-210` — разбор `CONTEXT` при `STATUS_SINGLE_STEP` и ручное
  чтение стека игры (`*sp`, `*sp.add(i)` до 48 слов).
- `drmod-core/src/render_hooks.rs`, `drmod-core/src/tas/hooks.rs` — `transmute`
  трамплинов MinHook в сигнатуры методов/детачей и вызовы оригиналов с аргументами игры.
- `drmod-core/src/lib.rs:368` — `transmute::<i32, GameMenuStatus>` под проверкой `0..=18`.
- `drmod-replay-types/src/lib.rs:217-228` — `to_bytes<T>`/`from_bytes<T>` через
  `from_raw_parts`/`read_unaligned`; инвариант «T — POD без padding» теперь записан в
  `SAFETY:`, но сам контракт по-прежнему не выражен в типах.
- `drmod-protocol/src/lib.rs:51-57` — `transmute` между `[u8; 28]` и `#[repr(C, packed)]`
  пакетом.

Осталось:

- **`drmod-hudhook` — 81 блок** без `SAFETY:` (кроме двух в `util.rs`). Это вендоренный
  форк: трогать его стиль в общем проходе не стали. Либо отдельный проход, либо явное
  исключение для `vendor`-кода.
- **Включить `clippy::undocumented_unsafe_blocks`** — иначе новые блоки снова появятся без
  комментариев. Для этого нужен `[workspace.lints]` в корне + `[lints] workspace = true` у
  членов (у `drmod-tas-editor`, `xtask`, `tools/disasm` — свои таблицы).
- `transmute` в `GameMenuStatus` и `transmute`-сериализация в `drmod-protocol` /
  `drmod-replay-types` остаются корректными, но хрупкими: `SAFETY:` фиксирует инвариант, а
  не заменяет `TryFrom` или безопасный энкодер (`to_le_bytes`).

Отдельно: `drmod-hudhook/src/hooks/dx9.rs:42-44` и `drmod-hudhook/src/lib.rs:132-133`
объявляют `static mut` для `OnceLock`/`OnceCell`, которым мутабельность не нужна, а
предупреждение о ссылках на `static mut` подавлено глобально
(`#![allow(static_mut_refs)]`, `lib.rs:96`). Для вендоренного форка — P3.

### 2. `Handling poison` — прямое нарушение

Гайд: *«Never reach inside a poisoned mutex to get at its insides, no matter how tempting
the use case may seem.»*

`drmod-core/src/tas/replay.rs:183-186, 276-279, 375-378, 472-474`:

```rust
let mut g = match PLAYBACK_FEED.lock() {
    Ok(x) => x,
    Err(e) => e.into_inner(),   // запрещено
};
```

Комментарий «чтобы не сорвать запись» понятен, но гайд запрещает именно это. Правильный
путь — не отравлять мьютекс (не паниковать под ним) либо явно задокументированное
исключение на уровне проекта.

### 3. `Don't use glob imports`

Производственные (не тестовые) нарушения:

- `drmod-core/src/lib.rs:3`, `drmod-core/src/overlay.rs:1`, `drmod-core/src/ui.rs:8` —
  `use imgui::*;`
- `drmod-core/src/d3d_render.rs:4` — `use windows::Win32::Graphics::Direct3D9::*;`
- `drmod-hudhook/src/renderer/input.rs:8,13`, `msg_filter.rs:4`,
  `renderer/backend/dx9.rs:10` (вендоренный)

Оправданы исключением: `use super::*` в inline-тестах (10 мест),
`tracing_subscriber::prelude::*` (сторонний prelude, reviewer discretion),
`pub use drmod_script::*` в `drmod-tas-editor/src/script/mod.rs:7` (non-leaf re-export).
`imgui::*` формально не prelude, но де-факто им пользуются как prelude — стоит либо сузить
до реально нужных имён, либо зафиксировать исключение.

### 4. `A use statement should import items from one module`

- `drmod-server/src/http.rs:3` — `use axum::{Router, extract::State, response::Html, routing::get};`
- `drmod-tas-editor/src/editor/workspace_panel.rs:15` —
  `use crate::editor::{mod_panel::ModPanelView, Editor, PendingFolder};`

### 5. `Prefer locally-scoped trait imports`

`drmod-script/src/json.rs:16` — `use std::fmt::Write as _;` на уровне модуля ради
единственного `write!`. То же по смыслу: `drmod-injector/src/main.rs:4` (`OsStrExt`, одно
использование). `use std::io::{Read, Write}` в `api.rs:33`, `net.rs:3`,
`drmod-cli/src/main.rs:20` — пограничные: используются 2-5 раз, допустимо.

## P2 — API-дизайн и обработка ошибок

### `Prefer enum over bool`

20 таких функций. Самые показательные — две `bool` подряд, вызов читается как «магия»:

- `drmod-core/src/api.rs:827` — `set_headless(base_addr: usize, on: bool, hold: bool)`
- `drmod-cli/src/main.rs:519` — `call(&self, post: bool, path: &str, …)`
- `drmod-server/src/system.rs:111` — `System::new(enable_mock: bool)`
- `drmod-core/src/game/cutscene_skip.rs:157`, `drmod-core/src/tas/hooks.rs:138,151,164`,
  `drmod-tas-editor/src/api/json.rs:18,51` и др.

Гайд разрешает `bool` с inline-комментарием на call-site, но два булева подряд лучше
свернуть в маленький enum.

### `Accept flexible references`

- `drmod-server/src/system.rs:90` — `mock_skeleton: Option<&Vec<u8>>` → должно быть
  `Option<&[u8]>` (буквально пример из таблицы гайда).
- `&Arc<Mutex<SharedState>>` в ~20 хендлерах `drmod-core/src/api.rs`
  (`1946`, `1957`, `2061`, `2125`, …) — внутри используется только `.lock()`, достаточно
  `&Mutex<SharedState>`; так сигнатуры перестают требовать конкретный smart pointer.
- `&Vec`/`&String` больше нигде нет — хорошо.

### `Prefer pattern matching over check-then-unwrap`

`drmod-core/src/api.rs:2472-2477`:

```rust
if guard.script.as_ref().is_some_and(|s| s.status.is_active()) {
    let s = guard.script.as_ref().unwrap();   // ровно антипаттерн из гайда
```

Лечится `if let Some(s) = guard.script.as_ref().filter(|s| s.status.is_active())`.

### `Prefer expect over unwrap`

132 `unwrap` против 121 `expect`. Большая часть `unwrap` — `lock().unwrap()` в `api.rs`
(22 места), для мьютекса это допустимо, но в DLL паника = падение игры; для инвариантов
лучше `expect("api state mutex")`. В тестах `unwrap` — норма.

### `Reporting errors`

- `drmod-protocol/src/lib.rs:221-225`: `serde_json::to_vec(value).unwrap_or_default()` —
  при ошибке сериализации в сокет уйдёт один `\0`, и клиент будет молча ждать. Должно быть
  `Result` или `expect` с обоснованием (сериализация `TcpMessage` не падает).
- `drmod-core/src/settings.rs:81-87` — `let _ = conn.execute(...)`: запись настройки молча
  не сохраняется; `load_all` (65-78) намеренно глотает ошибки, и это задокументировано —
  ок, но `save` не документирован.
- `drmod-server/src/system.rs:45,52,77,81,95,100` — `let _ = send(..)`/`send_to(..)`: отвал
  клиента не виден вообще нигде (нет логов). Для application-code это допустимо, но стоит
  хотя бы лог на ошибке.
- `Result<T, String>` в `drmod-cli`, `drmod-dbdump`, `drmod-tas-editor/src/api` — теряет
  структуру; в `drmod-script` уже есть образцовый `ScriptFormatException`
  (`src/error.rs`), на него стоило бы равняться в остальных библиотеках.

### Типы

- `drmod-core/src/lib.rs:33` — `fn init_db() -> (String, Option<String>, Option<Connection>)`:
  три поля без имён, вызывающая сторона читает `.0/.1/.2`. Гайд просит named struct
  (исключение для «very local» здесь не срабатывает — функция возвращает tuple наружу).
- `drmod-tas-editor/src/buffers.rs:27` (`type Buffers = BTreeMap<PathBuf, String>`) —
  пограничный случай, но тип имеет собственную семантику; в остальном 11 алиасов — это
  `fn`-указатели, норма.
- `drmod-hudhook/src/hooks/mod.rs:49` — `DummyHwnd(HWND, WNDCLASSEXW)` (вендоренный, P3).

### `#[must_use]` — 1 на весь репозиторий

И то в вендоренном `drmod-hudhook/src/mh.rs:10`. Чистые функции, чей результат
бессмысленно терять: `drmod-protocol::parse_udp` (`lib.rs:179`),
`PositionPacket::to_wire/to_bytes` (`34,51`), `SkeletonPacket::to_wire/from_wire`
(`111,126`), `drmod_script::dsl::render`, `drmod_script::json::read/write` — по гайду это
прямые кандидаты.

### `#[non_exhaustive]` — 0 на весь репозиторий

Публичные enum'ы, которые пересекают границу крейтов/процессов:

- `drmod-protocol/src/lib.rs:173,192` — `UdpPacket`, `TcpMessage` (wire-протокол; `_ =>` в
  матчах тут ещё и полезен для forward-compat);
- `drmod-script/src/commands.rs:16` (`ScriptCommandKind`),
  `drmod-script/src/dsl.rs:141` (`FrameStyle`);
- `drmod-tas-editor/src/mod_install/installer.rs:14` (`ModState`),
  `api/rules.rs:21` (`FpsCapMode`).

Плюс публичные структуры с публичными полями (`PositionPacket`, `InputUnit`, `PlayerState`,
`ScriptRequest`…) — гайд просит `#[non_exhaustive]` либо явное обоснование («описывают
существующую онтологию»). Для wire-DTO типа `PositionPacket` аргумент про фиксированный
layout валиден — стоит записать его в доке, чтобы это не выглядело забывчивостью.

### Дублирование DTO между крейтами

`drmod-replay-types/src/script.rs` (`ScriptInput`, `ScriptCommand`, `ScriptTrigger`,
`RestartSpec`) и `drmod-script/src/model.rs` (те же четыре имени) — независимые описания
одного JSON-контракта, без `From`-конвертеров. Синхронность держится golden-тестами
(`drmod-script-gen/tests/expected_json_is_accepted.rs` десериализует вывод редактора
типами мода) — это сильная защита, её надо сохранить. Но любое поле правится в двух
местах, и ошибка ловится только тестом с фикстурами.

### `Don't implement APIs in non-leaf modules`

- `drmod-tas-editor/src/editor/mod.rs` — 790 строк, не только `mod`/`pub use`, но и
  `Editor`, `ScriptActions`, `RunOutcome`; при этом `use`-блоки (строки 25-26 и 39-50)
  перемешаны с объявлением `pub struct ScriptActions` (33-37).
- `drmod-core/src/game/mod.rs` (105 строк) содержит `is_readable_ptr` и `GameMenuStatus` —
  попадает в исключение «small amount of ties-things-together», но это уже на грани.
- `drmod-core/src/lib.rs` — 1084 строки в корневом модуле (`HelloHud`, обработчик
  исключений, `init_db`), а весь крейт — 11 тыс. строк / 24 файла. Правило «Prefer small
  crates» помечено `[chrome]`, но дух («remarkably large root module») нарушен.

### Naming

- `fn get_*` — 16 мест (`settings.rs:91,100,107`, `cli/main.rs:133,142`,
  `hudhook/util.rs:137`, `inject.rs:111-188`, …). Гайд: геттер — `foo()`, сеттер —
  `set_foo()`. Для свободных helper'ов `settings::get_bool` терпимо, но
  `get_dll_path()`/`get_process_by_name()` — это конструкторы/поиски, им больше подходит
  `dll_path()`/`process_by_name()`.
- Имя либы `drmod_rs_lib` при пакете `drmod-core` (`drmod-injector/src/main.rs:1`) —
  устаревшее имя проекта; гайд требует, чтобы имя говорило о содержимом.
- Аббревиатуры: `db`, `api`, `cfg`, `ctx`, `idx` — общепринятые; `udp`/`tcp`/`hud` — тоже.
  Явных «загадочных» сокращений не найдено.

### Документация

Сильная сторона: у всех крейтов есть `//!` с описанием и ссылками на `docs/*.md`
(`api.rs:1-17`, `drmod-script/src/lib.rs:1-19`, `drmod-tas-editor/src/lib.rs:1-4`).
Пробелы точечные:

- `drmod-protocol/src/lib.rs`: без доков `PKT_POSITION`/`PKT_SKELETON` (5-6),
  `to_bytes`/`from_bytes` (51,55), `SkeletonBone::offset` (91),
  `SkeletonPacket::to_wire/from_wire` (111,126), `UdpPacket` (173).
- `#![deny(missing_docs)]` есть только у `drmod-hudhook/src/lib.rs:97`; для библиотечных
  `drmod-protocol`, `drmod-replay-types`, `drmod-script` стоило бы включить
  `#![warn(missing_docs)]`.
- Смешение языков: `drmod-core`/`drmod-cli`/`drmod-dbdump`/`drmod-replay-types` —
  по-русски, `drmod-script`/`drmod-tas-editor` — по-английски. Гайд язык не
  регламентирует, но внутри одного workspace это повышает цену переключения контекста.

### Assertions

123 `assert!` / 179 `assert_eq!`. Гайд просит контекст. Часть кода — образец
(`drmod-script-gen/tests/expected_json_is_accepted.rs:31-38,83-92`: сообщения объясняют,
что делать), часть — голые условия без пояснений (`drmod-core/src/settings.rs:145-148`,
`drmod-dbdump/src/script.rs:88-150`, `drmod-core/src/render_hooks.rs:420`). В тестах это
P3, но контекст в них дешёвый.

### Гигиена `#[allow]`

56 атрибутов `allow`. Самое заметное: ~36 `#[allow(dead_code)]`, из них 30 подряд в
`drmod-core/src/tas/addresses.rs` — просится один `#![allow(dead_code)]` на модуль с
комментарием «таблица адресов, часть ещё не подключена».
`drmod-dbdump/src/script.rs:19` — `#[allow(unused_imports)]` без пояснения (похоже на
остаток рефакторинга).

## Что сделано хорошо (не сломать)

- **Нет запрещённого**: `panic::catch_unwind` — 0, `#![feature]` — 0, `Rc` — 0,
  `dbg!`/`todo!`/`unimplemented!` — 0, `TODO/FIXME` — 1. По этим пунктам гайд выполнен
  полностью.
- **`Arc` только по делу**: 35 вхождений, все — разделяемое состояние между
  render-потоком и HTTP-потоком (`api.rs`) или tokio-задачи сервера; это ровно исключение
  из «Avoid Rc and Arc».
- **Итераторы**: нет перегруженных цепочек, единственный `for … .items_height(…)` —
  `drmod-tas-editor/src/editor/table.rs:120`; `for_each` — 2 и только в вендоренном
  hudhook.
- **`From`/`Display`/`Default`**: ручные `impl Default` (12) все содержательные
  (например `Settings::default` с `ghost_opacity: 0.5`, `drmod-core/src/settings.rs:24-33`),
  derive там, где можно; `Display for ScriptFormatException` — корректно, derive'ом не
  выразить.
- **`is_empty` без `len`** — не нарушение (правило одностороннее).
- **Строки**: ровно одно `+`-соединение (`drmod-tas-editor/src/settings.rs:176`, два куска
  — допустимо); `format!`/`write!` используются системно.
- **Тесты**: 119 `#[test]`, golden-фикстуры, приёмочный тест JSON «типами мода» — по
  разделу «Use the right per-codebase test framework» проект в выигрыше: `#[test]` здесь и
  есть правильный фреймворк (спец-правила есть только для Chrome/Fuchsia).
- **`drmod-script/src/error.rs`** — эталон для остальных крейтов: приватные поля, доки на
  каждый метод, `impl Into<String>` в конструкторе библиотечного типа, `ScriptResult<T>`.

## Форматирование

### Решение

Принят **дефолтный rustfmt** (`max_width = 100`, который у rustfmt и так по умолчанию), без
`rustfmt.toml`, плюс удалён `drmod-hudhook/rustfmt.toml`.

Почему так:

- Гайд требует rustfmt с `max_width = 100` — это дефолт. Дополнительных настроек у гайда
  нет, а собственный конфиг проекта (единственный был у вендоренного hudhook) на 10 из 15
  строк состоял из nightly-only опций, которые stable-rustfmt игнорирует с предупреждением.
  Из-за этого форматирование было невоспроизводимым: пофайловый `rustfmt --check` давал diff
  в 95 файлах из 115 (`cargo fmt --check` по трём workspace'ам — в 74).
- Замер альтернативы: `use_small_heuristics = "Max"` (то, что было в конфиге hudhook) даёт
  **хуже** совпадение с текущим кодом — 105 файлов / 1225 хунков против 95 файлов / 645
  хунков у дефолта. Автор форматировал код в основном дефолтом.
- `hex_literal_case = "Lower"` (гайд рекомендует) сознательно **не** включён: в коде 353
  hex-литерала с `A-F`, и почти все — адреса, которые в `docs/*.md` и в комментариях
  записаны в верхнем регистре (`base+0x177B4A4`). Смена регистра только в коде развела бы
  код и документацию. Дефолт rustfmt регистр сохраняет; вернуться к этому можно отдельным
  решением вместе с правкой документации.

### Что изменено

- `cargo fmt` во всех трёх Cargo-workspace'ах (root, `drmod-tas-editor`, `xtask`) и в
  `tools/disasm` (свой манифест, не входит в workspace). Итог: 86 файлов, +2391/−962.
- Удалён `drmod-hudhook/rustfmt.toml` (nightly-only настройки; после удаления форк
  форматируется тем же дефолтом, что и остальной репозиторий).
- Добавлен корневой `.gitattributes` с `*.rs text eol=lf` — иначе rustfmt пишет LF, а
  `core.autocrlf=true` возвращает CRLF при checkout, и рабочее дерево «мигает» между
  окончаниями строк. Сам `cargo fmt --check` проходит и на CRLF, но LF убирает шум в
  `git status`/`git diff`.
- `AGENTS.md` — обновлён пункт про отсутствие rustfmt-конфига.

### Как проверять

```powershell
cargo fmt --all -- --check
cd drmod-tas-editor; cargo fmt -- --check
cd ../xtask; cargo fmt -- --check
cd ../tools/disasm; cargo fmt -- --check
```

Все четыре команды должны завершаться с кодом 0. В CI этих проверок пока нет — добавить
их стоит вместе с `clippy` (сейчас CI собирает/пакует только на `v*`-теги).

### Pre-commit hook

`.githooks/pre-commit` (POSIX sh, версионируется) проверяет ровно эти же четыре workspace'а, но
только те, в которые попали staged `*.rs` — незакоммиченный неформатированный WIP в другом
крейте коммит не блокирует. Git запускает хук через свой sh и на Windows, поэтому отдельный
PowerShell-вариант не нужен.

Установка (один раз на клон, настройка не коммитится):

```powershell
cargo xtask install-hooks      # = git config core.hooksPath .githooks
```

Обойти один раз: `git commit --no-verify`. Если `cargo` нет в PATH (GUI-клиенты Git иногда
запускают хук с урезанным окружением), хук печатает предупреждение и пропускает проверку, а не
блокирует коммит.

На не-Windows клоне файл должен быть исполняемым в индексе — при добавлении в git:
`git add --chmod=+x .githooks/pre-commit`.

## Что осталось (по приоритету)

1. **Unsafe-аудит — сделан**: 214 `SAFETY:`-комментариев в first-party, `# Safety` у двух
   публичных `unsafe fn`. Осталось: включить `clippy::undocumented_unsafe_blocks` (чтобы
   не поехало снова) и решить судьбу `drmod-hudhook` (81 блок без `SAFETY:`).
2. **Poison**: убрать `into_inner()` в `tas/replay.rs` (4 места).
3. **API-мелочи**: `Option<&[u8]>`, `&Mutex<…>` вместо `&Arc<Mutex<…>>`, `if let` вместо
   check-then-unwrap, `#[must_use]` на чистые функции, `#[non_exhaustive]` на
   `UdpPacket`/`TcpMessage`/`ModState`.
4. **Доки и линты**: `#![warn(missing_docs)]` в трёх библиотечных крейтах, контекст в
   `assert!`, консолидация `#[allow(dead_code)]` в `tas/addresses.rs`.
5. **Глоб-импорты**: сузить `imgui::*` и `windows::…::*` до явных имён (или зафиксировать
   исключение в этом документе).
