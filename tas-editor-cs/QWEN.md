# tas-editor-cs

C# TAS editor for **Metal Gear Rising: Revengeance** — WinUI 3 through `Microsoft.UI.Reactor`
(declarative components and hooks, no XAML, no bindings, no ViewModels).

**This project has its own conventions.** The root `QWEN.md` describes the Rust repo (`drmod-rs`);
its rules do not carry over here — most importantly the Russian-UI rule. When a convention in this
file disagrees with the root one, this file wins for everything under `tas-editor-cs/`.

Build, publish and the publish gotcha: `README.md` in this folder.

## Language

- **English only.** In-app text, code comments, XML doc comments, identifiers, file names, and this
  file. No Cyrillic in source files.
- This overrides the root rule *"UI messages are in Russian"* — that rule belongs to the Rust
  overlay (`src/`, `tas-editor/`), not to this project.
- The Rust sibling's conventions stay Russian; do not copy its wording into this project and do not
  apply this project's language rule back to it.

## Components

- A component is a class: `class X : Component` with `public override Element Render()`.
- **One component per file**, file named after the class (`Editor.cs` → `class Editor`).
- `App.cs` keeps `ReactorApp.Run<App>(...)` and the root component — it is the entry point, not a
  place for screen contents.
- A piece used by more than one screen becomes its own component file; a piece only one screen uses
  stays in that screen's file until a second caller appears.
- Once an area has more than a couple of component files, group them in a folder named after the
  area (the Rust sibling's `panels/` is the precedent).

## Verification

- `mur check TasEditorCs.csproj` **is** the build (it runs `dotnet build` under the hood). Do not
  re-run `dotnet build` after a green `mur check`.
- The `REACTOR_*` analyzer warnings are part of the build; `mur check` links each one to the skill
  section that explains it — read the pointer instead of guessing, and use its `→ try: <name>`
  suggestions verbatim.
- Compiling is not enough: layout, hook-order and binding mistakes build fine. Before calling a UI
  change done, launch it (`dotnet run`, or the exe) and look at the window.

## Publishing

- `dotnet publish -c Release -o publish` — self-contained + NativeAOT, gives a native x64 exe that
  runs from any folder.
- ⚠️ The `_PublishAppPri` target in `TasEditorCs.csproj` is load-bearing: `dotnet publish` does not
  copy the app PRI on its own, and without it the published exe dies with `0xC000027B` before the
  first frame. The debug loop does not show this. Background in `README.md`.

## Status

- Hello world: `TitleBar`, a heading bound to `UseState`, a `TextBox`, Mica backdrop.
- The Rust version (`../tas-editor/`) is left untouched; this project is the candidate replacement.
