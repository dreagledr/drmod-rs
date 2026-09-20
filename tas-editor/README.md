# TAS Editor

Заглушка десктопного TAS-редактора для Metal Gear Rising: Revengeance — WinUI 3 на
[`windows-reactor`](https://crates.io/crates/windows-reactor) (декларативная компонентная модель поверх
нативных контролов WinUI 3, без XAML).

Сейчас это hello world: одно окно с заголовком `TAS Editor`. Логики редактора нет.

## Сборка и запуск

```bash
cd tas-editor
cargo run --release
```

Первый запуск `hello world` — проверка того, что toolchain и self-contained runtime на месте. Прямой запуск
`target/x86_64-pc-windows-msvc/release/tas-editor.exe` тоже работает, но только из каталога профиля: рядом с exe
должны лежать staged DLL runtime.

## Self-contained

`build.rs` вызывает `windows_reactor_setup::as_self_contained()`: приватный Windows App Runtime стейджится
рядом с exe, манифест с маркером `windows-reactor-self-contained` встраивается линкером. Поэтому:

- Первая сборка требует сеть — helper скачивает pinned NuGet-пакеты `Microsoft.WindowsAppSDK.Runtime` и
  `Microsoft.Web.WebView2`. Кэш: `%LOCALAPPDATA%\windows-reactor-setup\temp`.
- **Копировать в поставку надо весь каталог профиля** (exe + staged DLL и подкаталоги runtime), а не только exe.
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

## Свой `.cargo/config.toml`

Файл фиксирует `x86_64-pc-windows-msvc`: корневой конфиг drmod-rs задаёт `i686-pc-windows-msvc` (32-битная
игра), а WinUI 3 / Windows App SDK под i686 не собирается. Крейт — собственный `[workspace]`, в корневой
workspace не входит (как `mods/cutscene_skip`).

## Что читать по Reactor

- Гайд: `docs/crates/windows-reactor.md` в [microsoft/windows-rs](https://github.com/microsoft/windows-rs)
  (компоненты, view, layout, ввод, окна, таймеры, фоновая работа, деплой).
- Примеры: `crates/samples/reactor/*` — 55 сэмплов (`counter`, `form`, `navigation`, `notepad`, `virtual`…).
- API: <https://docs.rs/windows-reactor>.
