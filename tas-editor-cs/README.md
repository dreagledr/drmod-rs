# TAS Editor (C#)

Desktop TAS editor for Metal Gear Rising: Revengeance on **WinUI 3** through
[`Microsoft.UI.Reactor`](https://microsoft.github.io/microsoft-ui-reactor/) — a declarative
React-style component model (components, hooks, keyed lists), no XAML, no bindings, no ViewModels.

Status: **hello world** — a window, a `TitleBar`, a text box bound to state. This is the C# version
living next to the Rust one (`../tas-editor/`); that one is left untouched.

Folder conventions (language, component layout) — `QWEN.md`.

## Build and run

```bash
cd tas-editor-cs
dotnet build            # Debug, dev loop
dotnet run
```

Scaffolded with `dotnet new reactorapp -n TasEditorCs -o . --aot` (template
`Microsoft.UI.Reactor.ProjectTemplates` 0.1.0-preview.15; framework `Microsoft.UI.Reactor`
0.1.0-preview.15, `Microsoft.WindowsAppSDK` 2.5.1). The folder is separate — the root `cargo build`
does not see it.

The project shape comes from the template: `<Platforms>x64;ARM64;X86</Platforms>` and a
`RuntimeIdentifier` resolved from the host SDK (`$(NETCoreSdkPortableRuntimeIdentifier)`), so a bare
`dotnet build` works without `-p:Platform=x64` (the Windows App SDK self-contained path requires a
concrete RID).

Build check with Reactor diagnostics (skill pointers, `→ try:` suggestions):

```bash
mur check TasEditorCs.csproj
```

`mur check` **is** the build (it runs `dotnet build` under the hood); there is no point re-running
`dotnet build` after a green `mur check`.

## Publish

```bash
dotnet publish -c Release -o publish
```

The template's AOT block (`--aot`) matches the Native AOT section of the Reactor packaging guide:
`PublishAot=true` + `InvariantGlobalization=true` (the recommended pair — shipping full ICU data
under AOT yields actionable trim warnings), plus `WindowsAppSDKSelfContained=true` from the template.

- the output is a **native** x64 exe (~9.5 MB): no `coreclr.dll`, `hostpolicy.dll`, `TasEditorCs.dll`
  or `TasEditorCs.runtimeconfig.json` in the payload;
- a private Windows App Runtime sits next to it, so no separate .NET runtime install is needed — run
  it from any folder, ship it as a zip;
- the whole folder is ~170 MB / 296 files: Windows App SDK 2.5.1 pulls AI/ML pieces into the
  self-contained runtime as well (`DirectML.dll`, `onnxruntime.dll`, `Microsoft.Windows.AI.*`,
  SemanticSearch). The payload is not thinned — in the Rust version `tas-editor/pack.ps1` does that.

`PublishAot` is set without a configuration condition on purpose: the AOT compilation itself only
happens on `publish`, but this switch is what turns the trim/AOT analyzers on during an ordinary
build, so incompatible reflection surfaces in the dev loop instead of at packaging time.

## Gotchas

### `dotnet publish` does not copy the app PRI

The first published exe dies after ~100 ms with `0xC000027B` (`STATUS_STOWED_EXCEPTION`) — an
unhandled exception inside XAML. Cause: the publish folder is missing `TasEditorCs.pri`, the merged
resource index the XAML compiler writes next to the exe in `bin\<config>\<tfm>\win-x64\`.

The missing PRI has nothing to do with AOT: it reproduces on an untouched `dotnet new reactorapp`
scaffold (with and without `--aot`), on a JIT publish, and with the documented recipe
`dotnet publish -c Release -r win-x64`. The Windows App SDK hands `@(ProjectPriFile)` to its own MSIX
pipeline only (MrtCore.PriGen), while a plain unpackaged publish builds its file list from the
project output groups, which do not carry that file. Fixed by the `_PublishAppPri` target in
`TasEditorCs.csproj` (adds `$(TargetDir)$(TargetName).pri` to `ResolvedFileToPublish`).

⚠️ An XML comment cannot contain `--` (a double hyphen), and flag names like `--aot` practically
invite one — the build fails with `MSB4025`.

Sanity check that a publish is alive: `publish\` contains `TasEditorCs.pri` and
`publish\TasEditorCs.exe` opens a window.

### Other

- `CS0436` during the build (`ReactorApplication` conflicts with the type from `Reactor.dll`) is a
  Reactor generator warning — it is there on an untouched scaffold too, not ours.
- Reflection vs trim: `Factories.AutoColumns<T>()` (walks `typeof(T).GetProperties()`) and devtools
  (walks `Assembly.GetTypes()`). For retail/AOT the devtools switch stays off — in the csproj
  `Microsoft.UI.Reactor.Devtools` and `Reactor.DevtoolsSupport=true` are wired only for
  `Configuration=Debug`.
- `Microsoft.UI.Reactor.Advanced` (Win2D canvases, `DataGrid`, Markdown, charts, docking) is not
  referenced: it roots the WinRT activation chain for the AOT trimmer — add it only when actually
  needed.

## Template

`Microsoft.UI.Reactor.ProjectTemplates` **0.1.0-preview.15** is installed (updated 2026-09-21 from
`0.0.0-local`). Parameters: `--aot` (the ready-made AOT block), `-f net10.0`,
`-M/--MSUIReactorVersion <version>`. To check or update:

```bash
dotnet new list reactor
dotnet new install Microsoft.UI.Reactor.ProjectTemplates@<version>
```
