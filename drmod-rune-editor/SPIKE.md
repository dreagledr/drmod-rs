# Spike: can Rune carry the TAS editor?

The question behind this crate. The shipping editor already proves that ImGui can do a dock space,
a 20 000-row table and a code editor at once. Rune offers a different trade: plain-Rust views,
live theming and motion, but no dock space and a general text field instead of a code editor. This
spike measures the three hard things in that stack and records where it stops.

Environment: Linux, `x86_64-unknown-linux-gnu`, headless Rune (`--snap` / `--tree`), window
1440×900 at scale 1, Rune `dev` @ `0c8a62e7`. Numbers are Rune's own headless frame report.

## The three hard things

### 1. A 20 000-frame table

Fixtures: 200 frame lines at 100-frame spacing, projected by
`drmod_script::projection::project` into 20 000 rows. The table is `rune_kit::Table`: only the
visible rows are built, sorting is off, columns resize, the header is sticky.

| Build | Frame CPU p50 | p95 | Worst |
|---|---|---|---|
| release | **2.9 ms** | 6.4 ms | 166 ms (the frame that opened the script) |
| debug | 8.2 ms | 13.7 ms | 200 ms |

Scrolling the table in a scripted debug run stayed in the same range (9.6 ms p50). The worst frame is
the first frame after a script is opened: the text is parsed, projected into 20 000 rows, and the spans are built for the
first time. 166 ms is visible; a production editor should move the projection off the frame or
debounce it while typing.

### 2. The text editor

Rune's `TextArea` is a rich text field, not a code editor. The spike uses it in a kit whose `ty.ui`
is the mono face, with per-frame `Span`s from a small `.tas` lexer:

- comments, the `!` rules line, its `name=`/`trig=`/`restart` attributes, values, frame numbers,
  the six stick tokens and every button token colour correctly, including UTF-8 byte offsets
  (tested with Cyrillic comments);
- typing, selection, undo, IME, focus and the caret are Rune's;
- `Ctrl+S` is handled by the field's own `on_key` and by the window's `on_key`, so it works while
  typing;
- scrolling is internal to `TextArea` (its height is fixed to the pane and the caret is kept in
  view).

What is missing compared with `dear-imgui-cte`:

- **wrapping instead of horizontal scroll** — long `.tas` lines wrap;
- **no gutter and no line numbers**; a gutter would have to be a second column synced to the
  text's wrapped layout, which Rune does not expose;
- **no language-aware editing keys** (toggle comment, bracket matching, autocomplete);
- **no syntax-aware selection**.

Typing into a 200-line script stayed responsive. A multi-thousand-line script was not measured
while typing; the parse/projection/highlight pass is O(text) on every keystroke, and that is the
next thing to debounce or move to a worker.

### 3. The layout

`rune_kit::shell::SplitView` gives a resizable sidebar, a main pane and an inspector, with a
toolbar and a title-bar inset. The frames table is a vertical split added by hand: a 7 pt drag
handle storing its own height, clamped against the body's measured height in `on_layout_tx`.

This is not a dock space. Panels cannot float, be re-docked, or be rearranged into tabs; the
author gets one split layout. Headless `--tree` confirmed the geometry and no zero-height scroll
viewport after the table was given a bounded parent.

## What else works

- the workspace: folder picker (`native-file-dialog`, XDG portal on Linux), listing, buffers,
  dirty markers, save, new / duplicate / rename / delete (rename and delete in `Dialog`s);
- the run path, verified against a mock mod: `GET /state` twice, then `POST /dt`, `/fps`, `/rng`,
  `/script/run` in that order, with the seed last; `409` retried after `/script/stop`; the menu
  settler runs before the levers;
- the status poll every 500 ms, the game panel, the run message, the Stop button;
- light and dark themes, both snapshotted; settings persisted in the same file the ImGui editor
  uses on Windows.

## What the spike says

**Rune is viable for a lean TAS editor.** The table, the colouring, the split layout and the API
work; a release build keeps a 20 000-frame script at ~3 ms of CPU per frame, which is a third of a
60 Hz budget. The result looks like a desktop app rather than a demo.

**It is not yet a drop-in replacement for the ImGui editor.** The missing code editor (wrapping,
gutter, code keys), the missing dock space, and the synchronous parse/projection on the first
frame and on every keystroke are real product differences. Porting further means either accepting
a leaner editor or building a code area (gutter + horizontal viewport + virtual lines) on top of
Rune's text primitives — a project of its own.

The experiment stops here with a working editor and a measured answer, not with a shipping
replacement.
