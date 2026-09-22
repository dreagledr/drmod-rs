# drmod-rs

## Project Overview

A Rust-based mod injector and HUD overlay for **Metal Gear Rising: Revengeance**.

- **Binary (`drmod`)**: injects the DLL into a running game process
- **Library (`drmod_rs_lib`)**: DX9 hook + ImGui overlay, reads game memory live
- **Server (`server/`)**: multiplayer relay (axum 0.8 + tokio, Docker, 64-bit)
- **Protocol (`protocol/`)**: shared TCP (JSON) and UDP (binary `PositionPacket`) types
- **Replay-types (`replay-types/`)**: shared replay DTOs (`InputUnit`/`PlayerState`/`CameraState`/`EnemyState`, `#[repr(C)]`) + `to_bytes`/`from_bytes` — on-disk layout of replay BLOBs; `input_bits` — action bits in `InputUnit`; `key_codes` — encoding of game key codes in `m_aKeysDown` words (bit order reversed: `0x8000_0000 >> (code & 31)`); `script` — DTO of the `POST /script/run` body (`ScriptRequest`/`ScriptCommand`/`ScriptInput`/`ScriptTrigger`/`RestartSpec`/`EnemyCondition` + `MAX_SCRIPT_FRAMES`), shared by the mod and `tools/script_gen`
- **dbdump (`tools/dbdump/`)**: CLI export of Record/Replay frames from `runs.db` to CSV/Parquet (90 flat columns) + `--script` mode (frames → HTTP API JSON script)
- **script_gen (`tools/script_gen/`)**: generates the JSON script fixtures for the editor's round-trip tests out of the shared DTOs (`replay-types::script`) and accepts the editor's own JSON back — `tools/script_gen/README.md`
- **TAS Editor (`tas-editor/`)**: desktop TAS editor on WinUI 3 (`windows-reactor`, Rust, self-contained x64) — UI mock for now
- **TAS Editor C# (`tas-editor-cs/`)**: the same editor rebuilt on WinUI 3 via `Microsoft.UI.Reactor` — self-contained + NativeAOT; two-pane shell with a virtualized command table over mock frames, edited inline, a `.tas` text region with a live parse and the format's command reference in a pane beside it (the docking host's own splitter), and the script converter (API JSON ⇄ `.tas` text ⇄ table frames). ⚠️ **Own conventions, English-only UI and comments: `tas-editor-cs/QWEN.md`**
- **Mod `mods/cutscene_skip/`**: standalone crate — in-engine cutscene skip (launcher + embedded DLL, no imgui/networking)

Features:
- Segment-based autosplitter with SQLite persistence and ghost replay
- Multiplayer position sync (TCP + UDP)
- World-to-screen projection (camera matrix, D3D viewport)
- Debug panel with live game state (debug builds only)
- HTTP automation API (`127.0.0.1:5223`): input scripts, game state, ring-buffer logs
- Headless runs (`POST /render`) — disable rendering (overlay/Present/game geometry) while keeping the whole frame logic
- TAS reproducibility levers: fixed time step (`POST /dt`), AI decision RNG pin (`POST /rng`), frame cap (`POST /fps`)

Built with [hudhook](https://github.com/veeenu/hudhook) for DirectX hooking and [imgui-rs](https://github.com/imgui-rs/imgui-rs) for the UI.

## Documentation Map

Deep dives and chronicles live in `docs/` and the tool READMEs — this file only summarizes and points.

| Topic | Where |
|-------|-------|
| HTTP API, pacer, frame cap, scripts, Settings window | `docs/API.md` |
| Headless runs (`POST /render`), speedup measurements | `docs/HEADLESS.md` |
| Record/Playback: input mechanics, implementation stages | `docs/REPLAY.md` |
| Verified input hypotheses and pitfalls | `docs/REPLAY_FINDINGS.md` |
| What the record write breakpoint found | `docs/REPLAY_CROSS_REVIEW.md` |
| API input status (what is ✅, what is open) | `docs/INPUT_STATUS.md` |
| Script DSL (`.tas` text ⇄ JSON ⇄ frames) | `docs/SCRIPT_DSL.md` |
| Record→Playback desync | `docs/DESYNC_ANALYSIS.md` |
| Enemies: addresses, part hierarchy, AI and RNG | `docs/ENEMY_TRACKING.md` |
| Phases/subphases, console menu with Skip | `docs/PHASE.md` |
| Lightning strike (`forward-forward-heavy`, `anim 110`) | `docs/LIGHTNING_STRIKE.md` |
| Core-script tuning (chronicle, dead ends) | `docs/SCRIPT_TUNING.md` |
| Project-wide "what does not work" summary | `docs/PITFALLS.md` |
| dbdump: columns, `--script` | `tools/dbdump/README.md` |
| script_gen: script fixtures for the editor, order of format changes | `tools/script_gen/README.md` |
| script_tuning: tools, reference recipe | `tools/script_tuning/README.md` |
| cutscene_skip mod: launcher, flags, status | `mods/cutscene_skip/README.md` |
| TAS Editor: UI, packaging, Reactor gotchas | `tas-editor/README.md` |
| TAS Editor (C#): local conventions, language, one-component-per-file, script converter, PRI publish gotcha | `tas-editor-cs/QWEN.md`, `tas-editor-cs/README.md` |

## Architecture

```
src/
├── main.rs          # Injector binary — finds the game process, injects the DLL
├── lib.rs           # HUD library — DX9 hook, ImGui overlay, game memory, main loop
├── api.rs           # HTTP API (127.0.0.1:5223) — scripts, state, ring-buffer logs
├── segment.rs       # Segment tracking — start conditions, ASL-based finish triggers, DB cleanup
├── ui.rs            # ImGui windows — debug panel (debug only), multiplayer, settings
├── game/            # Game entities — player (Pl0000), camera (cCameraGame), menu status, phases, cutscene skip
│   ├── mod.rs       #   GameMenuStatus enum, is_readable_ptr, re-exports Player/Camera/phase/cutscene_skip
│   ├── player.rs    #   Player object cache, read_player_state/read_current_input/read_pl_input/read_enemies/read_skeleton
│   ├── camera.rs    #   Camera — read_camera_state/view_proj/pos
│   ├── phase.rs     #   Phases/subphases: hash_name + order_subphase (requesting a subphase change = cutscene skip)
│   └── cutscene_skip.rs #  Console-style in-engine cutscene skip (per-frame state machine in the render loop)
├── net.rs           # TCP + UDP client for multiplayer
├── overlay.rs       # world_to_screen projection, draw_world_pos
├── render_hooks.rs  # Headless mode (`POST /render`): overlay/Present/game-geometry skip flags + MinHook stubs on the live device vtable
├── settings.rs      # User settings (ghost opacity, show ghost toggle, cutscene skip toggle)
├── d3d_render.rs    # CylinderRenderer, SphereRenderer for 3D overlays
├── skeleton.rs      # Bone/skeleton data structures
├── logger.rs        # Logging to %LOCALAPPDATA%\drmod\ (debug.log + buffered state.log)
├── tas/             # TAS (tool-assisted speedrun) — input record/replay
│   ├── addresses.rs #   Input memory addresses/constants
│   ├── db.rs        #   Replay SQLite tables, column migration + bulk insert
│   ├── replay.rs    #   Record/playback logic, input override
│   ├── hooks.rs     #   MinHook input hooks (updateInputUnit/isKeybindPressed/isKeybindDown), frame updater, randRange/randFloat, ripper/blade emulation, raw input readers
│   ├── watch.rs     #   (debug) hardware write breakpoint: DR0 on all threads + VEH, writer stack chain
│   └── types.rs     #   Re-export of replay-types DTOs + ReplayFrame/internal types
server/              # Multiplayer server (axum 0.8 + tokio, 64-bit, Docker)
protocol/            # Shared protocol types (TCP JSON + UDP binary PositionPacket)
replay-types/        # Shared replay DTOs + to_bytes/from_bytes + input_bits + script DTOs
tools/
├── demo/            # TAS demo: demo_r03_tas.py — runs the first R-03 segment 3 times (`--headless`/`--uncapped`)
├── dbdump/          # Replay frames → CSV/Parquet + --script (HTTP API JSON) (x64, own .cargo/config.toml)
├── script_gen/      # Script JSON fixtures for tas-editor-cs round-trip tests (x64, own .cargo/config.toml)
├── desync_analysis/ # pandas scripts analyzing Record→Playback desync (CSV from dbdump)
├── script_tuning/   # Core-script timing and cutscene-skip scripts (python) — see tools/script_tuning/README.md
└── disasm/          # Disassembly scripts: scan_srm, disasm (llvm-objdump by RVA), find_vtable, find_strings, peek, mem_find_u32
mods/
└── cutscene_skip/   # Standalone mod: in-engine cutscene skip (launcher + embedded DLL, no imgui); own [workspace]
tas-editor/          # TAS Editor: desktop editor on WinUI 3 (windows-reactor), self-contained; own [workspace] and x64 config
tas-editor-cs/       # TAS Editor in C#: WinUI 3 via Microsoft.UI.Reactor, self-contained + NativeAOT; own QWEN.md (English-only)
ref/                 # Git submodules — read-only reference projects
```

## Memory Offsets

All addresses are relative to the game module base (`GetModuleHandleA(null)`).

### Player object (Pl0000)

Static pointer: `base + 0x177B4A4` → dereference to get player object.

| Offset | Type | Field |
|--------|------|-------|
| `0x50` | `f32` | Position X |
| `0x54` | `f32` | Position Y |
| `0x58` | `f32` | Position Z |
| `0x870` | `i32` | Current HP |
| `0xB74` | `i32` | Sword hidden flag |
| `0x13FC` | `i32` | Sword state |

### PlayerManagerImplement

Static pointer: `base + 0x17EA100`.

| Offset | Type | Field |
|--------|------|-------|
| `0xE0` | `i32` | Main weapon ID |
| `0xE4` | `i32` | Custom weapon ID |
| `0xE8` | `i32` | Sub weapon ID |

### Game state

| Address | Type | Field |
|---------|------|-------|
| `base + 0x17E9F9C` | `i32` | GameMenuStatus (enum 0–18; 1 = InGame, 3 = PauseMenu, 6 = CutscenePause (`cEventPauseMenu`, the console menu with Skip), 8 = Mission Fail, 12 = Pause1 (immediately sets 1 = InGame, no handler), 18 = ProcessOutOfPause; status table and analysis — `docs/PHASE.md`) |
| `base + 0x1764670` | `i32` | Current mission ID |
| `base + 0x1764674` | `*const i8` | Current mission name string |
| `base + 0x14B9181` | `*const i8` | gStr — game location string |
| `base + 0x14B91AD` | `*const i8` | gStr2 — game location string 2 |
| `base + 0x14B91A8` | `*const i8` | gStr4 — mission identifier ("P118", "EV60", etc.) |
| `base + 0x19D0814` | `u32` | Global AI-decision LCG state (`randRange` `0x9DE2A0` / signed `0x9DE2D0` / `randFloat` `0x9DE300`; `state = state*214013 + 2531011`, output is `state>>16`) — reproducibility lever, `POST /rng` |

### Frame pacer (frame cap, `POST /fps`)

FPS is held by a **software pacer**, not vsync. Mechanism, speedup measurements and modes — `docs/API.md` §3.13.

| Address | Type | Field |
|---------|------|-------|
| `base + 0x1B206EC` | `u32` | **Pacer frame period** in 3 ms units: `3000·period_s` → `50` = 1/60 s (60 FPS, gameplay), `100` = 1/30 s (30 FPS, menus/cutscenes). `0` → the pacer does not wait |
| `base + 0x1B206F0` | `u32` | Previous frame stamp (same 3 ms units) |
| `base + 0x1B206D0` | `u32` | Frame mode: `1` = 30 FPS, `0` = 60 FPS |
| `base + 0x1B206D4` | `*mut` | Live `IDirect3DDevice9` (vtable is the target of the `skip_draw` stubs); `IDirect3D9` — `base + 0x1B206D8`; windowed `D3DPRESENT_PARAMETERS` — `base + 0x1B20620`, fullscreen — `base + 0x1B205E8` |
| `0xB98070` | fn | **Pacer** (called from the main loop `0xB9D650`); `Present` — `0xB97F90` (vtable `+0x44`) |
| `0xB98AD0` | fn | Period setter (same code as `0xB98140`): `mode 1` → 1/30, otherwise 1/60 |
| `.rdata` | const | `0.016666667` (1/60), `0.033333335` (1/30); `[0x16B6980]` = `3.0`, `[0x16C5358]` = `1000.0` — "seconds → 3 ms" multipliers |

The mod (`api::apply_fps_cap`, called at the start of render) rewrites `[0x1B206EC]` **every frame**: render runs inside `Present`, i.e. after the setter and before the pacer waits. Together with `POST /dt {"fixed":true}`, a lifted cap speeds runs up (measured 2026-09-13: `period=0` → 238 engine frames per second, 3.96× real time; game cap → 58.9 and 0.98×).

### Camera

Static pointer: `base + 0x17EA1D0` (cCameraGame::Instance).

| Offset | Type | Field |
|--------|------|-------|
| `0x200` | `[f32; 16]` | View-projection matrix |

### Enemies (debug panel, `read_enemies`)

Scene entities come from `EntitySystem`: `ms_Instance` = `base + 0x17E9A98`, list `m_EntityList` at `+0x38` (size `+0x0C`, first node `+0x14`, iterate via `+0x08`). On `Entity`: name `+0x04`, Behavior (m_pSceneModel) `+0x3C`, m_pInstance `+0x48`; on Behavior: position `+0x50`, HP `+0x870`, animation `+0x618`, animation frame number `+0x8B4`. Enemy filter: names `Em*`/`Ba*`/`Pl001*` (player excluded), position not (0,0,0), HP 1..1 000 000.

Enemy animation is written by the setter `0x68CAF0`, called by the action state machine `0x739F00` (selector `0x745C60`, dispatcher `0x740880`); there is no single "attack decision function" — the action is chosen randomly, via the global LCG at `base + 0x19D0814` (`POST /rng`). Full analysis (part hierarchy, bladeY, RNG sites, no vtable getter) — `docs/ENEMY_TRACKING.md`.

### Animation (Raiden)

3-level pointer chain: `base + 0x019C14C4 → +0x788 → +0x618`

| Level | Type | Field |
|-------|------|-------|
| Final | `i32` | rAnim — Raiden's current animation ID |

## Segment Tracking

### Start conditions

Position-gated: `START_CONDITIONS` table in `segment.rs`. Each mission has a spawn position with ±0.1m (XY) / ±1.0m (Y) tolerance.

### Finish conditions (ASL-based)

Derived from [livesplit_asl_mgrr](https://github.com/hau5test/livesplit_asl_mgrr) reference. Use gStr/gStr2/rAnim — no InMenu waiting:

| Mission | Trigger |
|---------|---------|
| R-00 | `gstr2: "" → "BEACH"` && `gstr == ""` |
| R-01 | `gstr: "MISTRAL03" → "MIST_RESU"` |
| R-02 | `gstr: "EVENT2"` + `rAnim` transition to 43 |
| R-03 | `gstr: "FINISH_QT" → "MON_RESUL"` |
| R-04 | `gstr: "QTE" → "SUN_RESUL"` |
| R-05 | `gstr: "STREET" → ""` |
| R-06 | `gstr: "BOSS" → "BOSS_END"` |
| R-07 | `rAnim: 70 → 297` (Armstrong QTE) |

### Database

SQLite at `%LOCALAPPDATA%\drmod\runs.db` (schemas and migrations — `src/tas/db.rs`).

- **Segments:** `runs` → `segments` (`mission_id`, `mission_name`, `started_at`, `duration_ms`) → `segment_positions` (position per frame). On flush, only the best (lowest `duration_ms`) segment per `mission_id` is kept; the ghost reads it via `load_best_ghost()`. WAL + NORMAL synchronous for fast bulk inserts.
- **Record/Playback:** `replay_runs` (`kind` = `record`/`playback`, `source_replay_id` for playback) → `replay_record_frames` / `replay_playback_frames` (per frame: `input_unit` BLOB 48 B, `state` 88 B, `camera` 92 B, `enemy` 32 B, `blade_down`, `ripper_pressed`, `raw_down`, `raw_pressed`). BLOBs are raw bytes of the `replay-types/` structs; layout is versioned by size (camera 76 B = legacy before 2026-08-18, not read by dbdump). Old databases are migrated by `ensure_replay_frame_columns` (`ALTER TABLE`).
- The mod never reads the DB (playback comes from session memory) — the tables are for history/analytics only; export goes through `tools/dbdump`.

## Building and Running

### Prerequisites

- Rust toolchain with `i686-pc-windows-msvc` target (32-bit MSVC)
- MSVC C++ build tools

### Build

```bash
cargo build --release
```

The project is configured to compile for `i686-pc-windows-msvc` (32-bit), as specified in `.cargo/config.toml`. This is required because MGR:R is a 32-bit application.

### Run

```bash
cargo run --release
```

Or with a custom window name:

```bash
cargo run --release -- -n "Custom Window Name.exe"
```

### Output

- `target/i686-pc-windows-msvc/release/drmod.exe` — injector binary (DLL embedded via `include_bytes!`, extracted to `%TEMP%` at runtime)

### Smoke tests

- `test_connect.ps1` — multiplayer server (TCP connect/disconnect, dashboard)
- `test_api.ps1` — HTTP API (health/state/script run+get+stop/logs, error paths, 20 parallel requests, optional `-Eject` final step that unloads the DLL); requires the game running with the mod injected

## Development

### Commit Rules

- **Before every commit** check that `QWEN.md` is up to date: if the change touches architecture, dependencies, new modules, references, or anything else this file covers — update the relevant sections (details belong in `docs/`; keep only a summary and a pointer here).

### Language & Edition

- Rust 2024 edition
- UI messages are in Russian
- `QWEN.md` is written in English
- ⚠️ Exception: `tas-editor-cs/` (C#, not Rust) keeps **everything** in English — in-app text, code comments, identifiers, its own `QWEN.md`. Its conventions are local to that folder and override the two lines above.

### Dependencies

| Crate | Purpose |
|-------|---------|
| `hudhook` (0.9.0, vendored fork — `vendor/hudhook`) | DirectX hooking and injection |
| `imgui` (0.12.0) | ImGui bindings for UI rendering |
| `windows` (0.62.2) | Windows API (UI windows, module loading) |
| `windows-numerics` (0.3) | Vector/matrix math for D3D projections |
| `rusqlite` (0.40.1, bundled) | SQLite for persisting run data |
| `chrono` (0.4.45) | Time formatting for run timestamps |
| `serde` / `serde_json` (1) | JSON serialization for multiplayer protocol and HTTP API |
| `drmod-protocol` | Shared types for client-server communication |
| `drmod-replay-types` | Shared replay DTOs (`InputUnit`/`PlayerState`/`CameraState`/`EnemyState`) + `to_bytes`/`from_bytes` + `input_bits` (`InputUnit` bits) + `script` (the `POST /script/run` DTO, shared with `tools/script_gen`) |

`tools/dbdump` additionally pulls (for itself only, x64): `rusqlite`, `csv`, `arrow` + `parquet` (59.x) — CSV/Parquet export; `serde` + `serde_json` — `--script` mode (JSON for the HTTP API). `tools/script_gen` pulls (x64): `drmod-replay-types` + `serde_json` — script fixtures for the editor.

### Notes

- **Thread safety**: `HelloHud` has `unsafe impl Send/Sync` for hudhook's render loop; safe because all static addresses are computed once in `new()`, never per frame.
- **Debug-only features** (`#[cfg(debug_assertions)]`): `DrmodDebug` window (record/playback status, segment timer, mission/menu status, compact player state), `Actions` window (numpad hotkey reference), numpad hotkeys, saved position. Release builds keep only Multiplayer and Settings.
- **Settings window is also the TAS control panel** (`src/ui.rs` → `render_tas_controls`, both builds): fixed `dt`, RNG pin, frame cap, the three headless checkboxes. It writes the same runtime state as the HTTP handlers through shared setters, so it cannot drift from the API — `docs/API.md` §2.
- **Headless runs** (`src/render_hooks.rs`, `POST /render`): three independent switches — `skip_overlay`, `skip_present`, `skip_draw` (MinHook stubs on the live device's `DrawPrimitive*` vtable entries). Production mode = `skip_overlay` + `skip_draw` with the cap lifted; ⚠️ `skip_present` is not part of it — no gain, and combined with `skip_draw` it crashes the game (AV in `d3d9.dll`). ≈×1.25 over a lifted cap, ≈×1.5 over a normal run; beyond that the limit is simulation (~9.7 ms/tick). ⚠️ `skip_overlay` also hides the Settings window — only `POST /render {"reset": true}` brings rendering back. `docs/HEADLESS.md`.
- **HTTP API** (`src/api.rs`, both builds): hand-rolled single-threaded server on `127.0.0.1:5223` (raw `TcpListener`, non-blocking accept, 1 s timeouts, `shutdown()` returns in bounded time so eject does not hang). `GET`-style state: `/state`, `/logs`, `/health`, `/script/{id}`; `POST` actions: `/script/run`, `/script/stop`, `/dt`, `/fps`, `/rng`, `/render`, `/eject`, `/order`, `/phase`, `/watch`. Ring buffer of 3600 frames (60 s). **A script frame is a simulation tick** (fed from the `updateInputUnit` detour, `api::feed_tick`). Full spec — `docs/API.md`.
- **Console-style cutscene skip** (`src/game/cutscene_skip.rs`; on by default, Settings checkbox turns it off): in the `P370_RESTART`/`P370_IN` scenes it holds the console-menu flags in `staFlags` (`base + 0x17EA060`) and removes the menu through the engine's own path, then requests `P370_EVENT` on a confirmed SKIP. ⚠️ Requesting a subphase loads the scene only while unpaused; ⚠️ while the flags are held, the normal pause menu cannot be opened in that scene. Stage in `GET /state` → `cutscene_skip` (`off`/`armed`/`closing`/`skipped`). `docs/PHASE.md`, `mods/cutscene_skip/`.
- **Record→Playback desync** (`docs/DESYNC_ANALYSIS.md`): the main source is a **one-frame input feed lag** — a render(K) override lands on tick K+1 — plus Present↔tick phase uncertainty. Fixed by feeding frames from the `updateInputUnit` detour (`replay::PLAYBACK_FEED`) plus heading compensation (`rsx_correction`) → **record 110 → 111/112/113/114: 4/4 success, |Δpos| 0.7–1.0 m, |Δyaw| median 0.12–0.26°**.
- **Two game copies on one machine do not work**: the second MGR:R copy dies with exit code 0 ~100 ms after `steam_api.dll` loads; saves are Steam Cloud, one file per (steamid, appid) pair. `docs/PITFALLS.md`.
- **dbdump** (`tools/dbdump/`): Record/Playback frames → CSV/Parquet (90 flat columns, incl. the nearest enemy as `enemy_*`) and `--script` (recording → JSON for `POST /script/run`). x64-only and self-contained in its directory (arrow-rs is 64-bit; the root `cargo build` skips it) — `tools/dbdump/README.md`.
- **Script DSL and the converter** (`docs/SCRIPT_DSL.md`): a script has three representations — the API JSON (`POST /script/run`), the `.tas` text (one line per frame; tokens are console pad names — `a`/`x`/`y`/`b`, `lt`/`rt`/`lb`/`rb`, `lr`, `ax`, `du`…`mr`, `ok`, `esc`, `cd`, `wk` — which are also the command table's column headers, and movement is the stick: `ls:<angle>` on the compass, `lsx`/`lsy` exact values, `wk` halving) and the command table's frames. JSON is the only complete one: `raw_key`, `dik_key` and `when_enemy` have no text spelling and no column, so writing them out as text is an error rather than a silent loss, and a `forward` flag is written as the stick it stands for. The DTO lives in `replay-types/src/script.rs` (shared by the mod and the tool, so the generated JSON is accepted by construction); `tools/script_gen` (Rust) writes the JSON fixtures into `tas-editor-cs/TasEditorCs.Tests/Fixtures/`, the editor writes `.expected.json`/`.expected.tas` goldens next to them, and a Rust test deserializes the editor's JSON with the mod's own types. Order of changes and commands — `tools/script_gen/README.md`.
- **script_tuning** (`tools/script_tuning/`, python): core-script timings for the `P310_RESTART` barrier flight, run speedup (`/dt` + `/fps`), restart/menu/fail-recovery automation, cutscene skip — `tools/script_tuning/README.md`, `docs/SCRIPT_TUNING.md`, `docs/PITFALLS.md`.
- **`mods/cutscene_skip/`** (standalone crate, not in the root workspace): cutscene-skip port without imgui/hudhook-dx9/API/networking — launcher `cutscene_skip.exe` + embedded DLL, per-frame entry point is a MinHook on `updateFrameTime` (`0xA03970`). `mods/cutscene_skip/README.md`.
- **`tas-editor/`** (standalone crate: own `[workspace]`, `target/` and x64 `.cargo/config.toml` — the root forces i686, which WinUI 3 does not build for; lives at the repo root, not `tools/`): WinUI 3 via `windows-reactor` **0.100**, declarative, no XAML, **self-contained** via `windows-reactor-setup` in `build.rs`. `cargo run --release` to run, `pwsh -File pack.ps1 -Build -Zip` to ship (~56 MB, zip ≈20 MB). Status: UI mock; the on-disk workspace (`src/workspace.rs`) is not wired up. ⚠️ Self-contained gotchas (a truncated `.nupkg` = "green" build with no runtime) and Reactor rendering — `tas-editor/README.md`.
- User-facing errors use Windows `MessageBoxW`; the library is built as both `cdylib` (injection) and `rlib`.

### Reference Projects

In `ref/` as git submodules (read-only):

| Project | Source | For |
|---------|--------|-----|
| `mgr-plugin-sdk` | [Frouk3/mgr-plugin-sdk](https://github.com/Frouk3/mgr-plugin-sdk) | 529 reverse-engineered game headers (GPLv3) |
| `livesplit_asl_mgrr` | [hau5test/livesplit_asl_mgrr](https://github.com/hau5test/livesplit_asl_mgrr) | Reference autosplitter for checkpoint verification |
| `MGR-RedTrainer` | [Baromir19/MGR-RedTrainer](https://github.com/Baromir19/MGR-RedTrainer) | Reference trainer (C++) |
| `mmultiplayer` | [softsoundd/mmultiplayer](https://github.com/softsoundd/mmultiplayer) | Reference multiplayer mod for Mirror's Edge |

### Input Handling

The overlay supports keyboard input via hudhook's built-in WndProc hook — it intercepts `WM_KEYDOWN`/`WM_KEYUP` messages from the game window and feeds them to imgui-rs through `Io::add_key_event()`.

**Debug bindings** (`#[cfg(debug_assertions)]` only):

| Key | Action |
|-----|--------|
| `NumPad1` | +10m to player Y coordinate (direct memory write) |
| `NumPad2` | Save current position |
| `NumPad3` | Teleport to saved position |
| `NumPad4` | Script: run → jump → light attack → camera turn |
| `NumPad5` | Toggle record (arm → position trigger) |
| `NumPad6` | Toggle playback (arm → position trigger) |
| `NumPad7` | Emulate R (ripper) via isKeybindPressed, 1 frame |
| `NumPad8` | Blade mode (hold) toggle |

Memory writes use raw `*mut f32` pointers — since the DLL is injected, it has direct access to game memory.

### Multiplayer Protocol

- **TCP** (port 5222): JSON messages with `\0` delimiter — connect, disconnect, player list
- **UDP** (port 5222): Binary `PositionPacket` (28 bytes) — position + mission_id + HP, sent every frame
- Client: blocking TCP in separate `std::thread`, non-blocking UDP in render frame
- Server: axum-based, relays UDP to all clients in the same room
- Room concept: lobbies, no mission filtering on server side
