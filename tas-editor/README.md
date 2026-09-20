# TAS Editor

Десктопный TAS-редактор для Metal Gear Rising: Revengeance — WinUI 3 на
[`windows-reactor`](https://crates.io/crates/windows-reactor) (декларативная компонентная модель поверх
нативных контролов WinUI 3, без XAML).

Сейчас это **мок интерфейса**: данные (мок-скрипты) живут в памяти, диск не трогается — отлаживается компоновка.

## Интерфейс

```
┌──────────────┬───────────────────────────────────────────────────────┐
│ Мок-скрипты  │ ▾ Свойства скрипта                                    │
│ (SplitView)  │   name / trigger / restart / кадр / условные команды   │
│              ├───────────────────────────────────────────────────────┤
│ список +     │ ◀ Скрипты │ кадров: N · выбран кадр K │ + кадр │ − кадр │
│ Сбросить мок │ кадр fwd back left right walk jump L-atk H-atk blade … │
│              │  0    ☑    ☐    ☐   …                                  │
│              │  1    ☑    ☐    ☐   …                                  │
│              ├───────────────────────────────────────────────────────┤
│              │ JSON · 13 строк · 1 команда                            │
│              │ { "name": "blade-run", "commands": [ … ] }             │
└──────────────┴───────────────────────────────────────────────────────┘
```

- **Слева** — список мок-скриптов (разной длины и формы: короткий, с условной командой, длинный) и статус.
- **Сверху справа** — сворачиваемая полоса свойств: `name`, `trigger` (позиция/тики), `restart`, поля выбранного
  кадра (камера, стик) и правка условных команд (`when_enemy`).
- **Середина** — таймлайн-матрица: строка — кадр, колонки — действия-флаги (`forward`, `jump`, `light_attack`,
  `blade`, …) и числовые оси камеры/стика. Клик по галочке переключает действие в кадре, после чего команды
  скрипта пересобираются RLE-нормализацией (`matrix::FrameMatrix::to_commands`).
- **Снизу** — JSON-редактор (`RichEditBox`): правка текста сразу пересобирает модель и матрицу; мок и текст связаны
  двусторонне.

## Сборка и запуск

```bash
cd tas-editor
cargo run --release
```

Прямой запуск `target/x86_64-pc-windows-msvc/release/tas-editor.exe` тоже работает, но только из каталога профиля:
рядом с exe должны лежать staged DLL runtime.

## Модули

| Файл | Что там |
|---|---|
| `src/model.rs` | DTO формата скрипта (`Script`/`Command`/`Input`/`WhenEnemy`/`Trigger`/`Restart`), `deny_unknown_fields`, валидация по `docs/API.md` §4.4 |
| `src/matrix.rs` | производная «кадр × входы», `set_bit`/`set_number`, RLE-сборка команд, условные команды вне RLE |
| `src/mock.rs` | мок-скрипты в памяти |
| `src/editor.rs` | корневой компонент: состояние, сообщения, раскладка |
| `src/panels/*` | панели: список скриптов, свойства, таймлайн, JSON-редактор |
| `src/workspace.rs` | **не подключён** к сборке: рабочая область на диске (список/чтение/запись, настройки, `FolderPicker`) — вернётся, когда мок устоится |

## Тесты

```bash
cd tas-editor
cargo test          # 17 тестов: model, matrix, workspace, панель таймлайна, монтирование раскладки
cargo clippy --all-targets
```

Скриншот для отладки вёрстки (окно должно быть запущено):

```powershell
pwsh -File screenshot.ps1 -Out shot.png
```

- `model`/`matrix` — форматы, валидация, RLE-сборка команд, парсинг моков.
- `panels::timeline::tests` — расстановка колонок: `row_cells()` даёт ровно по номеру 0..N-1 на каждую ячейку.
  Это регрессия на реальный дефект: без `grid_column` все ячейки схлопывались в колонку 0.
- `layout_test` — headless-монтирование через `windows_reactor::test::RecordingRuntime` (`feature = "test"`
  в `[dev-dependencies]`). ⚠️ Проверяет только то, что планировщик принимает раскладку: **дерево снаружи не читается** —
  применение команд живёт в цикле `App` (`Pump::dispatch_events` не публичен), и после `mount_view` в записи остаётся
  один корневой узел (`nodes=1, kinds=[]`). Поэтому структуру (колонки, наличие панелей) проверяем чистыми функциями,
  а не деревом; полный рецепт — в `QWEN.md` и памяти проекта.

## Известные ограничения мока

- Горизонтальной прокрутки таймлайна нет: часть колонок справа (числовые оси) уходит за край.
- Высоты панелей считаются от клиентской высоты окна (`on_window_size`) и задаются самим контролам
  (`ListView::max_height`, `RichEditBox::height`): обёртке они не подчиняются.
- `String`-поля мока — `&'static str`; создание/удаление скриптов появится вместе с рабочей областью.
- Правка текста в `RichEditBox` раздувает хвост переводами строк — нормализуется при приёме (`editor.rs`).

## Self-contained

`build.rs` вызывает `windows_reactor_setup::as_self_contained()`: приватный Windows App Runtime стейджится
рядом с exe, манифест с маркером `windows-reactor-self-contained` встраивается линкером. Поэтому:

- Первая сборка требует сеть — helper скачивает pinned NuGet-пакеты `Microsoft.WindowsAppSDK.Runtime` и
  `Microsoft.Web.WebView2`. Кэш: `%LOCALAPPDATA%\windows-reactor-setup\temp`.
- **Копировать в поставку надо весь runtime-набор** (exe + staged DLL и подкаталоги runtime), а не только exe: манифест
  в exe помечен как self-contained, и приложение идёт за приватным runtime рядом с собой. Каталог профиля Cargo при
  этом — не дистрибутив: рядом с exe лежат ещё артефакты сборки. Готовую поставку собирает `pack.ps1` (см. ниже).
- Стейджить приложение следует из чистого профиля: в общем `target` могут лежать выходы других крейтов.
- **Если staged-файлов нет, а сборка зелёная** — смотреть кэш helper'а. `stage_pkg` считает пакет готовым по факту
  существования `.nupkg` и не проверяет ни код ответа `curl`, ни размер, поэтому обрезанная загрузка (наблюдалось
  13 890 байт вместо 164 053 946) навсегда остаётся «пакетом»: helper не находит ожидаемый MSIX, печатает
  `MSIX not found at …` (видно только при `cargo build -v` — обычный вывод build-скрипта Cargo скрывает) и молча
  не копирует ни одного файла. Лечение: удалить
  `%LOCALAPPDATA%\windows-reactor-setup\temp\Microsoft.WindowsAppSDK.Runtime.<ver>.nupkg` вместе с распакованным
  каталогом рядом, скачать пакет вручную
  (`https://api.nuget.org/v3-flatcontainer/microsoft.windowsappsdk.runtime/<ver>/microsoft.windowsappsdk.runtime.<ver>.nupkg`,
  ~156 МБ) и положить на место, затем сбросить фингерпринт build-скрипта
  (`target/x86_64-pc-windows-msvc/release/build/tas-editor-*/`) и пересобрать. **`cargo clean -p tas-editor`
  build-скрипт не перезапускает** — после него сборка просто печатает `Fresh tas-editor`.
- Проверка, что стейджинг удался: рядом с exe лежат `Microsoft.WindowsAppRuntime.dll`, `Microsoft.UI.Xaml.dll`,
  `CoreMessagingXP.dll`, `resources.pri`, `Microsoft.Web.WebView2.Core.dll` и каталоги локалей (`en-us`, …) —
  ~130 записей верхнего уровня, ~117 МБ.

## Поставка (`pack.ps1`)

Каталог профиля Cargo — это каталог сборки, а не дистрибутив: вместе с exe там лежат `deps/` (~55 МБ rlib-ов),
`build/`, `.fingerprint/`, `incremental/`, `examples/`, `tas_editor.pdb`, `tas-editor.d` — в поставку они не идут.
Чистый дистрибутив собирает `pack.ps1`:

```powershell
cd tas-editor
pwsh -File pack.ps1 -Build -Zip
```

Скрипт пересоздаёт `dist/` и кладёт туда exe со всем runtime-набором из `target/<triple>/<profile>`, **кроме**:

| Исключается | Причина |
|---|---|
| `deps/`, `build/`, `.fingerprint/`, `incremental/`, `examples/`, `*.d`, `*.pdb`, `.cargo-*` | артефакты сборки cargo (~57 МБ) |
| `Microsoft.Web.WebView2.Core.dll` | нужен только приложениям с XAML-контролом WebView2 (`-IncludeWebView2`) |
| языковые каталоги не из `-KeepCultures` | по умолчанию остаются `en-us`, `ru-RU`; `-KeepCultures all` — все (~3.5 МБ) |

Итог — **~56 МБ** вместо ~117 МБ. Флаги: `-Profile debug|release`, `-OutDir DIR`,
`-KeepCultures en-us,ru-RU|all`, `-IncludeWebView2`, `-Build`, `-Zip`.

Скрипт идемпотентен и падает с ненулевым кодом, если нет exe или в поставке не оказалось обязательного ядра
(`Microsoft.WindowsAppRuntime.dll`, `Microsoft.UI.Xaml.dll`, `Microsoft.UI.Xaml.Controls.dll`, `CoreMessagingXP.dll`,
`resources.pri`) — то есть заодно диагностирует незавершённый staging из раздела выше.

⚠️ Чистить нужно **копию** (`dist/`), а не рабочий `target/.../release`: удалив оттуда runtime, вы сломаете локальный
`cargo run`, и cargo его не вернёт, пока не сброшен фингерпринт build-скрипта.

## Свой `.cargo/config.toml`

Файл фиксирует `x86_64-pc-windows-msvc`: корневой конфиг drmod-rs задаёт `i686-pc-windows-msvc` (32-битная
игра), а WinUI 3 / Windows App SDK под i686 не собирается. Крейт — собственный `[workspace]`, в корневой
workspace не входит (как `mods/cutscene_skip`).

## Что читать по Reactor

- Гайд: `docs/crates/windows-reactor.md` в [microsoft/windows-rs](https://github.com/microsoft/windows-rs)
  (компоненты, view, layout, ввод, окна, таймеры, фоновая работа, деплой).
- Примеры: `crates/samples/reactor/*` — 55 сэмплов (`counter`, `form`, `navigation`, `notepad`, `virtual`…).
- API: <https://docs.rs/windows-reactor>.
