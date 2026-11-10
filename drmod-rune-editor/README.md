# TAS Editor (Rune) — experimental

An experimental second shell for the Metal Gear Rising TAS editor, built on
[Rune](https://github.com/anomalyco/rune) instead of `dear-app` + `dear-imgui-cte`.

The shipping editor is `../drmod-tas-editor/` and remains the tool. This crate asks one question:
**can a retained GPU UI carry the same editor at the same speed?** It shares the script formats
(`drmod-script`) and the mod's HTTP API; it owns only the view.

The UI strings are Russian, like the rest of the project's UI.

## What works

- the workspace: open a folder, list `.tas` files, new / duplicate / rename / delete, buffers and
  unsaved markers, Save;
- the text: Rune's `TextArea` in a monospace kit, syntax colouring for the DSL (rules line,
  comments, frame numbers, sticks, buttons), parser status, `Ctrl+S`;
- the frames table: a virtualised `rune_kit::Table`, one row per frame, read from
  `drmod_script::projection`;
- the run controls: Save / Run / Apply / Stop, the four playback rules, the seed and the headless
  toggle, a status poll every 500 ms, menu settling via the mod's HTTP API, `409` retried once;
- the settings file, shared with the ImGui editor on Windows
  (`%LOCALAPPDATA%\drmod-tas-editor\settings`) and at
  `$XDG_CONFIG_HOME/drmod-tas-editor/settings` (or `~/.config/...`) elsewhere;
- the reference dialog, the mod dialog, and the rename/delete confirmations.

The mod installer, Steam discovery, and bringing the game's window forward are not ported yet.
The first two stay in the ImGui editor; the window focus is behind `cfg(windows)` in
`src/platform.rs` and is a no-op elsewhere, so this build runs and edits on Linux.

## Build and run

The crate is its own workspace (like the ImGui editor) because the repository root forces the
game's 32-bit Windows target. Its `.cargo/config.toml` picks `x86_64-unknown-linux-gnu` for this
checkout; change that line to `x86_64-pc-windows-msvc` on Windows.

```sh
cargo run                 # the window
cargo test                # highlight, run-order and shell tests, headless
cargo build --release
```

Rune is pre-release, not on crates.io: `Cargo.toml` pins a `dev` rev
(`0c8a62e777b171f41cd0c99fb66c63d1b386e899`). `dev` is never rewritten, so the pin is reproducible.

### Windows build from GitHub

`.github/workflows/rune-editor.yml` builds the editor on a real `windows-latest` runner, runs its
tests, and uploads a runnable zip. Trigger it from **Actions → Rune editor (Windows) → Run
workflow** (or by pushing to `experiment/rune-editor`); download the
`drmod-rune-editor-windows` artifact. It is not part of the release-tag workflow: this crate is an
experiment, not a packaged tool.

The workflow passes `--target x86_64-pc-windows-msvc`, which overrides this checkout's Linux
target in `.cargo/config.toml`.

### Headless

Rune's agent tooling renders the same tree without a window, on a fixed virtual clock:

```sh
cargo run -- --snap out.png                       # one frame
cargo run -- --tree out.json                      # keys, rects, roles
cargo run -- --script events.json --snap out.png --at 1.5
```

The status poll is off in headless runs so a snapshot does not wait on a game. To drive the real
HTTP API from a scripted run, start a mock mod on `127.0.0.1:5223` and set `DRMOD_RUNE_POLL=1`:

```sh
DRMOD_RUNE_POLL=1 cargo run -- --script events.json --snap live.png --at 3
```

The first launch opens `./examples/` if it holds scripts (and `../drmod-tas-editor/examples/`
if this checkout has no examples of its own).

## Where things are

```
src/
├── main.rs              # AppSpec: title, families, poll, on_key
├── app/
│   ├── mod.rs           # App state, the one reading of the text, the poll
│   └── actions.rs       # workspace and run actions
├── panels/
│   ├── mod.rs           # toolbar, SplitView, vertical splitter
│   ├── sidebar.rs       # the script list and its file operations
│   ├── editor.rs        # the text, its status and its colouring
│   ├── frames.rs        # the virtualised frame table
│   ├── run.rs           # run controls, rules, game status
│   └── dialogs.rs       # rename, delete, commands, mod
├── highlight.rs         # the DSL lexer → Rune spans
├── tools/make-stress.py # regenerates examples/stress-long.tas (4 018 lines / 399 901 frames)
├── poller.rs            # GET /state on a worker thread
├── work.rs              # Run / Apply / Stop on a worker thread
├── api/                 # the mod's HTTP client (copied from the ImGui editor)
├── menu_settler.rs      # pause / codec / fail menus, through the API
├── workspace.rs         # the folder and its files (copied)
├── buffers.rs           # typed text per path (copied)
├── settings.rs          # what survives a restart (copied, cross-platform path)
└── platform.rs          # the game's window, on Windows
```

`workspace`, `buffers`, `settings` and `api` are adapted copies of the ImGui editor's modules. If
this shell proves itself, they should move into a shared `drmod-editor-core` rather than stay
duplicated.

## What Rune does not give us (and what it costs)

- **No dock space.** The shell is a sidebar / main / inspector split, plus a vertical splitter for
  the frames table. Panels cannot be dragged out or re-docked the way ImGui's can.
- **`TextArea` wraps.** There is no horizontal scroll, no gutter, no line numbers and no
  language-aware editing keys. Colouring is `Span`s rebuilt when the text changes.
- **`TextArea` lays out the whole text.** A 4 018-line stress script (`examples/stress-long.tas`)
  opens in ~320 ms and each edit costs ~180–250 ms; realistic scripts (tens of lines) are fine.
  Virtual lines would be the fix, and that is a feature of its own.
- **The table is vertical-only.** The frame table uses four readable columns (frame, both sticks,
  held buttons) instead of one lit column per DSL token; a 29-column matrix would need horizontal
  scrolling the kit's table does not provide.
- **Rune is pre-release.** A `dev` rev is pinned; updating it is a deliberate change with a
  headless snapshot diff.

See [`SPIKE.md`](SPIKE.md) for the measurements and the go/no-go notes. In short: a 19 900-row
table holds a p50 of 2.3–2.7 ms of CPU per frame (Rune headless, 1×/2×), but the first display of a
200-line script costs ~200 ms because `TextArea` shapes the whole text.
