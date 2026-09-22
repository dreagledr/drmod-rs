# TAS Editor (C#)

Desktop TAS editor for Metal Gear Rising: Revengeance on **WinUI 3** through
[`Microsoft.UI.Reactor`](https://microsoft.github.io/microsoft-ui-reactor/) — a declarative
React-style component model (components, hooks, keyed lists), no XAML, no bindings, no ViewModels.

Status: **two-pane shell over a real workspace, with a run.** The window is split by a draggable
divider — 1 part of workspace to 3 of document by default — the left pane holds the `.tas` files of a
folder the user picks (the list and the file actions are real: new, duplicate, rename, delete), the
right pane stacks three regions — script controls, command table, script text — split 1 : 2 : 3, with
the same drag-resize splitters between them. The command table region and the script text region are
both filled in; the table is a **read-only** picture of the text on screen (one column per DSL token,
no editing), the text is the file's, and the script controls region is where a script is **run**:
Save, the four run rules, Run / Cancel and the live state of the game (see *The run* below). The
three representations of a script — the API JSON, the `.tas` text and the table's frames — round-trip
through `Script/` (see *Script formats* below).

This is the C# version living next to the Rust one (`../tas-editor/`); that one is left untouched.
Folder conventions — language, component layout, what to run before calling something done —
are in `QWEN.md`.

## Layout

```
tas-editor-cs/
├── TasEditorCs.slnx        # solution: the app and its tests
├── pack.ps1                # thins a publish into a shippable zip (see Packaging)
├── TasEditorCs/            # the app (WinUI 3, self-contained + NativeAOT)
│   ├── App.cs              # entry point: ReactorApp.Run + docking registration
│   ├── Editor.cs           # window shell: the split, the workspace, the selection, the run
│   ├── Workspace.cs        #   the folder on disk: list, read, write, new, copy, rename, delete
│   ├── EditorSettings.cs   #   what survives a restart (the folder and the run rules)
│   ├── WorkspacePanel.cs   # left pane — the script list and its management
│   ├── ScriptPanel.cs      # right pane — the three stacked regions
│   ├── ScriptControls.cs   #   the controls region — Save, the run rules, Run / Cancel, the status
│   ├── PlaybackRules.cs    #     the four levers a run applies, and the bodies they become
│   ├── MenuSettler.cs      #     getting the game out of a menu before a run (pause / fail)
│   ├── GameWindow.cs       #     the game's window, and bringing it to the foreground
│   ├── Api/                #     the mod's HTTP API: the client, its JSON, the state it answers with
│   ├── CommandTable.cs     #   the command table region (read-only DataGrid over the text)
│   ├── ScriptTextEditor.cs #   the script text region (`.tas` text + its status line)
│   ├── ScriptTextStatus.cs #     what a text parses to, or why it does not
│   ├── ScriptEntry.cs      # the script model — one `.tas` file
│   ├── ScriptBuffers.cs    #   the text each script is being edited into, and what is unsaved
│   ├── CommandRow.cs       #   one frame in the converter's angle + deflection shape
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

### The workspace

The left pane is the folder on disk. `Open folder…` picks it through the library's own picker
(`UseFolderPickerAsync` — captured as a *method group* in `Render` and called from the click, which
is how the library's picker sample does it; the analyzer reads the `Use*` name and flags it inside a
branch, though it holds no hook slot), and the folder is then read as a **listing**: the top-level
`.tas` files, by name, each one read and parsed (`Workspace.List`). Parsing at listing time is what
puts a frame count in the row — and what lets a file that does not read as a script say so in its own
row instead of disappearing (`ScriptEntry.Error`).

⚠️ **The listing is built inside a render.** `Editor` memoizes it on `(folder, revision)`, and every
write to the folder — a save, a new file, a delete — bumps the revision, so the rows re-read the
files they describe. That makes the listing a disk read in the render path, which is why nothing in
`Workspace` throws: every failure — a folder that is gone, a file nobody can read or write — comes
back as a message and ends up in the pane's own line.

The file actions are `ScriptEntry`-level, not document-level: **New** writes an empty `script.tas`
(`script-2.tas`, … — an existing name is never overwritten), **Duplicate** copies the file to
`<name>-copy.tas`, **Rename** moves the file to the name the author types, **Delete** asks first, in a
declarative `ContentDialog` that stays in the tree with `IsOpen` toggled (a `ShowAsync` from a handler
gets no parent theme and is not testable — `REACTOR_DIALOG_001`). The delete dialog names the file and
says so when that file has unsaved text, because deleting it takes the text with it.

⚠️ **Rename is the file's, not the script's.** `name=` in a rules line is a different thing and is
edited in the text; the extension is the workspace's and is not part of the name a user types. A name
that is already taken is **refused with a message** rather than suffixed: New and Duplicate invent a
free name because they were asked for "a file", but a typed name is a name the author wants, and
`-2` would hand them something they did not ask for. The buffer travels with the file
(`ScriptBuffers.Renamed`), or a rename would quietly throw away text that was never written back. The
dialog's primary button is live only for a name that would change something — `Rename` for `Rename` is
not a question worth answering.

**Save is explicit** — a `StandardCommand.Save` button in the script controls region plus its own
Ctrl+S accelerator, registered for the subtree through `CommandHost`. What is enabled is
`IsDirty` and nothing else: a half-written script is still work worth keeping, so a text the parser
refuses saves like any other (the status line already says the mod would turn it down). The folder and
the run rules are remembered between runs — `%LOCALAPPDATA%\tas-editor-cs\settings`, `key=value` lines
— because `UsePersisted` is a process-lifetime cache (spec 033 §2), not a disk store. A file written
before the run rules existed is a bare folder path with no `=` in it, and it is read as the folder
rather than as a settings file nobody can parse; the seed travels as the text the field held, so a hex
seed (`0x55555555`) survives a restart spelled the way the notes spell it.

The text each script is *being edited into* is a **buffer** (`ScriptBuffers`), owned by the shell and
not by the pane: the list marks the scripts that are unsaved, the editor shows that text, and Save
writes it — three readers of one value. A buffer outlives the selection, so switching scripts and
coming back keeps what was typed.

⚠️ **A buffer is the text box's own text, and unsaved is a question about readings, not bytes.** A
WinUI `TextBox` separates its lines with a lone `\r`; the file is `\n`; a file edited elsewhere may be
`\r\n`. So the buffer keeps exactly what the control reported (storing the file's `\n` instead would
make the reconciler write the text back on the next keystroke and move the caret with it), while
`IsDirty` compares `ScriptDsl.Lines(buffer)` with `ScriptDsl.Lines(fileText)` — a line break is a line
break, whichever of the three it is. This was a **measured bug**, not a hypothetical: the first save
wrote the control's text through, and the file on disk ended up with 31 `\r` and not one `\n` — one
line to the parser, with the last frame and the next line's frame number glued together
(`… y:6` + `22` → `y:622`), while the editor — which normalises before parsing — showed the script as
perfectly fine. `Workspace.Write` now always writes the format's own separator, and `Workspace.Read`
normalises whatever it finds, so the same bytes can never read two ways.

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

The base sizes of both splits are **weights, not DIPs**. The host bootstraps a split's ratios from its
children's `Width`/`Height` hints only when *every* child carries one (`BootstrapRatios` in
`DockHostNativeComponent`), normalizing them into proportions; a mixed set falls back to an equal
share, which is what the first version of this pane was doing without meaning to. So the shell says
`Width: 1` / `Width: 3` and the pane says `Height: 1 / 2 / 3`, and what the host keeps from then on is
whatever the author dragged. Measured with the window maximized (2560 × 1344 physical, 150 %): the
two panes came out 636 : 1908 (1 : 3 exactly) and the three regions 206.7 / 413.3 / 620 DIP
(1 : 2 : 3 exactly).

A hint is *not* a size: a ratio is all the host can hold, which is also why the pane can no longer
say "the last region takes the rest" — the text region is three parts of six, not the remainder. What
each region gets is then still weighed against what its *content* asks for, so a long script pulls
height away from its neighbours inside the ratio; the splitter is the fix, and there is nothing to
hard-code here.

### The command table

`CommandTable.cs` fills the middle region: a `DataGrid<ScriptFrame>` from
`Microsoft.UI.Reactor.Advanced` (already referenced for docking), one row per script frame, one column
per **DSL token** — read-only, and a picture of the text region's own `.tas` text.

What "a picture of the text" means is the whole design:

- **The frames are read out of the text, token by token, not from the parsed document.**
  `Script/ScriptFrameProjection.cs` walks the lines, and each token becomes the column it spells:
  `ls:<angle>` fills the `ls` column, `lsx`/`lsy` fill `lsx`/`lsy`, a flag lights its own column. A
  `ScriptCommand` is no use here — by the time a document exists, `ls:0` has been resolved into the
  axes `(0, -1000)` and the four movement flags have been folded into a stick, so the two spellings
  are indistinguishable. The table is meant to show what the script *says*, so it reads the saying.
- **Nothing is converted between the two stick forms.** A line writes `ls:0` *or* `lsx`/`lsy`, never
  both (the parser refuses it), so exactly one of a stick's three columns carries a value and the
  other two are blank — never a resolved `0` standing in for a token the line did not write.
- **Columns are the DSL's own tokens, headers included**: `# | ls | lsx | lsy | rs | rsx | rsy | a |
  x | y | lr | lt | rt | wk | ax | rb | lb | dd | du | dl | cd | b | r | esc | ok | mu | md | ml |
  mr` (29 columns). The flag columns are named and headed by the token itself, so the table doubles as
  the text format's legend; only the frame column needs a header that is not a token (`#`, because the
  format spells a frame as the bare number at the start of the line). `FlagKeys.All` is that list,
  in the order a line writes its tokens.
- **A token's `:N` duration lights the whole run.** `4 lt:3` covers frames 4, 5 and 6 — three rows
  with `lt` lit — which is the run the text says. Two tokens on one frame both stand (the format ORs
  its bits, and the table keeps the longer run).
- **Frames run 0 .. last**, so a gap the text writes nothing for is a blank row rather than a missing
  one — again what the format says by omitting it.
- **A text that does not parse leaves the table empty.** The text region carries the parser's own
  message; the table does not repeat it a third way. Verified live: with a broken fixture selected the
  table reads *No data to display* while the region above it names the line.
- ⚠️ **Read-only is a code path, not a flag.** `editable: false`, `selectionMode: None`, and no
  `Editor`/`SetValue`/`onRowChanged` at all — plus a test asserting every column is read-only and the
  grid has no `OnRowChanged`, so the capability cannot creep back in unnoticed. Editing is the text's,
  in one place, instead of two representations that would have to be kept in step.

How the gapless look is put together:

- **The grid does not draw the gaps** — its cell padding is a fixed `CellPadLeft 8 / CellPadRight 12`
  in `DataGridComponent` (a deliberate gutter, per its own comment). Both `cellTemplate:` and
  `headerTemplate:` bypass it entirely, so a cell is one `Border` that stretches to its column and is
  exactly `RowHeight` tall.
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
- Flag columns are **square**: `FlagWidth` is `RowHeight` (24), so the matrix keeps reading as a
  matrix when either number moves. The angle and axis columns are wider (`44` / `40`) to hold
  `359.999` / `-1000` in the mono face at 10 px.
- ⚠️ `pin: PinPosition.Left` on the frame column is **metadata only** in this preview:
  `GetPinnedColumnGroups()` exists on `DataGridState` but nothing in `DataGridComponent` reads it, so
  the column does not stay put under a horizontal scroll.
- Rows are memoized in `Render()` with `UseMemo` on the **text** — the grid keys its mount off the
  source's identity, so a source rebuilt per render would remount it and drop the scroll position.
  The text is the right key because it is what the frames are derived from.

### Editing

There is none in the table — it is the text region's job, and that is deliberate: two editable
representations of one script would have to be kept in step, and the frame list is lossy in the
directions that matter (see *Script formats*). The grid's own inline editing (`editable: true`,
`EditMode.Cell`, per-column `Editor`/`SetValue`, `onRowChanged` written back through
`ListDataSource.UpdateAsync`) was built here first and then removed; what it taught is kept below in
case it comes back for another surface:

- ⚠️ **Every editable column needs its own `Editor`.** With an empty `TypeRegistry` (the default — the
  grid builds `new TypeRegistry()`) the fallback is a plain `TextBox` whose value arrives as a
  *string*, so a commit into a `bool` or `double` field throws.
- ⚠️ **Stock editors do not fit a cell.** `Editors.CheckBox()` is a WinUI `CheckBox` with a 120 DIP
  minimum width — four square flag columns' worth. The flag editor was that same control with
  `MinWidth(0)` / `MinHeight(0)`.
- ⚠️ **A numeric column edits in a plain `TextBox`, not a `NumberBox`.** A number box hosts its own
  text box, and the stock 32 DIP minimum height *of that inner box* is out of reach of the outer
  `MinHeight(0)` — measured on the live tree, the control overflowed the row and its clear button took
  the right half of the cell. The buffer has to stay the raw text (parsing per keystroke reformats
  under the caret), with the setter parsing and clamping on commit.
- ⚠️ **A commit only reaches the grid's optimism overlay, not the source.** `onRowChanged` is where the
  row has to be written back, or the next fetch brings the old value back. The grid may call it off the
  UI thread (its own contract says so); `ListDataSource` locks.

### The script text region

The bottom region is the `.tas` text of the selected script (`ScriptTextEditor.cs`), edited in place,
with a status line above it and the format's command reference beside it. The format and its tokens
are in `../docs/SCRIPT_DSL.md`; what is here is the editing surface:

- **Typing is the whole editor.** The box is a multiline `TextBox` (`AcceptsReturn`, monospaced, no
  wrap, spell-check off) — one line is one frame, so a wrapped line would read as two. No Format
  button, no line numbers, no reset: the text is the source and the converter reads it back.
- **The reference is a pane beside the editor, not a popup.** The `Commands` button in the status
  line opens it, and the boundary between the two is the docking host's own splitter — the same one
  that separates the regions of this pane (`ScriptPanel`) — so the ratio is the host's, survives
  every re-render, and can be dragged as wide as the reader wants. Which panes exist is still this
  region's state (`script:text:editor` / `script:text:commands`); the host merges a changed key set
  into its shape. Panes are bare `Document`s with the guards `ScriptPanel` uses (no close, float or
  move), which keeps the region out of the docking states the shell does not survive.
- ⚠️ **A hand-rolled splitter was tried first and taken out again.** `OnPan` on a `Border` went
  through component state, so every pan event re-rendered the whole region — 39 reference rows and
  the editor — and the boundary felt heavier than the shell's own splitters (the user's report).
  The host moves its splitter itself and costs a layout pass; the native one is what shipped.
- ⚠️ **A help line that outgrows a narrow panel is clipped, not wrapped.** The rows are grids with
  the spelling in an `Auto` column and the help in a `Star` one, so the help *has* a width to wrap
  into — but the list's `ScrollViewer` measures its content with unbounded width, which is what the
  clipping comes from (measured live). The splitter is the answer while it stays that way: drag the
  boundary and the whole line reads. If the wrap itself is wanted, it is
  `ScrollViewer.SetHorizontalScrollBarVisibility(list, ScrollBarVisibility.Disabled)` — the attached
  property, not `HorizontalScrollMode`, which only governs panning.
- **The status line is a live parse.** Every keystroke re-reads the text (`ScriptTextStatus.Of` →
  `ScriptDsl.Parse` + the mod's own cross-field limits), and the caption is either the document's
  summary — `name · N commands · last frame M` — or the converter's message as it stands, in the
  error color. A 3600-line text re-reads in ~1 ms, so there is nothing to debounce.
- ⚠️ **The box reports its lines separated by a lone `\r`** (a WinUI `TextBox` normalises its text
  that way, measured), while the format is `\n`. `ScriptDsl.Lines` puts the separator back wherever a
  text is read or written — never into the buffer: the buffer keeps exactly the bytes the control
  reported, which is what stops the reconciler writing the text back on every keystroke. And it is
  the *same* rule on the way out — `Workspace.Write` writes the file in `\n` — which is what the
  first save of this pass got wrong, see *The workspace* above.
- **A buffer per script, owned by the shell** (`ScriptBuffers`): switching to another script and back
  keeps what was typed. It sits with the shell rather than with the pane because three readers share
  it — the list's unsaved markers, the text region, and Save.
- ⚠️ **What the pane says about the script comes from the text, not from the file.** The controls
  region used to summarise the *entry* — the file as it was listed — so a script whose text had been
  fixed in the editor still read "not a script the mod would run" one line above a text region that
  parsed it perfectly. Both lines now read the same parse of the same text.
- ⚠️ **The editor takes its height from a `Grid` star row, not from a flex slot.** A `TextBox` hands
  a `FlexPanel` its *content* height, and Reactor arranges the child at its own WinUI size inside the
  slot it was given, so `Flex(grow: 1, basis: 0)` leaves the rest of the region empty — measured
  live: a 92.67 DIP box in a 116.75 DIP slot. `Grid([Star()], [Auto, Star()], …)` with the text in
  row 1 is what fills it (the Rust sibling's `Grid` with an Auto/STAR pair does the same). Measured
  after the change: the region 440 DIP, the box 368 of the 408 DIP content area, the remainder the
  caption row and the padding.
- ⚠️ **A `TextBox` keeps both scrollbars hidden until told otherwise** — a text taller than the box
  scrolls (the wheel works) with nothing on screen to say so. The attached `ScrollViewer` properties
  are the way in, and they land on the `ScrollViewer` inside the template (measured: `Auto`, local,
  12 DIP wide once the 20 000-frame script's text was in). They ride in a `.Set`, together with the
  monospace font — see the two notes below.
- ⚠️ **The font and the scrollbars are set through `.Set`, not modifiers.** `.FontFamily(…)` resolves
  the name into a `FontFamily` WinRT object while the element is *built*, which throws `COMException`
  in the headless unit layer — the `.SemiBold()` trap, measured again here. A setter runs against the
  mounted control instead, which only the app has. The cost is the usual one for `.Set`: what it
  writes is not something the headless tests can assert, so the scrollbar and the font are pinned by
  the live measurements above, not by a test.
- **The text the region shows is the file's** — read by the listing, edited in memory, written back by
  Save. There is no mock left on this side: the file is the source, and a text the parser refuses is
  the author's own half-written script, shown with the parser's message rather than hidden.
- The table reads the **same text**, on the same render: both regions are handed the pane's one parse
  of one string (`ScriptPanelView`), so what the table draws is what the editor shows. They are still
  unlinked in the *other* direction — an edit lands in the text, and the table follows on the next
  render because the text changed, not because anything pushes to it.

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

## The run

The controls region is the editor's one window onto the mod: `Save`, the four rules a run is
configured by, `Run` / `Cancel`, and a line that says what the game is doing. The mod's HTTP API is
documented in the sibling repo (`../docs/API.md`); what is here is the client.

`Api/ModApi.cs` is that client — `HttpClient` over `127.0.0.1:5223`, with every request carrying
`Connection: close` (the mod's server answers and then closes; the connection is not handed back to a
pool to be reused after the server has already dropped it) and one retry on a dropped connection,
like the python tools do. A call comes back as a **value, never an exception** — the same rule the
workspace follows, and for the same reason: the only thing a click handler or a poll tick can do with
a failure is paint it. `ApiResult<T>` spells the three ways a call can end: an answer, a refusal by
the mod (with the mod's own wording, which already names the field it refused), and no answer at all.

`Api/ApiJson.cs` holds the shapes: the request bodies leave unset fields out (`WhenWritingNull`) and
the responses **ignore** unknown keys — `/state` carries a player, a camera and a frame ring this
panel has no use for, and a new field on the mod's side must not break the editor. Serialization goes
through a source-generated context, like `ScriptJsonContext`: the app is published with NativeAOT.
`Api/GameStatus.cs` is the reader's half — online or not, the menu, the mission, the script slot and
what the mod says the levers are actually set to.

### The four rules

The rules are the levers the python tools set before a script goes out (`drmod_api.fixed_dt` /
`fps_cap` / `rng`, `r03_baseline.run_once`), plus the headless switch:

| Rule | Body | Means |
| --- | --- | --- |
| Fixed tick 1/60 | `POST /dt {"fixed":true,"ticks":true}` | one frame is exactly 1/60 s of simulation |
| Frame cap: default | `POST /fps {"cap":"game"}` | the pacer waits as the game wants it (60 in gameplay, 30 in menus) |
| Frame cap: unlimited | `POST /fps {"cap":"off"}` | the cap lifted — with a fixed tick, the run is faster than real time |
| Frame cap: custom | `POST /fps {"fps":N}` | a limit of the editor's own, clamped to `1…1000` |
| Freeze seed | `POST /rng {"pin":"freeze","seed":N}` | the AI's decisions stop depending on thread interleaving |
| Headless run | `POST /render {"headless":true}` | the picture is taken away while the run lasts |

⚠️ **The rules are not part of the script.** They are the mod's state for one run — the text format
says so itself (`../docs/SCRIPT_DSL.md` §6) — so they live in the editor's settings and no `.tas` file
is ever rewritten to hold them: the same script can be run pinned or unpinned. Nothing is sent to the
game as the panel is filled in; a run is what applies them, and the status line shows what the mod
says, never what the panel asked for.

⚠️ **The seed goes last of the three**, immediately before the script: the mod freezes the LCG on the
first tick of the *next* script, so a pin applied any earlier would land on nothing. `ApplyAsync`
sends `/dt`, `/fps`, `/rng` in that order and stops at the first refusal — a run whose levers were
refused would have gone out with the wrong configuration.

### What `Run` does

In this order, and each step is a refusal that stops the whole thing with a message:

1. **A fresh `/state`.** The poll can be half a second old, and the menu is what decides whether the
   script's own restart can be played at all.
2. **The game window to the foreground** (`GameWindow.FocusAndSettle`). The game reads its keyboard
   through `DirectInput::GetDeviceState` and gives up early when its window is not foreground, so the
   menu keys a `restart` plays arrive only while the game owns the input focus. `SetForegroundWindow`
   alone is refused unless the caller already owns the foreground, so the call goes through
   `AttachThreadInput` — the same dance `drmod_api.focus_and_settle` does, retried and followed by a
   350 ms settle. A window that will not come forward is a **warning, not a stop**: the script still
   runs, and the message says which part may be lost.
3. **The menu out of the way** (`MenuSettler`, see below).
4. **The rules** (`PlaybackRules.ApplyAsync`).
5. **`POST /script/run`** with `ScriptJson.Write` of the document — the text **on screen**, parsed
   again here, not the file: a run of a script whose edits were never saved is the run the author is
   looking at, and `Run` is not a save. A `409` (the mod's one script slot is taken) is answered by
   stopping whatever holds it and running once more — a script that ended between the poll and the
   click is a race the panel cannot see.

⚠️ **Headless is armed from the poll, and only once the script is really `running`.** The skip hooks
sit on the live device's draw calls, and putting them there while a level loads is what crashed the
game in `d3d9.dll` (measured — `../docs/HEADLESS.md` §5). The python tools apply it the same way,
from the loop that watches the status. The mod restores the render and the frame cap by itself when a
run ends, cancelled or not (`headless_service`), so nothing here counts runs back down; the run a
headless rule was applied for is remembered by its script id, and the setting is off by default.

`Cancel` is `POST /script/stop` and nothing else — the render and the cap come back on their own. It
is live exactly while the mod says a script is active, whoever started it: a run from the python tools
is a run this panel can stop.

### A menu is settled before the script arms

`MenuSettler.cs` is the editor's port of the python tools' menu work (`drmod_api.ensure_gameplay` /
`recover_fail`). It exists because of what a script's own `restart` is: a sequence of pause-menu keys,
which a menu that is *already open* simply swallows — so a run started from the pause menu, or after a
death left the fail menu up, would arm and then not move.

⚠️ **The steps are mod scripts, not synthesised input.** A one-command script — three frames of a
`pause` bit to toggle the pause menu, one `confirm` for the fail menu's preselected Retry — is how the
mod delivers menu input at all: the pause menu does not tick the input unit, so script frames are read
through the `isKeyDown`/`isKeyPressed` detours (`docs/API.md` §5). That is also why the menu is settled
*after* the window comes forward: the game reads that keyboard only while it is foreground.

What the settler will and will not do is deliberate. A game already playing is left **untouched** —
not a request beyond the status read the decision needs. The pause menu and the fail menus are pressed
out. **Anything else is refused by name** — the game's front end, a mission that has not finished
loading — because the alternative is pointing a blind confirm at a menu this client has never driven.
A menu that does not close is refused too, rather than armed into losing its keys.

### The status line

`In Game · R-03 · 58.4 fps · r03-barrier-ticks 412/1300 (running) · tick fixed 1/60 · cap 60 fps ·
rng freeze 1` — every part of it is read from `/state`, off the UI thread, **twice a second** (the
mod's HTTP server is single-threaded and lives in the game's render loop: it answers one request at a
time, so a couple of times a second is a control panel's cadence, not a TAS readout's). The poll is
one `UseEffect` + `Task.Run` + `PeriodicTimer` and the state setter marshals back to the UI thread on
its own (`docs/guide/effects-scheduling.md`); a status that has not changed re-renders nothing, since
`GameStatus` is a record. Nothing answering is its own line — `Offline — the game is not running the
mod, or the API is not reachable` — and `Run` goes quiet with it.

The region's last line says what a run of the text on screen would be — `"probe" starts after 5 ticks
of gameplay · restarts the mission first` — read from the panel's own parse of the text, or the
parser's message, or the mod's wording when the last action failed.

## Script formats

A script has three representations, and `Script/` is where they meet:

```
.tas text  ⇄  ScriptDocument  ⇄  API JSON      (the mod's POST /script/run body)

CommandRow frames      (the text converter's own intermediate — Expand/Collapse)
ScriptFrame frames     (the command table's view, read straight off the text)
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
- **`ScriptFrames.cs`** — `Expand`/`Collapse`, the text writer's own intermediate: a document in the
  angle-plus-deflection shape `Collapse` turns back into commands. Its bits are the same order as
  `CommandKeys.All`, so a column, its JSON key and its DSL token cannot drift apart — a test walks the
  list and asserts each field lights its own bit. **The command table does not use it**: `Expand`
  resolves `ls:<angle>` into axes and folds the direction flags into a stick, which is exactly what
  the table must not do (`ScriptFrameProjection` reads the text's own tokens instead).
- **`ScriptFrame.cs` / `ScriptFrameProjection.cs`** — the table's view, and the only reader of the
  text that is not `ScriptDsl.Parse`. `FlagKeys.All` is the DSL's flag tokens in the order a line
  writes them, and a `ScriptFrame` holds each stick as a `StickValue` in whichever of the two forms
  the line used.

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

- the output is a **native** x64 exe (~17 MB): no `coreclr.dll`, `hostpolicy.dll`, `TasEditorCs.dll`
  or `TasEditorCs.runtimeconfig.json` in the payload;
- a private Windows App Runtime sits next to it, so no separate .NET runtime install is needed — run
  it from any folder, ship it as a zip;
- the whole published folder is ~222 MB / 297 files. Most of that is not the app: NativeAOT's
  `.pdb` is ~88 MB, the Windows App SDK ships AI/ML pieces that a self-contained runtime carries
  along (`DirectML.dll`, `onnxruntime.dll`, `Microsoft.Windows.AI.*`, Search/SemanticIndex,
  Widgets, Workloads), and Reactor.Advanced adds Win2D. `pack.ps1` thins it down to ~75 MB — see
  *Packaging* below.

`PublishAot` is gated on `Configuration=Release`, not set unconditionally as the template does — see
the hot reload gotcha below. The cost of the gate: the trim/AOT analyzers no longer run in the Debug
dev loop, so they need a Release build before shipping.

## Packaging

The publish folder is a build artifact directory, not a distribution. `pack.ps1` turns it into one:

```powershell
pwsh -File pack.ps1 -Build -Zip     # publish Release, thin into dist/, zip it
```

⚠️ **`pwsh`, not `powershell`.** Windows PowerShell 5.1 reads `.ps1` files as ANSI, so a UTF-8
script with an em dash in a comment dies with `Unexpected token` — `pwsh` reads them as UTF-8.

**222 MB → 75 MB.** What is dropped, and why it is safe to drop:

| Dropped | Size | Why |
|---|---|---|
| `*.pdb` | 88 MB | NativeAOT debug symbols; the runtime never reads them |
| `onnxruntime.dll`, `DirectML.dll` | 38 MB | ML inference accelerators behind the Windows AI stack |
| `Microsoft.Windows.AI.*` | 3.8 MB | the Windows AI APIs (Text, Imaging, Video, …) |
| Search / SemanticIndex / PerceptiveStreaming | 8.8 MB | Search and semantic-index runtimes |
| Widgets / Workloads | 3.1 MB | the Widgets and ML Workloads runtimes |
| WebView2 (2 files) | 0.9 MB | needed only by apps that host a WebView2 |
| `Microsoft.UI.Designer`, `NPUDetect` | 0.5 MB | XAML designer host; NPU capability probing |
| locale folders except `en-us` | 3.3 MB | the UI is English-only |

The exclusions were **measured, not guessed**: the running editor loads 117 of the publish folder's
~150 binaries — `onnxruntime`, `DirectML` and every `Microsoft.Windows.*` AI/Search/Widgets binary
were never loaded in 34 of the 54 checked cases. Each thinning pass was then launched and looked at,
not just checked for a live process: the shell, the script list, the command table (29 DSL columns)
and the script text all render from the packed folder.

⚠️ **`AutoColumns<T>` would undo this** — it is reflection-based and the packaging guide flags it as
the reason trim/AOT analyzers complain. The command table builds its columns by hand (`Column<T>`),
so the reflection path stays out and the trimmer can drop what is unused.

The script is idempotent, recreates `dist/` from scratch, and **fails with a non-zero exit code**
when the publish folder has no exe or when a required core file did not survive — `TasEditorCs.exe`,
`TasEditorCs.pri`, `Reactor.pri`, `Microsoft.WindowsAppRuntime.dll`, `Microsoft.UI.Xaml*.dll`,
`CoreMessagingXP.dll`. Missing `TasEditorCs.pri` is exactly the `0xC000027B` failure documented
below, so the check doubles as a truncated-publish detector. `-IncludeDevtools` keeps the Debug-only
devtools payload, `-KeepCultures all|en-us,…` changes what locales stay.

⚠️ **Check `TasEditorCs.pri`, not `resources.pri`.** The Rust sibling's `pack.ps1` requires
`resources.pri`; this project has no such file — the app PRI is `TasEditorCs.pri` and Reactor's is
`Reactor.pri`. A check copied from the Rust script would fail on a healthy publish.

### CI

`pack.ps1` is what the release workflow calls — `.github/workflows/build.yml` runs it on a `v*` tag,
in the same job that builds the mod, so one release carries both:

```yaml
- name: Build TAS editor (C#)
  shell: pwsh
  run: |
    .\tas-editor-cs\pack.ps1 `
      -Build `
      -PublishDir .\tas-editor-cs\publish `
      -OutDir .\tas-editor-cs\out\tas-editor `
      -Zip `
      -ZipPath .\out\tas-editor-cs.zip
```

`-Build` does the `dotnet publish` itself, so the workflow needs no .NET steps of its own —
`windows-latest` ships the .NET 10 SDK and MSVC, which is what NativeAOT needs. `-ZipPath` puts the
archive in the root `out/` next to the mod's, so `upload-artifact` and `action-gh-release` name one
directory for both.

⚠️ **The editor is not uploaded to Yandex S3**, and that is deliberate: the mod's S3 step runs with
`clear: true`, which `yandex-storage-website-action` implements as an unfiltered bucket wipe
(`clearBucket()` takes no prefix — verified in the action's source). Adding a second upload would
either delete the mod's files or leave a stale copy, so the editor ships through the GitHub Release
and the workflow artifact only.

⚠️ `shell: pwsh` is **required** on the step: a `run:` on a Windows runner defaults to Windows
PowerShell 5.1, which cannot read a UTF-8 script.

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

The mod's API is the one part whose contract is a program in another repository, so it is pinned
against a **real socket**: `ModApiTests` starts a `TcpListener` that answers the way the mod's own
server does (`Connection: close`, `Content-Length`, JSON bodies) and asserts what left and in which
order — the four rules' request bodies byte for byte, a `409` arriving as its own kind of answer, a
refused lever stopping the sequence before the seed, and nothing listening reading as *offline* rather
than as a refusal. What those tests fake is the game, not the transport: a request that left with
`frames` instead of `fps` would pass against a mock and fail against the game.

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
