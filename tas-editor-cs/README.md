# TAS Editor (C#)

Десктопный TAS-редактор для Metal Gear Rising: Revengeance на **WinUI 3** через
[`Microsoft.UI.Reactor`](https://microsoft.github.io/microsoft-ui-reactor/) — декларативная
компонентная модель в стиле React (компоненты, хуки, keyed-списки), без XAML, без биндингов
и без ViewModel.

Статус: **hello world** — окно, `TitleBar`, поле ввода с состоянием. Это C#-версия рядом с
Rust-версией (`../tas-editor/`); та остаётся как есть и не трогается.

## Сборка и запуск

```bash
cd tas-editor-cs
dotnet build            # Debug, dev-цикл
dotnet run
```

Создан шаблоном `dotnet new reactorapp -n TasEditorCs -o . --aot` (шаблон
`Microsoft.UI.Reactor.ProjectTemplates` 0.1.0-preview.15, фреймворк — `Microsoft.UI.Reactor`
0.1.0-preview.15, `Microsoft.WindowsAppSDK` 2.5.1). Каталог отдельный, из корневого `cargo build`
не виден.

Форма проекта — из шаблона: `<Platforms>x64;ARM64;X86</Platforms>` и `RuntimeIdentifier`,
автоматически берущийся из host SDK (`$(NETCoreSdkPortableRuntimeIdentifier)`), чтобы обычный
`dotnet build` работал без `-p:Platform=x64` (self-contained путь Windows App SDK требует
конкретный RID).

Проверка сборки с диагностикой Reactor (ссылки на скиллы, `→ try:`-подсказки):

```bash
mur check TasEditorCs.csproj
```

`mur check` — это и есть сборка (`dotnet build` внутри), повторять `dotnet build` после зелёного
`mur check` не нужно.

## Публикация

```bash
dotnet publish -c Release -o publish
```

AOT-блок шаблона (`--aot`), совпадает с разделом Native AOT в руководстве по публикации Reactor:
`PublishAot=true` + `InvariantGlobalization=true` (рекомендованная пара — полные ICU-данные под AOT
дают actionable trim-предупреждения), плюс `WindowsAppSDKSelfContained=true` из шаблона.

- на выходе **нативный** x64 exe (~9.5 МБ): `coreclr.dll`, `hostpolicy.dll`, `TasEditorCs.dll`,
  `TasEditorCs.runtimeconfig.json` в поставке отсутствуют;
- рядом лежит приватный Windows App Runtime, .NET runtime отдельно ставить не нужно — запуск из
  любой папки, поставка = zip;
- вся папка ~170 МБ / 296 файлов: Windows App SDK 2.5.1 тащит в self-contained рантайм и AI/ML-части
  (`DirectML.dll`, `onnxruntime.dll`, `Microsoft.Windows.AI.*`, SemanticSearch). Прореживания
  поставки нет — у Rust-версии за это отвечает `tas-editor/pack.ps1`.

`PublishAot` стоит без условия на конфигурацию: сама компиляция AOT идёт только на `publish`, но
именно этот переключатель включает trim/AOT-анализаторы в обычной сборке, поэтому несовместимая
рефлексия всплывает в dev-цикле, а не на упаковке.

## Грабли

### `dotnet publish` не копирует PRI приложения

Первый же опубликованный exe падает через ~100 мс с `0xC000027B` (`STATUS_STOWED_EXCEPTION`) —
необработанное исключение внутри XAML. Причина: в publish-папке не оказывается
`TasEditorCs.pri` — слитого индекса ресурсов, который XAML-компилятор кладёт рядом с exe в
`bin\<config>\<tfm>\win-x64\`.

Отсутствие PRI к AOT отношения не имеет: воспроизводится на нетронутом скаффолде
`dotnet new reactorapp` (и с `--aot`, и без), на JIT-публикации, и по документированному рецепту
`dotnet publish -c Release -r win-x64`. Windows App SDK отдаёт `@(ProjectPriFile)` только своему
MSIX-пайплайну (MrtCore.PriGen), а обычная unpackaged-публикация собирает список файлов из
output-групп проекта, где этого файла нет. Лечится таргетом `_PublishAppPri` в `TasEditorCs.csproj`
(добавляет `$(TargetDir)$(TargetName).pri` в `ResolvedFileToPublish`).

⚠️ В XML-комментарии нельзя писать `--` (двойной дефис), а имена флагов вида `--aot` в него так и
просятся — сборка падает с `MSB4025`.

Проверка, что публикация живая: в `publish\` есть `TasEditorCs.pri`, и `publish\TasEditorCs.exe`
открывает окно.

### Прочее

- `CS0436` при сборке (`ReactorApplication` конфликтует с типом из `Reactor.dll`) — предупреждение
  генератора Reactor, есть и на нетронутом шаблоне; не наше.
- Рефлексия против trim: `Factories.AutoColumns<T>()` (обходит `typeof(T).GetProperties()`) и
  devtools (обходит `Assembly.GetTypes()`). Для retail/AOT devtools-переключатель остаётся
  выключенным — в `csproj` пакет `Microsoft.UI.Reactor.Devtools` и
  `Reactor.DevtoolsSupport=true` подключены только для `Configuration=Debug`.
- `Microsoft.UI.Reactor.Advanced` (Win2D-канвасы, `DataGrid`, Markdown, графики, docking) не
  подключён: он roots WinRT-цепочку активации для AOT-триммера — добавлять только когда реально
  понадобится.

## Шаблон

Установлен `Microsoft.UI.Reactor.ProjectTemplates` **0.1.0-preview.15** (обновлён 2026-09-21 с
`0.0.0-local`). Параметры: `--aot` (готовый AOT-блок), `-f net10.0`,
`-M/--MSUIReactorVersion <version>`. Проверить/обновить:

```bash
dotnet new list reactor
dotnet new install Microsoft.UI.Reactor.ProjectTemplates@<version>
```
