# AGENTS.md

Rust workspace for a **Metal Gear Rising: Revengeance** (32-bit) mod: DX9 hook + ImGui overlay,
segment autosplitter, TAS record/replay, an HTTP automation API, a multiplayer server, and tools.

## Reference docs

- `QWEN.md` — deep reference (architecture, memory offsets, per-feature notes). It is large and
  partly stale (it still calls the mod package `drmod-rs`; it is `drmod-core`). Verify against code.
- `docs/*.md` — focused deep dives (`API.md`, `HEADLESS.md`, `PHASE.md`, `SCRIPT_DSL.md`, `PITFALLS.md`, …).
- Per-tool READMEs: `drmod-dbdump/`, `drmod-script-gen/`, `drmod-tas-editor/`, `vendor/asi-loader/`.
- `README.md` is Russian; `QWEN.md` and `docs/` are English. **Keep UI strings Russian.**
- Repo commit rule: when architecture/dependencies/modules change, update `QWEN.md` (summary +
  pointer; details go in `docs/`).

## Build

- The root `.cargo/config.toml` forces `i686-pc-windows-msvc` (the game is 32-bit). Prerequisite:
  `rustup target add i686-pc-windows-msvc`.
- `cargo build --release` at the root builds only the i686 `[workspace] default-members`
  (`drmod-core`, `drmod-injector`, `drmod-hudhook`, `drmod-protocol`, `drmod-replay-types`), plus
  `drmod-script` as a dependency of `drmod-core`. The x64 crates are members but **not** default
  members, precisely so the root build does not cross-compile them to i686.
- Cargo resolves `.cargo/config.toml` from the **cwd's ancestors, not the manifest path** (a
  workspace member's own config is ignored when invoked from the root). Reach the x64 crates from the
  root via the aliases `cargo dbdump` / `cargo script-gen` / `cargo server` / `cargo cli` / `cargo editor`, or pass
  `--target x86_64-pc-windows-msvc` / `cd` in. `--manifest-path` does not switch the config.
- Package name ≠ output name: `drmod-core` → lib `drmod_rs_lib`; `drmod-injector` → bin `drmod`;
  `drmod-dbdump` → bin `dbdump`; `drmod-script-gen` → bin `script_gen`; `drmod-cli` → bin `drmod-tas`;
  `drmod-tas-editor` → bin `drmod-tas-editor`, lib `drmod_tas_editor`. All share the root `target/`.
- `drmod-injector` embeds the DLL via `include_bytes!("../target/i686-pc-windows-msvc/{debug,release}/drmod_rs_lib.dll")` — keep `target/` at the workspace root.
- `drmod-tas-editor/build.rs` runs a nested `cargo build --release --lib -p drmod-core`,
  PE32-checks the vendored ASI loader, and embeds both into `OUT_DIR/Mod/`.
  `TAS_EDITOR_SKIP_MOD_BUILD=1` skips the nested build but not the "DLL must exist" check. It strips
  leaked `RUSTFLAGS`/jobserver env so the editor and root builds share one cache.
- The workspace root, the editor's and xtask's `[workspace]` tables set `resolver = "3"` (all members
  are edition 2024), so no resolver warning is printed.
- The release version (the `vX.Y.Z` tag) lives once in the root `[workspace.package] version`; the
  first-party members inherit it with `version.workspace = true`. The editor has its own
  `[workspace.package]` (separate workspace) and must be bumped in the same commit; `drmod-hudhook`
  keeps its literal `0.9.0` (vendored fork tracking upstream). Only `drmod-core` surfaces it
  (`CARGO_PKG_VERSION` → `GET /health`).

## Test

- **CI does not run tests or lints.** `build.yml` builds/packages on `v*` tags only; `deploy.yml`
  deploys the server on pushes to `main`. Run verification yourself.
- Editor (largest suite, headless): `cd drmod-tas-editor && cargo test`. Golden fixtures live in
  `drmod-tas-editor/tests/fixtures/golden/` and are shared with `drmod-script-gen`.
- Mod core unit tests: `cargo test -p drmod-core` (i686).
- `drmod-hudhook` tests `dx9`/`hook` open a real DX9 device for ~7 s and `inject` tests are
  `#[ignore]`d, so a bare `cargo test` at the root triggers them. Test only the crate you changed.
- Smoke tests need a running game+mod or server: `cargo xtask test-api` (`--eject` unloads the DLL),
  `cargo xtask test-connect`.
- `clippy.toml` only raises thresholds; there is no root rustfmt config.

## Build automation (`cargo xtask`)

For ad-hoc dev from the root, the root `.cargo/config.toml` also defines x64 aliases: `cargo dbdump`,
`cargo script-gen`, `cargo server`, `cargo cli`, `cargo editor` (each adds `--target x86_64-pc-windows-msvc`;
`editor` also `--manifest-path`).

Alias in the root `.cargo/config.toml`; `xtask/` is its own workspace and `out/` is at the root.
The alias runs `cargo` from the root cwd, so xtask itself inherits the root's i686 target (its own
x64 `.cargo/config.toml` applies only if you build from inside `xtask/`). Output goes to root `out/`.

- `cargo xtask build` → `out/drmod-rs.zip` (launcher) + `out/drmod-asi.zip`
- `cargo xtask build-tools` → `out/dbdump.exe` (x64) + `out/drmod-tas.exe` (x64) + `out/drmod-tas.zip` + `out/dump-replay-input.exe` (i686)
- `cargo xtask pack-editor [--skip-cargo]` → `out/drmod-tas-editor/` + `.zip` (always builds;
  `--skip-cargo` sets `TAS_EDITOR_SKIP_MOD_BUILD=1` for the nested call only)
- CI release order: `cargo xtask build`, then `cargo xtask pack-editor --skip-cargo`.

## Layout

- `drmod-core/src/lib.rs` — the mod's real entrypoint (hudhook DX9 render loop). `api.rs` = HTTP API
  on `127.0.0.1:5223`; `game/` = memory reads/offsets; `tas/` = record/playback + SQLite
  (`%LOCALAPPDATA%\drmod\runs.db`); `render_hooks.rs` = headless.
- `drmod-replay-types/src/` — on-disk replay BLOB layout; `script.rs` is the `POST /script/run` DTO,
  shared by the mod, tools, and editor.
- `drmod-protocol/src/lib.rs` — TCP JSON + UDP `PositionPacket`. Server listens TCP+UDP `5222`; HTTP
  dashboard on `HTTP_PORT` (default 8080).
- `drmod-script/src/` — the script formats shared by the mod, editor and CLI: `dsl.rs` (`.tas` text),
  `json.rs` (API JSON + limits), `model.rs`, `frames.rs`/`projection.rs`, and `record.rs` (recorded
  frames → document). The editor re-exports it as `drmod_tas_editor::script`; the mod links it to
  accept `.tas` (`POST /script/run.tas`, `GET /script/{id}.tas`, `GET /logs.tas`).
- `drmod-cli` = `drmod-tas` CLI (`run`/`get`/`state`/`export`): own HTTP client, depends on
  `drmod-script`; no GUI. `drmod-dbdump` = `runs.db` → CSV/Parquet + `--script` (via `record`).
  `drmod-script-gen` = editor golden fixtures. `drmod-tas-editor` = the only TAS editor
  (standalone x64 workspace).
- `drmod-hudhook/` = vendored DX9-only hudhook fork; its `build.rs` compiles vendored MinHook from
  `vendor/minhook/`. `vendor/` = checked-in binaries; `ref/` = read-only git submodules (clone with
  `--recurse-submodules`).
- `test_inputs/*.json` = HTTP script fixtures; `tools/` = Python analysis + `tools/disasm`.

## Gotchas

- The HTTP API is single-threaded inside the game's render loop and stops responding while the game
  is paused/loading — always set request timeouts.
- Persisted settings (`/dt`, `/rng`, `/fps`, UI toggles) are applied on the **first render frame**,
  not in `HelloHud::new`; doing it at startup stalls game load.
- Debug-only windows/hotkeys are behind `#[cfg(debug_assertions)]`; release keeps only Multiplayer
  + Settings. File logs (`%LOCALAPPDATA%\drmod\`) are written only in debug builds or when
  `DRMOD_LOG` is set.
- Headless (`POST /render`): never enable `skip_present` (crashes with `skip_draw`); `skip_overlay`
  hides the Settings window until `POST /render {"reset":true}`.
- `/script/run` bodies may be gzipped (gzip only; the 64 KiB cap is measured on compressed bytes),
  so large scripts require gzip. Limits: `docs/API.md`.
- Two MGR:R copies on one machine do not work (Steam save/`steam_api.dll` conflict).
