# cutscene_skip

Самостоятельный мод для **Metal Gear Rising: Revengeance** — скип in-engine
катсцены «как на консоли» (в сценах `P370_RESTART` / `P370_IN` → `P370_EVENT`).

Логика — порт `src/game/cutscene_skip.rs` основного мода drmod-rs (там источник
истины; синхронизировать при правках). Здесь нет ни imgui, ни hudhook-dx9, ни
HTTP API, ни сети — только кадровый автомат в DLL и лаунчер.

## Состав

- **`cutscene_skip.exe`** — лаунчер: находит игру (рядом с собой → известный
  Steam-путь → `--exe`), при необходимости запускает, инжектит встроенную DLL,
  опционально показывает лог.
- **`cutscene_skip_lib.dll`** — мод: MinHook на `cSlowRateManager::updateFrameTime`
  (`0xA03970`, раз за итерацию главного цикла игры) и кадровый автомат скипа.

DLL встраивается в лаунчер (`include_bytes!`) и распаковывается в
`%LOCALAPPDATA%\cutscene_skip\` при запуске. Лог — там же:
`%LOCALAPPDATA%\cutscene_skip\cutscene_skip.log`.

## Как работает скип

Пока идёт сцена из `WATCH`, мод держит в `Trigger::staFlags` флаги
`STA_SOFT_EVENT` (код 4) и `STA_SOFT_EVENT_SKIP_OK` (код 37) — от них Esc
игрока открывает **консольное катсценное меню** (`GameMenuStatus` = 6,
`cEventPauseMenu`) с пунктами CONTINUE и SKIP. Собственный Skip в PC-версии
инертен, поэтому решение выполняет мод: убирает меню штатным путём движка
(`GameMenuStatus` = 6 + шаг `0x17EA118` = 6), снимает `STA_PAUSE` (код 19) и для
SKIP заказывает следующую подфазу движковой `request_subphase` (с `clear_event`).

Порядок «снять паузу → заказ» обязателен: `STA_PAUSE` стопорит машину загрузки,
и заказ без снятия паузы меняет только подпись.

## Сборка

```sh
cd mods/cutscene_skip
cargo build --release
```

На выходе `target/i686-pc-windows-msvc/release/cutscene_skip.exe` (+ DLL рядом).
Крейт собирается под `i686-pc-windows-msvc` (игра 32-битная) — это задано в
`.cargo/config.toml`; нужны MSVC C++ build tools (собирается C-код MinHook).

Тесты:

```sh
cargo test
```

## Запуск

```sh
cargo run                 # найти/запустить игру и инжектнуть
cargo run -- --follow     # то же + печатать лог мода в консоль
cargo run -- --kill-first --follow   # чистый старт игры (для свежей DLL)
```

Флаги: `--exe <путь>`, `--kill-first`, `--no-launch`, `--follow`,
`--timeout <сек>`. Если игра уже запущена — лаунчер инжектит в неё.

**Живой цикл разработки.** `LoadLibraryW` повторно вызванный `DllMain` не
зовёт, поэтому новая сборка DLL в уже загруженном процессе не подхватится —
для свежего кода перезапускай игру (`--kill-first`). `FreeLibrary`/выгрузку не
делаем: в основном моде горячая перезагрузка DLL крашила игру.

## Зависимости

- `windows` — Win32 API (память, потоки, ToolHelp, окна).
- MinHook — завендорен в `vendor/minhook` (BSD-2-Clause, `LICENSE.txt`), чтобы
  крейт был самодостаточен.

## Вынос в отдельный репозиторий

Крейт намеренно не входит в корневой workspace drmod-rs (свой `[workspace]` и
свой `target/`) и не ссылается на файлы за своими пределами: вынос = перенос
папки `mods/cutscene_skip` целиком в отдельный репозиторий.

## Статус

Проверено живьём 2026-09-12 (инжект лаунчером в уже запущенную игру,
`--no-launch`; прогон `P370_RESTART` → CONTINUE, затем `P370_IN` → SKIP):
патчей не потребовалось, падений нет.

- ✅ **Физический Esc** открывает консольное меню без досылки фронта `pause` —
  хук `cInput::updateInputUnit` (`0x9DAFE0`) **не нужен**: мод прочитал решение
  прямо из объекта меню (`+0x04 == -2` + индекс пункта) в обоих случаях
  (`CONTINUE`, затем `SKIP`).
- ✅ **Детур `updateFrameTime` работает и в паузе** (`GameMenuStatus` = 6) —
  пер-кадровая точка годится, vtable-хук `IDirect3DDevice9::Present` не нужен.
- ✅ Скип: `P370_IN` → `P370_EVENT` (в логе `заказ подфазы P370_EVENT → вызвано`,
  смена сцены подтверждена в игре).
