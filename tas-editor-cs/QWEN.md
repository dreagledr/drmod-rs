# tas-editor-cs

C# TAS editor for **Metal Gear Rising: Revengeance** — WinUI 3 through `Microsoft.UI.Reactor`
(declarative components and hooks, no XAML, no bindings, no ViewModels).

**This project has its own conventions.** The root `QWEN.md` describes the Rust repo (`drmod-rs`);
its rules do not carry over here — most importantly the Russian-UI rule. When a convention in this
file disagrees with the root one, this file wins for everything under `tas-editor-cs/`.

Commands, publish gotchas and measurements: `README.md` in this folder.

## The mod

- The editor **installs the mod into the game** from its own exe: `TasEditorCs\Mod\drmod_rs_lib.asi`
  and `TasEditorCs\Mod\d3d9.dll` are `EmbeddedResource`s `ModPayload.Read()` pulls out by name. The
  payload is generated, not committed (`.gitignore`), and `build-mod.ps1` is what produces it —
  `cargo build --release` at the repo root, the DLL renamed to `.asi`, plus the vendored loader.
- ⚠️ **The csproj fails the build when the payload is missing** (`_RequireModPayload`). Without it a
  fresh clone builds a green exe whose Install button has nothing to install — a defect that only
  shows up in front of the game, with the user watching.
- `ModInstaller` is the whole of the install: `Detect` (is the plugin there, and is it *these*
  bytes), `Loader` (is there a `d3d9.dll`, and is it ours), `CanRemove`, `Install`, `Remove`. It
  never throws — every answer is an `InstallResult` with a line the pane paints.
- ⚠️ **A `d3d9.dll` that is already there belongs to somebody else until proven otherwise.** Almost
  every machine with a ReShade, an ENB or another ASI mod has one. It is only *added* when the folder
  has none, and only *removed* when it is byte-for-byte ours; the plugin works with any ASI loader,
  so neither direction needs to touch that file. Both directions say which of the two they did.
- ⚠️ Writes go through a temporary beside the target and a move over it, and the order differs per
  direction on purpose: install writes the loader first and the plugin last, removal deletes the
  plugin first — so an interrupted run never leaves a plugin the game still tries to load.
- `SteamLibrary` finds the game the way Steam records it: `HKCU\Software\Valve\Steam\SteamPath` plus
  every `steamapps\libraryfolders.vdf` (both the modern object shape and the older numbered list),
  confirmed by `METAL GEAR RISING REVENGEANCE.exe` being there. A folder the user picks is checked
  the same way before anything is written into it. Nothing here throws either.
- The pane is **content of the workspace pane**, not a pane of its own (`WorkspacePanel` draws it
  above the list). ⚠️ Two tool windows in one column become *tabs*, and the docking host does not
  report a tab click back to the app — it hands `DockTabGroupRenderer` `onSelectedIndexChanged: null`
  and never subscribes to the `TabView`'s own `SelectionChanged`, while the renderer writes its
  default index on every render. A selection there is lost on the next re-render by construction, so
  there is one pane with one list of contents and nothing to lose.
- The install is content-sized (`.Flex(shrink: 0)`) and the script list is the child with
  `grow: 1`; giving both a `grow` split the column in half and left the scripts a short scroller.

## Language

- **English only.** In-app text, code comments, XML doc comments, identifiers, file names, and this
  file. No Cyrillic in source files.
- This overrides the root rule *"UI messages are in Russian"* — that rule belongs to the Rust
  overlay (`src/`, `tas-editor/`), not to this project.
- The Rust sibling's conventions stay Russian; do not copy its wording into this project and do not
  apply this project's language rule back to it.

## Layout

- `TasEditorCs.slnx` at the folder root, with the app and the tests as sibling folders:
  `TasEditorCs/` and `TasEditorCs.Tests/`. A project never contains another project.
- A solution-level command takes the `.slnx` (`dotnet build`, `dotnet test`); a tool that wants a
  project takes the project path.

## Components

- A component is a class: `class X : Component` (or `Component<TProps>`) with
  `public override Element Render()`.
- **One component per file**, file named after the class (`Editor.cs` → `class Editor`).
- A props-carrying component is a **thin shell**: `Render()` is a one-liner over
  `internal static Element View(...)`, and `View` holds the body. Two reasons, both load-bearing —
  `Component<TProps>.Props` is read-only and host-set, so a test outside the framework cannot render
  the component at all; and Reactor's own `MountComponent` is internal to the framework.
- `App.cs` keeps `ReactorApp.Run<Editor>(...)` and the host `configure:` hook — the entry point, not
  a place for screen contents.
- A piece used by more than one screen becomes its own component file; a piece only one screen uses
  stays in that screen's file until a second caller appears.
- Once an area has more than a couple of component files, group them in a folder named after the
  area (the Rust sibling's `panels/` is the precedent).

## Verification

- `mur check TasEditorCs/TasEditorCs.csproj` **is** the build (it runs `dotnet build` under the
  hood) and adds Reactor's own diagnostics. Do not re-run `dotnet build` after a green `mur check`.
- `dotnet build TasEditorCs.slnx` and `dotnet test TasEditorCs.slnx` for the whole thing.
- The `REACTOR_*` analyzer warnings are part of the build; `mur check` links each one to the skill
  section that explains it — read the pointer instead of guessing, and use its `→ try: <name>`
  suggestions verbatim.
- Compiling is not enough: layout, hook-order and binding mistakes build fine. Before calling a UI
  change done, launch it and look at it — `dotnet run --project TasEditorCs/TasEditorCs.csproj`, or
  `mur devtools TasEditorCs\TasEditorCs.csproj` plus `mur devtools screenshot` when you want the
  picture and the visual tree as evidence.
- Because `PublishAot` is Release-only here (see the publishing section), the trim/AOT analyzers do
  **not** run in the Debug loop: a Release build is what exercises them, so run one before shipping.
  `mur check TasEditorCs/TasEditorCs.csproj --final` is that Release build.

## Testing

- `TasEditorCs.Tests` is the headless unit layer: assert on the `Element` trees `View` returns.
  `Element` is a record, so pattern-match the shape you own instead of stringifying the tree.
- ⚠️ **Never reach for a WinRT/XAML object in a test.** The test host has no WinUI runtime, and
  anything that activates one throws `COMException: class not registered`. Measured casualty:
  `.SemiBold()` (it resolves `Microsoft.UI.Text.FontWeights`). `Heading`, `Caption` and `TextBlock`
  are safe. Take emphasis from the typography helpers, not from a hand-applied `.SemiBold()`.
- Never assert on pixels, timestamps or generated ids.
- `AccessibilityScanner.Scan(element)` works headlessly; assert on a specific rule id (e.g.
  `A11Y_001`) rather than on "no findings at all".
- The test project mirrors the app's TFM and pins a `RuntimeIdentifier` (Win2D's targets refuse an
  AnyCPU consumer and warn `WIN2D0001`). Components stay `internal`; the test assembly gets in
  through `InternalsVisibleTo` instead of a public surface only tests would call.

## Publishing

- ⚠️ **Build the mod payload first**: `pwsh -File build-mod.ps1` (or `pack.ps1 -BuildMod`, which calls
  it). The csproj refuses to build without it, so a publish of a tree that never ran it fails at the
  first target rather than shipping an editor that cannot install anything.
- `dotnet publish TasEditorCs/TasEditorCs.csproj -c Release -o publish` — self-contained +
  NativeAOT, gives a native x64 exe that runs from any folder.
- `pwsh -File pack.ps1 -BuildMod -Build -Zip` is the whole chain: payload, publish, thin the tree into
  the shippable zip (222 → 75 MB). ⚠️ **Run it with `pwsh`**, not `powershell`: Windows PowerShell 5.1
  reads `.ps1` as ANSI and chokes on UTF-8 punctuation. What it drops and the measurements behind it —
  `README.md` (*Packaging*).
- CI calls the same script (`.github/workflows/build.yml`, a `v*` tag) and puts the zip in the
  GitHub Release beside the mod's. ⚠️ The step needs `shell: pwsh` and `-BuildMod` (CI builds the mod
  itself for the ASI archive, and the editor's payload comes from that same build), and the editor
  does **not** go to Yandex S3 — that step wipes the whole bucket. `README.md` (*CI*).
- `PublishAot` is **gated on Release on purpose**: set unconditionally (as the template does) it
  leaks `MetadataUpdater.IsSupported: false` into Debug builds and kills hot reload. Keep it gated.
- ⚠️ The `_PublishAppPri` target in the csproj is load-bearing: `dotnet publish` does not copy the
  app PRI on its own, and without it the published exe dies with `0xC000027B` before the first
  frame. The debug loop does not show this. Background in `README.md`.

## Dev loop

- **Always launch it as** `mur devtools TasEditorCs\TasEditorCs.csproj --mcp-port 9000` — with the
  project path and the pinned port. ⚠️ It needs a `.csproj` in the current directory and does not read
  the `.slnx`, so a bare `mur devtools` from the solution root fails.
- It serves `tree` / `screenshot` / `state` / `click` / `wait` / `reload` / `shutdown` against the
  running app, attaching through the session lockfile.
- ⚠️ Do **not** register the MCP endpoint in a client's `mcpServers`: the bearer token is regenerated
  per launch and handed out only through the lockfile, so a static entry always answers `401`. Verb
  commands over the CLI are the working route; details and the measurement are in `README.md`.
- The devtools switch (`Microsoft.UI.Reactor.Devtools` + `Reactor.DevtoolsSupport`) is wired for
  `Configuration=Debug` only, and hot reload depends on it — see the publishing section.

## Status

- Two-pane window shell over a **real workspace**: the left pane holds the `.tas` files of a folder
  the user picks, the right pane stacks three regions — script controls, command table, script text
  — with the docking splitters between them. The title bar carries the dark / light toggle.
- The workspace is on disk (`Workspace.cs`, `EditorSettings.cs`): `Open folder…` through the
  library's own picker, the folder remembered between runs in `%LOCALAPPDATA%\tas-editor-cs\settings`
  along with the run rules (`key=value` lines; `UsePersisted` is a process-lifetime cache, not a disk
  store, and a file from before the rules existed reads as the folder), and the folder read as a
  listing —
  the top-level `.tas` files by name, each read and parsed, so a row carries its frame count and a
  file that does not read as a script says so in its own row instead of vanishing. **A first launch
  with nothing remembered opens on `examples/` next to the exe** (`EditorSettings.FirstFolder()`) —
  two scripts copied from the repo's `tools/demo/` into `TasEditorCs/examples/` — and a picked folder
  wins from then on. **New** writes an empty `script.tas` (`-2`, `-3` …), **Duplicate** copies to `<name>-copy.tas`, **Rename** moves the
  file to the name that was typed — a taken name is refused with a message rather than suffixed, and
  the buffer travels with the file — **Delete** asks first in a declarative `ContentDialog`. ⚠️ The
  file name is still *not* the script's `name=`, and the text never renames the file. ⚠️ The listing
  is built inside a render (memoized on
  `folder` + a revision the writes bump), which is why nothing in `Workspace` throws — every failure
  comes back as a message for the pane's own line. Details and measurements — `README.md`
  (*The workspace*).
- **Save is explicit**: a `StandardCommand.Save` button in the script controls region plus its own
  Ctrl+S accelerator (registered for the subtree through `CommandHost`). It is enabled by
  `ScriptBuffers.IsDirty` and nothing else — a half-written script is still work worth keeping, so a
  text the parser refuses saves like any other. The text each script is being edited into is a buffer
  owned by the *shell*, because the list's unsaved markers, the text region and Save all read it; a
  buffer outlives the selection.
- ⚠️ **Line separators are never strict, and that was a measured bug — twice.** A `TextBox` reports
  its lines with a lone `\r`, the file is `\n`, a file edited elsewhere may be `\r\n`. The buffer keeps
  what the control reported, `ScriptDsl.Lines` is the one place that says what a line break is,
  `Workspace.Write` writes the format's own separator and `Workspace.Read` normalises what it finds,
  while `IsDirty` compares *readings*. Before that, the first save wrote the control's text through:
  31 `\r` and no `\n` in the file (one line to the parser, frames glued: `… y:6` + `22` → `y:622`)
  while the editor showed the script as fine.
  ⚠️ **`ScriptBuffers.Resolve` hands the file's text over in the control's own separator too**
  (`Boxed`) — a script nobody has typed in yet has no buffer, and the file's `\n` is not what the
  `TextBox` reports back: unequal, so the reconciler wrote `Text` on **every** render and every write
  of `Text` drops the caret to the start. Polling `/state` twice a second was therefore a caret that
  jumped to the top of the script twice a second (`… observed live, 2026-09-23`; the fix is what makes
  *keeps what the control reported* true from the first render instead of from the first keystroke).
- The command table is real and **read-only**: a `DataGrid` over the selected script's own `.tas` text
  (`CommandTable.cs`), one column per DSL token, 20 000-frame scripts included. It is a picture of the
  text region, not a second editor — editing lives in the text alone. `Script/ScriptFrameProjection.cs`
  reads the text token by token (`ls:<angle>` fills the `ls` column, `lsx`/`lsy` fill theirs, a flag
  lights its own), because a parsed `ScriptCommand` has already resolved the two stick spellings into
  one and folded the direction flags into a stick. Nothing is converted between the forms: exactly one
  of a stick's three columns carries a value and the rest are blank. Columns and headers are the DSL
  tokens themselves (`# | ls | lsx | lsy | rs | rsx | rsy | a | x | … mr`), so the table doubles as the
  format's legend; a token's `:N` duration lights the whole run, frames run 0..last, and a text that
  does not parse leaves the table empty with the message staying in the text region.
- The script text region is real too (`ScriptTextEditor.cs`): the selected script's `.tas` text in a
  multiline monospaced `TextBox`, re-read on every keystroke, with what the text parses to — or the
  line the parser refused — as the line above it (`ScriptTextStatus`; the parse itself is the pane's,
  handed down so that the status line, the controls region and Save all read one answer). Its
  `Commands` button opens the format's command reference (`Script/ScriptCommands.cs`) as a **pane
  beside the editor**, separated by the docking host's own splitter — the same one the regions use —
  so the reader can drag the boundary as wide as they want. ⚠️ Measured traps are in `README.md`
  (*The script text region*): a `TextBox` reports its lines with a lone `\r`; it fills the region only
  from a `Grid` star row, not from a flex slot; its scrollbars have to be turned on through `.Set`
  (the font rides there for the same reason); a hand-rolled `OnPan` splitter re-rendered the region
  per pan event and felt heavier than the host's own, which is why the native one is what shipped; and
  a help line that outgrows a narrow panel is clipped rather than wrapped (the list's `ScrollViewer`
  measures unbounded).
- **The script controls region runs a script** (`ScriptControls.cs`, `PlaybackRules.cs`, `Api/`,
  `GameWindow.cs`, `MenuSettler.cs`): Save, the four rules a run is configured by — fixed tick 1/60,
  frame cap (default / unlimited / custom), a frozen seed (decimal or `0x` hex) and headless —
  `Run` / `Apply` / `Cancel`, and a line read from the mod's `/state` twice a second (menu · mission ·
  fps · script frame · what the levers are actually set to). `Run` goes: a fresh `/state`, the game
  window to the foreground, **the menu settled out of the way**, the rules, then `POST /script/run`
  with the **text on screen** (not the file — Run is not a save); a `409` stops the script holding the
  mod's slot and runs once more. **`Apply` sends the same three lever posts and nothing else** — no
  script, no window focus, no menu settle. An extreme rule (`1 fps`, a lifted cap) is what makes a run
  cheap and the mod keeps it after the run ends, so setting it deliberately — before a run, or between
  them — is worth its own button rather than being reachable only as a side effect of Run; it asks for
  nothing but the game answering, where `Run` also needs the mod's one script slot free. ⚠️ The menu
  step is a port of the python tools' `ensure_gameplay` / `recover_fail` (`MenuSettler`): a pause menu
  is toggled shut by a three-frame `pause` script and a fail menu by one `confirm` — mod scripts rather
  than keystrokes, because the pause menu does not tick the input unit and reads script frames through
  the `isKeyDown`/`isKeyPressed` detours. A game
  already playing is left untouched, and any other menu (front end, a mission still loading) is
  **refused by name** rather than guessed at with a blind confirm. ⚠️ The two rules that were
  measured, not chosen: the seed is applied **last** of the three levers (the mod freezes the LCG on
  the first tick of the *next* script), and headless is armed from the poll only once the script is
  really `running` (skip hooks during a level load crash the game — `../docs/HEADLESS.md` §5). ⚠️ **Every request body goes out gzipped** (`Content-Encoding: gzip`, `ModApi.Gzipped`): the
  mod's 64 KiB limit is on the compressed bytes, so a long script is only accepted compressed, and
  the frame ceiling is an upper guard rather than the real bound. Send the body as bytes, never as a
  string — `Content-Length` has to be the compressed length. ⚠️ The rules are the mod's state for one
  run, **not part of the script** (`../docs/SCRIPT_DSL.md` §6): they live in the editor's settings and
  no `.tas` file is rewritten to hold them. The client answers with values rather than exceptions, and
  its JSON goes through a source-generated context (NativeAOT). `README.md` (*The run*) has the order,
  the bodies and the measurements.
- The three script representations round-trip through `Script/`: the API JSON (`ScriptJson`), the
  `.tas` text (`ScriptDsl`) and the converter's frames (`ScriptFrames`), around the `ScriptDocument`
  hub. Text tokens are console pad names (`a` jump, `x` light attack, `lt` blade, `du` augment …) and
  they are exactly what the command table heads its columns with — so **a column, its JSON key and its
  token have to be renamed together**. Movement is the stick there (`ls:<angle>` on the compass,
  `lsx`/`lsy` exact values, `wk` halving); a direction flag from a JSON script is written as the stick
  it stands for, which is why the text has no direction tokens and the table no direction columns.
  Fixtures come from the Rust tool `tools/script_gen`, the editor's goldens sit next to them in
  `TasEditorCs.Tests/Fixtures/`, and format, guarantees and commands are in `../docs/SCRIPT_DSL.md`
  and `README.md` (*Script formats*). ⚠️ Read the `default`-overwrite gotcha there before trusting a
  property initializer to survive deserialization.
- **The editor installs the mod** (`ModPanel.cs`, `ModInstaller.cs`, `SteamLibrary.cs`,
  `ModPayload.cs`): the workspace pane carries, above the script list, where the game is, what is
  already in its folder, and two buttons — `Install` / `Reinstall` / `Update` and `Uninstall`, each
  live only when the click could do something, with the uninstall asking first in the same declarative
  `ContentDialog` the script delete uses. The game folder is found through Steam's own records and
  remembered in the settings file (`game_folder`), and one look at it (`LookAt`) reads every fact the
  pane paints — so a render never touches the disk. Nothing here launches, kills or injects anything:
  the files are the whole install and the message says the game has to be restarted. See *The mod*
  above.
- Next: the script controls region's remaining fields (`name=` in the rules line, the trigger and the
  restart policy, which are edited in the text today), and a close guard for unsaved buffers (today
  they live in the process's memory only — an explicit save is the whole of the save UX).
- The Rust version (`../tas-editor/`) is left untouched; this project is the candidate replacement.
