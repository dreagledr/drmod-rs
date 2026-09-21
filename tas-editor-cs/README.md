# TAS Editor (C#)

Desktop TAS editor for Metal Gear Rising: Revengeance on **WinUI 3** through
[`Microsoft.UI.Reactor`](https://microsoft.github.io/microsoft-ui-reactor/) — a declarative
React-style component model (components, hooks, keyed lists), no XAML, no bindings, no ViewModels.

Status: **two-pane shell with a live command table and the script formats wired.** The window is split
by a draggable divider — the left pane lists the workspace scripts (the selection is live, the
management buttons are placeholders), the right pane stacks three regions — script controls, command
table, script text — with the same drag-resize splitters between them. The command table region is
filled in (read-only, over mock frames); the other two regions are still a one-line note, and the
on-disk workspace is the next pass. The three representations of a script — the API JSON, the `.tas`
text and the table's frames — round-trip through `Script/` (see *Script formats* below).

This is the C# version living next to the Rust one (`../tas-editor/`); that one is left untouched.
Folder conventions — language, component layout, what to run before calling something done —
are in `QWEN.md`.

## Layout

```
tas-editor-cs/
├── TasEditorCs.slnx        # solution: the app and its tests
├── TasEditorCs/            # the app (WinUI 3, self-contained + NativeAOT)
│   ├── App.cs              # entry point: ReactorApp.Run + docking registration
│   ├── Editor.cs           # window shell: owns the split and the selection
│   ├── WorkspacePanel.cs   # left pane — the script list and its management
│   ├── ScriptPanel.cs      # right pane — the three stacked regions
│   ├── CommandTable.cs     #   the command table region (read-only DataGrid)
│   ├── ScriptEntry.cs      # the script model
│   ├── CommandRow.cs       #   one script frame + the generated mock frames
│   ├── Script/             #   the script formats: model, JSON, DSL text, frames
│   ├── Assets/ Properties/
│   └── TasEditorCs.csproj
└── TasEditorCs.Tests/      # xUnit, headless unit layer
    └── Fixtures/           # JSON from the Rust tool + the editor's goldens
```

## Build, run, test

```bash
dotnet build TasEditorCs.slnx
dotnet run --project TasEditorCs/TasEditorCs.csproj
dotnet test TasEditorCs.slnx
```

Reactor's own diagnostics (skill pointers, `→ try:` suggestions) come from `mur check`, which is a
build in its own right:

```bash
mur check TasEditorCs/TasEditorCs.csproj
```

Scaffolded with `dotnet new reactorapp -n TasEditorCs --aot` (template
`Microsoft.UI.Reactor.ProjectTemplates` 0.1.0-preview.15; framework `Microsoft.UI.Reactor`
0.1.0-preview.15, `Microsoft.WindowsAppSDK` 2.5.1). The folder is separate — the root `cargo build`
does not see it.

## The window shell

The two panes are a `DockManager` whose `Layout` is a `DockSplit(Orientation.Horizontal, …)`: the
left `ToolWindow` sits in a tool-window strip, the right `Document` in the document well. The
divider between them is the framework's drag-resize splitter — there is no hand-rolled splitter code.

Docking lives in `Microsoft.UI.Reactor.Advanced`, not in the base package, and needs a one-time
registration before the first render (`App.cs`):

```csharp
configure: host => DockingNativeInterop.Register(host.Reconciler)
```

It is the one dependency the Reactor packaging guide warns about: Advanced roots its WinRT
activation chain for the AOT trimmer and drags in Win2D (~1 MB managed + ~3 MB native interop) even
though docking itself does not need it. Measured cost here: the publish output went 169.8 → 181.3 MB.

Ownership rule that decides the shape of `Editor.cs`: **the app owns content, the host owns shape.**
Content is which panes exist and what they show — declared by `DockManager.Layout`, rebuilt from our
own state every render. Shape is the arrangement the user dragged into existence — the host keeps it
and matches the two by pane `Key`. Feeding the host's live layout back into our own state
double-owns the shape and breaks re-docking, tab switching and splitter drags.

### The three regions inside the right pane

`ScriptPanel.cs` is a second `DockManager` nested inside the shell's document pane: a
`DockSplit(Orientation.Vertical, …)` of three panes, one per region. `Orientation.Vertical` is what
makes the bars between the children horizontal, and it is the same framework splitter as the
divider between the shell's two panes.

A `DockSplit` child may be a **bare pane** (`Document` / `ToolWindow`), not just a `DockTabGroup` —
that is what the regions are. A group always renders a tab strip whose captions are the pane
titles, so a group would put a `Script controls` label above a region that only wanted a splitter.
Bare panes come with no chrome at all: the title stays on the pane as identity and is painted
nowhere. The panes are also pinned shut (`CanClose`, `CanFloat`, `CanMove`, `CanDockAsToolWindow`
off) for the same reason as the workspace tool window above — a region that can be dragged out
reaches the docking states this shell does not survive.

### The command table

`CommandTable.cs` fills the middle region: a `DataGrid<CommandRow>` from
`Microsoft.UI.Reactor.Advanced` (already referenced for docking), one row per script frame, one
column per script input — the frame number, each stick's direction and deflection, and the 26
booleans of the script format (`docs/API.md` §4.2 in the Rust repo, which is where the column names
come from). Frames are generated, not parsed: `CommandRow.cs` expands a phase table into a
deterministic mock script, so the 20 000-frame case the workspace is expected to hold can be
exercised before the real parser exists.

How the gapless look is put together:

- **The grid does not draw the gaps** — its cell padding is a fixed `CellPadLeft 8 / CellPadRight 12`
  in `DataGridComponent` (a deliberate gutter, per its own comment). Both `cellTemplate:` and
  `headerTemplate:` bypass it entirely, so a cell is one `Border` that stretches to its column and is
  exactly `RowHeight` tall. Measured on the live tree: cells of 44 / 46 / 46 / 40 / 40 and 26 × 18
  DIP, 18 tall, `margin 0`, `Stretch`, 31 children per row grid — no gap anywhere.
- **The checkerboard is the cell surface**, alternating on `(frame ^ columnIndex) & 1`, with the held
  inputs painted in the accent color.
- ⚠️ **Translucent theme fills are useless for this.** `Theme.LayerFill` / `Theme.CardBackground`
  resolve to `#80FFFFFF` / `#B3FFFFFF` in the light theme — two translucent whites 2 RGB steps apart
  over a light pane, i.e. an invisible checkerboard (measured with `mur devtools properties
  <cell> --name Background`). The table uses the *opaque* `SolidBackgroundFillColor*` family instead:
  `Base` / `BaseAlt` are `#F3F3F3` / `#DADADA` in light and `#202020` / `#0A0A0A` in dark, still
  theme-aware through the same `Theme.Ref` path, so the toggle keeps working.
- `placeholderCellTemplate:` gives not-yet-loaded rows the same block geometry — the grid's own
  shimmer is a translucent fill at 50 % opacity, which all but disappears on a dark table.
- Flag columns are **square**: `FlagWidth` is `RowHeight` (32), so the matrix keeps reading as a
  matrix when either number moves. The numeric columns stay wider than tall — `270.00` needs the
  room. 31 columns come to 1048 DIP, so the default 1100-DIP window shows ~13 of them and the rest is
  a sideways scroll (the pane gets ~590); all 31 fit once the pane is ~1050 DIP wide.
- ⚠️ `pin: PinPosition.Left` on the frame column is **metadata only** in this preview:
  `GetPinnedColumnGroups()` exists on `DataGridState` but nothing in `DataGridComponent` reads it, so
  the column does not stay put under a horizontal scroll.
- Rows are memoized in `Render()` with `UseMemo` on the script — the grid keys its mount off the
  source's identity, so a source rebuilt per render would remount it and drop the scroll position.

### Editing

Inline editing is the grid's own (`editable: true`, `EditMode.Cell`): a tap on a cell opens that
column's editor in place, Enter or a tap elsewhere commits, Esc cancels. What it took to wire:

- ⚠️ **Every editable column needs its own `Editor`.** With an empty `TypeRegistry` (the default — the
  grid builds `new TypeRegistry()`) the fallback is a plain `TextBox` whose value arrives as a
  *string*, so a commit into a `bool` or `double` field throws. The `Editors.*` catalog wraps the
  stock controls, which is a starting point but not a fit (below).
- ⚠️ **Stock editors do not fit a cell.** `Editors.CheckBox()` is a WinUI `CheckBox` with a 120 DIP
  minimum width — four square flag columns' worth. So the flag editor is the same control with
  `MinWidth(0)` / `MinHeight(0)` (measured: 24×20 in a 32 DIP cell).
- ⚠️ **The stick columns edit in a plain `TextBox`, not a `NumberBox`.** A number box hosts its own
  text box, and the stock 32 DIP minimum height *of that inner box* is out of reach of the outer
  `MinHeight(0)` — measured on the live tree, the control overflowed the 32 DIP row and its clear
  button took the right half of a 46 DIP cell. The `TextBox` carries an explicit `.Height(RowHeight - 6)`
  instead, and the *buffer is the raw text*: the setter parses and clamps it (`ReadNumber`), because
  parsing per keystroke would reformat the text under the caret. Garbage or an out-of-range value
  leaves the field as it was.
- **The setters are hand-wired.** A column builder derives its setter by reflection from a property
  named after the column; these columns are named after the script's `input` keys, so `Editable(…)`
  in `CommandTable.cs` supplies `SetValue` itself — `row.With(bit, held)` for the flags, `ReadNumber(…)`
  for the stick values.
- **Both editors carry an `.AutomationName(…)`**: a bare checkbox or text box has no caption of its
  own and the header next to it is a 1-2 letter label (`REACTOR_A11Y_003`).
- ⚠️ **A commit only reaches the grid's optimism overlay, not the source.** `onRowChanged` is where the
  row has to be written back (`ListDataSource.UpdateAsync`), or the next fetch brings the old value
  back. The grid may call it off the UI thread (its own contract says so); `ListDataSource` locks.
  A throwing commit is not lost silently: `FailAsyncCommit` reverts the overlay and the row shows an
  error bar with a Dismiss button.
- `RowHeight` is 32, not the 18 the matrix reads best at: the editors are what has to fit. The height
  is a knob — with the shrunk editors above, ~24 should still fit an open editor, at ~6 visible rows
  instead of 4.
- The headless tests pin what is reachable without a window: the editors' *shape* (a `TextBox` pinned
  to 26 DIP showing the cell's format, a `CheckBox` with the stock minimum width removed), the
  setters' round-trips including clamping and unreadable input, and the commit reaching the source.
  What a `NumberBox`/`TextBox` *is* at runtime — commit-on-blur, its inner minimum height — is not
  something a headless test can reach; those were measured on the live tree instead.
- The frame column stays read-only: no editor, no setter.
- Dropdown-style flows (validators, `EditMode.Row`) are not wired — the clamp lives in `ReadNumber`
  and an unreadable value is simply ignored.

### Theme

The title bar carries a `ToggleSwitch` (`onContent` / `offContent` = `Dark` / `Light`), and the shell
holds the choice in one `ElementTheme?`: `null` follows the system, an explicit `Dark` / `Light` is
what the user pinned. The value lands as `.RequestedTheme(...)` on the shell root — the region that
wraps every pane — so one pass re-themes the lot: WinUI resolves every `ThemeResource` brush against
it, the Mica backdrop follows, and the host re-renders so our own `Theme.*` tokens are re-resolved.

`UseIsDarkTheme()` reads the **app-global** scheme and does not observe that per-element override, so
it only decides where the toggle starts; after a click the pinned value is the truth. It also has to
run unconditionally: calling the hook inside the `??` that folds it into the choice is a hook-order
violation, and `REACTOR_HOOKS_001` flags it (`mur check`).

## Script formats

A script has three representations, and `Script/` is where they meet:

```
.tas text  ⇄  ScriptDocument  ⇄  API JSON      (the mod's POST /script/run body)
                    ⇅
              CommandRow frames                (the command table's view)
```

- **`ScriptDocument` is the hub, and the JSON is the source of truth.** It is the only representation
  that carries the whole format: `raw_key`, `dik_key` and `when_enemy` (the adaptive enemy condition)
  have no text spelling and no table column, so writing such a command out as a `.tas` file is an
  error naming the command rather than a silent loss. The text format itself is specified in
  `../docs/SCRIPT_DSL.md`.
- **`ScriptJson.cs`** — `Read`/`Write` plus the mod's own cross-field limits (`docs/API.md` §4.4 in
  the Rust repo), so the editor refuses a script the game would answer `400` on. Serialization goes
  through the source-generated `ScriptJsonContext`: the app is published with NativeAOT, where
  reflection-based `JsonSerializer` does not work. Unknown keys are refused at the type level
  (`JsonUnmappedMemberHandling.Disallow`), mirroring the mod's `deny_unknown_fields`. What is unset
  stays out of the written JSON — a `false` flag and a `null` field mean the same to the mod as an
  absent key — so the editor writes the same readable shape the fixtures use. Only `t` and `duration`
  opt out of that rule: `"t": 0` is a real frame number.
- **`ScriptDsl.cs`** — the `.tas` text: one line per frame, tokens are console pad names (`a` jump,
  `x` light attack, `lt` blade, `du` augment …) plus `ls`/`rs` for the sticks — and they are the
  same names the command table heads its columns with, so the grid doubles as the format's legend.
  **Movement is the stick**, not a direction flag: `ls:<angle>` is a full press on the compass
  (0 forward, 90 right), `lsx`/`lsy`/`rsx`/`rsy` carry exact axis values, `wk` halves the left
  stick, and a direction flag a JSON script holds is written as the stick it stands for — the stick
  alone drives the character (`docs/API.md` §10.2). `Write` is canonical (fixed token order, a
  one-frame duration left out, invariant numbers, `y`+`b` folded into `by`), so `Write(Parse(text))`
  is the same text again; `Parse` names the line it refuses.
- **`ScriptFrames.cs`** — `Expand`/`Collapse` for the table. Its bits are the same order as
  `CommandKeys.All`, so a column, its JSON key and its DSL token cannot drift apart — a test walks the
  list and asserts each field lights its own bit.

### Fixtures and goldens

The JSON fixtures are **generated by the Rust tool** `tools/script_gen` in the sibling repo, from the
mod's own DTOs (`replay-types/src/script.rs`) — so "would the mod accept this JSON" is answered by the
shared type rather than by comparing text. The editor writes its goldens next to them, and a Rust test
reads those back:

```
Fixtures/all_inputs.json           # from Rust: every input of §4.2, all keys
Fixtures/all_inputs.expected.json  # from the editor: its own write of the same script
Fixtures/all_inputs.expected.tas   # from the editor: the same script as text
```

- `dotnet test TasEditorCs.slnx` compares the goldens;
  `set TAS_REGEN_GOLDENS=1 && dotnet test TasEditorCs.slnx` rewrites them — the way to bless a
  deliberate format change.
- `cd ../tools/script_gen && cargo test` (re)generates the fixtures and accepts the editor's JSON:
  `expected_json_is_accepted.rs` deserializes every `*.expected.json` with the mod's own types and
  asserts it equals the fixture it came from.
- Fixtures are read from the **source** directory (`CallerFilePath`), not from the build output, so
  nothing has to be copied and a new fixture is picked up without touching the csproj.

### ⚠️ The source-generated deserializer writes `default` over absent properties

Measured on .NET 10: a JSON without `name` deserializes into a document whose `Name` is `null` — the
property initializer is not what survives. That reaches the numbers too: with a plain
`uint Hold { get; init; } = 6`, an absent `hold` inside `restart` arrives as a literal 0, i.e. "hold
the arrow for no frames at all". Two consequences, both deliberate:

- `RestartPolicy` parameters are nullable, and `null` means *the mod's default* — `ScriptDsl` compares
  against a fresh instance when it decides which parameters it has to write.
- `ScriptJson.Read` restores the mod's fallbacks explicitly (`Name ?? DefaultName`, `Commands ?? []`)
  rather than trusting the declarations, and a test pins that down so a framework change cannot
  quietly turn `hold` into 0.

## Publish

```bash
dotnet publish TasEditorCs/TasEditorCs.csproj -c Release -o publish
```

AOT block of the template — `PublishAot=true` + `InvariantGlobalization=true`, matching the Native
AOT section of the Reactor packaging guide — plus `WindowsAppSDKSelfContained=true`.

- the output is a **native** x64 exe (~9.6 MB): no `coreclr.dll`, `hostpolicy.dll`, `TasEditorCs.dll`
  or `TasEditorCs.runtimeconfig.json` in the payload;
- a private Windows App Runtime sits next to it, so no separate .NET runtime install is needed — run
  it from any folder, ship it as a zip;
- the whole folder is ~181 MB / 297 files: Windows App SDK 2.5.1 pulls AI/ML pieces into the
  self-contained runtime as well (`DirectML.dll`, `onnxruntime.dll`, `Microsoft.Windows.AI.*`,
  SemanticSearch), and Reactor.Advanced adds Win2D. The payload is not thinned — the Rust version
  does that in `tas-editor/pack.ps1`.

`PublishAot` is gated on `Configuration=Release`, not set unconditionally as the template does — see
the hot reload gotcha below. The cost of the gate: the trim/AOT analyzers no longer run in the Debug
dev loop, so they need a Release build before shipping.

## Dev tooling

`dotnet watch run` gives hot reload but no MCP endpoint; `mur devtools` is the launcher that does
both, and it respawns the app on reload:

```bash
mur devtools TasEditorCs\TasEditorCs.csproj
```

⚠️ It wants a `.csproj` in the **current** directory — it does not discover the project through the
`.slnx`. Bare `mur devtools` from `tas-editor-cs/` fails with `No .csproj found in the current
directory`; pass the path as above.

What you get: the app running with `--devtools run`, an MCP endpoint, and `mur devtools <verb>`
commands that attach to the live session through its lockfile — `tree`, `screenshot`, `state`,
`click`, `type`, `select`, `scroll`, `wait`, `windows.list`, `reload`, `shutdown`. This is how the
layout was checked while building it:

```bash
mur devtools screenshot --window main --out shot.png
mur devtools tree --window main --view summary
```

The verbs drive UIA, and there is no pointer-drag verb among them: that a splitter renders can be
checked from here, moving it cannot — a drag-resize splitter stays a by-hand check.

⚠️ The same limit covers the table's click-to-edit: a cell is a plain `Border` with no UIA pattern, so
`mur devtools click` errors with `no-pattern` (only `Invoke` / `Toggle` / `SelectionItem` are tried,
and no pointer event is synthesized). The edit path is therefore pinned by the headless tests —
`SetValue` round-trips, the commit reaching the source, the editors being present — while "tap a cell
and watch an editor appear" stays a by-hand check.

The endpoint is HTTP at `http://127.0.0.1:<port>/mcp`, and it is **locked with a per-launch bearer
token**: the server generates a fresh one on every start and writes it — with the endpoint, the pid
and the project path — into a lockfile at `%TEMP%\reactor-devtools\<hash>.json` (`<hash>` is the
canonicalized `.csproj` path). The documented client flow is *read the lockfile, then build the client
from its endpoint + token*; the CLI verbs below do exactly that.

Pin the port so the URL stays stable across runs, which is what a lockfile-reading client wants:

```bash
mur devtools TasEditorCs\TasEditorCs.csproj --mcp-port 9000
```

⚠️ A **static** MCP entry therefore does not work — verified, not assumed. Registering
`http://127.0.0.1:9000/mcp` in a client's `mcpServers` gets `401 Unauthorized`
(`WWW-Authenticate: Bearer realm="reactor-devtools"`): the token differs between two launches of the
same project on the same port (`qjX2Op2…`, then `wMap2gbJ…`), and `mur devtools --print-config` emits
the bare URL with no `Authorization` header even while a session is live. Pinning the port does not
help with that; the token still rotates.

So from an agent, use the CLI — it authenticates through the lockfile on its own:

```bash
mur devtools tree --window main --view summary
mur devtools screenshot --window main --out shot.png
```

`%TEMP%\reactor-devtools\` is also the first place to look when a session will not attach: **no
lockfile** means the build-time gate (`Reactor.DevtoolsSupport`) or the run-time flag (`--devtools run`)
never fired, while **a lockfile whose pid is dead** is stale — deleting it unblocks the next launch.

## Testing

`TasEditorCs.Tests` is the headless unit layer: it asserts on the `Element` trees our view functions
produce and never creates a WinUI control.

⚠️ A headless test **cannot activate any WinRT/XAML object** — the app's own runtime is registered in
the app process, not in the test host. Anything that reaches for one throws
`COMException: class not registered`. Measured casualties: the `.SemiBold()` modifier resolves
`Microsoft.UI.Text.FontWeights` and dies; `Heading`, `Caption` and `TextBlock` are fine. That is why
emphasis in the panes comes from the typography helpers rather than a hand-applied `.SemiBold()`.

⚠️ `Component<TProps>.Props` is read-only and set by the host, so a test outside the framework
cannot render a props-carrying component at all. Both panes therefore keep their body in an
`internal static Element View(...)`, and `Render()` is a one-liner over `Props` — the tests assert on
`View`. Reactor's own `MountComponent` helper is internal to the framework and not available here.

The test project needs the app's TFM (`net10.0-windows10.0.22621.0`), a pinned
`RuntimeIdentifier` (Win2D's targets refuse to wire up their native DLL for an AnyCPU consumer), and
`InternalsVisibleTo` from the app (the components stay internal). `AccessibilityScanner.Scan(element)`
runs headlessly, so each pane is also scanned for `A11Y_001`.

```bash
dotnet test TasEditorCs.slnx
```

## Gotchas

### `dotnet publish` does not copy the app PRI

The first published exe dies after ~100 ms with `0xC000027B` (`STATUS_STOWED_EXCEPTION`) — an
unhandled exception inside XAML. Cause: the publish folder is missing `TasEditorCs.pri`, the merged
resource index the XAML compiler writes next to the exe in
`TasEditorCs/bin/<config>/<tfm>/win-x64/`.

The missing PRI has nothing to do with AOT: it reproduces on an untouched `dotnet new reactorapp`
scaffold (with and without the AOT switch), on a JIT publish, and with the documented recipe
`dotnet publish -c Release -r win-x64`. The Windows App SDK hands `@(ProjectPriFile)` to its own MSIX
pipeline only (MrtCore.PriGen), while a plain unpackaged publish builds its file list from the
project output groups, which do not carry that file. Fixed by the `_PublishAppPri` target in
`TasEditorCs/TasEditorCs.csproj` (adds `$(TargetDir)$(TargetName).pri` to `ResolvedFileToPublish`).

Sanity check that a publish is alive: `publish\` contains `TasEditorCs.pri` and
`publish\TasEditorCs.exe` opens a window.

### `PublishAot` in every configuration kills hot reload

The template's AOT switch sets `PublishAot=true` unconditionally. Measured 2026-09-21: the switch
leaks its feature switches into ordinary Debug builds, and the Debug runtimeconfig then carries

```json
"System.Reflection.Metadata.MetadataUpdater.IsSupported": false
```

— so `dotnet watch run` and the reload loop inside `mur devtools` have no migration subsystem to
work with. `PublishAot` is therefore gated on `Configuration=Release` here; `InvariantGlobalization`
stays on for every configuration, since it is a globalization mode and does not touch the updater.

### Docking chrome can put the shell into states it does not survive

The workspace tool window has `CanPin`, `CanAutoHide`, `CanHide` and `CanFloat` turned off, and that
guard is load-bearing rather than cosmetic. Measured 2026-09-21 on the pin / auto-hide affordance:
collapsed, the pane unrenders its content while the split keeps reserving the space — an empty left
column with an orphaned `Scripts` tab and no obvious way back; brought back, it returns as an
unthemed floating overlay (dark surface, dark heading text on top of it, caption clipped at the
edge) that covers the document. Both halves are library-side — the collapse does not release the
split space, and the floating chrome gets no app theme — so nothing in this project can fix them;
the guard only keeps the pane docked. Re-enable the flags together with a fix for those states.

One affordance is still live and unverified: the tab strip's add-tab button (`AddButton`,
"Добавить новую вкладку"). Worth a click before trusting the strip.

### An XML comment cannot contain `--`

Flag names like `--aot` practically invite one, and the build fails with `MSB4025` when one lands in a
csproj comment. Cost me two round trips.

### Other

- `CS0436` during the build (`ReactorApplication` conflicts with the type from `Reactor.dll`) is a
  Reactor generator warning — it is there on an untouched scaffold too, not ours.
- Reflection vs trim: `Factories.AutoColumns<T>()` (walks `typeof(T).GetProperties()`) and devtools
  (walks `Assembly.GetTypes()`). For retail/AOT the devtools switch stays off — in the csproj
  `Microsoft.UI.Reactor.Devtools` and `Reactor.DevtoolsSupport=true` are wired only for
  `Configuration=Debug`.
- The docking chrome carries its own strings, and at least one is Russian: the tab group's add-tab
  button reports `Добавить новую вкладку` as its automation name. Nothing local controls that — it
  is library chrome.
- The selected row's highlight in the left pane does not line up with the row body yet (the
  `ListView` item container is wider than the row element). Cosmetic, and the row is a stub.

## Template

`Microsoft.UI.Reactor.ProjectTemplates` **0.1.0-preview.15** is installed (updated 2026-09-21 from
`0.0.0-local`). Parameters: `--aot` (the ready-made AOT block), `-f net10.0`,
`-M/--MSUIReactorVersion <version>`. To check or update:

```bash
dotnet new list reactor
dotnet new install Microsoft.UI.Reactor.ProjectTemplates@<version>
```
