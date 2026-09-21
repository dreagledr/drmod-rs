# TAS Editor (C#)

Desktop TAS editor for Metal Gear Rising: Revengeance on **WinUI 3** through
[`Microsoft.UI.Reactor`](https://microsoft.github.io/microsoft-ui-reactor/) — a declarative
React-style component model (components, hooks, keyed lists), no XAML, no bindings, no ViewModels.

Status: **two-pane shell with stub panes.** The window is split by a draggable divider — the left
pane lists the workspace scripts (the selection is live, the management buttons are placeholders),
the right pane shows the selected script. The frame timeline, the per-frame properties strip, the
JSON editor and the on-disk workspace are the next passes.

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
│   ├── ScriptPanel.cs      # right pane — the selected script
│   ├── ScriptEntry.cs      # the script model
│   ├── Assets/ Properties/
│   └── TasEditorCs.csproj
└── TasEditorCs.Tests/      # xUnit, headless unit layer
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
