# TAS Editor (Rust)

Desktop TAS editor for Metal Gear Rising: Revengeance, on the stack the spike proved out:
[`dear-app`](https://github.com/dear-imgui-rs/dear-app) (window, dock space, wgpu) plus
[`dear-imgui-cte`](https://github.com/dear-imgui-rs/dear-imgui-cte) (the text editor).

This is a **full port of the C# editor** (formerly `../tas-editor-cs/`, since removed from the
repository) — same panels, same workspace, same script formats, same run controls — rebuilt on the
Rust stack instead of WinUI 3. That editor was the reference for what the editor *does*; this one is
what it does *on this stack*, and its own fixtures pin the format down byte for byte (`cargo test`).
This crate is now the only editor here.

The project grew out of a stack spike: it began as a
measurement of `dear-app` + `dear-imgui-cte` (a dock space, a 20 000-row table, a code editor) and is
now the editor. What the spike established — that this stack holds the editor's three hardest things
at once — is why the port is on it.

## Layout

```
tas-editor-rs/
├── src/
│   ├── main.rs              # the app: the window, the frame loop, the CTE text editor
│   ├── lib.rs               # the same modules as a library, so the formats are testable headlessly
│   ├── editor/              # the shell — the port of the C# sibling's Editor.cs and its panels
│   │   ├── mod.rs           #   the state, the actions, the status poll, the run
│   │   ├── shell.rs         #   one frame: the window, the dock space, the six panels, the dialogs
│   │   ├── mod_panel.rs     #   the mod's install into the game folder
│   │   ├── workspace_panel.rs # the script list and its management
│   │   ├── script_controls.rs #   Save, the run rules, Run / Apply / Cancel, the status line
│   │   ├── table.rs         #   the command table (read-only, one column per DSL token)
│   │   └── text_pane.rs     #   the `.tas` text, its status line and the command reference
│   ├── script/              # the three representations of a script, and the hub between them
│   │   ├── model.rs         #   ScriptDocument — the whole format, the hub
│   │   ├── json.rs          #   the API JSON (POST /script/run) + the mod's cross-field limits
│   │   ├── dsl.rs           #   the `.tas` text — parse and the canonical write
│   │   ├── frames.rs        #   the converter's own view (expand / collapse)
│   │   ├── projection.rs    #   the command table's view, read token by token off the text
│   │   ├── keys.rs          #   the two bit orders, and the mask helpers
│   │   ├── commands.rs      #   the DSL's vocabulary with help, for the reference panel
│   │   └── error.rs         #   ScriptFormatException — a refusal with its line and frame
│   ├── api/                 # the mod's HTTP API, as a client
│   │   ├── mod.rs           #   ModApi — gzipped bodies, one retry, failures as values
│   │   ├── json.rs          #   the request bodies and the response readers
│   │   ├── status.rs        #   GameStatus — what the panel knows about the game
│   │   └── rules.rs         #   PlaybackRules — the four levers a run applies
│   ├── workspace.rs         # the folder on disk: list, read, write, new, copy, rename, delete
│   ├── buffers.rs           # the text each script is being edited into, and what is unsaved
│   ├── settings.rs          # what survives a restart (the folders and the run rules)
│   ├── menu_settler.rs      # getting the game out of a menu before a run
│   ├── game_window.rs       # the game's window, and bringing it to the foreground
│   ├── steam.rs             # where the game is installed, from Steam's own records
│   └── mod_install/         # installing the mod into the game folder
│       ├── payload.rs       #   the two files, embedded from OUT_DIR (build.rs staged them)
│       └── installer.rs     #   detect / install / remove, by content
├── build.rs                 # builds the mod, verifies the loader's arch, stages the payload
├── pack.ps1                 # assembles out\ + the zip for a release (needs pwsh)
├── examples/                # example `.tas` scripts, opened on a first launch (copies of tools/demo)
└── tests/
    ├── golden.rs            # the C# editor's own fixtures: byte-identical JSON and text out
    ├── playback_rules.rs    # the run-rule request bodies, byte for byte
    ├── script_formats.rs    # the round trips, the refusals, the key tables
    ├── settings.rs          # the settings file: spellings, legacy shapes, unreadable values
    ├── steam_library.rs     # Steam's own records: both vdf shapes, escapes, missing drives
    ├── workspace.rs         # the on-disk file operations, on a temp folder
    └── fixtures/
        ├── all_inputs.tas   # the formats' own fixture, every input of the DSL
        └── golden/          # the canonical fixtures + goldens (originally the C# editor's)
```

## Build, run, test

```bash
cargo build
cargo run
cargo test
```

The crate has its own `[workspace]` and its own x64 `.cargo/config.toml`: the repo root forces
`i686-pc-windows-msvc` for the mod (MGR:R is a 32-bit process), which the window stack does not
build for.

`SPIKE_SECONDS=10 cargo run` stops the frame loop after ten seconds — the way the spike measured
itself, kept because it makes the app runnable from a script.

## Packaging

```powershell
pwsh -File pack.ps1 -Build -Zip                        # build, assemble, zip
pwsh -File pack.ps1 -SkipCargo -Build `
    -OutDir .\out\tas-editor-rs -ZipPath .\out\tas-editor-rs.zip   # what CI runs
```

The distribution is **three files**: the release exe and an `examples/` folder. There is nothing to
trim, because the payload is inside the exe — which is the whole reason the zip is 11 MB of a 21 MB
executable.

⚠️ **`-SkipCargo` sets `TAS_EDITOR_SKIP_MOD_BUILD=1`** for the editor's build script. That skips the
*nested mod build*, not the check for the mod: the DLL still has to exist, and the build fails naming
it if none does. The release job runs the root `build.ps1` first, so the DLL is there and the mod is
not compiled twice.

⚠️ **The exe's size is checked, not trusted.** A build made with `TAS_EDITOR_SKIP_MOD_BUILD` and no
mod DLL anywhere else would produce an editor that installs nothing — the one failure that reaches a
user as "the mod is broken" rather than "the build was wrong". `pack.ps1` refuses anything under
10 MB, and the build script refuses a missing DLL with a message naming it. Both were exercised:
hiding the DLL makes `cargo build --release` stop with

```
mod payload: the mod is not built: ...drmod_rs_lib.dll does not exist, so there is nothing to embed.
```

## The window shell

The main window **is** the dock space host: the panels dock into it, so `TAS Editor` is one window
with its own menu bar rather than a strip of panels beside it. The declared layout goes in **once**
(`DockLayoutApply::IfMissing`); after that ImGui keeps whatever the author dragged, in
`tas-editor-layout.ini` beside the exe.

⚠️ **Every section is its own window, and the default layout groups nothing.** The mod's install, the
script list, the run controls, the command table, the text and the reference are six dockable panels
— not sections stacked inside two of them. A section nested in another cannot be moved, resized
against its neighbours, or floated on its own, and the author's own arrangement is the point of a
dock space. Each panel sits in its own leaf of the split tree (`DockLayout::tabs` with one entry is
how a leaf is spelled), so the default is a *starting* shape and not a cage: what the numbers do is
pick sensible proportions, and where each panel goes is then the author's to change.

The default shape, 26 % of the width on the left:

```
┌─────────────┬──────────────────────────────────────────┐
│ Mod         │ Run                                      │
├─────────────┼──────────────────────────────────────────┤
│             │ Frames                                   │
│ Scripts     ├───────────────────────┬──────────────────┤
│             │ Script                │ Commands         │
└─────────────┴───────────────────────┴──────────────────┘
```

Panels are hidden and shown from `View` in the menu bar, or by their own close button; a closed panel
is remembered in the same `.ini`. `Commands` starts hidden and the text panel's button flips it —
one flag, so the button's word and the panel's presence cannot disagree.

⚠️ **A click is recorded, not applied, inside a frame.** The panels draw from `&`-borrows of the
editor's state, so nothing in a frame can mutate it; every action is collected into a
`FrameActions` and applied once afterwards (`Spike::apply`). This is the same rule the C# sibling
enforces by keeping its bodies in `internal static Element View(...)`; here it is the borrow checker
that enforces it, which is the nicer half of the port.

## The workspace

The folder is picked through the system dialog, read as a listing, and edited as buffers:

* **The listing is read on every write to the folder** — a save, a new file, a rename, a delete —
  and each row carries the frame count the file's own text ends on, or the parser's refusal.
* **A buffer is the text box's own text.** The CTE editor normalises its lines to `\n` (measured:
  that is what the C# sibling's `TextBox` does with `\r`), so the buffer keeps exactly what the
  control reported and `IsDirty` compares `ScriptTextStatus` readings rather than bytes.
* **Save is explicit** — the button in the controls region. A text the parser refuses saves like any
  other: a half-written script is still work worth keeping.
* **Rename is the file's, not the script's.** `name=` in a rules line is a different thing and is
  edited in the text. A name already taken is refused with a message rather than suffixed.

The folder and the run rules are remembered in `%LOCALAPPDATA%\tas-editor-rs\settings` —
`key=value` lines, **the same shape and place as the C# sibling's**, so a user moving between the two
editors keeps their workspace. A file written before the run rules existed (a bare path, no `=` in
it) is read as the folder rather than as a settings file nobody can parse.

A first launch opens on `examples/` next to the exe. ⚠️ That is a **default, not a pinned
workspace**: a picked folder is remembered and wins from then on. These are copies of
`tools/demo/`, and they drift — the crate is self-contained and a `Link` into a Rust tool directory
would break the moment it is built on its own.

## Script formats

Three representations, and `script/` is where they meet:

```
.tas text  ⇄  ScriptDocument  ⇄  API JSON      (the mod's POST /script/run body)

CommandRow frames      (the converter's own intermediate — expand / collapse)
ScriptFrame frames     (the command table's view, read straight off the text)
```

* **`ScriptDocument` is the hub, and the JSON is the source of truth.** It is the only
  representation that carries the whole format: `raw_key`, `dik_key` and `when_enemy` have no text
  spelling and no table column, so writing such a command out as a `.tas` file is an **error naming
  the command** rather than a silent loss. The format is specified in `../docs/SCRIPT_DSL.md`.
* **The DSL is strict.** An unknown token, an unknown rules attribute, a stick said twice on one
  line and an angle mixed with an axis are all errors that name their **1-based line** and carry the
  **frame the parse had reached** — the two coordinates an author navigates by.
* **`Write` is canonical** — tokens in the DSL's own order, a one-frame duration left out, invariant
  numbers, `y`+`b` folded into `by` — so `write(parse(text))` is the same text again. A stick at rest
  is written as the axes that mean it, and a full press as the compass angle.
* **The command table reads the text, not the document.** A document has already resolved
  `ls:<angle>` into axis numbers and folded the movement flags into a stick, so the two spellings are
  indistinguishable by then — and the table is meant to show what the script *says*.

## The run

The controls region is the editor's one window onto the mod (`../docs/API.md`). The four rules are
the levers the python tools set before a script goes out:

| Rule | Body | Means |
| --- | --- | --- |
| Fixed tick 1/60 | `POST /dt {"fixed":true,"ticks":true}` | one frame is exactly 1/60 s of simulation |
| Frame cap: default | `POST /fps {"cap":"game"}` | the pacer waits as the game wants it |
| Frame cap: unlimited | `POST /fps {"cap":"off"}` | the cap lifted — with a fixed tick, faster than real time |
| Frame cap: custom | `POST /fps {"fps":N}` | a limit of the editor's own, `1…1000` |
| Freeze seed | `POST /rng {"pin":"freeze","seed":N}` | the AI's decisions stop depending on thread interleaving |
| Headless run | `POST /render {"headless":true}` | the picture is taken away while the run lasts |

⚠️ **The rules are not part of the script.** They are the mod's state for one run — the text format
says so itself — so they live in the editor's settings and no `.tas` file is ever rewritten to hold
them. Nothing is sent to the game as the panel is filled in; a run is what applies them, and the
status line shows what the mod says, never what the panel asked for.

**`Run` does, in this order**, each step a refusal that stops the whole thing:

1. a **fresh `/state`** (the poll can be half a second old, and the menu decides whether the script's
   restart can be played at all);
2. the **game window to the foreground** — the game reads its keyboard through
   `DirectInput::GetDeviceState` and gives up early when its window is not foreground, so the menu
   keys a `restart` plays arrive only while the game owns the focus (a warning, not a stop);
3. a **menu settled out of the way** (`menu_settler`, the port of `drmod_api.ensure_gameplay` /
   `recover_fail`) — a menu already open swallows the restart's keys;
4. the **rules**, with the seed **last**: the mod freezes the LCG on the first tick of the *next*
   script, so a pin applied earlier would land on nothing;
5. **`POST /script/run`** with the text **on screen**, parsed again — a run of a script whose edits
   were never saved is the run the author is looking at, and `Run` is not a save. A `409` is answered
   by stopping whatever holds the slot and running once more.

⚠️ **`Run` and `Apply` happen on a worker thread.** Every step blocks — the window, the menu, posts
and a script — and doing that inside a frame would freeze the editor for as long as the game takes to
answer. What lands back is one message and one status.

⚠️ **Headless is armed from the poll, and only once the script is really `running`.** The skip hooks
sit on the live device's draw calls, and putting them there while a level loads is what crashed the
game in `d3d9.dll` (measured — `../docs/HEADLESS.md` §5). The mod restores the render by itself when
a run ends, cancelled or not, so the run a headless rule was applied for is remembered by its script
id and never re-armed.

⚠️ **The status poll is a worker thread, twice a second.** The mod's HTTP server is single-threaded
and lives in the game's render loop: it answers one request at a time, so a couple of times a second
is a control panel's cadence. A poll still in flight is not restarted until its patience runs out — a
game that is loading blocks its render loop, and the panel must not stack requests behind it.

## Installing the mod

The editor puts the mod into the game itself, so a user who wants to run a script never touches a
launcher or an injector:

```
Metal Gear Rising REVENGEANCE\
|-- METAL GEAR RISING REVENGEANCE.exe
|-- d3d9.dll                   <- the ASI loader
|-- plugins\
     |-- drmod_rs_lib.asi      <- the mod itself
```

The game folder is found the way Steam records it (`HKCU\Software\Valve\Steam\SteamPath`, then each
`libraryfolders.vdf`), and both spellings of the vdf are read — it has been a list of paths and a map
of objects over the years. Nothing there fails at its caller: no Steam, no vdf, a folder that is not
the game — all the same answer, an empty one, and the pane offers the folder picker instead.

⚠️ **A `d3d9.dll` that is already there belongs to somebody else until proven otherwise.** Almost
every machine with a ReShade, an ENB or another ASI mod has one, and overwriting it would break that
mod to install this one. The plugin is loaded by any ASI loader, so the loader is only ever **added**
when the folder has none, and only **removed** when it is byte-for-byte ours. Both writes go through
a temporary beside the target and a move over it — a truncated DLL is the one failure that looks like
a broken mod rather than a failed install.

⚠️ **Nothing here launches, kills or injects anything.** The files are the whole install, and the
message says the game has to be restarted.

⚠️ **The payload is not committed and is not a separate step — `cargo build` stages it.** `build.rs`
builds the mod (`cargo build --release --lib -p drmod-rs` in the repository root, which the root's
`.cargo/config.toml` makes i686), copies the vendored ASI loader, and puts both in `OUT_DIR/Mod/`,
where `include_bytes!` embeds them in the editor's binary. So there is no `build-mod.ps1` to run and no
`Mod/` folder to keep in sync: the bytes a running editor installs are, by construction, the ones the
build it was compiled by produced.

The two consequences worth knowing:

* **the editor's build requires the mod to exist**, not to be current — a mod mid-edit falls back to
  the DLL the last successful build left, with a `cargo:warning` saying the embedded payload is that
  older one. With no DLL anywhere the build stops and says so. `TAS_EDITOR_SKIP_MOD_BUILD=1` skips the
  build but not the check, for CI and for editor-only changes;
* **the loader's architecture is verified** before it is staged (`MZ`, `PE\0\0`, `0x014C`). A 64-bit
  `d3d9.dll` installs silently and then does nothing, which is the worst failure to debug.

⚠️ **The nested build strips the flags cargo leaked into it.** Cargo hands a build script the flags it
was invoked with, and a nested cargo inherits them; those differ from a plain `cargo build` at the
root, and cargo fingerprints rustflags — so the two builds fight over one `target/`, each finding the
other's artifacts dirty and rebuilding imgui, hudhook and sqlite3 (measured: 31 s and
`Dirty drmod-rs: the rustflags changed`, on *every* editor build). `build.rs` removes them, which makes
the nested invocation the command the user would have typed. Verified in both directions: after an
editor build the root build is a no-op, and after a root build the editor build is a no-op.

## Testing

`cargo test` runs headlessly — no window, no game: **five suites, 60 tests**. The first four are ports
of the C# sibling's own test files, which is the point — they came with their assertions, so they check
the port against a *different implementation's* expectations rather than against itself.

**`tests/golden.rs` — the format pinned byte for byte.** These fixtures were the C# editor's own,
copied from its `TasEditorCs.Tests/Fixtures/` before that editor was removed from the repository —
they are now the only copy, and `tests/fixtures/golden/` is where `drmod-script-gen` writes the
inputs. The same three assertions its `ScriptGoldenTests.cs` made are made here:

1. **the JSON round-trips into its golden** — `read(json)` → `write` is the golden `.json`, byte for
   byte. The JSON is the source of truth, so this is the strict one.
2. **the text round-trips into its golden** — `write(read(json))` is the golden `.tas`, and reading
   a golden text back writes the same text again. A golden is what a user edits, so every round trip
   through the editor must leave the file alone.
3. **the text and the JSON are the same script** — compared as the rules line plus the *frames* each
   expands to, and **not** as documents: the text moves with the stick while the JSON moves with the
   direction flags, and the four movement bits are what the two forms are allowed to differ in.

This is the strongest check the port has. It is not "the format reads back as itself", which a
private dialect would also pass — it is "the text and the JSON this editor writes are the ones the
other editor writes", which is what makes the two interchangeable.

⚠️ **The JSON is written by hand, not by `serde_json`** (`src/script/json.rs`), and the goldens are
why: .NET's `System.Text.Json` writes an integral float as an integer (`1000`, not `1000.0`), while
`serde_json` deliberately keeps the `.0` so a float stays distinguishable from an integer. Both
parse to the same value; only one matches the goldens. Reading goes through `serde` — the permissive
direction is the one where tolerance is right.

⚠️ The fixtures are a **copy, not a link**: when the C# side adds a fixture, copy it here; when a
golden changes, both editors changed the format and both goldens change. `drmod-script-gen`
regenerates the inputs from the shared DTOs.

**`tests/playback_rules.rs` — the run-rule bodies** (`PlaybackRulesTests.cs`). Every body asserted
byte for byte — they are a contract with a program in another repository, so `{"fixed":true,"ticks":true}`
and `{"cap":"off"}` are checked as strings, along with the `1..1000` clamp the mod would answer `400`
outside of. This is where porting paid for itself twice: the C# test's expectations are the mod's own
spelling, so a field renamed on one side fails here rather than at the game.

**`tests/script_formats.rs` — the format's own behaviour.** The round trips (`write(parse(text))` is
the same text, `read(write(doc))` the same document), the refusals (an unknown token, an unknown
rules attribute and a stick said twice are errors naming their line and the frame the parse had
reached), and the key tables (the converter's bit order and the table's column order each agree with
the DSL's tokens, so a column, a JSON key and a token cannot drift apart unnoticed).

**`tests/settings.rs` — what survives a restart** (`EditorSettingsTests.cs` + the settings half of
`WorkspaceTests.cs`). The file is shared with the C# editor, so the spellings are the contract: a cap
remembered by the panel's own word *and* the mod's older ones, a bare path with no `=` read as the
folder of a pre-rules file, and a seed half-typed (`not-a-number`) kept as written rather than
rewritten. ⚠️ Through a temp path, never `%LOCALAPPDATA%` — `Settings::load_from`/`save_to` exist as
the seam that makes that possible.

**`tests/steam_library.rs` — Steam's own records** (`SteamLibraryTests.cs`). Both shapes of
`libraryfolders.vdf` (the modern `"path"` objects and the old numbered list), the escape/unescape round
trip that Windows paths depend on, a library on a missing drive dropped, and garbage answered with "no
libraries" rather than a panic. ⚠️ **This suite found a real bug in the shipped code** — see
`parse_library_paths`' own docs: the old list shape was silently ignored, so a game in a non-default
library written by an old Steam was reported as not installed. The C# test for exactly that shape is
what caught it.

**`tests/workspace.rs` — the file operations.** On a temporary folder, because they are what a click
runs: the listing's trace into `.tas` files only, a refused file listed with the parser's own line, a
new file that never overwrites one that is there, a rename refused onto a taken name, and a write
that normalises the lone `\r` a text box hands back (measured) into the `\n` the format reads.

⚠️ **What these suites do not cover, and cannot:** that the UI *behaves* like the reference. They pin
the format contract, the request bodies, the file operations and the parsers — the parts with an
answer that can be written down. Whether a panel looks and feels the same next to the C# editor is an
observation only a person can make.

## Notes

* **This is the only editor in the repository.** The C# editor it was ported from (`tas-editor-cs/`)
  and the abandoned WinUI spike (`tas-editor/`) have both been removed.
* **Everything the app needs is in the crate.** `dear-app` re-exports its matching core crate as
  `dear_app::imgui`, so the UI types cannot drift out of version with the runtime.
* **`skip_present` is not used and must not be**: it gives no gain and, combined with `skip_draw`,
  crashes the game (`../docs/HEADLESS.md`).
* **`SPIKE_FPS`** caps the idle frame loop (default 200). `SPIKE_FPS=0` lifts it — only then is the
  stack's real ceiling visible. The cap is held by sleeping, not vsync: moving the window sends the
  thread into a modal loop, the swap chain loses its vsync and `Present(1)` waits for a flip event
  that never comes (measured: 13–20 fps at 0.20 ms of UI). With the cap at 200 the loop actually runs
  at ~130 fps: `Present` costs ~7 ms whatever the UI does (measured: 0.55 ms UI, 7.6 ms frame), so the
  cap is never the limiter.
* ⚠️ **The fps line's window starts at the first *frame*, not at process start.** The initialization
  before it (wgpu, the font atlas, the `.ini`) is seconds during which no frame is drawn, and a
  window that began at process start divided the frame count by it — the first window read 113–127 fps
  while its own average interval said 141. `frames × avg` now equals the reported window exactly
  (measured: 658 × 7.60 ms = 5.0 s), so the two numbers in the line cannot disagree.
* **The table's header is frozen** (`freeze(1, 1)`): `headers(true)` only submits the header row, and
  without the freeze it scrolls away with the first screenful — with 29 columns, so does the
  frame-number column. The table is submitted even with no script selected, so the header is always
  there to read a token's column against; its one body row says why it is empty.
